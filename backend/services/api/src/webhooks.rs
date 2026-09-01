use super::AppState;
use ::auth::{hash_token, webhook_secret};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use url::Url;
use uuid::Uuid;

const ALLOWED_EVENTS: &[&str] = &[
    "email.delivery",
    "email.bounce",
    "email.complaint",
    "email.reject",
    "email.rendering_failure",
    "email.open",
    "email.click",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateEndpoint {
    environment: String,
    url: String,
    subscriptions: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateEndpoint {
    url: Option<String>,
    subscriptions: Option<Vec<String>>,
    enabled: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EndpointView {
    environment: String,
    id: Uuid,
    url: String,
    subscriptions: serde_json::Value,
    enabled: bool,
    failure_count: i32,
    last_success_at: Option<chrono::DateTime<chrono::Utc>>,
    last_failure_at: Option<chrono::DateTime<chrono::Utc>>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/webhooks", get(list).post(create))
        .route("/v1/webhooks/{id}", patch(update).delete(remove))
        .route("/v1/webhooks/{id}/rotate-secret", post(rotate_secret))
        .route("/v1/webhooks/{id}/deliveries", get(list_deliveries))
        .route(
            "/v1/webhooks/{id}/deliveries/{delivery_id}/attempts",
            get(list_attempts),
        )
        .route(
            "/v1/webhooks/{id}/deliveries/{delivery_id}/retry",
            post(retry_delivery),
        )
}

async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateEndpoint>,
) -> Response {
    let workspace_id = match workspace_id(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let url = match validate_url(&input.url) {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let subscriptions = match validate_subscriptions(input.subscriptions) {
        Ok(value) => value,
        Err(response) => return *response,
    };
    if !matches!(input.environment.as_str(), "test" | "production") {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_environment",
            "Choose test or production",
        );
    }
    let endpoint_id = Uuid::new_v4();
    let secret = webhook_secret(&state.webhook_signing_master_key, endpoint_id, 1);
    let row = match sqlx::query("INSERT INTO webhook_endpoints (id, workspace_id, url, signing_secret_hash, subscriptions, environment) VALUES ($1, $2, $3, $4, $5, $6) RETURNING created_at, updated_at")
        .bind(endpoint_id)
        .bind(workspace_id)
        .bind(url.as_str())
        .bind(hash_token(&secret))
        .bind(json!(subscriptions))
        .bind(&input.environment)
        .fetch_one(&state.db)
        .await
    {
        Ok(value) => value,
        Err(error_value) => {
            tracing::error!(error = %error_value, workspace_id = %workspace_id, "failed to create webhook endpoint");
            return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to create webhook endpoint");
        }
    };
    Json(json!({"data": {"endpoint": {"environment": input.environment, "id": endpoint_id, "url": url.as_str(), "subscriptions": subscriptions, "enabled": true, "failureCount": 0, "createdAt": row.get::<chrono::DateTime<chrono::Utc>, _>("created_at"), "updatedAt": row.get::<chrono::DateTime<chrono::Utc>, _>("updated_at")}, "secret": secret}})).into_response()
}

async fn list(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let workspace_id = match workspace_id(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let rows = match sqlx::query("SELECT environment, id, url, subscriptions, enabled, failure_count, last_success_at, last_failure_at, created_at, updated_at FROM webhook_endpoints WHERE workspace_id = $1 ORDER BY created_at DESC")
        .bind(workspace_id)
        .fetch_all(&state.db)
        .await
    {
        Ok(value) => value,
        Err(error_value) => {
            tracing::error!(error = %error_value, workspace_id = %workspace_id, "failed to list webhook endpoints");
            return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to list webhook endpoints");
        }
    };
    let endpoints: Vec<EndpointView> = rows.into_iter().map(endpoint_view).collect();
    Json(json!({"data": endpoints})).into_response()
}

async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateEndpoint>,
) -> Response {
    let workspace_id = match workspace_id(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if input.url.is_none() && input.subscriptions.is_none() && input.enabled.is_none() {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_webhook",
            "No webhook changes were supplied",
        );
    }
    let url = match input.url {
        Some(value) => match validate_url(&value) {
            Ok(value) => Some(value.to_string()),
            Err(response) => return *response,
        },
        None => None,
    };
    let subscriptions = match input.subscriptions {
        Some(value) => match validate_subscriptions(value) {
            Ok(value) => Some(json!(value)),
            Err(response) => return *response,
        },
        None => None,
    };
    let row = match sqlx::query("UPDATE webhook_endpoints SET url = COALESCE($3, url), subscriptions = COALESCE($4, subscriptions), enabled = COALESCE($5, enabled), failure_count = CASE WHEN $5 = true THEN 0 ELSE failure_count END, updated_at = now() WHERE id = $1 AND workspace_id = $2 RETURNING environment, id, url, subscriptions, enabled, failure_count, last_success_at, last_failure_at, created_at, updated_at")
        .bind(id)
        .bind(workspace_id)
        .bind(url)
        .bind(subscriptions)
        .bind(input.enabled)
        .fetch_optional(&state.db)
        .await
    {
        Ok(Some(value)) => value,
        Ok(None) => return error(StatusCode::NOT_FOUND, "webhook_not_found", "Webhook endpoint was not found"),
        Err(error_value) => {
            tracing::error!(error = %error_value, endpoint_id = %id, "failed to update webhook endpoint");
            return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to update webhook endpoint");
        }
    };
    Json(json!({"data": endpoint_view(row)})).into_response()
}

async fn remove(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let workspace_id = match workspace_id(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    match sqlx::query("DELETE FROM webhook_endpoints WHERE id = $1 AND workspace_id = $2")
        .bind(id)
        .bind(workspace_id)
        .execute(&state.db)
        .await
    {
        Ok(result) if result.rows_affected() == 1 => {
            Json(json!({"data": {"deleted": true}})).into_response()
        }
        Ok(_) => error(
            StatusCode::NOT_FOUND,
            "webhook_not_found",
            "Webhook endpoint was not found",
        ),
        Err(error_value) => {
            tracing::error!(error = %error_value, endpoint_id = %id, "failed to delete webhook endpoint");
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to delete webhook endpoint",
            )
        }
    }
}

async fn rotate_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let workspace_id = match workspace_id(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let current = match sqlx::query_scalar::<_, i32>(
        "SELECT signing_secret_version FROM webhook_endpoints WHERE id = $1 AND workspace_id = $2",
    )
    .bind(id)
    .bind(workspace_id)
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some(value)) => value,
        Ok(None) => {
            return error(
                StatusCode::NOT_FOUND,
                "webhook_not_found",
                "Webhook endpoint was not found",
            )
        }
        Err(error_value) => {
            tracing::error!(error = %error_value, endpoint_id = %id, "failed to load webhook secret version");
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to rotate webhook secret",
            );
        }
    };
    let version = current.saturating_add(1);
    let secret = webhook_secret(&state.webhook_signing_master_key, id, version);
    match sqlx::query("UPDATE webhook_endpoints SET signing_secret_version = $3, signing_secret_hash = $4, updated_at = now() WHERE id = $1 AND workspace_id = $2 AND signing_secret_version = $5")
        .bind(id)
        .bind(workspace_id)
        .bind(version)
        .bind(hash_token(&secret))
        .bind(current)
        .execute(&state.db)
        .await
    {
        Ok(result) if result.rows_affected() == 1 => Json(json!({"data": {"secret": secret}})).into_response(),
        Ok(_) => error(StatusCode::CONFLICT, "webhook_changed", "Webhook endpoint changed; retry rotation"),
        Err(error_value) => {
            tracing::error!(error = %error_value, endpoint_id = %id, "failed to rotate webhook secret");
            error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to rotate webhook secret")
        }
    }
}

async fn list_deliveries(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let workspace_id = match workspace_id(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let exists = match sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM webhook_endpoints WHERE id = $1 AND workspace_id = $2)",
    )
    .bind(id)
    .bind(workspace_id)
    .fetch_one(&state.db)
    .await
    {
        Ok(value) => value,
        Err(error_value) => {
            tracing::error!(error = %error_value, endpoint_id = %id, "failed to resolve webhook endpoint");
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to list webhook deliveries",
            );
        }
    };
    if !exists {
        return error(
            StatusCode::NOT_FOUND,
            "webhook_not_found",
            "Webhook endpoint was not found",
        );
    }
    let rows = match sqlx::query("SELECT delivery.id, delivery.status, delivery.attempts, delivery.next_attempt_at, delivery.last_error, delivery.completed_at, delivery.created_at, event.event_type, event.recipient, event.occurred_at FROM webhook_deliveries delivery JOIN delivery_events event ON event.id = delivery.event_id WHERE delivery.endpoint_id = $1 ORDER BY delivery.created_at DESC LIMIT 100")
        .bind(id)
        .fetch_all(&state.db)
        .await
    {
        Ok(value) => value,
        Err(error_value) => {
            tracing::error!(error = %error_value, endpoint_id = %id, "failed to list webhook deliveries");
            return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to list webhook deliveries");
        }
    };
    let deliveries: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|row| {
            json!({
                "id": row.get::<Uuid, _>("id"),
                "status": row.get::<String, _>("status"),
                "attempts": row.get::<i32, _>("attempts"),
                "nextAttemptAt": row.get::<chrono::DateTime<chrono::Utc>, _>("next_attempt_at"),
                "lastError": row.get::<Option<String>, _>("last_error"),
                "completedAt": row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("completed_at"),
                "createdAt": row.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
                "eventType": row.get::<String, _>("event_type"),
                "recipient": row.get::<Option<String>, _>("recipient"),
                "occurredAt": row.get::<chrono::DateTime<chrono::Utc>, _>("occurred_at"),
            })
        })
        .collect();
    Json(json!({"data": deliveries})).into_response()
}

async fn retry_delivery(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, delivery_id)): Path<(Uuid, Uuid)>,
) -> Response {
    let workspace_id = match workspace_id(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    match sqlx::query("UPDATE webhook_deliveries delivery SET status = 'pending', attempts = 0, retry_generation = retry_generation + 1, next_attempt_at = now(), last_error = NULL, completed_at = NULL, updated_at = now() FROM webhook_endpoints endpoint WHERE delivery.id = $1 AND delivery.endpoint_id = $2 AND endpoint.id = delivery.endpoint_id AND endpoint.workspace_id = $3 AND endpoint.enabled = true AND delivery.status = 'failed'")
        .bind(delivery_id)
        .bind(id)
        .bind(workspace_id)
        .execute(&state.db)
        .await
    {
        Ok(result) if result.rows_affected() == 1 => Json(json!({"data": {"queued": true}})).into_response(),
        Ok(_) => error(StatusCode::CONFLICT, "delivery_not_retryable", "Delivery is not failed, the endpoint is disabled, or it was not found"),
        Err(error_value) => {
            tracing::error!(error = %error_value, endpoint_id = %id, delivery_id = %delivery_id, "failed to retry webhook delivery");
            error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to retry webhook delivery")
        }
    }
}

async fn list_attempts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, delivery_id)): Path<(Uuid, Uuid)>,
) -> Response {
    let workspace_id = match workspace_id(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let rows = match sqlx::query("SELECT attempt.attempt_number, attempt.status_code, attempt.error, attempt.next_retry_at, attempt.delivered_at, attempt.created_at FROM webhook_attempts attempt JOIN webhook_deliveries delivery ON delivery.endpoint_id = attempt.endpoint_id AND delivery.event_id = attempt.event_id JOIN webhook_endpoints endpoint ON endpoint.id = delivery.endpoint_id WHERE delivery.id = $1 AND endpoint.id = $2 AND endpoint.workspace_id = $3 ORDER BY attempt.attempt_number")
        .bind(delivery_id)
        .bind(id)
        .bind(workspace_id)
        .fetch_all(&state.db)
        .await
    {
        Ok(value) => value,
        Err(error_value) => {
            tracing::error!(error = %error_value, endpoint_id = %id, delivery_id = %delivery_id, "failed to list webhook attempts");
            return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to list webhook attempts");
        }
    };
    let attempts: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|row| {
            json!({
                "attemptNumber": row.get::<i32, _>("attempt_number"),
                "statusCode": row.get::<Option<i32>, _>("status_code"),
                "error": row.get::<Option<String>, _>("error"),
                "nextRetryAt": row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("next_retry_at"),
                "deliveredAt": row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("delivered_at"),
                "createdAt": row.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
            })
        })
        .collect();
    Json(json!({"data": attempts})).into_response()
}

fn endpoint_view(row: sqlx::postgres::PgRow) -> EndpointView {
    EndpointView {
        environment: row.get("environment"),
        id: row.get("id"),
        url: row.get("url"),
        subscriptions: row.get("subscriptions"),
        enabled: row.get("enabled"),
        failure_count: row.get("failure_count"),
        last_success_at: row.get("last_success_at"),
        last_failure_at: row.get("last_failure_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn validate_subscriptions(mut values: Vec<String>) -> Result<Vec<String>, Box<Response>> {
    values.sort();
    values.dedup();
    if values.is_empty()
        || values
            .iter()
            .any(|value| !ALLOWED_EVENTS.contains(&value.as_str()))
    {
        return Err(Box::new(error(
            StatusCode::BAD_REQUEST,
            "invalid_subscriptions",
            "One or more webhook event subscriptions are invalid",
        )));
    }
    Ok(values)
}

fn validate_url(value: &str) -> Result<Url, Box<Response>> {
    ::auth::public_webhook_url(value.trim()).map_err(|_| {
        Box::new(error(
            StatusCode::BAD_REQUEST,
            "unsafe_webhook_url",
            "Use HTTPS with a public DNS hostname; IP addresses are not allowed",
        ))
    })
}

async fn workspace_id(state: &AppState, headers: &HeaderMap) -> Result<Uuid, Response> {
    super::api_keys::access(state, headers, "webhooks:manage", true)
        .await
        .map(|v| v.0)
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"code": code, "message": message}))).into_response()
}

#[cfg(test)]
mod tests {
    use super::{validate_subscriptions, validate_url};

    #[test]
    fn production_webhooks_require_safe_https_urls() {
        assert!(validate_url("https://hooks.example.com/events").is_ok());
        assert!(validate_url("http://hooks.example.com/events").is_err());
        assert!(validate_url("https://127.0.0.1/events").is_err());
    }

    #[test]
    fn subscriptions_are_validated_and_deduplicated() {
        let values = vec!["email.delivery".into(), "email.delivery".into()];
        assert_eq!(
            validate_subscriptions(values).expect("valid events"),
            vec!["email.delivery"]
        );
        assert!(validate_subscriptions(vec!["unknown".into()]).is_err());
    }
}
