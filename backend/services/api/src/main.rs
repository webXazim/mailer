mod activity;
mod api_keys;
mod auth;
mod dns_automation;
mod domains;
mod emails;
mod ses_events;
mod stalwart;
mod stalwart_events;
mod suppressions;
mod webhooks;

use axum::{
    extract::{DefaultBodyLimit, State},
    http::{header, HeaderValue, Method, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use config::Settings;
use serde_json::json;
use std::time::Duration;
use tokio::{net::TcpListener, signal};
use tower_http::{
    catch_panic::CatchPanicLayer,
    cors::CorsLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    set_header::SetResponseHeaderLayer,
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone)]
pub(crate) struct AppState {
    environment: String,
    db: db::DbPool,
    nats: async_nats::Client,
    ses: Option<aws_sdk_sesv2::Client>,
    domain_provider: String,
    stalwart: Option<stalwart::Client>,
    mta_public_host: Option<String>,
    mta_public_ipv4: Option<String>,
    mta_return_path_prefix: String,
    stalwart_webhook_token: Option<String>,
    stalwart_webhook_signing_key: Option<String>,
    delivery_provider: String,
    ses_delivery_available: bool,
    smtp_delivery_available: bool,
    aws_region: String,
    console_origin: String,
    account_email_from: Option<String>,
    auth_email_delivery_enabled: bool,
    turnstile_site_key: Option<String>,
    turnstile_secret_key: Option<String>,
    cloudflare_oauth_client_id: Option<String>,
    cloudflare_oauth_client_secret: Option<String>,
    cloudflare_oauth_scopes: String,
    event_ingest_token: String,
    webhook_signing_master_key: String,
    object_store: Option<storage::ObjectStore>,
    api_key_rate_limit_per_minute: u32,
    client_ip_rate_limit_per_minute: u32,
    trust_proxy_headers: bool,
    workspace_monthly_email_limit: u64,
    workspace_concurrent_email_limit: u32,
    email_content_retention_days: u32,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // AWS SDK and Turnstile HTTPS enable different rustls providers. Select one
    // explicitly before either client is constructed to prevent request panics.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let settings = Settings::from_env()?;
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            settings.log_level.clone(),
        ))
        .with(tracing_subscriber::fmt::layer().json())
        .init();
    let db = db::connect(&settings).await?;
    db::migrate(&db).await?;
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
    let ses = if settings.domain_provider == "ses" {
        let aws = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new(settings.aws_region.clone()))
            .load()
            .await;
        Some(aws_sdk_sesv2::Client::new(&aws))
    } else {
        None
    };
    let object_store = storage::ObjectStore::from_settings(&settings).await?;
    let stalwart = match (&settings.stalwart_api_url, &settings.stalwart_api_token) {
        (Some(url), Some(token)) => Some(stalwart::Client::new(url.clone(), token.clone())),
        _ => None,
    };
    let console_origin: HeaderValue = settings.console_origin.parse()?;
    let state = AppState {
        environment: settings.app_env.clone(),
        db,
        nats,
        ses,
        domain_provider: settings.domain_provider.clone(),
        stalwart,
        mta_public_host: settings.mta_public_host.clone(),
        mta_public_ipv4: settings.mta_public_ipv4.clone(),
        mta_return_path_prefix: settings.mta_return_path_prefix.clone(),
        stalwart_webhook_token: settings.stalwart_webhook_token.clone(),
        stalwart_webhook_signing_key: settings.stalwart_webhook_signing_key.clone(),
        delivery_provider: settings.delivery_provider.clone(),
        ses_delivery_available: settings.ses_configuration_set.is_some()
            && settings.ses_events_queue_url.is_some(),
        smtp_delivery_available: settings.smtp_host.is_some()
            && settings.smtp_username.is_some()
            && settings.smtp_password.is_some()
            && settings.smtp_helo_name.is_some()
            && settings.stalwart_webhook_token.is_some()
            && settings.stalwart_webhook_signing_key.is_some(),
        aws_region: settings.aws_region.clone(),
        console_origin: settings.console_origin.clone(),
        account_email_from: settings.account_email_from.clone(),
        auth_email_delivery_enabled: settings.auth_email_delivery_enabled,
        turnstile_site_key: settings.turnstile_site_key.clone(),
        turnstile_secret_key: settings.turnstile_secret_key.clone(),
        cloudflare_oauth_client_id: settings.cloudflare_oauth_client_id.clone(),
        cloudflare_oauth_client_secret: settings.cloudflare_oauth_client_secret.clone(),
        cloudflare_oauth_scopes: settings.cloudflare_oauth_scopes.clone(),
        event_ingest_token: settings.event_ingest_token.clone(),
        webhook_signing_master_key: settings.webhook_signing_master_key.clone(),
        object_store,
        api_key_rate_limit_per_minute: settings.api_key_rate_limit_per_minute,
        client_ip_rate_limit_per_minute: settings.client_ip_rate_limit_per_minute,
        trust_proxy_headers: settings.trust_proxy_headers,
        workspace_monthly_email_limit: settings.workspace_monthly_email_limit,
        workspace_concurrent_email_limit: settings.workspace_concurrent_email_limit,
        email_content_retention_days: settings.email_content_retention_days,
    };
    let _domain_verifier = tokio::spawn(domains::run_verifier(state.clone()));
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/operationalz", get(operationalz))
        .merge(auth::routes())
        .merge(api_keys::routes())
        .merge(domains::routes())
        .merge(dns_automation::routes())
        .merge(emails::routes())
        .merge(ses_events::routes())
        .merge(stalwart_events::routes())
        .merge(webhooks::routes())
        .merge(activity::routes())
        .merge(suppressions::routes())
        .with_state(state)
        .layer(DefaultBodyLimit::max(36_000_000))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
        ))
        .layer(
            CorsLayer::new()
                .allow_origin(console_origin)
                .allow_credentials(true)
                .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
                .allow_headers([
                    header::CONTENT_TYPE,
                    header::AUTHORIZATION,
                    axum::http::HeaderName::from_static("idempotency-key"),
                ]),
        )
        .layer(SetRequestIdLayer::new(
            axum::http::HeaderName::from_static("x-request-id"),
            MakeRequestUuid,
        ))
        .layer(PropagateRequestIdLayer::new(
            axum::http::HeaderName::from_static("x-request-id"),
        ))
        .layer(CatchPanicLayer::new())
        .layer(TraceLayer::new_for_http());
    let listener = TcpListener::bind(settings.http_addr).await?;
    tracing::info!(address = %settings.http_addr, environment = %settings.app_env, "api server started");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({"status":"ok"})))
}

async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    let database = tokio::time::timeout(Duration::from_secs(2), db::ping(&state.db));
    let nats = tokio::time::timeout(Duration::from_secs(2), state.nats.flush());
    let (database, nats) = tokio::join!(database, nats);
    let database_ready = matches!(database, Ok(Ok(())));
    let nats_ready = matches!(nats, Ok(Ok(())));
    let status = if database_ready && nats_ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(json!({
            "status": if status == StatusCode::OK { "ready" } else { "unavailable" },
            "environment": state.environment,
            "dependencies": { "database": database_ready, "nats": nats_ready }
        })),
    )
}

async fn operationalz(State(state): State<AppState>) -> impl IntoResponse {
    let worker = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM service_heartbeats WHERE component='worker' AND updated_at>now()-interval '45 seconds')",
    )
    .fetch_one(&state.db);
    let queue = sqlx::query_scalar::<_, bool>(
        "SELECT NOT EXISTS(SELECT 1 FROM emails WHERE status='queued' AND accepted_at<now()-interval '15 minutes')",
    )
    .fetch_one(&state.db);
    let webhooks = sqlx::query_scalar::<_, bool>(
        "SELECT NOT EXISTS(SELECT 1 FROM webhook_deliveries WHERE status='pending' AND next_attempt_at<now()-interval '15 minutes')",
    )
    .fetch_one(&state.db);
    let cleanup = sqlx::query_scalar::<_, bool>(
        "SELECT count(*)<=5000 FROM emails WHERE raw_object_key IS NOT NULL AND content_deleted_at IS NULL AND COALESCE(completed_at,sent_at,accepted_at)<now()-make_interval(days=>$1)",
    )
    .bind(i32::try_from(state.email_content_retention_days).unwrap_or(30))
    .fetch_one(&state.db);
    let (worker, queue, webhooks, cleanup) = tokio::join!(worker, queue, webhooks, cleanup);
    let worker = worker.unwrap_or(false);
    let queue = queue.unwrap_or(false);
    let webhooks = webhooks.unwrap_or(false);
    let cleanup = cleanup.unwrap_or(false);
    let status = if worker && queue && webhooks && cleanup {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(json!({
            "status": if status == StatusCode::OK { "operational" } else { "degraded" },
            "checks": { "worker": worker, "deliveryQueue": queue, "customerWebhooks": webhooks, "contentCleanup": cleanup }
        })),
    )
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
    tracing::info!("shutdown signal received");
    tokio::time::sleep(Duration::from_millis(100)).await;
}
