use super::{api_keys, AppState};
use axum::{
    extract::{ConnectInfo, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::Timelike;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::net::{IpAddr, SocketAddr};
use uuid::Uuid;

const MAX_RECIPIENTS: usize = 50;
const MAX_SUBJECT_BYTES: usize = 998;
const MAX_BODY_BYTES: usize = 1_000_000;
const MAX_ATTACHMENTS: usize = 10;
const MAX_ATTACHMENT_BYTES: usize = 10_000_000;
const MAX_TOTAL_ATTACHMENT_BYTES: usize = 20_000_000;

#[derive(Deserialize, Serialize)]
struct SendEmailRequest {
    from: String,
    to: Vec<String>,
    cc: Option<Vec<String>>,
    bcc: Option<Vec<String>>,
    subject: String,
    text: Option<String>,
    html: Option<String>,
    reply_to: Option<String>,
    headers: Option<serde_json::Value>,
    tags: Option<serde_json::Value>,
    metadata: Option<serde_json::Value>,
    environment: Option<String>,
    attachments: Option<Vec<AttachmentInput>>,
}

#[derive(Deserialize, Serialize, Clone)]
struct AttachmentInput {
    filename: String,
    content: String,
    content_type: Option<String>,
    content_disposition: Option<String>,
    content_id: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/emails", post(send_email))
}

async fn send_email(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(mut input): Json<SendEmailRequest>,
) -> Response {
    let (api_key_id, workspace_id, key_environment) = if headers.contains_key(header::AUTHORIZATION)
    {
        let raw_key = bearer_token(&headers).unwrap_or_default();
        match api_keys::verify(&raw_key, &state.db, "emails:send").await {
            Ok((id, workspace, environment)) => (Some(id), workspace, environment),
            Err(_) => {
                return error(
                    StatusCode::UNAUTHORIZED,
                    "invalid_api_key",
                    "Valid sending key required",
                )
            }
        }
    } else {
        match api_keys::access(&state, &headers, "emails:send", true).await {
            Ok((workspace, _)) => (
                None,
                workspace,
                input.environment.clone().unwrap_or_else(|| "test".into()),
            ),
            Err(response) => return response,
        }
    };
    let environment = input
        .environment
        .clone()
        .unwrap_or_else(|| key_environment.clone());
    if environment != key_environment || !matches!(environment.as_str(), "test" | "production") {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_environment",
            "The API key and request environment must match",
        );
    }
    if environment == "production" && !api_keys::production_enabled(&state, workspace_id).await {
        return error(
            StatusCode::FORBIDDEN,
            "production_access_required",
            "Production sending requires a verified sending domain",
        );
    }
    let Some(idempotency_key) = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
    else {
        return error(
            StatusCode::BAD_REQUEST,
            "missing_idempotency_key",
            "Idempotency-Key is required",
        );
    };
    if idempotency_key.len() > 255 {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_idempotency_key",
            "Idempotency-Key is too long",
        );
    }
    let Some(from_domain) = sender_domain(&input.from) else {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_sender",
            "from must be a valid email address",
        );
    };
    if input.subject.trim().is_empty() || input.subject.len() > MAX_SUBJECT_BYTES {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_subject",
            "subject is required and too long",
        );
    }
    if input.text.as_deref().unwrap_or_default().is_empty()
        && input.html.as_deref().unwrap_or_default().is_empty()
    {
        return error(
            StatusCode::BAD_REQUEST,
            "missing_content",
            "Provide text or html content",
        );
    }
    if input.text.as_deref().unwrap_or_default().len()
        + input.html.as_deref().unwrap_or_default().len()
        > MAX_BODY_BYTES
    {
        return error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "content_too_large",
            "Email content exceeds the size limit",
        );
    }
    let attachments = match validate_attachments(input.attachments.clone()) {
        Ok(value) => value,
        Err(response) => return *response,
    };
    if input
        .reply_to
        .as_deref()
        .is_some_and(|value| mailbox_address(value).is_none())
    {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_reply_to",
            "reply_to must be a valid email address",
        );
    }
    if input
        .headers
        .as_ref()
        .is_some_and(|value| !value.is_object())
        || input
            .metadata
            .as_ref()
            .is_some_and(|value| !value.is_object())
        || input.tags.as_ref().is_some_and(|value| !value.is_array())
    {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_properties",
            "headers and metadata must be objects, and tags must be an array",
        );
    }
    let mut recipients = Vec::new();
    let mut unique_recipients = std::collections::HashSet::new();
    for (kind, values) in [
        ("to", input.to.clone()),
        ("cc", input.cc.clone().unwrap_or_default()),
        ("bcc", input.bcc.clone().unwrap_or_default()),
    ] {
        for address in values {
            let Some(address) = mailbox_address(&address).map(str::to_lowercase) else {
                return error(
                    StatusCode::BAD_REQUEST,
                    "invalid_recipient",
                    "Every recipient must be a valid email address",
                );
            };
            if unique_recipients.insert((kind, address.clone())) {
                recipients.push((kind, address));
            }
        }
    }
    if recipients.is_empty() || recipients.len() > MAX_RECIPIENTS {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_recipients",
            "Provide between 1 and 50 recipients",
        );
    }
    // Canonicalize omitted/explicit environment before hashing.
    input.environment = Some(environment.clone());
    if input
        .headers
        .as_ref()
        .is_some_and(|v| v.as_object().is_some_and(|o| !o.is_empty()))
        || input
            .tags
            .as_ref()
            .is_some_and(|v| v.as_array().is_some_and(|a| !a.is_empty()))
    {
        return error(StatusCode::BAD_REQUEST, "unsupported_fields", "Custom headers and tags are not supported in this release; use metadata for correlation");
    }
    if state.object_store.is_none() && !attachments.is_empty() {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "storage_required",
            "Attachments require configured object storage",
        );
    }
    let request_hash = request_hash_legacy(&input);
    let email_id = Uuid::new_v4();
    let object_key = format!("workspaces/{workspace_id}/emails/{email_id}/content.json");
    let mut tx = match state.db.begin().await {
        Ok(value) => value,
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to accept email",
            )
        }
    };
    let bucket = chrono::Utc::now()
        .with_second(0)
        .and_then(|value| value.with_nanosecond(0))
        .unwrap_or_else(chrono::Utc::now);
    let limits = match sqlx::query("SELECT monthly_email_limit, concurrent_email_limit, api_key_rate_limit_per_minute FROM workspace_limits WHERE workspace_id = $1")
        .bind(workspace_id).fetch_optional(&mut *tx).await {
        Ok(value) => value,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to resolve workspace limits"),
    };
    let key_rate_limit = limits
        .as_ref()
        .and_then(|row| row.get::<Option<i32>, _>("api_key_rate_limit_per_minute"))
        .map(i64::from)
        .unwrap_or(i64::from(state.api_key_rate_limit_per_minute));
    let monthly_limit = limits
        .as_ref()
        .and_then(|row| row.get::<Option<i64>, _>("monthly_email_limit"))
        .unwrap_or(state.workspace_monthly_email_limit as i64);
    let concurrent_limit = limits
        .as_ref()
        .and_then(|row| row.get::<Option<i32>, _>("concurrent_email_limit"))
        .map(i64::from)
        .unwrap_or(i64::from(state.workspace_concurrent_email_limit));
    let ip = client_ip(peer.ip(), &headers, state.trust_proxy_headers);
    let ip_rate = match sqlx::query_scalar::<_, i32>("INSERT INTO client_ip_rate_limits (client_ip, bucket_start, request_count) VALUES ($1, $2, 1) ON CONFLICT (client_ip, bucket_start) DO UPDATE SET request_count = client_ip_rate_limits.request_count + 1 RETURNING request_count")
        .bind(ip.to_string()).bind(bucket).fetch_one(&mut *tx).await {
        Ok(value) => value,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to enforce client limit"),
    };
    if ip_rate > state.client_ip_rate_limit_per_minute as i32 {
        return error(
            StatusCode::TOO_MANY_REQUESTS,
            "client_rate_limited",
            "Client request rate exceeded",
        );
    }
    if let Some(api_key_id) = api_key_id {
        let rate = match sqlx::query_scalar::<_, i32>("INSERT INTO api_key_rate_limits (api_key_id, bucket_start, request_count) VALUES ($1, $2, 1) ON CONFLICT (api_key_id, bucket_start) DO UPDATE SET request_count = api_key_rate_limits.request_count + 1 RETURNING request_count")
        .bind(api_key_id).bind(bucket).fetch_one(&mut *tx).await {
        Ok(value) => value,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to enforce request limit"),
    };
        if i64::from(rate) > key_rate_limit {
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "API key request rate exceeded",
            );
        }
    }
    if sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("{workspace_id}:{environment}:{idempotency_key}"))
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Unable to reserve idempotency key",
        );
    }
    match sqlx::query("SELECT request_hash, response FROM idempotency_keys WHERE workspace_id = $1 AND key = $2 AND environment = $3 FOR UPDATE").bind(workspace_id).bind(idempotency_key).bind(&environment).fetch_optional(&mut *tx).await {
        Ok(Some(row)) => {
            let previous_hash: Vec<u8> = row.get("request_hash");
            let legacy_hash = { let saved = input.environment.take(); let hash = request_hash_legacy(&input); input.environment = saved; hash };
            if previous_hash != request_hash && previous_hash != legacy_hash { return error(StatusCode::CONFLICT, "idempotency_conflict", "This idempotency key was used with a different request"); }
            if let Some(response) = row.get::<Option<serde_json::Value>, _>("response") { return (StatusCode::OK, Json(response)).into_response(); }
        },
        Ok(None) => {},
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to check idempotency key"),
    }
    let domain_id = if environment == "test" && from_domain == "sandbox.mailer.invalid" {
        None
    } else {
        Some(match sqlx::query_scalar::<_, Uuid>("SELECT id FROM domains WHERE workspace_id = $1 AND lower(name) = $2 AND status = 'verified'").bind(workspace_id).bind(&from_domain).fetch_optional(&mut *tx).await { Ok(Some(value)) => value, Ok(None) => return error(StatusCode::BAD_REQUEST, "sender_domain_unverified", "The sender domain is not verified for this workspace"), Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to validate sender domain") })
    };
    let suppressed = match sqlx::query_scalar::<_, String>(
        "SELECT address FROM suppressions WHERE workspace_id = $1 AND lower(address) = ANY($2)",
    )
    .bind(workspace_id)
    .bind(
        recipients
            .iter()
            .map(|(_, address)| address.clone())
            .collect::<Vec<_>>(),
    )
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(value) => value,
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to check suppressions",
            )
        }
    };
    if suppressed.is_some() {
        return error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "recipient_suppressed",
            "One or more recipients are suppressed",
        );
    }
    if sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("workspace-admission:{workspace_id}"))
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Unable to reserve workspace capacity",
        );
    }
    let accepted = match sqlx::query_scalar::<_, i64>("INSERT INTO usage_counters (workspace_id, period_start, emails_accepted) VALUES ($1, date_trunc('month', now())::date, 1) ON CONFLICT (workspace_id, period_start) DO UPDATE SET emails_accepted = usage_counters.emails_accepted + 1 RETURNING emails_accepted")
        .bind(workspace_id).fetch_one(&mut *tx).await {
        Ok(value) => value,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to enforce usage limit"),
    };
    if accepted > monthly_limit {
        return error(
            StatusCode::TOO_MANY_REQUESTS,
            "monthly_limit_reached",
            "Workspace monthly email limit reached",
        );
    }
    let active: i64 = match sqlx::query_scalar("SELECT count(*) FROM emails WHERE workspace_id = $1 AND status IN ('queued', 'processing')")
        .bind(workspace_id).fetch_one(&mut *tx).await {
        Ok(value) => value,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to enforce concurrency limit"),
    };
    if active >= concurrent_limit {
        return error(
            StatusCode::TOO_MANY_REQUESTS,
            "concurrency_limit_reached",
            "Workspace concurrent email limit reached",
        );
    }
    if sqlx::query("INSERT INTO emails (id, workspace_id, domain_id, api_key_id, idempotency_key, environment, sender, subject, text_body, html_body, reply_to, headers, tags, metadata, status) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, 'queued')")
        .bind(email_id).bind(workspace_id).bind(domain_id).bind(api_key_id).bind(idempotency_key).bind(&environment).bind(input.from.trim()).bind(input.subject.trim()).bind(input.text.clone()).bind(input.html.clone()).bind(input.reply_to.clone()).bind(input.headers.clone().unwrap_or_else(|| json!({}))).bind(input.tags.clone().unwrap_or_else(|| json!([]))).bind(input.metadata.clone().unwrap_or_else(|| json!({}))).execute(&mut *tx).await.is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to store email");
    }
    for (kind, address) in recipients {
        if sqlx::query(
            "INSERT INTO email_recipients (email_id, address, recipient_type) VALUES ($1, $2, $3)",
        )
        .bind(email_id)
        .bind(address)
        .bind(kind)
        .execute(&mut *tx)
        .await
        .is_err()
        {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to store recipients",
            );
        }
    }
    let response = json!({"data": {"id": email_id, "status": "queued"}});
    if sqlx::query("INSERT INTO idempotency_keys (workspace_id, key, request_hash, response, environment) VALUES ($1, $2, $3, $4, $5)").bind(workspace_id).bind(idempotency_key).bind(request_hash).bind(response.clone()).bind(&environment).execute(&mut *tx).await.is_err() { return error(StatusCode::CONFLICT, "idempotency_conflict", "This idempotency key is already in use"); }
    if sqlx::query("INSERT INTO outbox_events (aggregate_type, aggregate_id, event_type, payload) VALUES ('email', $1, 'email.accepted', $2)").bind(email_id).bind(json!({"emailId": email_id, "workspaceId": workspace_id})).execute(&mut *tx).await.is_err() { return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to queue email"); }
    if let Some(store) = &state.object_store {
        let content = serde_json::to_vec(
            &json!({"text": input.text, "html": input.html, "attachments": attachments}),
        )
        .expect("email content is serializable");
        let checksum = match store.put(&object_key, content).await {
            Ok(value) => value,
            Err(error_value) => {
                tracing::error!(error = %error_value, email_id = %email_id, "failed to store email content object");
                return error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "storage_unavailable",
                    "Unable to store email content",
                );
            }
        };
        if sqlx::query("UPDATE emails SET text_body = NULL, html_body = NULL, raw_object_key = $2, content_checksum = $3 WHERE id = $1")
            .bind(email_id).bind(&object_key).bind(checksum).execute(&mut *tx).await.is_err() {
            let _ = store.delete(&object_key).await;
            return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to record email storage");
        }
    }
    if tx.commit().await.is_err() {
        // Commit may have succeeded despite a lost reply. Never delete its content here.
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Unable to accept email",
        );
    }
    (StatusCode::ACCEPTED, Json(response)).into_response()
}

fn request_hash_legacy(input: &SendEmailRequest) -> Vec<u8> {
    let encoded = serde_json::to_vec(input).unwrap_or_default();
    Sha256::digest(encoded).to_vec()
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}
fn sender_domain(value: &str) -> Option<String> {
    let address = mailbox_address(value)?;
    let (_, domain) = address.rsplit_once('@')?;
    Some(domain.to_ascii_lowercase())
}
fn mailbox_address(value: &str) -> Option<&str> {
    let value = value.trim();
    let address = if value.ends_with('>') {
        let start = value.rfind('<')?;
        &value[start + 1..value.len() - 1]
    } else {
        value
    };
    if valid_email(address) {
        Some(address)
    } else {
        None
    }
}
fn valid_email(value: &str) -> bool {
    value.len() <= 254
        && !value.chars().any(char::is_whitespace)
        && value.rsplit_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && domain.contains('.')
                && domain.split('.').all(|label| {
                    !label.is_empty()
                        && !label.starts_with('-')
                        && !label.ends_with('-')
                        && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                })
        })
}

pub(crate) fn client_ip(peer: IpAddr, headers: &HeaderMap, trust_proxy: bool) -> IpAddr {
    if trust_proxy && peer.is_loopback() {
        if let Some(value) = headers
            .get("x-real-ip")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok())
        {
            return value;
        }
    }
    peer
}
fn validate_attachments(
    values: Option<Vec<AttachmentInput>>,
) -> Result<Vec<AttachmentInput>, Box<Response>> {
    let values = values.unwrap_or_default();
    if values.len() > MAX_ATTACHMENTS {
        return Err(Box::new(error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_many_attachments",
            "A maximum of 10 attachments is allowed",
        )));
    }
    let mut total = 0_usize;
    for attachment in &values {
        if attachment.filename.trim().is_empty()
            || attachment.filename.len() > 255
            || attachment.filename.contains(['\r', '\n', '/', '\\'])
            || attachment
                .content_type
                .as_deref()
                .is_some_and(|value| value.len() > 127 || value.contains(['\r', '\n']))
            || attachment
                .content_id
                .as_deref()
                .is_some_and(|value| value.len() > 255 || value.contains(['\r', '\n']))
            || attachment
                .content_disposition
                .as_deref()
                .is_some_and(|value| !matches!(value, "attachment" | "inline"))
        {
            return Err(Box::new(error(
                StatusCode::BAD_REQUEST,
                "invalid_attachment",
                "Attachment metadata is invalid",
            )));
        }
        let decoded = STANDARD.decode(&attachment.content).map_err(|_| {
            Box::new(error(
                StatusCode::BAD_REQUEST,
                "invalid_attachment",
                "Attachment content must be valid base64",
            ))
        })?;
        if decoded.len() > MAX_ATTACHMENT_BYTES {
            return Err(Box::new(error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "attachment_too_large",
                "An attachment exceeds 10 MB",
            )));
        }
        total = total.saturating_add(decoded.len());
    }
    if total > MAX_TOTAL_ATTACHMENT_BYTES {
        return Err(Box::new(error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "attachments_too_large",
            "Attachments exceed the 20 MB total limit",
        )));
    }
    Ok(values)
}
fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"code": code, "message": message}))).into_response()
}

#[cfg(test)]
mod tests {
    use super::{
        mailbox_address, sender_domain, valid_email, validate_attachments, AttachmentInput,
    };

    #[test]
    fn parses_display_name_sender() {
        assert_eq!(
            sender_domain("CrescentSphere <hello@mailer.example.com>"),
            Some("mailer.example.com".into())
        );
    }

    #[test]
    fn validates_mailboxes() {
        assert_eq!(
            mailbox_address("Person <person@example.com>"),
            Some("person@example.com")
        );
        assert!(valid_email("person@example.com"));
        assert!(!valid_email("person@localhost"));
        assert!(!valid_email("person @example.com"));
    }

    #[test]
    fn validates_attachment_content_and_metadata() {
        let valid = AttachmentInput {
            filename: "report.pdf".into(),
            content: "aGVsbG8=".into(),
            content_type: Some("application/pdf".into()),
            content_disposition: Some("attachment".into()),
            content_id: None,
        };
        assert!(validate_attachments(Some(vec![valid])).is_ok());
        let invalid = AttachmentInput {
            filename: "../secret".into(),
            content: "%%%".into(),
            content_type: None,
            content_disposition: None,
            content_id: None,
        };
        assert!(validate_attachments(Some(vec![invalid])).is_err());
    }
}
