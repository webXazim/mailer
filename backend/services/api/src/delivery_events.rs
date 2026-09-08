use super::AppState;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeliveryEvent {
    pub(crate) event_id: String,
    pub(crate) message_id: String,
    pub(crate) event_type: String,
    pub(crate) occurred_at: DateTime<Utc>,
    #[serde(default)]
    pub(crate) recipients: Vec<String>,
    pub(crate) bounce_type: Option<String>,
    #[serde(default)]
    pub(crate) details: serde_json::Value,
}

pub(crate) async fn ingest_event(
    state: &AppState,
    event: DeliveryEvent,
    provider: &str,
    correlation: Option<(Uuid, Option<Uuid>)>,
) -> Response {
    if event.event_id.trim().is_empty()
        || event.message_id.trim().is_empty()
        || !matches!(
            event.event_type.as_str(),
            "queued"
                | "deferred"
                | "delivery"
                | "bounce"
                | "complaint"
                | "reject"
                | "rendering_failure"
                | "open"
                | "click"
        )
    {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_event",
            "The delivery event is invalid",
        );
    }
    let has_recipient = !event.recipients.is_empty();
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
            tracing::error!(error = %error_value, provider, "failed to begin delivery event transaction");
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to ingest event",
            );
        }
    };
    let email_result = match correlation {
        Some((email_id, Some(attempt_id))) => sqlx::query(
            "SELECT email.id, email.workspace_id FROM emails AS email JOIN delivery_provider_attempts AS attempt ON attempt.email_id=email.id WHERE email.id=$1 AND attempt.id=$2 AND email.delivery_provider=$3 FOR UPDATE OF email",
        )
        .bind(email_id)
        .bind(attempt_id)
        .bind(provider)
        .fetch_optional(&mut *tx)
        .await,
        Some((email_id, None)) => sqlx::query(
            "SELECT email.id, email.workspace_id FROM emails AS email JOIN delivery_provider_attempts AS attempt ON attempt.email_id=email.id WHERE email.id=$1 AND email.delivery_provider=$2 AND attempt.provider=$2 AND attempt.status='submitted' ORDER BY attempt.started_at DESC LIMIT 1 FOR UPDATE OF email",
        )
        .bind(email_id)
        .bind(provider)
        .fetch_optional(&mut *tx)
        .await,
        None => sqlx::query(
            "SELECT id, workspace_id FROM emails WHERE provider_message_id = $1 AND delivery_provider=$2 FOR UPDATE",
        )
        .bind(event.message_id.trim())
        .bind(provider)
        .fetch_optional(&mut *tx)
        .await,
    };
    let email = match email_result {
        Ok(Some(value)) => value,
        Ok(None) => {
            return error(
                StatusCode::NOT_FOUND,
                "email_not_found",
                "No email matches this provider message ID",
            );
        }
        Err(error_value) => {
            tracing::error!(error = %error_value, provider_message_id = %event.message_id, provider, "failed to resolve provider email");
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to resolve email",
            );
        }
    };
    let email_id: Uuid = email.get("id");
    let workspace_id: Uuid = email.get("workspace_id");
    let mut payload = serde_json::to_value(&event).unwrap_or_else(|_| json!({}));
    let context: (String, serde_json::Value) =
        match sqlx::query_as("SELECT environment, metadata FROM emails WHERE id = $1")
            .bind(email_id)
            .fetch_one(&mut *tx)
            .await
        {
            Ok(value) => value,
            Err(_) => {
                return error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "database_unavailable",
                    "Unable to load email context",
                )
            }
        };
    payload["emailId"] = json!(email_id);
    payload["environment"] = json!(context.0);
    payload["metadata"] = context.1;
    payload["provider"] = json!(provider);
    let mut inserted = 0_u64;
    for recipient in recipients {
        let base_event_id = format!("{provider}:{}", event.event_id);
        let provider_event_id = recipient.as_ref().map_or_else(
            || base_event_id.clone(),
            |address| format!("{base_event_id}:{address}"),
        );
        let delivery_event_id = match sqlx::query_scalar::<_, Uuid>("INSERT INTO delivery_events (email_id, provider_event_id, event_type, recipient, payload, occurred_at) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (provider_event_id) DO NOTHING RETURNING id")
            .bind(email_id).bind(&provider_event_id).bind(&event.event_type).bind(recipient.clone()).bind(payload.clone()).bind(event.occurred_at).fetch_optional(&mut *tx).await {
                Ok(Some(value)) => value,
                Ok(None) => continue,
                Err(error_value) => {
                    tracing::error!(error = %error_value, provider_event_id = %provider_event_id, provider, "failed to store delivery event");
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
        let status = if has_recipient {
            match sqlx::query_scalar::<_, Option<String>>(
                "SELECT CASE WHEN bool_or(status='complained') THEN 'complained' WHEN bool_and(status='delivered') THEN 'delivered' WHEN bool_and(status NOT IN ('pending','sent')) AND bool_or(status='bounced') THEN 'bounced' WHEN bool_and(status NOT IN ('pending','sent')) AND bool_or(status='failed') THEN 'failed' END FROM email_recipients WHERE email_id=$1",
            )
            .bind(email_id)
            .fetch_one(&mut *tx)
            .await
            {
                Ok(value) => value,
                Err(error_value) => {
                    tracing::error!(error = %error_value, email_id = %email_id, "failed to aggregate recipient state");
                    return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to aggregate recipient state");
                }
            }
        } else {
            aggregate_status(&event.event_type).map(str::to_owned)
        };
        if let Some(status) = status {
            let update = match status.as_str() {
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
        tracing::error!(error = %error_value, email_id = %email_id, provider, "failed to commit delivery event transaction");
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
fn should_suppress(event: &DeliveryEvent) -> bool {
    event.event_type == "complaint"
        || (event.event_type == "bounce"
            && event.bounce_type.as_deref().is_some_and(|value| {
                matches!(value.to_ascii_lowercase().as_str(), "permanent" | "hard")
            }))
}
fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"code": code, "message": message}))).into_response()
}

#[cfg(test)]
mod tests {
    use super::{aggregate_status, normalized_recipients, should_suppress, DeliveryEvent};
    use chrono::Utc;
    use serde_json::json;

    fn event(event_type: &str, bounce_type: Option<&str>) -> DeliveryEvent {
        DeliveryEvent {
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
