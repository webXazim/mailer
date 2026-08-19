use anyhow::Result;
use async_nats::{header::NATS_MESSAGE_ID, jetstream, HeaderMap};
use auth::{webhook_secret, webhook_signature};
use futures::StreamExt;
use http_body_util::Full;
use hyper::{body::Bytes, Request, StatusCode};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{
    client::legacy::{
        connect::{dns::Name, HttpConnector},
        Client,
    },
    rt::TokioExecutor,
};
use serde::Deserialize;
use sqlx::Row;
use std::{
    future::Future,
    io,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tower_service::Service;
use trust_dns_resolver::{config::ResolverConfig, TokioAsyncResolver};
use url::Url;
use uuid::Uuid;

const MAX_DELIVERIES: i32 = 8;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebhookJob {
    delivery_id: Uuid,
}

struct Delivery {
    id: Uuid,
    endpoint_id: Uuid,
    event_id: Uuid,
    url: String,
    secret_version: i32,
    event_type: String,
    occurred_at: chrono::DateTime<chrono::Utc>,
    data: serde_json::Value,
}

enum Outcome {
    Delivered(u16),
    Retry(Option<u16>, String),
    Failed(Option<u16>, String),
    AlreadyHandled,
}

#[derive(Clone)]
struct PublicResolver(TokioAsyncResolver);

impl Service<Name> for PublicResolver {
    type Response = std::vec::IntoIter<SocketAddr>;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, name: Name) -> Self::Future {
        let resolver = self.0.clone();
        Box::pin(async move {
            let addresses: Vec<SocketAddr> = resolver
                .lookup_ip(name.as_str())
                .await
                .map_err(io::Error::other)?
                .iter()
                .filter(|address| !is_private_ip(*address))
                .map(|address| SocketAddr::new(address, 0))
                .collect();
            if addresses.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "webhook host has no public addresses",
                ));
            }
            Ok(addresses.into_iter())
        })
    }
}

pub async fn run(pool: db::DbPool, context: jetstream::Context, master_key: String) -> Result<()> {
    let stream = context
        .get_or_create_stream(jetstream::stream::Config {
            name: "MAILER_WEBHOOKS".into(),
            subjects: vec!["mailer.webhook.>".into()],
            max_age: Duration::from_secs(14 * 86_400),
            duplicate_window: Duration::from_secs(86_400),
            ..Default::default()
        })
        .await?;
    let consumer = stream
        .get_or_create_consumer(
            "webhook-delivery",
            jetstream::consumer::pull::Config {
                durable_name: Some("webhook-delivery".into()),
                filter_subject: "mailer.webhook.deliver".into(),
                ack_policy: jetstream::consumer::AckPolicy::Explicit,
                ack_wait: Duration::from_secs(30),
                max_deliver: i64::from(MAX_DELIVERIES),
                max_ack_pending: 100,
                ..Default::default()
            },
        )
        .await?;
    let resolver = TokioAsyncResolver::tokio(ResolverConfig::default(), Default::default());
    let mut http = HttpConnector::new_with_resolver(PublicResolver(resolver));
    http.enforce_http(false);
    http.set_connect_timeout(Some(Duration::from_secs(5)));
    let https = HttpsConnectorBuilder::new()
        .with_native_roots()?
        .https_only()
        .enable_http1()
        .wrap_connector(http);
    let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new())
        .pool_idle_timeout(Duration::from_secs(30))
        .build(https);
    let dispatcher = tokio::spawn(dispatch_due(pool.clone(), context.clone()));
    let mut messages = consumer.messages().await?;
    while let Some(message) = messages.next().await {
        let message = match message {
            Ok(value) => value,
            Err(error) => {
                tracing::error!(error = %error, "NATS webhook read failed");
                continue;
            }
        };
        let job = match serde_json::from_slice::<WebhookJob>(&message.payload) {
            Ok(value) => value,
            Err(error) => {
                tracing::error!(error = %error, "invalid webhook job");
                message
                    .double_ack()
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                continue;
            }
        };
        let outcome = process(&pool, &client, &master_key, job.delivery_id)
            .await
            .unwrap_or_else(|error| Outcome::Retry(None, error.to_string()));
        match outcome {
            Outcome::Delivered(status) => {
                record_success(&pool, job.delivery_id, status).await?;
                message
                    .double_ack()
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            }
            Outcome::AlreadyHandled => {
                message
                    .double_ack()
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            }
            Outcome::Retry(status, reason) => {
                let attempt = delivery_attempt(&pool, job.delivery_id).await?;
                if attempt < MAX_DELIVERIES {
                    record_retry(&pool, job.delivery_id, attempt, status, &reason).await?;
                } else {
                    record_failure(&pool, job.delivery_id, status, &reason).await?;
                }
                message
                    .double_ack()
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            }
            Outcome::Failed(status, reason) => {
                record_failure(&pool, job.delivery_id, status, &reason).await?;
                message
                    .double_ack()
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            }
        }
    }
    dispatcher.abort();
    Ok(())
}

async fn process<C>(
    pool: &db::DbPool,
    client: &Client<C, Full<Bytes>>,
    master_key: &str,
    delivery_id: Uuid,
) -> Result<Outcome>
where
    C: hyper_util::client::legacy::connect::Connect + Clone + Send + Sync + 'static,
{
    let row = sqlx::query("SELECT delivery.id, delivery.status, delivery.endpoint_id, delivery.event_id, endpoint.url, endpoint.enabled, endpoint.signing_secret_version, event.event_type, event.occurred_at, event.payload FROM webhook_deliveries delivery JOIN webhook_endpoints endpoint ON endpoint.id = delivery.endpoint_id JOIN delivery_events event ON event.id = delivery.event_id WHERE delivery.id = $1")
        .bind(delivery_id)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else {
        return Ok(Outcome::AlreadyHandled);
    };
    if row.get::<String, _>("status") != "pending" {
        return Ok(Outcome::AlreadyHandled);
    }
    if !row.get::<bool, _>("enabled") {
        return Ok(Outcome::Failed(None, "webhook endpoint is disabled".into()));
    }
    let delivery = Delivery {
        id: row.get("id"),
        endpoint_id: row.get("endpoint_id"),
        event_id: row.get("event_id"),
        url: row.get("url"),
        secret_version: row.get("signing_secret_version"),
        event_type: row.get("event_type"),
        occurred_at: row.get("occurred_at"),
        data: row.get("payload"),
    };
    let url = Url::parse(&delivery.url)?;
    let body = serde_json::to_vec(&serde_json::json!({
        "id": delivery.event_id,
        "type": delivery.event_type,
        "createdAt": delivery.occurred_at,
        "data": delivery.data,
    }))?;
    let timestamp = chrono::Utc::now().timestamp();
    let webhook_id = delivery.id.to_string();
    let secret = webhook_secret(master_key, delivery.endpoint_id, delivery.secret_version);
    let signature = webhook_signature(&secret, &webhook_id, timestamp, &body);
    let request = Request::post(url.as_str())
        .header("content-type", "application/json")
        .header("user-agent", "CrescentSphere-Mailer-Webhooks/1.0")
        .header("webhook-id", &webhook_id)
        .header("webhook-timestamp", timestamp)
        .header("webhook-signature", signature)
        .body(Full::new(Bytes::from(body)))?;
    let response =
        match tokio::time::timeout(Duration::from_secs(10), client.request(request)).await {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => return Ok(Outcome::Retry(None, error.to_string())),
            Err(_) => return Ok(Outcome::Retry(None, "webhook request timed out".into())),
        };
    let status = response.status();
    Ok(if status.is_success() {
        Outcome::Delivered(status.as_u16())
    } else if retryable_status(status) {
        Outcome::Retry(Some(status.as_u16()), format!("webhook returned {status}"))
    } else {
        Outcome::Failed(Some(status.as_u16()), format!("webhook returned {status}"))
    })
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(value) => {
            value.is_private()
                || value.is_loopback()
                || value.is_link_local()
                || value.is_unspecified()
                || value.is_broadcast()
        }
        IpAddr::V6(value) => {
            value.is_loopback()
                || value.is_unspecified()
                || value.is_unique_local()
                || value.is_unicast_link_local()
        }
    }
}

async fn record_success(pool: &db::DbPool, id: Uuid, status: u16) -> Result<()> {
    let mut tx = pool.begin().await?;
    let changed = sqlx::query("UPDATE webhook_deliveries SET status = 'succeeded', attempts = attempts + 1, total_attempts = total_attempts + 1, last_error = NULL, completed_at = now(), updated_at = now() WHERE id = $1 AND status = 'pending' RETURNING endpoint_id, event_id, total_attempts")
        .bind(id).fetch_optional(&mut *tx).await?;
    if let Some(row) = changed {
        let endpoint_id: Uuid = row.get("endpoint_id");
        sqlx::query("INSERT INTO webhook_attempts (endpoint_id, event_id, attempt_number, status_code, delivered_at) VALUES ($1, $2, $3, $4, now()) ON CONFLICT DO NOTHING")
            .bind(endpoint_id)
            .bind(row.get::<Uuid, _>("event_id"))
            .bind(row.get::<i32, _>("total_attempts"))
            .bind(i32::from(status))
            .execute(&mut *tx).await?;
        sqlx::query("UPDATE webhook_endpoints SET failure_count = 0, last_success_at = now(), updated_at = now() WHERE id = $1")
            .bind(endpoint_id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn record_retry(
    pool: &db::DbPool,
    id: Uuid,
    attempt: i32,
    status: Option<u16>,
    reason: &str,
) -> Result<()> {
    let next = retry_delay(i64::from(attempt)) as i64;
    let mut tx = pool.begin().await?;
    let changed = sqlx::query("UPDATE webhook_deliveries SET attempts = attempts + 1, total_attempts = total_attempts + 1, next_attempt_at = now() + make_interval(secs => $2), last_error = $3, updated_at = now() WHERE id = $1 AND status = 'pending' RETURNING endpoint_id, event_id, total_attempts, next_attempt_at")
        .bind(id).bind(next).bind(reason).fetch_optional(&mut *tx).await?;
    if let Some(row) = changed {
        sqlx::query("INSERT INTO webhook_attempts (endpoint_id, event_id, attempt_number, status_code, error, next_retry_at) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING")
            .bind(row.get::<Uuid, _>("endpoint_id"))
            .bind(row.get::<Uuid, _>("event_id"))
            .bind(row.get::<i32, _>("total_attempts"))
            .bind(status.map(i32::from))
            .bind(reason)
            .bind(row.get::<chrono::DateTime<chrono::Utc>, _>("next_attempt_at"))
            .execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn delivery_attempt(pool: &db::DbPool, id: Uuid) -> Result<i32> {
    Ok(sqlx::query_scalar(
        "SELECT attempts + 1 FROM webhook_deliveries WHERE id = $1 AND status = 'pending'",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .unwrap_or(MAX_DELIVERIES))
}

async fn dispatch_due(pool: db::DbPool, context: jetstream::Context) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        interval.tick().await;
        if let Err(error) = publish_due(&pool, &context).await {
            tracing::error!(error = %error, "webhook dispatcher failed");
        }
    }
}

async fn publish_due(pool: &db::DbPool, context: &jetstream::Context) -> Result<()> {
    let rows = sqlx::query("SELECT id, attempts, retry_generation FROM webhook_deliveries WHERE status = 'pending' AND next_attempt_at <= now() ORDER BY next_attempt_at LIMIT 100")
        .fetch_all(pool)
        .await?;
    for row in rows {
        let id: Uuid = row.get("id");
        let attempts: i32 = row.get("attempts");
        let generation: i32 = row.get("retry_generation");
        let mut headers = HeaderMap::new();
        headers.insert(NATS_MESSAGE_ID, format!("{id}:{generation}:{attempts}"));
        context
            .publish_with_headers(
                "mailer.webhook.deliver",
                headers,
                serde_json::to_vec(&serde_json::json!({"deliveryId": id}))?.into(),
            )
            .await?
            .await?;
    }
    Ok(())
}

async fn record_failure(
    pool: &db::DbPool,
    id: Uuid,
    status: Option<u16>,
    reason: &str,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let changed = sqlx::query("UPDATE webhook_deliveries SET status = 'failed', attempts = attempts + 1, total_attempts = total_attempts + 1, last_error = $2, completed_at = now(), updated_at = now() WHERE id = $1 AND status = 'pending' RETURNING endpoint_id, event_id, total_attempts")
        .bind(id).bind(reason).fetch_optional(&mut *tx).await?;
    if let Some(row) = changed {
        let endpoint_id: Uuid = row.get("endpoint_id");
        sqlx::query("INSERT INTO webhook_attempts (endpoint_id, event_id, attempt_number, status_code, error) VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING")
            .bind(endpoint_id)
            .bind(row.get::<Uuid, _>("event_id"))
            .bind(row.get::<i32, _>("total_attempts"))
            .bind(status.map(i32::from))
            .bind(reason)
            .execute(&mut *tx).await?;
        sqlx::query("INSERT INTO webhook_dead_letters (delivery_id, reason) VALUES ($1, $2)")
            .bind(id)
            .bind(reason)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE webhook_endpoints SET failure_count = failure_count + 1, last_failure_at = now(), enabled = CASE WHEN failure_count + 1 >= 20 THEN false ELSE enabled END, updated_at = now() WHERE id = $1")
            .bind(endpoint_id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

fn retryable_status(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

fn retry_delay(delivered: i64) -> u64 {
    match delivered {
        1 => 10,
        2 => 60,
        3 => 300,
        4 => 1_800,
        5 => 7_200,
        6 => 21_600,
        _ => 86_400,
    }
}

#[cfg(test)]
mod tests {
    use super::{retry_delay, retryable_status};
    use hyper::StatusCode;

    #[test]
    fn retries_transient_http_responses() {
        assert!(retryable_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(retryable_status(StatusCode::SERVICE_UNAVAILABLE));
        assert!(!retryable_status(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn retry_schedule_reaches_one_day() {
        assert_eq!(retry_delay(1), 10);
        assert_eq!(retry_delay(8), 86_400);
    }
}
