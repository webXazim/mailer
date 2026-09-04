use super::{ses_events, AppState};
use axum::{
    body::Bytes,
    extract::DefaultBodyLimit,
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const MAX_WEBHOOK_BYTES: usize = 1_048_576;

#[derive(Deserialize)]
struct WebhookBatch {
    events: Vec<StalwartEvent>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StalwartEvent {
    id: String,
    created_at: DateTime<Utc>,
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    data: Value,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/internal/v1/stalwart/events", post(ingest))
        .layer(DefaultBodyLimit::max(MAX_WEBHOOK_BYTES))
}

async fn ingest(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    let (Some(token), Some(signing_key)) = (
        state.stalwart_webhook_token.as_deref(),
        state.stalwart_webhook_signing_key.as_deref(),
    ) else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "stalwart_events_disabled",
            "Stalwart event ingestion is not configured",
        );
    };
    if body.len() > MAX_WEBHOOK_BYTES {
        return error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "event_batch_too_large",
            "The Stalwart event batch is too large",
        );
    }
    if !bearer_authorized(&headers, token) || !signature_valid(&headers, &body, signing_key) {
        return error(
            StatusCode::UNAUTHORIZED,
            "invalid_stalwart_webhook_auth",
            "Stalwart webhook authentication failed",
        );
    }
    let batch: WebhookBatch = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_event_batch",
                "The Stalwart webhook body is invalid",
            )
        }
    };
    if batch.events.len() > 1_000 {
        return error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "event_batch_too_large",
            "The Stalwart event batch contains too many events",
        );
    }

    let mut accepted = 0_u64;
    let mut ignored = 0_u64;
    for event in batch.events {
        let Some(normalized) = normalize(event) else {
            ignored += 1;
            continue;
        };
        let response = ses_events::ingest_event(
            &state,
            normalized.event,
            "smtp",
            Some(normalized.correlation),
        )
        .await;
        match response.status() {
            status if status.is_success() => accepted += 1,
            StatusCode::NOT_FOUND => ignored += 1,
            _ => return response,
        }
    }
    Json(json!({"data":{"accepted":true,"events":accepted,"ignored":ignored}})).into_response()
}

struct NormalizedEvent {
    event: ses_events::SesEvent,
    correlation: (Uuid, Uuid),
}

fn normalize(event: StalwartEvent) -> Option<NormalizedEvent> {
    let (event_type, bounce_type) = match event.event_type.as_str() {
        "queue.authenticated-message-queued" | "queue.message-queued" => ("queued", None),
        "delivery.delivered" => ("delivery", None),
        "delivery.failed"
        | "delivery.rcpt-to-failed"
        | "delivery.connect-error"
        | "delivery.greeting-failed"
        | "delivery.start-tls-error"
        | "delivery.concurrency-limit-exceeded"
        | "delivery.rate-limit-exceeded"
        | "queue.rescheduled"
        | "queue.rate-limit-exceeded"
        | "queue.concurrency-limit-exceeded"
        | "queue.back-pressure" => ("deferred", None),
        "delivery.rcpt-to-rejected"
        | "delivery.message-rejected"
        | "delivery.null-mx"
        | "delivery.dsn-perm-fail"
        | "delivery.double-bounce" => ("bounce", Some("Permanent".to_owned())),
        "delivery.mail-from-rejected" | "queue.blob-not-found" | "queue.quota-exceeded" => {
            ("reject", None)
        }
        "incoming-report.abuse-report" | "incoming-report.fraud-report" => ("complaint", None),
        _ => return None,
    };
    let message_id = find_string(&event.data, &["messageid", "rfcmessageid"])?;
    let correlation = parse_correlation(&message_id)?;
    let recipients = find_addresses(&event.data);
    Some(NormalizedEvent {
        event: ses_events::SesEvent {
            event_id: event.id,
            message_id,
            event_type: event_type.to_owned(),
            occurred_at: event.created_at,
            recipients,
            bounce_type,
            details: json!({
                "stalwartType": event.event_type,
                "stalwartData": event.data,
            }),
        },
        correlation,
    })
}

fn parse_correlation(message_id: &str) -> Option<(Uuid, Uuid)> {
    let value = message_id
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>');
    let local = value.split_once('@')?.0;
    let mut parts = local.split('.');
    let email_id = Uuid::parse_str(parts.next()?).ok()?;
    let attempt_id = Uuid::parse_str(parts.next()?).ok()?;
    Some((email_id, attempt_id))
}

fn find_string(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                let normalized = key
                    .chars()
                    .filter(|character| character.is_ascii_alphanumeric())
                    .flat_map(char::to_lowercase)
                    .collect::<String>();
                if keys.contains(&normalized.as_str()) {
                    if let Some(value) = value.as_str() {
                        return Some(value.to_owned());
                    }
                }
            }
            values.values().find_map(|value| find_string(value, keys))
        }
        Value::Array(values) => values.iter().find_map(|value| find_string(value, keys)),
        _ => None,
    }
}

fn find_addresses(value: &Value) -> Vec<String> {
    let mut addresses = Vec::new();
    collect_addresses(value, &mut addresses);
    addresses.sort();
    addresses.dedup();
    addresses
}

fn collect_addresses(value: &Value, addresses: &mut Vec<String>) {
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                let key = key.to_ascii_lowercase();
                if matches!(key.as_str(), "to" | "recipient" | "recipients") {
                    collect_address_values(value, addresses);
                } else {
                    collect_addresses(value, addresses);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_addresses(value, addresses);
            }
        }
        _ => {}
    }
}

fn collect_address_values(value: &Value, addresses: &mut Vec<String>) {
    match value {
        Value::String(value) if value.contains('@') => addresses.push(value.trim().to_lowercase()),
        Value::Array(values) => {
            for value in values {
                collect_address_values(value, addresses);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                if matches!(
                    key.to_ascii_lowercase().as_str(),
                    "to" | "recipient" | "recipients" | "address"
                ) {
                    collect_address_values(value, addresses);
                }
            }
        }
        _ => {}
    }
}

fn bearer_authorized(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|actual| constant_time_eq(actual.trim().as_bytes(), expected.as_bytes()))
}

fn signature_valid(headers: &HeaderMap, body: &[u8], key: &str) -> bool {
    let Some(signature) = headers
        .get("x-signature")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| BASE64.decode(value.trim()).ok())
    else {
        return false;
    };
    Hmac::<Sha256>::new_from_slice(key.as_bytes()).is_ok_and(|mut mac| {
        mac.update(body);
        mac.verify_slice(&signature).is_ok()
    })
}

fn constant_time_eq(actual: &[u8], expected: &[u8]) -> bool {
    let actual_hash = Sha256::digest(actual);
    let expected_hash = Sha256::digest(expected);
    actual_hash
        .iter()
        .zip(expected_hash.iter())
        .fold(0_u8, |difference, (actual, expected)| {
            difference | (actual ^ expected)
        })
        == 0
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"code":code,"message":message}))).into_response()
}

#[cfg(test)]
mod tests {
    use super::{normalize, parse_correlation, signature_valid, StalwartEvent};
    use axum::http::{HeaderMap, HeaderValue};
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    use chrono::Utc;
    use hmac::{Hmac, Mac};
    use serde_json::json;
    use sha2::Sha256;
    use uuid::Uuid;

    #[test]
    fn verifies_the_raw_body_signature() {
        let body = br#"{"events":[]}"#;
        let key = "a-long-random-webhook-signing-key";
        let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).unwrap();
        mac.update(body);
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-signature",
            HeaderValue::from_str(&BASE64.encode(mac.finalize().into_bytes())).unwrap(),
        );
        assert!(signature_valid(&headers, body, key));
        assert!(!signature_valid(&headers, b"changed", key));
    }

    #[test]
    fn parses_mailer_message_correlation() {
        let email_id = Uuid::new_v4();
        let attempt_id = Uuid::new_v4();
        assert_eq!(
            parse_correlation(&format!("<{email_id}.{attempt_id}@smtp.example.com>")),
            Some((email_id, attempt_id))
        );
    }

    #[test]
    fn maps_delivery_and_temporary_failures_without_fake_delivery() {
        let email_id = Uuid::new_v4();
        let attempt_id = Uuid::new_v4();
        let event = |kind: &str| StalwartEvent {
            id: "event-1".into(),
            created_at: Utc::now(),
            event_type: kind.into(),
            data: json!({
                "messageId": format!("{email_id}.{attempt_id}@smtp.example.com"),
                "to": ["Recipient@Example.com"]
            }),
        };
        let delivered = normalize(event("delivery.delivered")).unwrap();
        assert_eq!(delivered.event.event_type, "delivery");
        assert_eq!(delivered.event.recipients, ["recipient@example.com"]);
        let deferred = normalize(event("delivery.failed")).unwrap();
        assert_eq!(deferred.event.event_type, "deferred");
    }
}
