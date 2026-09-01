use super::{emails::client_ip, AppState};
use ::auth::{generate_token, hash_password, hash_token, verify_password};
use axum::{
    extract::{ConnectInfo, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Timelike;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use std::{net::SocketAddr, sync::LazyLock};
use uuid::Uuid;

const SESSION_COOKIE: &str = "cs_session";
const SESSION_DAYS: i64 = 30;
static DUMMY_PASSWORD_HASH: LazyLock<String> = LazyLock::new(|| {
    hash_password("not-a-real-user-password")
        .expect("the static dummy password must always be hashable")
});

#[derive(Deserialize)]
pub struct SignupRequest {
    pub email: String,
    pub password: String,
    pub first_name: String,
    pub last_name: String,
    pub workspace_name: Option<String>,
    pub signup_token: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    pub remember: Option<bool>,
}

#[derive(Deserialize)]
pub struct ResetRequest {
    pub email: String,
}

#[derive(Deserialize)]
pub struct CompleteResetRequest {
    pub token: String,
    pub password: String,
}

#[derive(Serialize)]
pub(crate) struct SessionContext {
    pub(crate) user: UserContext,
    pub(crate) workspace: WorkspaceContext,
}

#[derive(Serialize)]
pub(crate) struct UserContext {
    pub(crate) id: Uuid,
    pub(crate) email: String,
    pub(crate) name: String,
    pub(crate) role: String,
}

#[derive(Serialize)]
pub(crate) struct WorkspaceContext {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) slug: String,
    pub(crate) plan: String,
    pub(crate) usage: Usage,
}

#[derive(Serialize)]
pub(crate) struct Usage {
    pub(crate) sent: i64,
    pub(crate) limit: i64,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/auth/config", get(auth_config))
        .route("/v1/auth/signup", post(signup))
        .route("/v1/auth/login", post(login))
        .route("/v1/auth/logout", post(logout))
        .route("/v1/auth/session", get(session))
        .route("/v1/workspace", get(workspace))
        .route("/v1/auth/password-reset/request", post(request_reset))
        .route("/v1/auth/password-reset/complete", post(complete_reset))
}

async fn signup(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<SignupRequest>,
) -> Response {
    let ip = client_ip(peer.ip(), &headers, state.trust_proxy_headers);
    if let Err(response) = enforce_auth_limit(&state, &format!("signup:ip:{ip}"), 10).await {
        return response;
    }
    if state.signup_token.as_ref().is_some_and(|expected| {
        !::auth::token_matches(input.signup_token.as_deref().unwrap_or(""), expected)
    }) {
        return error(
            StatusCode::FORBIDDEN,
            "invite_required",
            "A valid private signup token is required",
        );
    }
    let email = input.email.trim().to_lowercase();
    let first = input.first_name.trim();
    let last = input.last_name.trim();
    if !valid_email(&email) || first.is_empty() || last.is_empty() {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Enter a valid name and email address",
        );
    }
    if input.password.len() < 12 || input.password.len() > 256 {
        return error(
            StatusCode::BAD_REQUEST,
            "weak_password",
            "Password must be between 12 and 256 characters",
        );
    }
    let display_name = format!("{first} {last}");
    let workspace_name = input
        .workspace_name
        .as_deref()
        .unwrap_or("My Workspace")
        .trim();
    if workspace_name.is_empty() || workspace_name.len() > 80 {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_workspace",
            "Workspace name is invalid",
        );
    }
    let password_hash = match password_hash_async(input.password.clone()).await {
        Ok(value) => value,
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to create account",
            )
        }
    };
    let token = generate_token();
    let token_hash = hash_token(&token);
    let slug = format!(
        "{}-{}",
        slugify(workspace_name),
        &Uuid::new_v4().simple().to_string()[..8]
    );
    let mut tx = match state.db.begin().await {
        Ok(value) => value,
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to create account",
            )
        }
    };
    let user_id = match sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO users (email, password_hash, display_name) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(&email)
    .bind(password_hash)
    .bind(&display_name)
    .fetch_one(&mut *tx)
    .await
    {
        Ok(value) => value,
        Err(db_error) if db_error.to_string().contains("duplicate key") => {
            return error(
                StatusCode::CONFLICT,
                "email_in_use",
                "An account already exists for this email",
            )
        }
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to create account",
            )
        }
    };
    let workspace_id = match sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO workspaces (name, slug, created_by) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(workspace_name)
    .bind(&slug)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    {
        Ok(value) => value,
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to create workspace",
            )
        }
    };
    if sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'owner')",
    )
    .bind(workspace_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .is_err()
    {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Unable to create workspace",
        );
    }
    if sqlx::query("INSERT INTO sessions (user_id, token_hash, expires_at) VALUES ($1, $2, now() + make_interval(days => $3::int))")
        .bind(user_id).bind(token_hash).bind(SESSION_DAYS).execute(&mut *tx).await.is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to create session");
    }
    if sqlx::query("INSERT INTO audit_events (workspace_id, actor_user_id, action) VALUES ($1, $2, 'account.created')")
        .bind(workspace_id).bind(user_id).execute(&mut *tx).await.is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to create account");
    }
    if tx.commit().await.is_err() {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Unable to create account",
        );
    }
    let body = Json(
        json!({"data": {"user": {"id": user_id, "email": email, "name": display_name, "role": "owner"}, "workspace": {"id": workspace_id, "name": workspace_name, "slug": slug, "plan": "free", "usage": {"sent": 0, "limit": 1000}}}}),
    );
    with_cookie(
        body.into_response(),
        cookie(&token, true, false, state.environment == "production"),
    )
}

async fn login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<LoginRequest>,
) -> Response {
    if input.password.len() > 256 || input.email.len() > 254 {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Invalid credentials",
        );
    }
    let email = input.email.trim().to_lowercase();
    let bucket = chrono::Utc::now()
        .with_second(0)
        .and_then(|v| v.with_nanosecond(0))
        .unwrap_or_else(chrono::Utc::now);
    let ip_key = format!(
        "login:ip:{}",
        client_ip(peer.ip(), &headers, state.trust_proxy_headers)
    );
    let email_key = format!("login:email:{}", email);
    for key in [ip_key, email_key] {
        let attempt = match sqlx::query_scalar::<_, i32>("INSERT INTO auth_rate_limits (bucket_key, bucket_start, attempt_count) VALUES ($1, $2, 1) ON CONFLICT (bucket_key, bucket_start) DO UPDATE SET attempt_count = auth_rate_limits.attempt_count + 1 RETURNING attempt_count")
            .bind(key).bind(bucket).fetch_one(&state.db).await {
            Ok(value) => value,
            Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable", "Unable to sign in right now"),
        };
        if attempt > 20 {
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_attempts",
                "Too many sign-in attempts; try again later",
            );
        }
    }
    let row = match sqlx::query(
        "SELECT id, email, display_name, password_hash FROM users WHERE lower(email) = $1",
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await
    {
        Ok(value) => value,
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to sign in",
            )
        }
    };
    let Some(row) = row else {
        let _ = password_verify_async(input.password.clone(), DUMMY_PASSWORD_HASH.clone()).await;
        return error(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "Email or password is incorrect",
        );
    };
    let password_hash: String = row.get("password_hash");
    if !password_verify_async(input.password.clone(), password_hash).await {
        return error(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "Email or password is incorrect",
        );
    }
    let user_id: Uuid = row.get("id");
    let token = generate_token();
    let remember = input.remember.unwrap_or(true);
    if sqlx::query("INSERT INTO sessions (user_id, token_hash, expires_at) VALUES ($1, $2, now() + make_interval(days => $3::int))")
        .bind(user_id).bind(hash_token(&token)).bind(if remember { SESSION_DAYS } else { 1 }).execute(&state.db).await.is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to sign in");
    }
    let context = match load_context(&state, &token).await {
        Ok(value) => value,
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to sign in",
            )
        }
    };
    with_cookie(
        Json(json!({"data": context})).into_response(),
        cookie(&token, remember, false, state.environment == "production"),
    )
}

async fn session(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(token) = read_cookie(&headers) else {
        return error(
            StatusCode::UNAUTHORIZED,
            "not_authenticated",
            "Authentication required",
        );
    };
    match load_context(&state, &token).await {
        Ok(context) => Json(json!({"data": context})).into_response(),
        Err(_) => error(
            StatusCode::UNAUTHORIZED,
            "not_authenticated",
            "Authentication required",
        ),
    }
}

async fn workspace(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let (id, _) = match super::api_keys::access(&state, &headers, "workspace:read", false).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match workspace_context(&state, id).await {
        Ok(v) => Json(json!({"data":v})).into_response(),
        Err(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "Unable to load workspace",
        ),
    }
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(token) = read_cookie(&headers) {
        if sqlx::query(
            "UPDATE sessions SET revoked_at = now() WHERE token_hash = $1 AND revoked_at IS NULL",
        )
        .bind(hash_token(&token))
        .execute(&state.db)
        .await
        .is_err()
        {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "database_unavailable",
                "Sign out failed; please retry",
            );
        }
    }
    with_cookie(
        Json(json!({"data": {"signedOut": true}})).into_response(),
        cookie("", false, true, state.environment == "production"),
    )
}

async fn request_reset(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<ResetRequest>,
) -> Response {
    if state.account_email_from.is_none() {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "recovery_unconfigured",
            "Account email delivery has not been configured by the operator",
        );
    }
    let email = input.email.trim().to_lowercase();
    let ip = client_ip(peer.ip(), &headers, state.trust_proxy_headers);
    if enforce_auth_limit(&state, &format!("reset:ip:{ip}"), 10)
        .await
        .is_err()
    {
        return Json(json!({"data": {"accepted": true}})).into_response();
    }
    if valid_email(&email) {
        if let Ok(Some(user_id)) =
            sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE lower(email) = $1")
                .bind(&email)
                .fetch_optional(&state.db)
                .await
        {
            let token = generate_token();
            let mut tx = match state.db.begin().await {
                Ok(tx) => tx,
                Err(_) => {
                    return error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "unavailable",
                        "Recovery temporarily unavailable",
                    )
                }
            };
            let link = format!(
                "{}/#/reset-password?token={}",
                state.console_origin.trim_end_matches('/'),
                token
            );
            let body = format!("Reset your Mailer password using this link (valid for one hour):\n\n{link}\n\nIf you did not request this, ignore this email.");
            if sqlx::query("INSERT INTO password_reset_tokens (user_id, token_hash, expires_at) VALUES ($1,$2,now()+interval '1 hour')").bind(user_id).bind(hash_token(&token)).execute(&mut *tx).await.is_err()
                || sqlx::query("INSERT INTO account_emails (recipient,subject,body,expires_at) VALUES ($1,'Reset your Mailer password',$2,now()+interval '1 hour')").bind(&email).bind(body).execute(&mut *tx).await.is_err()
                || tx.commit().await.is_err() {
                return error(StatusCode::SERVICE_UNAVAILABLE, "unavailable", "Recovery temporarily unavailable");
            }
        }
    }
    Json(json!({"data": {"accepted": true}})).into_response()
}

async fn complete_reset(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<CompleteResetRequest>,
) -> Response {
    let ip = client_ip(peer.ip(), &headers, state.trust_proxy_headers);
    if let Err(response) = enforce_auth_limit(&state, &format!("reset-complete:{ip}"), 10).await {
        return response;
    }
    if input.token.len() > 256 {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_reset_token",
            "Invalid reset token",
        );
    }
    let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM password_reset_tokens WHERE token_hash=$1 AND used_at IS NULL AND expires_at>now())")
        .bind(hash_token(&input.token)).fetch_one(&state.db).await.unwrap_or(false);
    if !valid {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_reset_token",
            "Reset link is invalid or expired",
        );
    }
    if input.password.len() < 12 || input.password.len() > 256 {
        return error(
            StatusCode::BAD_REQUEST,
            "weak_password",
            "Password must be between 12 and 256 characters",
        );
    }
    let password_hash = match password_hash_async(input.password.clone()).await {
        Ok(value) => value,
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to reset password",
            )
        }
    };
    let mut tx = match state.db.begin().await {
        Ok(value) => value,
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to reset password",
            )
        }
    };
    let user_id = match sqlx::query_scalar::<_, Uuid>("SELECT user_id FROM password_reset_tokens WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now() FOR UPDATE").bind(hash_token(&input.token)).fetch_optional(&mut *tx).await { Ok(Some(value)) => value, _ => return error(StatusCode::BAD_REQUEST, "invalid_reset_token", "Reset link is invalid or expired") };
    if sqlx::query("UPDATE users SET password_hash = $1, updated_at = now() WHERE id = $2")
        .bind(password_hash)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .is_err()
        || sqlx::query("UPDATE password_reset_tokens SET used_at = now() WHERE user_id = $1 AND used_at IS NULL")
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .is_err()
        || sqlx::query(
            "UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Unable to reset password",
        );
    }
    if tx.commit().await.is_err() {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Unable to reset password",
        );
    }
    Json(json!({"data": {"reset": true}})).into_response()
}

pub(crate) async fn load_context(
    state: &AppState,
    token: &str,
) -> Result<SessionContext, sqlx::Error> {
    let row = sqlx::query("SELECT u.id, u.email, u.display_name, wm.role, w.id AS workspace_id, w.name AS workspace_name, w.slug FROM sessions s JOIN users u ON u.id = s.user_id JOIN workspace_members wm ON wm.user_id = u.id JOIN workspaces w ON w.id = wm.workspace_id WHERE s.token_hash = $1 AND s.revoked_at IS NULL AND s.expires_at > now() ORDER BY wm.created_at ASC LIMIT 1")
        .bind(hash_token(token)).fetch_one(&state.db).await?;
    let user_id: Uuid = row.get("id");
    let role: String = row.get("role");
    let _ = sqlx::query("UPDATE sessions SET last_seen_at = now() WHERE token_hash = $1")
        .bind(hash_token(token))
        .execute(&state.db)
        .await;
    Ok(SessionContext {
        user: UserContext {
            id: user_id,
            email: row.get("email"),
            name: row.get("display_name"),
            role: if role == "member" {
                "developer".into()
            } else {
                role
            },
        },
        workspace: workspace_context(state, row.get("workspace_id")).await?,
    })
}

fn valid_email(email: &str) -> bool {
    email.len() <= 254
        && email.contains('@')
        && email
            .rsplit('@')
            .next()
            .is_some_and(|domain| domain.contains('.'))
}
fn slugify(value: &str) -> String {
    let slug: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let trimmed: String = slug.trim_matches('-').chars().take(40).collect();
    if trimmed.is_empty() {
        "workspace".into()
    } else {
        trimmed
    }
}
pub(crate) fn read_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|pair| {
            pair.strip_prefix(&format!("{SESSION_COOKIE}="))
                .or_else(|| pair.strip_prefix("__Host-cs_session="))
        })
        .map(str::to_owned)
}
fn cookie(token: &str, persistent: bool, clear: bool, production: bool) -> String {
    let secure = if production { "; Secure" } else { "" };
    let name = if production {
        "__Host-cs_session"
    } else {
        SESSION_COOKIE
    };
    let max_age = if clear {
        "Max-Age=0"
    } else if persistent {
        "Max-Age=2592000"
    } else {
        "Max-Age=86400"
    };
    format!("{name}={token}; Path=/; HttpOnly; SameSite=Lax; {max_age}{secure}")
}
fn with_cookie(mut response: Response, value: String) -> Response {
    if let Ok(header_value) = HeaderValue::from_str(&value) {
        response
            .headers_mut()
            .insert(header::SET_COOKIE, header_value);
    }
    response
}
async fn auth_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(
        json!({"data":{"inviteRequired":state.signup_token.is_some(),"passwordRecovery":state.account_email_from.is_some()}}),
    )
}

async fn workspace_context(state: &AppState, id: Uuid) -> Result<WorkspaceContext, sqlx::Error> {
    let row = sqlx::query("SELECT w.name,w.slug,COALESCE(u.emails_accepted,0)::bigint AS sent,l.monthly_email_limit FROM workspaces w LEFT JOIN usage_counters u ON u.workspace_id=w.id AND u.period_start=date_trunc('month',now())::date LEFT JOIN workspace_limits l ON l.workspace_id=w.id WHERE w.id=$1").bind(id).fetch_one(&state.db).await?;
    Ok(WorkspaceContext {
        id,
        name: row.get("name"),
        slug: row.get("slug"),
        plan: "private".into(),
        usage: Usage {
            sent: row.get("sent"),
            limit: row
                .get::<Option<i64>, _>("monthly_email_limit")
                .unwrap_or(state.workspace_monthly_email_limit as i64),
        },
    })
}

static PASSWORD_WORK: LazyLock<std::sync::Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| std::sync::Arc::new(tokio::sync::Semaphore::new(2)));
async fn password_hash_async(password: String) -> anyhow::Result<String> {
    let permit = PASSWORD_WORK.clone().acquire_owned().await?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        hash_password(&password)
    })
    .await?
}
async fn password_verify_async(password: String, hash: String) -> bool {
    let Ok(permit) = PASSWORD_WORK.clone().acquire_owned().await else {
        return false;
    };
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        verify_password(&password, &hash)
    })
    .await
    .unwrap_or(false)
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"code": code, "message": message}))).into_response()
}

async fn enforce_auth_limit(state: &AppState, key: &str, limit: i32) -> Result<(), Response> {
    let bucket = chrono::Utc::now()
        .with_second(0)
        .and_then(|value| value.with_nanosecond(0))
        .unwrap_or_else(chrono::Utc::now);
    let attempt = sqlx::query_scalar::<_, i32>("INSERT INTO auth_rate_limits (bucket_key, bucket_start, attempt_count) VALUES ($1, $2, 1) ON CONFLICT (bucket_key, bucket_start) DO UPDATE SET attempt_count = auth_rate_limits.attempt_count + 1 RETURNING attempt_count")
        .bind(key).bind(bucket).fetch_one(&state.db).await
        .map_err(|_| error(StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable", "Request cannot be processed right now"))?;
    if attempt > limit {
        return Err(error(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_attempts",
            "Too many attempts; try again later",
        ));
    }
    Ok(())
}
