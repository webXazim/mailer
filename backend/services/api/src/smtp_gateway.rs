use super::{api_keys, AppState};
use ::auth::hash_token;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::json;

pub fn routes() -> Router<AppState> {
    Router::new().route("/internal/v1/smtp/verify", post(verify_key))
}

pub(crate) fn trusted(state: &AppState, headers: &HeaderMap) -> bool {
    let (Some(expected), Some(provided)) = (
        state.smtp_gateway_secret.as_deref(),
        headers
            .get("x-smtp-gateway-secret")
            .and_then(|v| v.to_str().ok()),
    ) else {
        return false;
    };
    let a = hash_token(expected);
    let b = hash_token(provided);
    a.iter()
        .zip(b.iter())
        .fold(0u8, |diff, (left, right)| diff | (left ^ right))
        == 0
}

async fn verify_key(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !trusted(&state, &headers) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(key) = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match api_keys::verify(key, &state.db, "emails:send").await {
        Ok((_, _, environment)) => {
            (StatusCode::OK, Json(json!({"environment": environment}))).into_response()
        }
        Err(_) => StatusCode::UNAUTHORIZED.into_response(),
    }
}
