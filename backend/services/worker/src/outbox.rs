use anyhow::Result;
use async_nats::{header::NATS_MESSAGE_ID, jetstream, HeaderMap};
use sqlx::Row;
use std::time::Duration;
use uuid::Uuid;

pub async fn run(pool: db::DbPool, context: jetstream::Context) {
    let mut interval = tokio::time::interval(Duration::from_millis(500));
    loop {
        interval.tick().await;
        if let Err(error) = publish_batch(&pool, &context).await {
            tracing::error!(error = %error, "outbox publisher failed");
        }
    }
}

async fn publish_batch(pool: &db::DbPool, context: &jetstream::Context) -> Result<()> {
    let mut tx = pool.begin().await?;
    let rows = sqlx::query("SELECT id, aggregate_id, event_type, payload FROM outbox_events WHERE published_at IS NULL AND available_at <= now() ORDER BY created_at FOR UPDATE SKIP LOCKED LIMIT 50")
        .fetch_all(&mut *tx).await?;
    for row in rows {
        let id: Uuid = row.get("id");
        let aggregate_id: Uuid = row.get("aggregate_id");
        let event_type: String = row.get("event_type");
        let payload: serde_json::Value = row.get("payload");
        let result = match event_type.as_str() {
            "email.accepted" => {
                publish(
                    context,
                    "mailer.email.send",
                    id,
                    serde_json::to_vec(&payload)?,
                )
                .await
            }
            value if value.starts_with("email.") => {
                create_webhook_deliveries(&mut tx, aggregate_id, value).await
            }
            _ => {
                sqlx::query("UPDATE outbox_events SET published_at = now(), last_error = 'unsupported event type' WHERE id = $1").bind(id).execute(&mut *tx).await?;
                continue;
            }
        };
        match result {
            Ok(()) => {
                sqlx::query("UPDATE outbox_events SET published_at = now(), attempts = attempts + 1, last_error = NULL WHERE id = $1").bind(id).execute(&mut *tx).await?;
            }
            Err(error) => {
                sqlx::query("UPDATE outbox_events SET attempts = attempts + 1, available_at = now() + interval '5 seconds', last_error = $2 WHERE id = $1").bind(id).bind(error.to_string()).execute(&mut *tx).await?;
            }
        }
    }
    tx.commit().await?;
    Ok(())
}

async fn create_webhook_deliveries(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    event_id: Uuid,
    event_type: &str,
) -> Result<()> {
    let endpoint_ids = sqlx::query_scalar::<_, Uuid>("SELECT endpoint.id FROM webhook_endpoints endpoint JOIN delivery_events event ON event.id = $1 JOIN emails email ON email.id = event.email_id WHERE endpoint.workspace_id = email.workspace_id AND endpoint.enabled = true AND endpoint.subscriptions ? $2")
        .bind(event_id)
        .bind(event_type)
        .fetch_all(&mut **tx)
        .await?;
    for endpoint_id in endpoint_ids {
        sqlx::query("INSERT INTO webhook_deliveries (endpoint_id, event_id) VALUES ($1, $2) ON CONFLICT (endpoint_id, event_id) DO NOTHING")
            .bind(endpoint_id)
            .bind(event_id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

async fn publish(
    context: &jetstream::Context,
    subject: &str,
    message_id: Uuid,
    payload: Vec<u8>,
) -> Result<()> {
    let mut headers = HeaderMap::new();
    headers.insert(NATS_MESSAGE_ID, message_id.to_string());
    context
        .publish_with_headers(subject.to_owned(), headers, payload.into())
        .await?
        .await?;
    Ok(())
}
