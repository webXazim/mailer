use std::time::Duration;

pub async fn run(pool: db::DbPool) {
    let mut interval = tokio::time::interval(Duration::from_secs(3_600));
    loop {
        interval.tick().await;
        if let Err(error) = clean(&pool).await {
            tracing::error!(error = %error, "abuse counter cleanup failed");
        }
    }
}

async fn clean(pool: &db::DbPool) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM api_key_rate_limits WHERE bucket_start < now() - interval '2 days'")
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM client_ip_rate_limits WHERE bucket_start < now() - interval '2 days'")
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM auth_rate_limits WHERE bucket_start < now() - interval '2 days'")
        .execute(pool)
        .await?;
    Ok(())
}
