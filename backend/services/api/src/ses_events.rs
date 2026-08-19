use super::AppState;
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SesEvent {
    event_id: String,
    message_id: String,
    event_type: String,
    occurred_at: DateTime<Utc>,
    #[serde(default)]
    recipients: Vec<String>,
    bounce_type: Option<String>,
    #[serde(default)]
    details: serde_json::Value,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/internal/v1/ses/events", post(ingest))
}

async fn ingest(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(event): Json<SesEvent>,
) -> Response {
    if !authorized(&headers, &state.event_ingest_token) {
        return error(
            StatusCode::UNAUTHORIZED,
            "invalid_event_token",
            "Event authentication failed",
        );
    }
    if event.event_id.trim().is_empty()
        || event.message_id.trim().is_empty()
        || !matches!(
            event.event_type.as_str(),
            "delivery" | "bounce" | "complaint" | "reject" | "rendering_failure" | "open" | "click"
        )
    {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_event",
            "The delivery event is invalid",
        );
    }
    let recipients = match normalized_recipients(&event.recipients) {
        Some(recipients) => recipients,
        None => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_event",
                "Event recipients must be non-empty email addresses",
            )
        }
    };
    let mut tx = match state.db.begin().await {
        Ok(value) => value,
        Err(error_value) => {
            tracing::error!(error = %error_value, "failed to begin SES event transaction");
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to ingest event",
            );
        }
    };
    let email = match sqlx::query(
        "SELECT id, workspace_id FROM emails WHERE provider_message_id = $1 FOR UPDATE",
    )
    .bind(event.message_id.trim())
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(Some(value)) => value,
        Ok(None) => {
            return error(
                StatusCode::NOT_FOUND,
                "email_not_found",
                "No email matches this provider message ID",
            )
        }
        Err(error_value) => {
            tracing::error!(error = %error_value, provider_message_id = %event.message_id, "failed to resolve SES email");
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to resolve email",
            );
        }
    };
    let email_id: Uuid = email.get("id");
    let workspace_id: Uuid = email.get("workspace_id");
    let payload = serde_json::to_value(&event).unwrap_or_else(|_| json!({}));
    let mut inserted = 0_u64;
    for recipient in recipients {
        let provider_event_id = recipient.as_ref().map_or_else(
            || event.event_id.clone(),
            |address| format!("{}:{}", event.event_id, address),
        );
        let delivery_event_id = match sqlx::query_scalar::<_, Uuid>("INSERT INTO delivery_events (email_id, provider_event_id, event_type, recipient, payload, occurred_at) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (provider_event_id) DO NOTHING RETURNING id")
            .bind(email_id).bind(&provider_event_id).bind(&event.event_type).bind(recipient.clone()).bind(payload.clone()).bind(event.occurred_at).fetch_optional(&mut *tx).await {
                Ok(Some(value)) => value,
                Ok(None) => continue,
                Err(error_value) => {
                    tracing::error!(error = %error_value, provider_event_id = %provider_event_id, "failed to store SES delivery event");
                    return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to store delivery event");
                }
            };
        inserted += 1;
        if let Some(address) = recipient.as_deref() {
            let recipient_status = match event.event_type.as_str() {
                "delivery" => Some("delivered"),
                "bounce" => Some("bounced"),
                "complaint" => Some("complained"),
                "reject" | "rendering_failure" => Some("failed"),
                _ => None,
            };
            if let Some(status) = recipient_status {
                let update = recipient_status_update(status);
                if let Err(error_value) = sqlx::query(update)
                    .bind(email_id)
                    .bind(address)
                    .execute(&mut *tx)
                    .await
                {
                    tracing::error!(error = %error_value, email_id = %email_id, recipient = %address, "failed to update recipient status");
                    return error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal_error",
                        "Unable to update recipient state",
                    );
                }
            }
            if should_suppress(&event) {
                let reason = if event.event_type == "complaint" {
                    "complained"
                } else {
                    "bounced"
                };
                if let Err(error_value) = sqlx::query("INSERT INTO suppressions (workspace_id, address, reason, source_email_id) VALUES ($1, $2, $3, $4) ON CONFLICT (workspace_id, lower(address)) DO UPDATE SET reason = EXCLUDED.reason, source_email_id = EXCLUDED.source_email_id")
                    .bind(workspace_id)
                    .bind(address)
                    .bind(reason)
                    .bind(email_id)
                    .execute(&mut *tx)
                    .await
                {
                    tracing::error!(error = %error_value, email_id = %email_id, recipient = %address, "failed to create recipient suppression");
                    return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to suppress recipient");
                }
            }
        }
        if let Err(error_value) = sqlx::query("INSERT INTO outbox_events (aggregate_type, aggregate_id, event_type, payload) VALUES ('delivery_event', $1, $2, $3)").bind(delivery_event_id).bind(format!("email.{}", event.event_type)).bind(json!({"deliveryEventId": delivery_event_id, "emailId": email_id, "workspaceId": workspace_id})).execute(&mut *tx).await {
            tracing::error!(error = %error_value, email_id = %email_id, "failed to queue delivery webhook event");
            return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to queue delivery event");
        }
    }
    if inserted > 0 {
        let status = aggregate_status(&event.event_type);
        if let Some(status) = status {
            let update = match status {
                "complained" => "UPDATE emails SET status = 'complained', completed_at = now() WHERE id = $1 AND status <> 'complained'",
                "bounced" => "UPDATE emails SET status = 'bounced', completed_at = now() WHERE id = $1 AND status NOT IN ('complained', 'bounced')",
                "delivered" => "UPDATE emails SET status = 'delivered', completed_at = now() WHERE id = $1 AND status IN ('sent', 'processing', 'queued')",
                "failed" => "UPDATE emails SET status = 'failed', completed_at = now() WHERE id = $1 AND status IN ('sent', 'processing', 'queued')",
                _ => unreachable!(),
            };
            let changed = match sqlx::query(update).bind(email_id).execute(&mut *tx).await {
                Ok(value) => value.rows_affected(),
                Err(error_value) => {
                    tracing::error!(error = %error_value, email_id = %email_id, "failed to update email delivery state");
                    return error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal_error",
                        "Unable to update email state",
                    );
                }
            };
            if status == "delivered" && changed == 1 {
                if let Err(error_value) = sqlx::query("INSERT INTO usage_counters (workspace_id, period_start, emails_delivered) VALUES ($1, date_trunc('month', now())::date, 1) ON CONFLICT (workspace_id, period_start) DO UPDATE SET emails_delivered = usage_counters.emails_delivered + 1")
                    .bind(workspace_id)
                    .execute(&mut *tx)
                    .await
                {
                    tracing::error!(error = %error_value, email_id = %email_id, "failed to increment delivered usage");
                    return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to record delivered usage");
                }
            }
        }
    }
    if let Err(error_value) = tx.commit().await {
        tracing::error!(error = %error_value, email_id = %email_id, "failed to commit SES event transaction");
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Unable to commit delivery event",
        );
    }
    Json(json!({"data": {"accepted": true, "newEvents": inserted}})).into_response()
}

fn aggregate_status(event_type: &str) -> Option<&'static str> {
    match event_type {
        "delivery" => Some("delivered"),
        "bounce" => Some("bounced"),
        "complaint" => Some("complained"),
        "reject" | "rendering_failure" => Some("failed"),
        _ => None,
    }
}
fn recipient_status_update(status: &str) -> &'static str {
    match status {
        "complained" => "UPDATE email_recipients SET status = 'complained' WHERE email_id = $1 AND lower(address) = $2 AND status <> 'complained'",
        "bounced" => "UPDATE email_recipients SET status = 'bounced' WHERE email_id = $1 AND lower(address) = $2 AND status NOT IN ('complained', 'bounced')",
        "delivered" => "UPDATE email_recipients SET status = 'delivered' WHERE email_id = $1 AND lower(address) = $2 AND status IN ('pending', 'sent')",
        "failed" => "UPDATE email_recipients SET status = 'failed' WHERE email_id = $1 AND lower(address) = $2 AND status IN ('pending', 'sent')",
        _ => unreachable!(),
    }
}
fn normalized_recipients(recipients: &[String]) -> Option<Vec<Option<String>>> {
    if recipients.is_empty() {
        return Some(vec![None]);
    }
    let mut normalized = Vec::with_capacity(recipients.len());
    for recipient in recipients {
        let recipient = recipient.trim().to_lowercase();
        if recipient.is_empty() || !recipient.contains('@') {
            return None;
        }
        if !normalized.contains(&Some(recipient.clone())) {
            normalized.push(Some(recipient));
        }
    }
    Some(normalized)
}
fn should_suppress(event: &SesEvent) -> bool {
    event.event_type == "complaint"
        || (event.event_type == "bounce"
            && event.bounce_type.as_deref().is_some_and(|value| {
                matches!(value.to_ascii_lowercase().as_str(), "permanent" | "hard")
            }))
}
fn authorized(headers: &HeaderMap, expected: &str) -> bool {
    let Some(actual) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return false;
    };
    constant_time_eq(actual.trim().as_bytes(), expected.as_bytes())
}

fn constant_time_eq(actual: &[u8], expected: &[u8]) -> bool {
    let actual_hash = Sha256::digest(actual);
    let expected_hash = Sha256::digest(expected);
    let mut difference = 0_u8;
    for (left, right) in actual_hash.iter().zip(expected_hash.iter()) {
        difference |= left ^ right;
    }
    difference == 0
}
fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"code": code, "message": message}))).into_response()
}

#[cfg(test)]
mod tests {
    use super::{aggregate_status, normalized_recipients, should_suppress, SesEvent};
    use chrono::Utc;
    use serde_json::json;

    fn event(event_type: &str, bounce_type: Option<&str>) -> SesEvent {
        SesEvent {
            event_id: "evt_1".into(),
            message_id: "msg_1".into(),
            event_type: event_type.into(),
            occurred_at: Utc::now(),
            recipients: vec!["user@example.com".into()],
            bounce_type: bounce_type.map(str::to_owned),
            details: json!({}),
        }
    }

    #[test]
    fn only_permanent_bounces_and_complaints_suppress() {
        assert!(should_suppress(&event("bounce", Some("Permanent"))));
        assert!(!should_suppress(&event("bounce", Some("Transient"))));
        assert!(should_suppress(&event("complaint", None)));
    }

    #[test]
    fn engagement_events_do_not_change_delivery_state() {
        assert_eq!(aggregate_status("open"), None);
        assert_eq!(aggregate_status("click"), None);
    }

    #[test]
    fn recipients_are_normalized_and_deduplicated() {
        let values = vec![" User@Example.com ".into(), "user@example.com".into()];
        assert_eq!(
            normalized_recipients(&values),
            Some(vec![Some("user@example.com".into())])
        );
    }

    #[test]
    fn invalid_recipients_are_rejected() {
        assert!(normalized_recipients(&["not-an-address".into()]).is_none());
    }
}
