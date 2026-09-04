use sqlx::Row;
use std::time::Duration;
use uuid::Uuid;

const BATCH_SIZE: usize = 250;
const MAX_BATCHES_PER_RUN: usize = 20;

pub async fn run(pool: db::DbPool, store: Option<storage::ObjectStore>, retention_days: u32) {
    let Some(store) = store else { return };
    let mut interval = tokio::time::interval(Duration::from_secs(3_600));
    loop {
        interval.tick().await;
        for _ in 0..MAX_BATCHES_PER_RUN {
            match clean_batch(&pool, &store, retention_days).await {
                Ok(count) if count < BATCH_SIZE => break,
                Ok(_) => {}
                Err(error) => {
                    tracing::error!(error = %error, "message object lifecycle cleanup failed");
                    break;
                }
            }
        }
    }
}

async fn clean_batch(
    pool: &db::DbPool,
    store: &storage::ObjectStore,
    retention_days: u32,
) -> anyhow::Result<usize> {
    let rows = sqlx::query("SELECT id, raw_object_key FROM emails WHERE raw_object_key IS NOT NULL AND content_deleted_at IS NULL AND status IN ('sent', 'delivered', 'bounced', 'complained', 'failed', 'cancelled') AND COALESCE(completed_at,sent_at,accepted_at) < now() - make_interval(days => $1) AND (content_cleanup_attempted_at IS NULL OR content_cleanup_attempted_at<now()-interval '1 hour') ORDER BY COALESCE(completed_at,sent_at,accepted_at),id LIMIT 250")
        .bind(i32::try_from(retention_days)?)
        .fetch_all(pool)
        .await?;
    let selected = rows.len();
    for row in rows {
        let id: Uuid = row.get("id");
        let key: String = row.get("raw_object_key");
        let claimed = sqlx::query("UPDATE emails SET content_cleanup_attempted_at=now() WHERE id=$1 AND raw_object_key=$2 AND content_deleted_at IS NULL AND (content_cleanup_attempted_at IS NULL OR content_cleanup_attempted_at<now()-interval '1 hour')")
            .bind(id).bind(&key).execute(pool).await?;
        if claimed.rows_affected() != 1 {
            continue;
        }
        if let Err(error) = store.delete(&key).await {
            tracing::error!(error = %error, email_id = %id, object_key = %key, "failed to delete retained email object");
            continue;
        }
        let result = sqlx::query("UPDATE emails SET raw_object_key = NULL, content_checksum = NULL, content_deleted_at = now() WHERE id = $1 AND raw_object_key = $2 AND status IN ('sent', 'delivered', 'bounced', 'complained', 'failed', 'cancelled')")
            .bind(id).bind(&key).execute(pool).await?;
        if result.rows_affected() != 1 {
            tracing::error!(email_id = %id, object_key = %key, "object deleted but database reference changed concurrently");
        }
    }
    Ok(selected)
}
