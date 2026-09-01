use super::delivery::{classify_provider_error, ProviderFailure};
use anyhow::Result;
use aws_sdk_sesv2::types::{Body, Content, Destination, EmailContent, Message};
use sqlx::Row;
use std::time::Duration;
use uuid::Uuid;

pub async fn run(
    pool: db::DbPool,
    ses: aws_sdk_sesv2::Client,
    from: Option<String>,
    mut stop: tokio::sync::watch::Receiver<bool>,
) -> Result<()> {
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    loop {
        tokio::select! {_=stop.changed()=>break,_=interval.tick()=>{}}
        let Some(from) = from.as_deref() else {
            continue;
        };
        let row=sqlx::query("UPDATE account_emails SET status='processing',attempts=attempts+1,updated_at=now() WHERE id=(SELECT id FROM account_emails WHERE status='queued' AND available_at<=now() AND expires_at>now() ORDER BY available_at FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING id,recipient,subject,body,attempts")
            .fetch_optional(&pool).await?;
        let Some(row) = row else { continue };
        let id: Uuid = row.get("id");
        let content = Content::builder()
            .data(row.get::<String, _>("body"))
            .charset("UTF-8")
            .build()?;
        let message = Message::builder()
            .subject(
                Content::builder()
                    .data(row.get::<String, _>("subject"))
                    .charset("UTF-8")
                    .build()?,
            )
            .body(Body::builder().text(content).build())
            .build();
        let result = ses
            .send_email()
            .from_email_address(from)
            .destination(
                Destination::builder()
                    .to_addresses(row.get::<String, _>("recipient"))
                    .build(),
            )
            .content(EmailContent::builder().simple(message).build())
            .send()
            .await;
        match result {
            Ok(response) => {
                sqlx::query("UPDATE account_emails SET status='sent',body='',provider_message_id=$2,updated_at=now() WHERE id=$1").bind(id).bind(response.message_id()).execute(&pool).await?;
            }
            Err(error) => {
                let failure = classify_provider_error(error);
                let (retry, reason) = match failure {
                    ProviderFailure::Retryable(reason) => {
                        (row.get::<i32, _>("attempts") < 5, reason)
                    }
                    ProviderFailure::Permanent(reason) | ProviderFailure::Ambiguous(reason) => {
                        (false, reason)
                    }
                };
                sqlx::query("UPDATE account_emails SET status=$2,body=CASE WHEN $2='queued' THEN body ELSE '' END,last_error=$3,available_at=now()+interval '30 seconds',updated_at=now() WHERE id=$1").bind(id).bind(if retry {"queued"} else {"failed"}).bind(reason).execute(&pool).await?;
            }
        }
    }
    Ok(())
}
