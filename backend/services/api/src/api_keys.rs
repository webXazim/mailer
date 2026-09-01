use super::{auth::load_context, AppState};
use ::auth::{generate_token, hash_token};
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

const ALLOWED_SCOPES: &[&str] = &[
    "emails:send",
    "emails:read",
    "domains:read",
    "domains:write",
    "webhooks:manage",
    "suppressions:manage",
    "workspace:read",
];

#[derive(Deserialize)]
struct CreateKeyRequest {
    name: String,
    environment: String,
    scopes: Vec<String>,
    expires_in_days: Option<i32>,
}

#[derive(Serialize)]
struct KeyView {
    id: Uuid,
    name: String,
    prefix: String,
    environment: String,
    scopes: serde_json::Value,
    expires_at: Option<String>,
    last_used_at: Option<String>,
    created_at: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/api-keys", get(list_keys).post(create_key))
        .route("/v1/api-keys/{id}", delete(revoke_key))
        .route("/v1/api-keys/{id}/rotate", post(rotate_key))
}

async fn create_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateKeyRequest>,
) -> Response {
    let workspace_id = match workspace_id(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if input.name.trim().is_empty()
        || input.name.len() > 80
        || !matches!(input.environment.as_str(), "test" | "production")
    {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_api_key",
            "Name and environment are invalid",
        );
    }
    if input.scopes.is_empty()
        || input
            .scopes
            .iter()
            .any(|scope| !ALLOWED_SCOPES.contains(&scope.as_str()))
    {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_scopes",
            "One or more API key scopes are invalid",
        );
    }
    let expires_in_days = input.expires_in_days.unwrap_or(0);
    if !(expires_in_days == 0 || (1..=365).contains(&expires_in_days)) {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_expiry",
            "Expiry must be between 1 and 365 days",
        );
    }
    let secret = format!(
        "cs_{}_{}",
        if input.environment == "production" {
            "live"
        } else {
            "test"
        },
        generate_token()
    );
    let prefix: String = secret.chars().take(16).collect();
    let scopes = serde_json::to_value(&input.scopes).expect("scopes are serializable");
    let row = match sqlx::query("INSERT INTO api_keys (workspace_id, name, key_prefix, secret_hash, environment, scopes, expires_at) VALUES ($1, $2, $3, $4, $5, $6, CASE WHEN $7 = 0 THEN NULL ELSE now() + make_interval(days => $7) END) RETURNING id, expires_at, created_at")
        .bind(workspace_id).bind(input.name.trim()).bind(&prefix).bind(hash_token(&secret)).bind(&input.environment).bind(scopes.clone()).bind(expires_in_days).fetch_one(&state.db).await {
            Ok(value) => value,
            Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to create API key"),
        };
    Json(json!({"data": {"key": {"id": row.get::<Uuid, _>("id"), "name": input.name.trim(), "prefix": prefix, "environment": input.environment, "scopes": scopes, "expiresAt": row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("expires_at"), "createdAt": row.get::<chrono::DateTime<chrono::Utc>, _>("created_at")}, "secret": secret}})).into_response()
}

async fn list_keys(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let workspace_id = match workspace_id(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let rows = match sqlx::query("SELECT id, name, key_prefix, environment, scopes, expires_at, last_used_at, created_at FROM api_keys WHERE workspace_id = $1 AND revoked_at IS NULL ORDER BY created_at DESC").bind(workspace_id).fetch_all(&state.db).await { Ok(value) => value, Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to list API keys") };
    let keys: Vec<KeyView> = rows
        .into_iter()
        .map(|row| KeyView {
            id: row.get("id"),
            name: row.get("name"),
            prefix: row.get("key_prefix"),
            environment: row.get("environment"),
            scopes: row.get("scopes"),
            expires_at: row
                .get::<Option<chrono::DateTime<chrono::Utc>>, _>("expires_at")
                .map(|value| value.to_rfc3339()),
            last_used_at: row
                .get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_used_at")
                .map(|value| value.to_rfc3339()),
            created_at: row
                .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
                .to_rfc3339(),
        })
        .collect();
    Json(json!({"data": keys})).into_response()
}

async fn revoke_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let workspace_id = match workspace_id(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    match sqlx::query("UPDATE api_keys SET revoked_at = now() WHERE id = $1 AND workspace_id = $2 AND revoked_at IS NULL").bind(id).bind(workspace_id).execute(&state.db).await {
        Ok(result) if result.rows_affected() == 1 => Json(json!({"data": {"revoked": true}})).into_response(),
        Ok(_) => error(StatusCode::NOT_FOUND, "api_key_not_found", "API key was not found"),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to revoke API key"),
    }
}

async fn rotate_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let workspace_id = match workspace_id(&state, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "database_unavailable",
                "Unable to rotate API key",
            )
        }
    };
    let existing = match sqlx::query("SELECT name, environment, scopes, expires_at FROM api_keys WHERE id = $1 AND workspace_id = $2 AND revoked_at IS NULL FOR UPDATE").bind(id).bind(workspace_id).fetch_optional(&mut *tx).await { Ok(Some(value)) => value, Ok(None) => return error(StatusCode::NOT_FOUND, "api_key_not_found", "API key was not found"), Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to rotate API key") };
    let secret = format!(
        "cs_{}_{}",
        if existing.get::<String, _>("environment") == "production" {
            "live"
        } else {
            "test"
        },
        generate_token()
    );
    let prefix: String = secret.chars().take(16).collect();
    let new_id = match sqlx::query_scalar::<_, Uuid>("INSERT INTO api_keys (workspace_id, name, key_prefix, secret_hash, environment, scopes, expires_at) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id")
        .bind(workspace_id).bind(existing.get::<String, _>("name")).bind(&prefix).bind(hash_token(&secret)).bind(existing.get::<String, _>("environment")).bind(existing.get::<serde_json::Value, _>("scopes")).bind(existing.get::<Option<chrono::DateTime<chrono::Utc>>, _>("expires_at")).fetch_one(&mut *tx).await { Ok(value) => value, Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to rotate API key") };
    if sqlx::query("UPDATE api_keys SET revoked_at = now() WHERE id = $1 AND workspace_id = $2")
        .bind(id)
        .bind(workspace_id)
        .execute(&mut *tx)
        .await
        .is_err()
        || tx.commit().await.is_err()
    {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "rotation_failed",
            "API key rotation did not complete",
        );
    }
    Json(json!({"data": {"id": new_id, "prefix": prefix, "secret": secret}})).into_response()
}

#[allow(dead_code)]
pub(crate) async fn verify(
    raw_key: &str,
    pool: &db::DbPool,
    required_scope: &str,
) -> Result<(Uuid, Uuid, String), ()> {
    let hash = hash_token(raw_key);
    let row = sqlx::query("SELECT id, workspace_id, environment, scopes FROM api_keys WHERE secret_hash = $1 AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > now())").bind(hash).fetch_optional(pool).await.map_err(|_| ())?.ok_or(())?;
    let scopes: serde_json::Value = row.get("scopes");
    if !scopes.as_array().is_some_and(|values| {
        values
            .iter()
            .any(|value| value.as_str() == Some(required_scope))
    }) {
        return Err(());
    }
    let key_id: Uuid = row.get("id");
    let workspace_id: Uuid = row.get("workspace_id");
    let environment: String = row.get("environment");
    let _ = sqlx::query("UPDATE api_keys SET last_used_at = now() WHERE id = $1")
        .bind(key_id)
        .execute(pool)
        .await;
    Ok((key_id, workspace_id, environment))
}

pub(crate) async fn workspace_id(state: &AppState, headers: &HeaderMap) -> Result<Uuid, Response> {
    let token = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value.split(';').map(str::trim).find_map(|pair| {
                pair.strip_prefix("cs_session=")
                    .or_else(|| pair.strip_prefix("__Host-cs_session="))
                    .map(str::to_owned)
            })
        })
        .ok_or_else(|| {
            error(
                StatusCode::UNAUTHORIZED,
                "not_authenticated",
                "Authentication required",
            )
        })?;
    let context = load_context(state, &token).await.map_err(|_| {
        error(
            StatusCode::UNAUTHORIZED,
            "not_authenticated",
            "Authentication required",
        )
    })?;
    if !matches!(context.user.role.as_str(), "owner" | "admin") {
        return Err(error(
            StatusCode::FORBIDDEN,
            "insufficient_role",
            "Owner or admin access is required",
        ));
    }
    Ok(context.workspace.id)
}

/// Authorization is explicit: API keys never fall back to a browser session.
pub(crate) async fn access(
    state: &AppState,
    headers: &HeaderMap,
    scope: &str,
    admin: bool,
) -> Result<(Uuid, Option<String>), Response> {
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        let raw = value
            .to_str()
            .ok()
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        return verify(raw, &state.db, scope)
            .await
            .map(|(_, workspace, environment)| (workspace, Some(environment)))
            .map_err(|_| {
                error(
                    StatusCode::UNAUTHORIZED,
                    "invalid_api_key",
                    "Valid API key with the required scope needed",
                )
            });
    }
    let token = super::auth::read_cookie(headers).ok_or_else(|| {
        error(
            StatusCode::UNAUTHORIZED,
            "not_authenticated",
            "Sign in required",
        )
    })?;
    let context = load_context(state, &token).await.map_err(|_| {
        error(
            StatusCode::UNAUTHORIZED,
            "not_authenticated",
            "Sign in required",
        )
    })?;
    if admin && !matches!(context.user.role.as_str(), "owner" | "admin") {
        return Err(error(
            StatusCode::FORBIDDEN,
            "insufficient_role",
            "Owner or admin required",
        ));
    }
    Ok((context.workspace.id, None))
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"code": code, "message": message}))).into_response()
}
