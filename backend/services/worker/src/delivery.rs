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

pub(crate) struct Email {
    pub(crate) id: Uuid,
    pub(crate) attempt_id: Uuid,
    pub(crate) environment: String,
    pub(crate) delivery_provider: String,
    pub(crate) domain_id: Option<Uuid>,
    pub(crate) sender: String,
    pub(crate) subject: String,
    pub(crate) text: Option<String>,
    pub(crate) html: Option<String>,
    pub(crate) raw_object_key: Option<String>,
    pub(crate) content_checksum: Option<Vec<u8>>,
    pub(crate) reply_to: Option<String>,
    pub(crate) to: Vec<String>,
    pub(crate) cc: Vec<String>,
    pub(crate) bcc: Vec<String>,
    pub(crate) attachments: Vec<Attachment>,
}

pub(crate) struct Attachment {
    pub(crate) filename: String,
    pub(crate) content_type: String,
    pub(crate) content: Vec<u8>,
    pub(crate) disposition: Option<String>,
    pub(crate) content_id: Option<String>,
}

enum Outcome {
    Sent(String),
    Retry(String),
    Deferred(String),
    Failed(String),
    AlreadyHandled,
}

#[derive(Debug, PartialEq, Eq)]
enum ProviderControl {
    Continue,
    RollbackToSes,
    Defer,
}

enum AttemptPreparation {
    Ready,
    Deferred,
    WorkspacePaused,
    DomainUnauthorized,
}

#[derive(Debug)]
pub(crate) enum ProviderFailure {
    Retryable(String),
    Ambiguous(String),
    Permanent(String),
}

pub async fn run(
    pool: db::DbPool,
    context: jetstream::Context,
    providers: super::provider::DeliveryProviders,
    object_store: Option<storage::ObjectStore>,
    mut stop: tokio::sync::watch::Receiver<bool>,
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
    let mut messages = consumer
        .stream()
        .max_messages_per_batch(1)
        .messages()
        .await?;
    loop {
        let message =
            tokio::select! { _ = stop.changed() => break, value = messages.next() => value };
        let Some(message) = message else { break };
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
                .await?;
                message
                    .double_ack()
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                continue;
            }
        };
        let outcome = process(&pool, &providers, object_store.as_ref(), job.email_id)
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
                reset_for_retry(&pool, job.email_id, &reason).await?;
                message
                    .ack_with(AckKind::Nak(Some(Duration::from_secs(retry_delay(
                        delivered,
                    )))))
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            }
            Outcome::Deferred(reason) => {
                tracing::warn!(email_id = %job.email_id, error = %reason, "email delivery deferred by operator control");
                defer_for_operator(&pool, job.email_id, &reason).await?;
                message
                    .double_ack()
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            }
            Outcome::Retry(reason) | Outcome::Failed(reason) => {
                fail_email(&pool, job.email_id, &reason).await?;
                record_dead_letter(
                    &pool,
                    &context,
                    Some(job.email_id),
                    &message.payload,
                    reason,
                )
                .await?;
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
    providers: &super::provider::DeliveryProviders,
    object_store: Option<&storage::ObjectStore>,
    email_id: Uuid,
) -> Result<Outcome> {
    let attempt_id = Uuid::new_v4();
    let claimed = sqlx::query("UPDATE emails SET status = 'processing', processing_started_at = now(), processing_attempts = processing_attempts + 1, last_error = NULL WHERE id = $1 AND status = 'queued' RETURNING id, workspace_id, domain_id, environment, delivery_provider, sender, subject, text_body, html_body, raw_object_key, content_checksum, reply_to, processing_attempts")
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
            Some(row) if row.get::<String, _>("status") == "processing" => Outcome::AlreadyHandled,
            Some(_) => Outcome::AlreadyHandled,
        });
    };
    let attempt_number = row.get::<i32, _>("processing_attempts");
    let workspace_id = row.get::<Uuid, _>("workspace_id");
    let mut email = Email {
        id: row.get("id"),
        attempt_id,
        environment: row.get("environment"),
        delivery_provider: row.get("delivery_provider"),
        domain_id: row.get("domain_id"),
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
    let suppressed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM suppressions s JOIN emails e ON e.workspace_id=s.workspace_id JOIN email_recipients r ON r.email_id=e.id AND lower(r.address)=lower(s.address) WHERE e.id=$1)")
        .bind(email_id).fetch_one(pool).await?;
    if suppressed {
        return Ok(Outcome::Failed(
            "Recipient became suppressed before delivery".into(),
        ));
    }
    if email.environment == "test" {
        return simulate(pool, &email).await;
    }
    match prepare_provider_attempt(pool, providers, workspace_id, &mut email, attempt_number)
        .await?
    {
        AttemptPreparation::Ready => {}
        AttemptPreparation::Deferred => {
            return Ok(Outcome::Deferred(
                "SMTP delivery is paused; no provider attempt was started".into(),
            ))
        }
        AttemptPreparation::WorkspacePaused => {
            return Ok(Outcome::Deferred(
                "Workspace production sending is paused; no provider attempt was started".into(),
            ))
        }
        AttemptPreparation::DomainUnauthorized => {
            return Ok(Outcome::Failed(
                "Sender domain was disabled or lost verification before delivery".into(),
            ))
        }
    }
    let provider_id = match providers.submit(&email.delivery_provider, &email).await {
        Ok(value) => value,
        Err(ProviderFailure::Retryable(reason)) => {
            if let Err(error) =
                finish_provider_attempt(pool, email.attempt_id, "retryable", None, Some(&reason))
                    .await
            {
                tracing::error!(attempt_id = %email.attempt_id, error = %error, "unable to record retryable provider attempt");
            }
            return Ok(Outcome::Retry(reason));
        }
        Err(ProviderFailure::Permanent(reason)) => {
            if let Err(error) =
                finish_provider_attempt(pool, email.attempt_id, "failed", None, Some(&reason)).await
            {
                tracing::error!(attempt_id = %email.attempt_id, error = %error, "unable to record permanent provider attempt");
            }
            return Ok(Outcome::Failed(reason));
        }
        Err(ProviderFailure::Ambiguous(reason)) => {
            if let Err(error) =
                finish_provider_attempt(pool, email.attempt_id, "ambiguous", None, Some(&reason))
                    .await
            {
                tracing::error!(attempt_id = %email.attempt_id, error = %error, "unable to record ambiguous provider attempt");
            }
            return Ok(Outcome::Failed(format!(
                "Ambiguous provider result; manual review required: {reason}"
            )));
        }
    };
    if let Err(error) = finish_provider_attempt(
        pool,
        email.attempt_id,
        "submitted",
        Some(&provider_id),
        None,
    )
    .await
    {
        return Ok(Outcome::Failed(format!(
            "Provider accepted message; attempt recording failed; manual review required: {error}"
        )));
    }
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            return Ok(Outcome::Failed(format!(
                "Provider accepted message; state transaction failed; manual review required: {e}"
            )))
        }
    };
    let updated = match sqlx::query("UPDATE emails SET status = CASE WHEN status = 'processing' THEN 'sent' ELSE status END, provider_message_id = COALESCE(provider_message_id, $2), sent_at = COALESCE(sent_at, now()), processing_started_at = NULL WHERE id = $1 AND status IN ('processing', 'delivered', 'bounced', 'complained', 'failed')").bind(email.id).bind(&provider_id).execute(&mut *tx).await {
        Ok(value) => value,
        Err(error) => return Ok(Outcome::Failed(format!("provider accepted message but state recording failed; manual review required: {error}"))),
    };
    if updated.rows_affected() != 1 {
        return Ok(Outcome::Failed(
            "email state changed during provider send".into(),
        ));
    }
    if let Err(error) = sqlx::query(
        "UPDATE email_recipients SET status = 'sent' WHERE email_id = $1 AND status = 'pending'",
    )
    .bind(email.id)
    .execute(&mut *tx)
    .await
    {
        return Ok(Outcome::Failed(format!(
            "Provider accepted message; recipient state failed; manual review required: {error}"
        )));
    }
    if let Err(error) = tx.commit().await {
        return Ok(Outcome::Failed(format!(
            "Provider accepted message; commit uncertain; manual review required: {error}"
        )));
    }
    Ok(Outcome::Sent(provider_id))
}

async fn prepare_provider_attempt(
    pool: &db::DbPool,
    providers: &super::provider::DeliveryProviders,
    workspace_id: Uuid,
    email: &mut Email,
    attempt_number: i32,
) -> Result<AttemptPreparation> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('delivery-routing-control',0))")
        .execute(&mut *tx)
        .await?;
    let domain_authorized = match email.domain_id {
        Some(domain_id) => sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM domains WHERE id=$1 AND workspace_id=$2 AND status='verified' FOR SHARE",
        )
        .bind(domain_id)
        .bind(workspace_id)
        .fetch_optional(&mut *tx)
        .await?
        .is_some(),
        None => false,
    };
    if !domain_authorized {
        return Ok(AttemptPreparation::DomainUnauthorized);
    }
    let workspace_ready = sqlx::query_scalar::<_, bool>(
        "SELECT sending_paused_at IS NULL FROM workspaces WHERE id=$1 FOR SHARE",
    )
    .bind(workspace_id)
    .fetch_one(&mut *tx)
    .await?;
    if !workspace_ready {
        return Ok(AttemptPreparation::WorkspacePaused);
    }
    if email.delivery_provider == "smtp" {
        let controls = sqlx::query(
            "SELECT smtp_paused,ses_rollback_enabled FROM delivery_operator_controls WHERE singleton=true",
        )
        .fetch_one(&mut *tx)
        .await?;
        match provider_control(
            &email.delivery_provider,
            controls.get("smtp_paused"),
            controls.get("ses_rollback_enabled"),
            providers.is_available("ses"),
        ) {
            ProviderControl::RollbackToSes => {
                let changed = sqlx::query("UPDATE emails SET delivery_provider='ses' WHERE id=$1 AND workspace_id=$2 AND status='processing' AND NOT EXISTS(SELECT 1 FROM delivery_provider_attempts WHERE email_id=$1)")
                    .bind(email.id)
                    .bind(workspace_id)
                    .execute(&mut *tx)
                    .await?;
                if changed.rows_affected() != 1 {
                    return Ok(AttemptPreparation::Deferred);
                }
                email.delivery_provider = "ses".into();
            }
            ProviderControl::Defer => return Ok(AttemptPreparation::Deferred),
            ProviderControl::Continue => {}
        }
    }
    sqlx::query("INSERT INTO delivery_provider_attempts(id,email_id,provider,attempt_number) VALUES($1,$2,$3,$4)")
        .bind(email.attempt_id)
        .bind(email.id)
        .bind(&email.delivery_provider)
        .bind(attempt_number)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(AttemptPreparation::Ready)
}

fn provider_control(
    provider: &str,
    smtp_paused: bool,
    rollback_enabled: bool,
    ses_available: bool,
) -> ProviderControl {
    if provider != "smtp" || !smtp_paused {
        ProviderControl::Continue
    } else if rollback_enabled && ses_available {
        ProviderControl::RollbackToSes
    } else {
        ProviderControl::Defer
    }
}

async fn finish_provider_attempt(
    pool: &db::DbPool,
    attempt_id: Uuid,
    status: &str,
    provider_message_id: Option<&str>,
    error: Option<&str>,
) -> Result<()> {
    sqlx::query("UPDATE delivery_provider_attempts SET status=$2,provider_message_id=$3,error=$4,completed_at=now() WHERE id=$1 AND status='processing'")
        .bind(attempt_id)
        .bind(status)
        .bind(provider_message_id)
        .bind(error)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn send_ses(
    client: &aws_sdk_sesv2::Client,
    email: &Email,
    configuration_set: Option<&str>,
) -> Result<String, ProviderFailure> {
    if !email.attachments.is_empty() {
        return send_raw_ses(client, email, configuration_set).await;
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
        .set_configuration_set_name(configuration_set.map(str::to_owned))
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
    configuration_set: Option<&str>,
) -> Result<String, ProviderFailure> {
    let raw = build_raw_message(email, None)?;
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
        .set_configuration_set_name(configuration_set.map(str::to_owned))
        .destination(destination)
        .content(EmailContent::builder().raw(raw_message).build())
        .send()
        .await
        .map_err(classify_provider_error)?;
    response.message_id().map(str::to_owned).ok_or_else(|| {
        ProviderFailure::Ambiguous("SES raw response did not include a message ID".into())
    })
}

pub(crate) fn build_raw_message(
    email: &Email,
    message_id: Option<&str>,
) -> Result<Vec<u8>, ProviderFailure> {
    let mut builder = MessageBuilder::new().from(email.sender.as_str()).to(email
        .to
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>());
    if !email.cc.is_empty() {
        builder = builder.cc(email.cc.iter().map(String::as_str).collect::<Vec<_>>());
    }
    builder = builder.subject(email.subject.as_str());
    if let Some(message_id) = message_id {
        builder = builder.message_id(message_id);
    }
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
    builder
        .write_to_vec()
        .map_err(|error| ProviderFailure::Permanent(error.to_string()))
}

async fn reset_for_retry(pool: &db::DbPool, id: Uuid, reason: &str) -> Result<()> {
    sqlx::query("UPDATE emails SET status = 'queued', processing_started_at = NULL, last_error = $2 WHERE id = $1 AND status = 'processing'").bind(id).bind(reason).execute(pool).await?;
    Ok(())
}

async fn defer_for_operator(pool: &db::DbPool, id: Uuid, reason: &str) -> Result<()> {
    let mut tx = pool.begin().await?;
    let changed = sqlx::query("UPDATE emails SET status='queued',processing_started_at=NULL,processing_attempts=GREATEST(processing_attempts-1,0),last_error=$2 WHERE id=$1 AND status='processing' AND NOT EXISTS(SELECT 1 FROM delivery_provider_attempts WHERE email_id=$1)")
        .bind(id)
        .bind(reason)
        .execute(&mut *tx)
        .await?;
    if changed.rows_affected() == 1 {
        sqlx::query("INSERT INTO outbox_events(aggregate_type,aggregate_id,event_type,payload,available_at) VALUES('email',$1,'email.accepted',$2,now()+interval '5 minutes')")
            .bind(id)
            .bind(serde_json::json!({"emailId":id}))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}
async fn fail_email(pool: &db::DbPool, id: Uuid, reason: &str) -> Result<()> {
    sqlx::query("UPDATE emails SET status='failed',processing_started_at=NULL,completed_at=now(),last_error=$2 WHERE id=$1 AND status IN ('queued','processing')")
        .bind(id)
        .bind(reason)
        .execute(pool)
        .await?;
    Ok(())
}
async fn record_dead_letter(
    pool: &db::DbPool,
    context: &jetstream::Context,
    email_id: Option<Uuid>,
    payload: &[u8],
    reason: String,
) -> Result<()> {
    let envelope =
        serde_json::json!({"reason": reason, "payload": String::from_utf8_lossy(payload)});
    sqlx::query(
        "INSERT INTO delivery_dead_letters (email_id, reason, payload) VALUES ($1, $2, $3)",
    )
    .bind(email_id)
    .bind(envelope["reason"].as_str().unwrap_or("unknown failure"))
    .bind(envelope.clone())
    .execute(pool)
    .await?;
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
    Ok(())
}

pub(crate) fn classify_provider_error(
    error: aws_sdk_sesv2::error::SdkError<aws_sdk_sesv2::operation::send_email::SendEmailError>,
) -> ProviderFailure {
    use aws_sdk_sesv2::error::{ProvideErrorMetadata, SdkError};
    match error {
        SdkError::ServiceError(context) => {
            let provider_error = context.err();
            let code = provider_error
                .code()
                .unwrap_or("UnknownServiceError")
                .to_owned();
            let reason = provider_error
                .message()
                .map(str::trim)
                .filter(|message| !message.is_empty() && *message != code.as_str())
                .map(|message| format!("{code}: {message}"))
                .unwrap_or_else(|| code.clone());
            if matches!(
                code.as_str(),
                "TooManyRequestsException"
                    | "Throttling"
                    | "ThrottlingException"
                    | "LimitExceededException"
            ) {
                ProviderFailure::Retryable(reason)
            } else if context.raw().status().as_u16() >= 500 {
                ProviderFailure::Ambiguous(reason)
            } else {
                ProviderFailure::Permanent(reason)
            }
        }
        SdkError::ConstructionFailure(_) => {
            ProviderFailure::Permanent("Invalid provider request".into())
        }
        _ => ProviderFailure::Ambiguous("Provider transport result is uncertain".into()),
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

async fn simulate(pool: &db::DbPool, email: &Email) -> Result<Outcome> {
    use serde_json::json;
    let mut tx = pool.begin().await?;
    let row = sqlx::query("SELECT workspace_id,metadata FROM emails WHERE id=$1 FOR UPDATE")
        .bind(email.id)
        .fetch_one(&mut *tx)
        .await?;
    let workspace: Uuid = row.get("workspace_id");
    let metadata: serde_json::Value = row.get("metadata");
    let provider_id = format!("test_{}", email.id);
    let mut aggregate = "delivered";
    for address in email.to.iter().chain(&email.cc).chain(&email.bcc) {
        let (kind, status) = if address == "complaint@simulator.mailer.invalid" {
            ("complaint", "complained")
        } else if address == "bounce@simulator.mailer.invalid" {
            ("bounce", "bounced")
        } else {
            ("delivery", "delivered")
        };
        if status == "complained" || (status == "bounced" && aggregate != "complained") {
            aggregate = status;
        }
        let event_id = Uuid::new_v4();
        let payload = json!({"emailId":email.id,"environment":"test","metadata":metadata,"eventId":format!("test_{event_id}"),"messageId":provider_id,"eventType":kind,"occurredAt":chrono::Utc::now(),"recipients":[address],"bounceType":if kind=="bounce" {Some("Permanent")} else {None},"details":{"simulated":true}});
        sqlx::query("UPDATE email_recipients SET status=$2 WHERE email_id=$1 AND address=$3")
            .bind(email.id)
            .bind(status)
            .bind(address)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO delivery_events(id,email_id,provider_event_id,event_type,recipient,payload,occurred_at) VALUES($1,$2,$3,$4,$5,$6,now())").bind(event_id).bind(email.id).bind(format!("test_{event_id}")).bind(kind).bind(address).bind(payload).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO outbox_events(aggregate_type,aggregate_id,event_type,payload) VALUES('delivery_event',$1,$2,$3)").bind(event_id).bind(format!("email.{kind}")).bind(json!({"emailId":email.id})).execute(&mut *tx).await?;
        if status != "delivered" {
            sqlx::query("INSERT INTO suppressions(workspace_id,address,reason,source_email_id) VALUES($1,$2,$3,$4) ON CONFLICT(workspace_id,lower(address)) DO NOTHING").bind(workspace).bind(address).bind(status).bind(email.id).execute(&mut *tx).await?;
        }
    }
    sqlx::query("UPDATE emails SET status=$2,provider_message_id=$3,sent_at=now(),completed_at=now(),processing_started_at=NULL WHERE id=$1").bind(email.id).bind(aggregate).bind(&provider_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Outcome::Sent(provider_id))
}

#[cfg(test)]
mod tests {
    use super::{build_raw_message, provider_control, Attachment, Email, ProviderControl};
    use uuid::Uuid;

    #[test]
    fn applies_pause_only_before_a_provider_attempt() {
        assert_eq!(
            provider_control("smtp", true, true, true),
            ProviderControl::RollbackToSes
        );
        assert_eq!(
            provider_control("smtp", true, false, true),
            ProviderControl::Defer
        );
        assert_eq!(
            provider_control("smtp", false, true, true),
            ProviderControl::Continue
        );
        assert_eq!(
            provider_control("ses", true, true, true),
            ProviderControl::Continue
        );
    }

    #[test]
    fn shared_mime_contains_correlation_and_never_exposes_bcc() {
        let email = Email {
            id: Uuid::new_v4(),
            attempt_id: Uuid::new_v4(),
            environment: "production".into(),
            delivery_provider: "smtp".into(),
            domain_id: Some(Uuid::new_v4()),
            sender: "Crescent Mail <sender@example.com>".into(),
            subject: "Provider adapter test".into(),
            text: Some("plain body".into()),
            html: Some("<p>html body</p>".into()),
            raw_object_key: None,
            content_checksum: None,
            reply_to: Some("reply@example.com".into()),
            to: vec!["to@example.com".into()],
            cc: vec!["cc@example.com".into()],
            bcc: vec!["secret@example.com".into()],
            attachments: vec![Attachment {
                filename: "test.txt".into(),
                content_type: "text/plain".into(),
                content: b"attachment".to_vec(),
                disposition: None,
                content_id: None,
            }],
        };
        let message_id = format!("{}.{}@smtp.example.com", email.id, email.attempt_id);
        let raw = build_raw_message(&email, Some(&message_id)).expect("MIME must build");
        let raw = String::from_utf8(raw).expect("MIME headers are UTF-8");

        assert!(raw.contains(&message_id));
        assert!(raw.contains("to@example.com"));
        assert!(raw.contains("cc@example.com"));
        assert!(!raw.contains("secret@example.com"));
        assert!(!raw.to_ascii_lowercase().contains("\nbcc:"));
        assert!(raw.contains("test.txt"));
    }
}
