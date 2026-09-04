use std::time::Duration;
mod account_mail;
mod delivery;
mod events;
mod lifecycle;
mod maintenance;
mod outbox;
mod provider;
mod webhook;

use config::Settings;
use tokio::signal;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // AWS SDK and HTTPS webhooks enable different rustls providers. Select one
    // explicitly before building any TLS client, rather than panicking at runtime.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let settings = Settings::from_env()?;
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            settings.log_level.clone(),
        ))
        .with(tracing_subscriber::fmt::layer().json())
        .init();
    let db = db::connect(&settings).await?;
    db::ping(&db).await?;
    // async-nats does not automatically use credentials embedded in the URL.
    let nats_server: async_nats::ServerAddr = settings
        .nats_url
        .parse()
        .map_err(|_| anyhow::anyhow!("NATS_URL must be a valid NATS server URL"))?;
    let mut nats_options = async_nats::ConnectOptions::new();
    if let Some(username) = nats_server.username() {
        nats_options = nats_options.user_and_password(
            username.to_owned(),
            nats_server.password().unwrap_or_default().to_owned(),
        );
    }
    let nats = nats_options.connect(nats_server).await?;
    nats.flush().await?;
    let jetstream = async_nats::jetstream::new(nats);
    let needs_aws = settings.delivery_provider == "ses"
        || settings.ses_events_queue_url.is_some();
    let aws = if needs_aws {
        Some(
            aws_config::defaults(aws_config::BehaviorVersion::latest())
                .region(aws_config::Region::new(settings.aws_region.clone()))
                .load()
                .await,
        )
    } else {
        None
    };
    let ses = aws.as_ref().map(|aws| {
        let ses_config = aws_sdk_sesv2::config::Builder::from(aws)
            .retry_config(aws_config::retry::RetryConfig::standard().with_max_attempts(1))
            .timeout_config(
                aws_config::timeout::TimeoutConfig::builder()
                    .operation_timeout(Duration::from_secs(30))
                    .build(),
            )
            .build();
        aws_sdk_sesv2::Client::from_conf(ses_config)
    });
    let providers = provider::DeliveryProviders::new(ses, &settings)?;
    if settings.auth_email_delivery_enabled
        && settings.account_email_from.is_some()
        && settings
            .account_email_api_key
            .as_deref()
            .is_none_or(|key| !key.starts_with("cs_live_"))
    {
        anyhow::bail!("AUTH_EMAIL_DELIVERY_ENABLED=true requires a live ACCOUNT_EMAIL_API_KEY");
    }
    let (shutdown, stop) = tokio::sync::watch::channel(false);
    let mut account_mail = tokio::spawn(account_mail::run(
        db.clone(),
        settings.account_email_from.clone(),
        settings.internal_api_url.clone(),
        settings.account_email_api_key.clone(),
        stop.clone(),
    ));
    let object_store = storage::ObjectStore::from_settings(&settings).await?;
    let sqs = aws.as_ref().map(aws_sdk_sqs::Client::new);
    let stale = sqlx::query("UPDATE emails SET status = 'failed', completed_at = now(), processing_started_at = NULL, last_error = 'ambiguous stale provider attempt; manual review required' WHERE status = 'processing' AND processing_started_at < now() - interval '15 minutes'")
        .execute(&db).await?;
    sqlx::query("UPDATE delivery_provider_attempts SET status='ambiguous',error='Worker stopped before recording the provider result; manual review required',completed_at=now() WHERE status='processing' AND started_at < now() - interval '15 minutes'")
        .execute(&db).await?;
    if stale.rows_affected() > 0 {
        tracing::warn!(
            count = stale.rows_affected(),
            "stale delivery claims marked failed for manual review"
        );
    }
    tracing::info!(
        database = "connected",
        nats = "connected",
        region = %settings.aws_region,
        "delivery worker started"
    );
    let mut outbox = tokio::spawn(outbox::run(db.clone(), jetstream.clone()));
    let mut delivery = tokio::spawn(delivery::run(
        db.clone(),
        jetstream.clone(),
        providers,
        object_store,
        stop.clone(),
    ));
    let lifecycle = tokio::spawn(lifecycle::run(
        db.clone(),
        storage::ObjectStore::from_settings(&settings).await?,
        settings.email_content_retention_days,
    ));
    let maintenance = tokio::spawn(maintenance::run(db.clone()));
    let mut webhook = tokio::spawn(webhook::run(
        db,
        jetstream,
        settings.webhook_signing_master_key.clone(),
        stop.clone(),
    ));
    let mut events = settings.ses_events_queue_url.clone().map(|queue_url| {
        let sqs = sqs
            .clone()
            .expect("AWS client exists when SES event queue is configured");
        tokio::spawn(events::run(
            sqs,
            queue_url,
            settings.ses_events_topic_arn.clone(),
            settings.internal_api_url.clone(),
            settings.event_ingest_token.clone(),
            settings.aws_region.clone(),
        ))
    });
    tokio::select! {
        _ = shutdown_signal() => {},
        result = &mut outbox => { tracing::error!(?result,"outbox stopped"); },
        result = &mut account_mail => { tracing::error!(?result,"account mail stopped"); },
        result = &mut delivery => {
            match result { Ok(Ok(())) => tracing::warn!("delivery loop stopped"), Ok(Err(error)) => tracing::error!(error = %error, "delivery loop failed"), Err(error) => tracing::error!(error = %error, "delivery task panicked") }
        }
        result = &mut webhook => {
            match result { Ok(Ok(())) => tracing::warn!("webhook loop stopped"), Ok(Err(error)) => tracing::error!(error = %error, "webhook loop failed"), Err(error) => tracing::error!(error = %error, "webhook task panicked") }
        }
        result = async { match &mut events { Some(task) => Some(task.await), None => std::future::pending::<Option<Result<Result<(), anyhow::Error>, tokio::task::JoinError>>>().await } } => {
            if let Some(result) = result { match result { Ok(Ok(())) => tracing::warn!("SES event transport stopped"), Ok(Err(error)) => tracing::error!(error = %error, "SES event transport failed"), Err(error) => tracing::error!(error = %error, "SES event transport panicked") } }
        }
    }
    let _ = shutdown.send(true);
    // Stop fetching, but allow bounded in-flight provider requests to finish recording state.
    let _ = tokio::time::timeout(Duration::from_secs(40), async {
        if !delivery.is_finished() {
            let _ = (&mut delivery).await;
        }
        if !webhook.is_finished() {
            let _ = (&mut webhook).await;
        }
        if !account_mail.is_finished() {
            let _ = (&mut account_mail).await;
        }
    })
    .await;
    account_mail.abort();
    outbox.abort();
    delivery.abort();
    webhook.abort();
    if let Some(task) = events {
        task.abort();
    }
    lifecycle.abort();
    maintenance.abort();
    tracing::info!("worker stopped");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler")
    };
    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
