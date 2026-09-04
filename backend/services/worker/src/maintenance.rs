use std::time::Duration;

pub async fn run(pool: db::DbPool) {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        if let Err(error) = clean(&pool).await {
            tracing::error!(error = %error, "abuse counter cleanup failed");
        }
    }
}

async fn clean(pool: &db::DbPool) -> anyhow::Result<()> {
    sqlx::query("WITH expired AS (UPDATE emails SET status='failed',completed_at=now(),last_error='Queue age or attempt limit reached; manual review required' WHERE status='queued' AND (accepted_at<now()-interval '7 days' OR processing_attempts>=5) RETURNING id) INSERT INTO delivery_dead_letters(email_id,reason,payload) SELECT id,'Queue age or attempt limit reached','{}'::jsonb FROM expired").execute(pool).await?;
    // Uncertain provider attempts must be reviewed, never blindly resent.
    sqlx::query("WITH stale AS (UPDATE emails SET status='failed',completed_at=now(),processing_started_at=NULL,last_error='Stale provider attempt; manual review required' WHERE status='processing' AND processing_started_at<now()-interval '15 minutes' RETURNING id) INSERT INTO delivery_dead_letters(email_id,reason,payload) SELECT id,'Stale provider attempt; manual review required','{}'::jsonb FROM stale").execute(pool).await?;
    sqlx::query("UPDATE delivery_provider_attempts SET status='ambiguous',error='Worker stopped before recording the provider result; manual review required',completed_at=now() WHERE status='processing' AND started_at < now() - interval '15 minutes'").execute(pool).await?;
    sqlx::query("UPDATE account_emails SET status='failed',body='',last_error='Expired or interrupted account email',updated_at=now() WHERE (status='processing' AND updated_at<now()-interval '5 minutes') OR (status='queued' AND expires_at<now())").execute(pool).await?;
    sqlx::query("DELETE FROM account_emails WHERE updated_at<now()-interval '7 days' AND status IN ('submitted','sent','failed')").execute(pool).await?;
    sqlx::query("DELETE FROM password_reset_tokens WHERE expires_at<now()-interval '1 day'")
        .execute(pool)
        .await?;
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
