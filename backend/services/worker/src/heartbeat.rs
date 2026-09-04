use serde_json::json;
use std::time::Duration;
use uuid::Uuid;

pub async fn run(pool: db::DbPool, mut stop: tokio::sync::watch::Receiver<bool>) {
    let instance_id = Uuid::new_v4();
    let mut interval = tokio::time::interval(Duration::from_secs(10));
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            _ = interval.tick() => {
                if let Err(error) = sqlx::query("INSERT INTO service_heartbeats(component,instance_id,details,updated_at) VALUES('worker',$1,$2,now()) ON CONFLICT(component) DO UPDATE SET instance_id=EXCLUDED.instance_id,details=EXCLUDED.details,updated_at=now()")
                    .bind(instance_id)
                    .bind(json!({"status":"running"}))
                    .execute(&pool)
                    .await
                {
                    tracing::error!(error = %error, "worker heartbeat update failed");
                }
            }
        }
    }
}
