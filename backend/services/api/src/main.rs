mod activity;
mod api_keys;
mod auth;
mod domains;
mod emails;
mod ses_events;
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
    aws_region: String,
    console_origin: String,
    account_email_from: Option<String>,
    signup_token: Option<String>,
    event_ingest_token: String,
    webhook_signing_master_key: String,
    object_store: Option<storage::ObjectStore>,
    api_key_rate_limit_per_minute: u32,
    client_ip_rate_limit_per_minute: u32,
    trust_proxy_headers: bool,
    workspace_monthly_email_limit: u64,
    workspace_concurrent_email_limit: u32,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
    let console_origin: HeaderValue = settings.console_origin.parse()?;
    let state = AppState {
        environment: settings.app_env.clone(),
        db,
        nats,
        ses,
        aws_region: settings.aws_region.clone(),
        console_origin: settings.console_origin.clone(),
        account_email_from: settings.account_email_from.clone(),
        signup_token: settings.signup_token.clone(),
        event_ingest_token: settings.event_ingest_token.clone(),
        webhook_signing_master_key: settings.webhook_signing_master_key.clone(),
        object_store,
        api_key_rate_limit_per_minute: settings.api_key_rate_limit_per_minute,
        client_ip_rate_limit_per_minute: settings.client_ip_rate_limit_per_minute,
        trust_proxy_headers: settings.trust_proxy_headers,
        workspace_monthly_email_limit: settings.workspace_monthly_email_limit,
        workspace_concurrent_email_limit: settings.workspace_concurrent_email_limit,
    };
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .merge(auth::routes())
        .merge(api_keys::routes())
        .merge(domains::routes())
        .merge(emails::routes())
        .merge(ses_events::routes())
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
