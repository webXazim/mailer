//! CS Connect identity creates a private test workspace on first use. No
//! sending, production approval, or existing membership is inherited.
use super::{auth, dns_automation::http_request, emails::client_ip, AppState};
use ::auth::{generate_token, hash_token};
use axum::{
    extract::{ConnectInfo, State},
    http::{header, HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::Row;
use std::net::SocketAddr;
use uuid::Uuid;

const AUTH_URL: &str = "https://connect.crescentsphere.com/o/authorize/";
const TOKEN_URL: &str = "https://connect.crescentsphere.com/o/token/";
const PROFILE_URL: &str = "https://connect.crescentsphere.com/api/v1/accounts/federation/userinfo/";

#[derive(Deserialize)]
struct CompleteIn {
    code: String,
    code_verifier: String,
}
#[derive(Deserialize)]
struct TokenOut {
    access_token: String,
}
#[derive(Deserialize)]
struct Identity {
    sub: String,
    email: String,
    email_verified: bool,
    name: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/auth/connect/config", get(config))
        .route("/v1/auth/connect/complete", post(complete))
}

fn available(state: &AppState) -> bool {
    state.connect_client_id.is_some()
        && state.connect_client_secret.is_some()
        && state.console_origin.starts_with("https://")
}

async fn config(State(state): State<AppState>) -> Json<serde_json::Value> {
    if !available(&state) {
        return Json(json!({"data":{"enabled":false}}));
    }
    Json(json!({"data":{
        "enabled":true,
        "clientId":state.connect_client_id,
        "authorizeUrl":AUTH_URL,
        "redirectUri":format!("{}/auth/connect/callback",state.console_origin.trim_end_matches('/')),
    }}))
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"code":code,"message":message}))).into_response()
}

async fn complete(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<CompleteIn>,
) -> Response {
    if !available(&state) {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "federation_unavailable",
            "CS Connect sign-in is unavailable",
        );
    }
    let ip = client_ip(peer.ip(), &headers, state.trust_proxy_headers);
    if let Err(response) =
        auth::enforce_auth_limit(&state, &format!("connect-signin:ip:{ip}"), 12).await
    {
        return response;
    }
    if input.code.is_empty()
        || input.code.len() > 2048
        || !(43..=128).contains(&input.code_verifier.len())
        || !input
            .code_verifier
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-._~".contains(&c))
    {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Invalid sign-in response",
        );
    }
    let redirect_uri = format!(
        "{}/auth/connect/callback",
        state.console_origin.trim_end_matches('/')
    );
    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "authorization_code")
        .append_pair("code", &input.code)
        .append_pair(
            "client_id",
            state.connect_client_id.as_deref().unwrap_or_default(),
        )
        .append_pair(
            "client_secret",
            state.connect_client_secret.as_deref().unwrap_or_default(),
        )
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("code_verifier", &input.code_verifier)
        .finish();
    let (status, body) = match http_request(
        Method::POST,
        TOKEN_URL,
        &[(
            header::CONTENT_TYPE,
            "application/x-www-form-urlencoded".into(),
        )],
        form.into_bytes(),
    )
    .await
    {
        Ok(response) => response,
        Err(_) => {
            return error(
                StatusCode::BAD_GATEWAY,
                "provider_unavailable",
                "CS Connect could not complete sign-in",
            )
        }
    };
    if !status.is_success() {
        return error(
            StatusCode::BAD_REQUEST,
            "authorization_rejected",
            "CS Connect authorization expired or was rejected",
        );
    }
    let token: TokenOut = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_provider_response",
                "Invalid CS Connect authorization response",
            )
        }
    };
    if token.access_token.is_empty() {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_provider_response",
            "Missing CS Connect authorization",
        );
    }
    let (status, body) = match http_request(
        Method::GET,
        PROFILE_URL,
        &[(
            header::AUTHORIZATION,
            format!("Bearer {}", token.access_token),
        )],
        Vec::new(),
    )
    .await
    {
        Ok(response) => response,
        Err(_) => {
            return error(
                StatusCode::BAD_GATEWAY,
                "provider_unavailable",
                "CS Connect profile is unavailable",
            )
        }
    };
    if !status.is_success() {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_provider_response",
            "CS Connect profile could not be verified",
        );
    }
    let profile: Identity = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_provider_response",
                "Invalid CS Connect profile",
            )
        }
    };
    let email = profile.email.trim().to_lowercase();
    if profile.sub.is_empty()
        || profile.sub.len() > 255
        || !profile.email_verified
        || email.len() > 254
        || !email.contains('@')
    {
        return error(
            StatusCode::FORBIDDEN,
            "unverified_email",
            "A verified CS Connect email is required",
        );
    }
    let mut tx = match state.db.begin().await {
        Ok(value) => value,
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "database_unavailable",
                "Sign-in is temporarily unavailable",
            )
        }
    };
    let linked=sqlx::query_scalar::<_,Uuid>("SELECT user_id FROM federated_identities WHERE provider='connect' AND subject=$1 FOR UPDATE")
        .bind(&profile.sub).fetch_optional(&mut *tx).await;
    let user_id = match linked {
        Ok(Some(id)) => id,
        Ok(None) => {
            let existing =
                sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE lower(email)=$1")
                    .bind(&email)
                    .fetch_optional(&mut *tx)
                    .await;
            let existing = match existing {
                Ok(value) => value,
                Err(_) => {
                    return error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "database_unavailable",
                        "Sign-in is temporarily unavailable",
                    )
                }
            };
            if let Some(id) = existing {
                let raw_cookie = auth::read_cookie(&headers).unwrap_or_default();
                let owned = sqlx::query_scalar::<_, Uuid>(
                    "SELECT u.id FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=$1 AND u.id=$2 AND lower(u.email)=$3 AND u.email_verified_at IS NOT NULL AND u.mfa_enabled=false AND s.revoked_at IS NULL AND s.expires_at>now()"
                ).bind(hash_token(&raw_cookie)).bind(id).bind(&email).fetch_optional(&mut *tx).await;
                if !matches!(owned, Ok(Some(_))) {
                    return error(StatusCode::CONFLICT,"local_account_link_required","A CS Mailer account already uses this email. Sign in locally first to link it. MFA accounts need a separate step-up flow.");
                }
                if sqlx::query("INSERT INTO federated_identities (provider,subject,user_id,email_at_link) VALUES ('connect',$1,$2,$3)")
                    .bind(&profile.sub).bind(id).bind(&email).execute(&mut *tx).await.is_err() {
                    return error(StatusCode::CONFLICT,"account_conflict","Unable to link this CS Connect account");
                }
                id
            } else {
                let random_password = format!("{}{}", Uuid::new_v4(), Uuid::new_v4());
                let password_hash = match auth::password_hash_async(random_password).await {
                    Ok(value) => value,
                    Err(_) => {
                        return error(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "unavailable",
                            "Sign-in is temporarily unavailable",
                        )
                    }
                };
                let name = profile.name.as_deref().unwrap_or("").trim();
                let name = if name.is_empty() || name.len() > 120 {
                    email.split('@').next().unwrap_or("Member")
                } else {
                    name
                };
                let id=match sqlx::query_scalar::<_,Uuid>("INSERT INTO users (email,password_hash,display_name,email_verified_at) VALUES ($1,$2,$3,now()) RETURNING id")
                .bind(&email).bind(password_hash).bind(name).fetch_one(&mut *tx).await {
                Ok(value)=>value,Err(_)=>return error(StatusCode::CONFLICT,"account_conflict","This email already has a CS Mailer account"),
            };
                let workspace_name = format!("{}'s Workspace", name);
                let slug = format!(
                    "{}-{}",
                    name.to_ascii_lowercase()
                        .chars()
                        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                        .take(30)
                        .collect::<String>(),
                    &Uuid::new_v4().simple().to_string()[..8]
                );
                let workspace_id = match sqlx::query_scalar::<_, Uuid>(
                    "INSERT INTO workspaces (name,slug,created_by) VALUES ($1,$2,$3) RETURNING id",
                )
                .bind(workspace_name)
                .bind(slug)
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                {
                    Ok(value) => value,
                    Err(_) => {
                        return error(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "database_unavailable",
                            "Workspace setup failed",
                        )
                    }
                };
                if sqlx::query("INSERT INTO workspace_members (workspace_id,user_id,role) VALUES ($1,$2,'owner')")
                .bind(workspace_id).bind(id).execute(&mut *tx).await.is_err()
                || sqlx::query("INSERT INTO federated_identities (provider,subject,user_id,email_at_link) VALUES ('connect',$1,$2,$3)")
                    .bind(&profile.sub).bind(id).bind(&email).execute(&mut *tx).await.is_err() {
                return error(StatusCode::SERVICE_UNAVAILABLE,"database_unavailable","Account setup failed");
            }
                id
            }
        }
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "database_unavailable",
                "Sign-in is temporarily unavailable",
            )
        }
    };
    let row = sqlx::query("SELECT email_verified_at,mfa_enabled FROM users WHERE id=$1")
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await;
    if !matches!(row,Ok(Some(ref value)) if value.get::<Option<chrono::DateTime<chrono::Utc>>,_>("email_verified_at").is_some())
    {
        return error(
            StatusCode::FORBIDDEN,
            "account_unavailable",
            "This CS Mailer account is unavailable",
        );
    }
    if matches!(row, Ok(Some(ref value)) if value.get::<bool,_>("mfa_enabled")) {
        return error(
            StatusCode::FORBIDDEN,
            "local_mfa_required",
            "This CS Mailer account requires a local MFA sign-in",
        );
    }
    if sqlx::query("UPDATE federated_identities SET last_login_at=now() WHERE provider='connect' AND subject=$1")
        .bind(&profile.sub).execute(&mut *tx).await.is_err() || tx.commit().await.is_err() {
        return error(StatusCode::SERVICE_UNAVAILABLE,"database_unavailable","Sign-in is temporarily unavailable");
    }
    let session_token = generate_token();
    if sqlx::query("INSERT INTO sessions (user_id,token_hash,expires_at) VALUES ($1,$2,now()+interval '30 days')")
        .bind(user_id).bind(hash_token(&session_token)).execute(&state.db).await.is_err() {
        return error(StatusCode::SERVICE_UNAVAILABLE,"database_unavailable","Sign-in is temporarily unavailable");
    }
    let context = match auth::load_context(&state, &session_token).await {
        Ok(value) => value,
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "database_unavailable",
                "Sign-in is temporarily unavailable",
            )
        }
    };
    tracing::info!(user_id=%user_id, ip=%ip, "CS Connect sign-in completed");
    auth::with_cookie(
        Json(json!({"data":context})).into_response(),
        auth::cookie(
            &session_token,
            true,
            false,
            state.environment == "production",
        ),
    )
}
