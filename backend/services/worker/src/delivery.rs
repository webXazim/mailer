use anyhow::{Context, Result};
use async_nats::jetstream::{self, message::AckKind};
use aws_sdk_sesv2::types::{Body, Content, Destination, EmailContent, Message};
use base64::Engine;
use futures::StreamExt;
use mail_builder::MessageBuilder;
use serde::Deserialize;
use sqlx::Row;
use std::time::Duration;
use uuid::Uuid;

const MAX_DELIVERIES: i64 = 5;

#[derive(Deserialize)]
struct SendJob {
    #[serde(rename = "emailId")]
    email_id: Uuid,
}

struct Email {
    id: Uuid,
    environment: String,
    sender: String,
    subject: String,
    text: Option<String>,
    html: Option<String>,
    raw_object_key: Option<String>,
    content_checksum: Option<Vec<u8>>,
    reply_to: Option<String>,
    to: Vec<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
    attachments: Vec<Attachment>,
}

struct Attachment {
    filename: String,
    content_type: String,
    content: Vec<u8>,
    disposition: Option<String>,
    content_id: Option<String>,
}

enum Outcome {
    Sent(String),
    Retry(String),
    Failed(String),
    AlreadyHandled,
}

enum ProviderFailure {
    Retryable(String),
    Ambiguous(String),
    Permanent(String),
}

pub async fn run(
    pool: db::DbPool,
    context: jetstream::Context,
    ses: aws_sdk_sesv2::Client,
    object_store: Option<storage::ObjectStore>,
) -> Result<()> {
    let stream = context
        .get_or_create_stream(jetstream::stream::Config {
            name: "MAILER_DELIVERY".into(),
            subjects: vec!["mailer.email.>".into()],
            max_age: Duration::from_secs(7 * 86_400),
            duplicate_window: Duration::from_secs(86_400),
            ..Default::default()
        })
        .await?;
    context
        .get_or_create_stream(jetstream::stream::Config {
            name: "MAILER_DLQ".into(),
            subjects: vec!["mailer.dlq.>".into()],
            max_age: Duration::from_secs(30 * 86_400),
            ..Default::default()
        })
        .await?;
    let consumer = stream
        .get_or_create_consumer(
            "email-delivery",
            jetstream::consumer::pull::Config {
                durable_name: Some("email-delivery".into()),
                filter_subject: "mailer.email.send".into(),
                ack_policy: jetstream::consumer::AckPolicy::Explicit,
                ack_wait: Duration::from_secs(120),
                max_deliver: MAX_DELIVERIES,
                max_ack_pending: 100,
                ..Default::default()
            },
        )
        .await?;
    let mut messages = consumer.messages().await?;
    while let Some(message) = messages.next().await {
        let message = match message {
            Ok(value) => value,
            Err(error) => {
                tracing::error!(error = %error, "NATS delivery read failed");
                continue;
            }
        };
        let delivered = message.info().map(|info| info.delivered).unwrap_or(1);
        let job = match serde_json::from_slice::<SendJob>(&message.payload) {
            Ok(value) => value,
            Err(error) => {
                record_dead_letter(
                    &pool,
                    &context,
                    None,
                    &message.payload,
                    format!("invalid job: {error}"),
                )
                .await;
                message
                    .double_ack()
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                continue;
            }
        };
        let outcome = process(&pool, &ses, object_store.as_ref(), job.email_id)
            .await
            .unwrap_or_else(|error| Outcome::Retry(error.to_string()));
        match outcome {
            Outcome::Sent(provider_id) => {
                tracing::info!(email_id = %job.email_id, provider_message_id = %provider_id, "email sent");
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
            Outcome::Retry(reason) if delivered < MAX_DELIVERIES => {
                tracing::warn!(email_id = %job.email_id, delivery = delivered, error = %reason, "email send will retry");
                reset_for_retry(&pool, job.email_id, &reason).await;
                message
                    .ack_with(AckKind::Nak(Some(Duration::from_secs(retry_delay(
                        delivered,
                    )))))
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            }
            Outcome::Retry(reason) | Outcome::Failed(reason) => {
                fail_email(&pool, job.email_id, &reason).await;
                record_dead_letter(
                    &pool,
                    &context,
                    Some(job.email_id),
                    &message.payload,
                    reason,
                )
                .await;
                message
                    .double_ack()
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            }
        }
    }
    Ok(())
}

async fn process(
    pool: &db::DbPool,
    ses: &aws_sdk_sesv2::Client,
    object_store: Option<&storage::ObjectStore>,
    email_id: Uuid,
) -> Result<Outcome> {
    let claimed = sqlx::query("UPDATE emails SET status = 'processing', processing_started_at = now(), processing_attempts = processing_attempts + 1, last_error = NULL WHERE id = $1 AND status = 'queued' RETURNING id, environment, sender, subject, text_body, html_body, raw_object_key, content_checksum, reply_to")
        .bind(email_id).fetch_optional(pool).await?;
    let Some(row) = claimed else {
        let state = sqlx::query(
            "SELECT status, provider_message_id, processing_started_at FROM emails WHERE id = $1",
        )
        .bind(email_id)
        .fetch_optional(pool)
        .await?;
        return Ok(match state {
            None => Outcome::Failed("email record not found".into()),
            Some(row) if row.get::<String, _>("status") == "processing" => Outcome::Failed(
                "ambiguous previous provider attempt; manual review required".into(),
            ),
            Some(_) => Outcome::AlreadyHandled,
        });
    };
    let mut email = Email {
        id: row.get("id"),
        environment: row.get("environment"),
        sender: row.get("sender"),
        subject: row.get("subject"),
        text: row.get("text_body"),
        html: row.get("html_body"),
        raw_object_key: row.get("raw_object_key"),
        content_checksum: row.get("content_checksum"),
        reply_to: row.get("reply_to"),
        to: vec![],
        cc: vec![],
        bcc: vec![],
        attachments: vec![],
    };
    match (
        email.raw_object_key.as_deref(),
        email.content_checksum.as_deref(),
    ) {
        (Some(key), Some(checksum)) => {
            let store = object_store.context("email content requires object storage")?;
            let content = store.get_verified(key, checksum).await?;
            let parsed: serde_json::Value = serde_json::from_slice(&content)?;
            email.text = parsed
                .get("text")
                .and_then(|value| value.as_str())
                .map(str::to_owned);
            email.html = parsed
                .get("html")
                .and_then(|value| value.as_str())
                .map(str::to_owned);
            if let Some(values) = parsed.get("attachments").and_then(|value| value.as_array()) {
                for value in values {
                    let content = base64::engine::general_purpose::STANDARD.decode(
                        value
                            .get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default(),
                    )?;
                    email.attachments.push(Attachment {
                        filename: value
                            .get("filename")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_owned(),
                        content_type: value
                            .get("content_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("application/octet-stream")
                            .to_owned(),
                        content,
                        disposition: value
                            .get("content_disposition")
                            .and_then(|v| v.as_str())
                            .map(str::to_owned),
                        content_id: value
                            .get("content_id")
                            .and_then(|v| v.as_str())
                            .map(str::to_owned),
                    });
                }
            }
        }
        (None, None) => {}
        _ => {
            return Ok(Outcome::Failed(
                "email content storage metadata is incomplete".into(),
            ))
        }
    }
    let recipients = sqlx::query(
        "SELECT address, recipient_type FROM email_recipients WHERE email_id = $1 ORDER BY id",
    )
    .bind(email_id)
    .fetch_all(pool)
    .await?;
    for recipient in recipients {
        match recipient.get::<String, _>("recipient_type").as_str() {
            "to" => email.to.push(recipient.get("address")),
            "cc" => email.cc.push(recipient.get("address")),
            "bcc" => email.bcc.push(recipient.get("address")),
            _ => {}
        }
    }
    let provider_id = if email.environment == "test" {
        format!("test_{}", email.id)
    } else {
        match send_ses(ses, &email).await {
            Ok(value) => value,
            Err(ProviderFailure::Retryable(reason)) => return Ok(Outcome::Retry(reason)),
            Err(ProviderFailure::Permanent(reason)) => return Ok(Outcome::Failed(reason)),
            Err(ProviderFailure::Ambiguous(reason)) => {
                return Ok(Outcome::Failed(format!(
                    "ambiguous provider result; manual review required: {reason}"
                )))
            }
        }
    };
    let updated = match sqlx::query("UPDATE emails SET status = 'sent', provider_message_id = $2, sent_at = now(), processing_started_at = NULL WHERE id = $1 AND status = 'processing'").bind(email.id).bind(&provider_id).execute(pool).await {
        Ok(value) => value,
        Err(error) => return Ok(Outcome::Failed(format!("provider accepted message but state recording failed; manual review required: {error}"))),
    };
    if updated.rows_affected() != 1 {
        return Ok(Outcome::Failed(
            "email state changed during provider send".into(),
        ));
    }
    if let Err(error) =
        sqlx::query("UPDATE email_recipients SET status = 'sent' WHERE email_id = $1")
            .bind(email.id)
            .execute(pool)
            .await
    {
        tracing::error!(email_id = %email.id, error = %error, "recipient status update failed after provider acceptance");
    }
    Ok(Outcome::Sent(provider_id))
}

async fn send_ses(
    client: &aws_sdk_sesv2::Client,
    email: &Email,
) -> Result<String, ProviderFailure> {
    if !email.attachments.is_empty() {
        return send_raw_ses(client, email).await;
    }
    let content = |value: String| Content::builder().data(value).charset("UTF-8").build();
    let body = Body::builder()
        .set_text(
            email
                .text
                .clone()
                .map(content)
                .transpose()
                .map_err(|error| ProviderFailure::Permanent(error.to_string()))?,
        )
        .set_html(
            email
                .html
                .clone()
                .map(content)
                .transpose()
                .map_err(|error| ProviderFailure::Permanent(error.to_string()))?,
        )
        .build();
    let message = Message::builder()
        .subject(
            content(email.subject.clone())
                .map_err(|error| ProviderFailure::Permanent(error.to_string()))?,
        )
        .body(body)
        .build();
    let destination = Destination::builder()
        .set_to_addresses(Some(email.to.clone()))
        .set_cc_addresses(Some(email.cc.clone()))
        .set_bcc_addresses(Some(email.bcc.clone()))
        .build();
    let mut request = client
        .send_email()
        .from_email_address(&email.sender)
        .destination(destination)
        .content(EmailContent::builder().simple(message).build());
    if let Some(reply_to) = &email.reply_to {
        request = request.reply_to_addresses(reply_to);
    }
    let response = request.send().await.map_err(classify_provider_error)?;
    response.message_id().map(str::to_owned).ok_or_else(|| {
        ProviderFailure::Ambiguous("SES response did not include a message ID".into())
    })
}

async fn send_raw_ses(
    client: &aws_sdk_sesv2::Client,
    email: &Email,
) -> Result<String, ProviderFailure> {
    let mut builder = MessageBuilder::new().from(email.sender.as_str()).to(email
        .to
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>());
    if !email.cc.is_empty() {
        builder = builder.cc(email.cc.iter().map(String::as_str).collect::<Vec<_>>());
    }
    builder = builder.subject(email.subject.as_str());
    if let Some(value) = &email.reply_to {
        builder = builder.reply_to(value.as_str());
    }
    if let Some(value) = &email.text {
        builder = builder.text_body(value.as_str());
    }
    if let Some(value) = &email.html {
        builder = builder.html_body(value.as_str());
    }
    for attachment in &email.attachments {
        if attachment.disposition.as_deref() == Some("inline") {
            builder = builder.inline(
                attachment.content_type.as_str(),
                attachment
                    .content_id
                    .as_deref()
                    .unwrap_or(attachment.filename.as_str()),
                attachment.content.clone(),
            );
        } else {
            builder = builder.attachment(
                attachment.content_type.as_str(),
                attachment.filename.as_str(),
                attachment.content.clone(),
            );
        }
    }
    let raw = builder
        .write_to_vec()
        .map_err(|error| ProviderFailure::Permanent(error.to_string()))?;
    let raw_message = aws_sdk_sesv2::types::RawMessage::builder()
        .data(raw.into())
        .build()
        .map_err(|error| ProviderFailure::Permanent(error.to_string()))?;
    let destination = Destination::builder()
        .set_to_addresses(Some(email.to.clone()))
        .set_cc_addresses(Some(email.cc.clone()))
        .set_bcc_addresses(Some(email.bcc.clone()))
        .build();
    let response = client
        .send_email()
        .destination(destination)
        .content(EmailContent::builder().raw(raw_message).build())
        .send()
        .await
        .map_err(classify_provider_error)?;
    response.message_id().map(str::to_owned).ok_or_else(|| {
        ProviderFailure::Ambiguous("SES raw response did not include a message ID".into())
    })
}

async fn reset_for_retry(pool: &db::DbPool, id: Uuid, reason: &str) {
    let _ = sqlx::query("UPDATE emails SET status = 'queued', processing_started_at = NULL, last_error = $2 WHERE id = $1 AND status = 'processing'").bind(id).bind(reason).execute(pool).await;
}
async fn fail_email(pool: &db::DbPool, id: Uuid, reason: &str) {
    let _ = sqlx::query("UPDATE emails SET status = 'failed', processing_started_at = NULL, completed_at = now(), last_error = $2 WHERE id = $1 AND status IN ('queued', 'processing')").bind(id).bind(reason).execute(pool).await;
}
async fn record_dead_letter(
    pool: &db::DbPool,
    context: &jetstream::Context,
    email_id: Option<Uuid>,
    payload: &[u8],
    reason: String,
) {
    let envelope =
        serde_json::json!({"reason": reason, "payload": String::from_utf8_lossy(payload)});
    let _ = sqlx::query(
        "INSERT INTO delivery_dead_letters (email_id, reason, payload) VALUES ($1, $2, $3)",
    )
    .bind(email_id)
    .bind(envelope["reason"].as_str().unwrap_or("unknown failure"))
    .bind(envelope.clone())
    .execute(pool)
    .await;
    match context
        .publish(
            "mailer.dlq.email",
            serde_json::to_vec(&envelope).unwrap_or_default().into(),
        )
        .await
    {
        Ok(ack) => {
            let _ = ack.await;
        }
        Err(error) => tracing::error!(error = %error, "dead-letter publish failed"),
    }
}

fn classify_provider_error(
    error: aws_sdk_sesv2::error::SdkError<aws_sdk_sesv2::operation::send_email::SendEmailError>,
) -> ProviderFailure {
    let message = error.to_string();
    let normalized = message.to_ascii_lowercase();
    if normalized.contains("throttl")
        || normalized.contains("too many request")
        || normalized.contains("service unavailable")
    {
        ProviderFailure::Retryable(message)
    } else if normalized.contains("dispatch failure")
        || normalized.contains("timeout")
        || normalized.contains("connection")
        || normalized.contains("response error")
    {
        ProviderFailure::Ambiguous(message)
    } else {
        ProviderFailure::Permanent(message)
    }
}
fn retry_delay(delivered: i64) -> u64 {
    match delivered {
        1 => 5,
        2 => 30,
        3 => 120,
        _ => 600,
    }
}
