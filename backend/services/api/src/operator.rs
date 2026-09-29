//! VPS-only site controls. Customer workspace roles never authorize these routes.
use super::AppState;
use axum::{
    extract::{Form, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/operator/", get(index))
        .route("/operator/smtp", post(smtp))
        .route("/operator/workspace/{id}", post(workspace))
}

fn equal_secret(candidate: &str, expected: &str) -> bool {
    let a = Sha256::digest(candidate.as_bytes());
    let b = Sha256::digest(expected.as_bytes());
    a.iter()
        .zip(b.iter())
        .fold(0u8, |diff, (x, y)| diff | (*x ^ *y))
        == 0
}

fn authorize(state: &AppState, headers: &HeaderMap) -> Result<(), Response> {
    let Some(password) = state.operator_password.as_deref() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
    };
    if headers
        .get("x-mailer-operator-local")
        .and_then(|v| v.to_str().ok())
        != Some("1")
        || headers.get(header::HOST).and_then(|v| v.to_str().ok()) != Some("localhost:18085")
    {
        return Err(StatusCode::NOT_FOUND.into_response());
    }
    let valid = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|v| STANDARD.decode(v).ok())
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|v| {
            v.split_once(':')
                .map(|(user, pass)| user == "operator" && equal_secret(pass, password))
        })
        .unwrap_or(false);
    if valid {
        Ok(())
    } else {
        let mut response = StatusCode::UNAUTHORIZED.into_response();
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            "Basic realm=\"CS Mailer site operator\", charset=\"UTF-8\""
                .parse()
                .unwrap(),
        );
        Err(response)
    }
}

fn csrf(password: &str) -> String {
    hex::encode(Sha256::digest(format!(
        "cs-mailer-operator-csrf:{password}"
    )))
}

fn authorize_change(state: &AppState, headers: &HeaderMap, token: &str) -> Result<(), Response> {
    authorize(state, headers)?;
    if headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) != Some("http://localhost:18085")
        || !equal_secret(
            token,
            &csrf(state.operator_password.as_deref().unwrap_or_default()),
        )
    {
        return Err(StatusCode::FORBIDDEN.into_response());
    }
    Ok(())
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

async fn index(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = authorize(&state, &headers) {
        return response;
    }
    let control = sqlx::query("SELECT smtp_paused,smtp_daily_email_limit,updated_at::text AS updated_at FROM delivery_operator_controls WHERE singleton=true")
        .fetch_one(&state.db).await;
    let workspaces = sqlx::query("SELECT id,name,production_enabled,sending_paused_at IS NOT NULL AS paused FROM workspaces ORDER BY created_at DESC LIMIT 100")
        .fetch_all(&state.db).await;
    let (Ok(control), Ok(workspaces)) = (control, workspaces) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let paused: bool = control.get("smtp_paused");
    let cap: i64 = control.get("smtp_daily_email_limit");
    let updated: String = control.get("updated_at");
    let token = csrf(state.operator_password.as_deref().unwrap_or_default());
    let mut rows = String::new();
    for row in workspaces {
        let id: Uuid = row.get("id");
        let name: String = row.get("name");
        let enabled: bool = row.get("production_enabled");
        let stopped: bool = row.get("paused");
        let action = if stopped { "resume" } else { "pause" };
        rows.push_str(&format!("<tr><td>{}</td><td><code>{id}</code></td><td>{}</td><td>{}</td><td><form method=post action=\"/operator/workspace/{id}\"><input type=hidden name=csrf value=\"{token}\"><input type=hidden name=action value=\"{action}\"><button type=submit>{action}</button></form></td></tr>",
            escape(&name), if enabled { "Enabled" } else { "Pending" }, if stopped { "Paused" } else { "Active" }));
    }
    let html = format!(
        r#"<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>CS Mailer · Site operator</title><style>body{{font:16px system-ui;max-width:1100px;margin:2rem auto;padding:0 1rem;color:#172334}}h1{{font-size:1.8rem}}section{{border:1px solid #ccd4dc;border-radius:10px;padding:1.3rem;margin:1rem 0}}button,input{{font:inherit;padding:.5rem}}button{{cursor:pointer}}form{{display:inline-block;margin:.25rem}}table{{border-collapse:collapse;width:100%}}td,th{{border-bottom:1px solid #ddd;padding:.55rem;text-align:left}}code{{font-size:.8rem}}.scroll{{overflow-x:auto}}.warning{{color:#9a331c}}</style><main><h1>CS Mailer site operator</h1><p>Private SSH access · global sending and workspace containment</p><section><h2>SMTP delivery</h2><p>Status: <strong>{status}</strong> · Daily cap: <strong>{cap}</strong> · Last changed: {updated}</p><form method="post" action="/operator/smtp"><input type="hidden" name="csrf" value="{token}"><input type="hidden" name="action" value="{toggle}"><button type="submit">{toggle_label}</button></form><form method="post" action="/operator/smtp"><input type="hidden" name="csrf" value="{token}"><input type="hidden" name="action" value="cap"><label>Daily cap <input type="number" name="limit" min="1" max="100000000" value="{cap}" required></label><button type="submit">Set cap</button></form></section><section><h2>Workspaces</h2><p>Showing newest 100. Pause blocks production sends for a workspace. Resume releases eligible queued mail.</p><div class="scroll"><table><thead><tr><th>Name</th><th>ID</th><th>Production</th><th>Sending</th><th>Action</th></tr></thead><tbody>{rows}</tbody></table></div></section><p class="warning">Changes take effect immediately and are audited. Check the workspace before resuming sending.</p></main></html>"#,
        status = if paused { "Paused" } else { "Active" },
        updated = escape(&updated),
        toggle = if paused { "resume" } else { "pause" },
        toggle_label = if paused { "Resume SMTP" } else { "Pause SMTP" }
    );
    Html(html).into_response()
}

#[derive(Deserialize)]
struct SmtpForm {
    csrf: String,
    action: String,
    limit: Option<i64>,
}

async fn smtp(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(input): Form<SmtpForm>,
) -> Response {
    if let Err(response) = authorize_change(&state, &headers, &input.csrf) {
        return response;
    }
    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    if sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('delivery-routing-control',0))")
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let result = match input.action.as_str() {
        "pause" | "resume" => {
            let paused = input.action == "pause";
            sqlx::query("UPDATE delivery_operator_controls SET smtp_paused=$1,updated_at=now() WHERE singleton=true")
                .bind(paused).execute(&mut *tx).await
        }
        "cap" if input.limit.is_some_and(|v| (1..=100_000_000).contains(&v)) => {
            sqlx::query("UPDATE delivery_operator_controls SET smtp_daily_email_limit=$1,updated_at=now() WHERE singleton=true")
                .bind(input.limit.unwrap()).execute(&mut *tx).await
        }
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    if result.is_err() || sqlx::query("INSERT INTO delivery_control_audit(action,details) VALUES($1,jsonb_build_object('source','ssh_operator','limit',$2))")
        .bind(format!("smtp-{}", input.action)).bind(input.limit).execute(&mut *tx).await.is_err()
        || tx.commit().await.is_err() { return StatusCode::SERVICE_UNAVAILABLE.into_response(); }
    Redirect::to("/operator/").into_response()
}

#[derive(Deserialize)]
struct WorkspaceForm {
    csrf: String,
    action: String,
}

async fn workspace(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Form(input): Form<WorkspaceForm>,
) -> Response {
    if let Err(response) = authorize_change(&state, &headers, &input.csrf) {
        return response;
    }
    if !matches!(input.action.as_str(), "pause" | "resume") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let paused = input.action == "pause";
    let updated = sqlx::query("UPDATE workspaces SET sending_paused_at=CASE WHEN $2 THEN COALESCE(sending_paused_at,now()) ELSE NULL END,sending_pause_reason=CASE WHEN $2 THEN COALESCE(sending_pause_reason,'manual_operator_pause') ELSE NULL END,sending_paused_by=CASE WHEN $2 THEN COALESCE(sending_paused_by,'ssh_operator') ELSE NULL END,updated_at=now() WHERE id=$1")
        .bind(id).bind(paused).execute(&mut *tx).await;
    let Ok(updated) = updated else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if updated.rows_affected() == 0 {
        return StatusCode::NOT_FOUND.into_response();
    }
    let action = if paused {
        "security.workspace_paused"
    } else {
        "security.workspace_resumed"
    };
    if sqlx::query("INSERT INTO audit_events(workspace_id,action,resource_type,resource_id,metadata) VALUES($1,$2,'workspace',$1,jsonb_build_object('source','ssh_operator'))")
        .bind(id).bind(action).execute(&mut *tx).await.is_err() { return StatusCode::SERVICE_UNAVAILABLE.into_response(); }
    if !paused && sqlx::query("INSERT INTO outbox_events(aggregate_type,aggregate_id,event_type,payload) SELECT 'email',email.id,'email.accepted',jsonb_build_object('emailId',email.id,'workspaceId',email.workspace_id) FROM emails email WHERE email.workspace_id=$1 AND email.environment='production' AND email.status='queued' AND NOT EXISTS(SELECT 1 FROM delivery_provider_attempts attempt WHERE attempt.email_id=email.id)")
        .bind(id).execute(&mut *tx).await.is_err() { return StatusCode::SERVICE_UNAVAILABLE.into_response(); }
    if tx.commit().await.is_err() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    Redirect::to("/operator/").into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credential_comparison_and_html_escape() {
        assert!(equal_secret("correct", "correct"));
        assert!(!equal_secret("wrong", "correct"));
        assert_eq!(escape("<a&\""), "&lt;a&amp;&quot;");
    }
}
