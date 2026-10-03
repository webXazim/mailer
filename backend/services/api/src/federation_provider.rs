//! CS Mailer OAuth provider for a single CS Connect confidential client.
use super::{auth as mailer_auth, AppState};
use ::auth::{generate_token, hash_token, token_matches};
use axum::{
    extract::{Form, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const CONNECT_CALLBACK: &str =
    "https://connect.crescentsphere.com/api/v1/accounts/federation/callback/";

#[derive(Deserialize)]
struct AuthorizeIn {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    scope: String,
    state: String,
    code_challenge: String,
    code_challenge_method: String,
}
#[derive(Deserialize)]
struct TokenIn {
    grant_type: String,
    code: String,
    redirect_uri: String,
    client_id: String,
    client_secret: String,
    code_verifier: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/auth/federation/authorize", get(authorize))
        .route("/v1/auth/federation/token", post(token))
        .route("/v1/auth/federation/userinfo", get(userinfo))
}

fn configured(state: &AppState) -> bool {
    state.federation_client_id.is_some()
        && state.federation_client_secret.is_some()
        && state.console_origin.starts_with("https://")
}

async fn authorize(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(input): Query<AuthorizeIn>,
) -> Response {
    if !configured(&state)
        || input.response_type != "code"
        || input.client_id != state.federation_client_id.as_deref().unwrap_or_default()
        || input.redirect_uri != CONNECT_CALLBACK
        || input.code_challenge_method != "S256"
        || input.state.len() < 16
        || input.state.len() > 512
        || input.code_challenge.len() != 43
        || !input
            .code_challenge
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        || !["openid", "email", "profile"]
            .iter()
            .all(|scope| input.scope.split_whitespace().any(|part| part == *scope))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid_request"})),
        )
            .into_response();
    }
    let raw_cookie = mailer_auth::read_cookie(&headers).unwrap_or_default();
    let user:Option<Uuid>=sqlx::query_scalar("SELECT u.id FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=$1 AND s.revoked_at IS NULL AND s.expires_at>now() AND u.email_verified_at IS NOT NULL")
        .bind(hash_token(&raw_cookie)).fetch_optional(&state.db).await.unwrap_or(None);
    let Some(user_id) = user else {
        let mut next = url::Url::parse(&format!(
            "{}/login",
            state.console_origin.trim_end_matches('/')
        ))
        .unwrap();
        let mut authorize = url::Url::parse(&format!(
            "{}/api/v1/auth/federation/authorize",
            state.console_origin.trim_end_matches('/')
        ))
        .unwrap();
        authorize
            .query_pairs_mut()
            .append_pair("response_type", &input.response_type)
            .append_pair("client_id", &input.client_id)
            .append_pair("redirect_uri", &input.redirect_uri)
            .append_pair("scope", &input.scope)
            .append_pair("state", &input.state)
            .append_pair("code_challenge", &input.code_challenge)
            .append_pair("code_challenge_method", &input.code_challenge_method);
        next.query_pairs_mut().append_pair(
            "return",
            &format!(
                "{}?{}",
                authorize.path(),
                authorize.query().unwrap_or_default()
            ),
        );
        return Redirect::to(next.as_str()).into_response();
    };
    let code = generate_token();
    if sqlx::query("INSERT INTO federation_authorization_codes (code_hash,user_id,challenge,redirect_uri,expires_at) VALUES ($1,$2,$3,$4,now()+interval '3 minutes')")
        .bind(hash_token(&code)).bind(user_id).bind(&input.code_challenge).bind(CONNECT_CALLBACK).execute(&state.db).await.is_err() {
        return (StatusCode::SERVICE_UNAVAILABLE,Json(json!({"error":"temporarily_unavailable"}))).into_response();
    }
    let mut redirect = url::Url::parse(CONNECT_CALLBACK).unwrap();
    redirect
        .query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("state", &input.state);
    Redirect::to(redirect.as_str()).into_response()
}

async fn token(State(state): State<AppState>, Form(input): Form<TokenIn>) -> Response {
    if !configured(&state)
        || input.grant_type != "authorization_code"
        || input.redirect_uri != CONNECT_CALLBACK
        || input.client_id != state.federation_client_id.as_deref().unwrap_or_default()
        || !token_matches(
            &input.client_secret,
            state
                .federation_client_secret
                .as_deref()
                .unwrap_or_default(),
        )
        || input.code.len() > 256
        || !(43..=128).contains(&input.code_verifier.len())
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"invalid_client"})),
        )
            .into_response();
    }
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(input.code_verifier.as_bytes()));
    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":"temporarily_unavailable"})),
            )
                .into_response()
        }
    };
    let row:Option<(Uuid,String)>=sqlx::query_as("UPDATE federation_authorization_codes SET used_at=now() WHERE code_hash=$1 AND used_at IS NULL AND expires_at>now() AND redirect_uri=$2 RETURNING user_id,challenge")
        .bind(hash_token(&input.code)).bind(CONNECT_CALLBACK).fetch_optional(&mut *tx).await.unwrap_or(None);
    let Some((user_id, stored_challenge)) = row else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid_grant"})),
        )
            .into_response();
    };
    if !token_matches(&challenge, &stored_challenge) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid_grant"})),
        )
            .into_response();
    }
    let access = generate_token();
    if sqlx::query("INSERT INTO federation_access_tokens (token_hash,user_id,expires_at) VALUES ($1,$2,now()+interval '5 minutes')")
        .bind(hash_token(&access)).bind(user_id).execute(&mut *tx).await.is_err() || tx.commit().await.is_err() {
        return (StatusCode::SERVICE_UNAVAILABLE,Json(json!({"error":"temporarily_unavailable"}))).into_response();
    }
    let mut response =
        Json(json!({"access_token":access,"token_type":"Bearer","expires_in":300})).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}

async fn userinfo(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !configured(&state) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");
    if token.len() < 32 || token.len() > 256 {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let row:Option<(Uuid,String,String)>=sqlx::query_as("SELECT u.id,u.email,u.display_name FROM federation_access_tokens t JOIN users u ON u.id=t.user_id WHERE t.token_hash=$1 AND t.expires_at>now() AND u.email_verified_at IS NOT NULL")
        .bind(hash_token(token)).fetch_optional(&state.db).await.unwrap_or(None);
    let Some((id, email, name)) = row else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let mut response =
        Json(json!({"sub":id.to_string(),"email":email,"email_verified":true,"name":name}))
            .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}
