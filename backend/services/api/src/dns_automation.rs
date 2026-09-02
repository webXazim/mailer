use super::AppState;
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, Method, Request, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use http_body_util::{BodyExt, Full};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

const CLOUDFLARE_API: &str = "https://api.cloudflare.com/client/v4";
const CLOUDFLARE_AUTH: &str = "https://dash.cloudflare.com/oauth2/auth";
const CLOUDFLARE_TOKEN: &str = "https://dash.cloudflare.com/oauth2/token";
const CLOUDFLARE_REVOKE: &str = "https://dash.cloudflare.com/oauth2/revoke";

#[derive(Deserialize)]
struct OAuthCallback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Deserialize)]
struct CloudflareEnvelope<T> {
    success: bool,
    result: T,
    #[serde(default)]
    errors: Vec<CloudflareError>,
    #[serde(default)]
    result_info: Option<CloudflareResultInfo>,
}

#[derive(Deserialize)]
struct CloudflareResultInfo {
    total_pages: Option<u32>,
}

#[derive(Deserialize)]
struct CloudflareError {
    message: String,
}

#[derive(Deserialize)]
struct CloudflareZone {
    id: String,
    name: String,
}

#[derive(Deserialize)]
struct CloudflareRecord {
    #[serde(rename = "type")]
    record_type: String,
    name: String,
    content: String,
    priority: Option<u16>,
}

#[derive(Serialize)]
struct CreateRecord<'a> {
    #[serde(rename = "type")]
    record_type: &'a str,
    name: &'a str,
    content: &'a str,
    ttl: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    proxied: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<u16>,
    comment: &'static str,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/domains/{id}/dns-automation/cloudflare",
            post(start_cloudflare),
        )
        .route(
            "/v1/dns-automation/cloudflare/callback",
            get(cloudflare_callback),
        )
}

async fn start_cloudflare(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(domain_id): Path<Uuid>,
) -> Response {
    let workspace_id = match super::api_keys::access(&state, &headers, "domains:write", true).await
    {
        Ok(value) => value.0,
        Err(response) => return response,
    };
    let (Some(client_id), Some(_)) = (
        state.cloudflare_oauth_client_id.as_deref(),
        state.cloudflare_oauth_client_secret.as_deref(),
    ) else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "dns_automation_unavailable",
            "Automatic Cloudflare setup is not configured yet",
        );
    };
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM domains WHERE id=$1 AND workspace_id=$2 AND status <> 'disabled')",
    )
    .bind(domain_id)
    .bind(workspace_id)
    .fetch_one(&state.db)
    .await;
    if !matches!(exists, Ok(true)) {
        return error(
            StatusCode::NOT_FOUND,
            "domain_not_found",
            "Domain was not found",
        );
    }

    let raw_state = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let state_hash = Sha256::digest(raw_state.as_bytes()).to_vec();
    let _ = sqlx::query("DELETE FROM dns_oauth_states WHERE expires_at < now()-interval '1 day'")
        .execute(&state.db)
        .await;
    if sqlx::query("INSERT INTO dns_oauth_states (workspace_id,domain_id,provider,state_hash,expires_at) VALUES ($1,$2,'cloudflare',$3,now()+interval '10 minutes')")
        .bind(workspace_id)
        .bind(domain_id)
        .bind(state_hash)
        .execute(&state.db)
        .await
        .is_err()
    {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "Unable to start DNS authorization",
        );
    }
    let redirect_uri = callback_url(&state);
    let mut authorization = match url::Url::parse(CLOUDFLARE_AUTH) {
        Ok(value) => value,
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to start DNS authorization",
            )
        }
    };
    authorization
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("scope", &state.cloudflare_oauth_scopes)
        .append_pair("state", &raw_state);
    Json(json!({"data":{"authorizationUrl":authorization.as_str()}})).into_response()
}

async fn cloudflare_callback(
    State(state): State<AppState>,
    Query(input): Query<OAuthCallback>,
) -> Response {
    let Some(raw_state) = input.state.filter(|value| value.len() <= 256) else {
        return redirect_result(&state, "failed");
    };
    let state_hash = Sha256::digest(raw_state.as_bytes()).to_vec();
    let row = match sqlx::query("UPDATE dns_oauth_states SET used_at=now() WHERE state_hash=$1 AND provider='cloudflare' AND used_at IS NULL AND expires_at>now() RETURNING domain_id")
        .bind(state_hash)
        .fetch_optional(&state.db)
        .await
    {
        Ok(Some(value)) => value,
        _ => return redirect_result(&state, "expired"),
    };
    let domain_id: Uuid = row.get("domain_id");
    if input.error.is_some() {
        return redirect_result(&state, "cancelled");
    }
    let Some(code) = input
        .code
        .filter(|value| !value.is_empty() && value.len() <= 4096)
    else {
        return redirect_result(&state, "failed");
    };
    match authorize_and_publish(&state, domain_id, &code).await {
        Ok(()) => {
            let _ = sqlx::query("UPDATE domains SET updated_at=now() WHERE id=$1")
                .bind(domain_id)
                .execute(&state.db)
                .await;
            redirect_result(&state, "published")
        }
        Err(provider_error) => {
            tracing::warn!(error=%provider_error, %domain_id, "automatic Cloudflare DNS setup failed");
            redirect_result(&state, "failed")
        }
    }
}

async fn authorize_and_publish(
    state: &AppState,
    domain_id: Uuid,
    code: &str,
) -> anyhow::Result<()> {
    let client_id = state
        .cloudflare_oauth_client_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Cloudflare OAuth client ID is missing"))?;
    let client_secret = state
        .cloudflare_oauth_client_secret
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Cloudflare OAuth client secret is missing"))?;
    let redirect_uri = callback_url(state);
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "authorization_code")
        .append_pair("code", code)
        .append_pair("redirect_uri", &redirect_uri)
        .finish();
    let basic = BASE64.encode(format!("{client_id}:{client_secret}"));
    let (status, bytes) = http_request(
        Method::POST,
        CLOUDFLARE_TOKEN,
        &[
            (
                header::CONTENT_TYPE,
                "application/x-www-form-urlencoded".into(),
            ),
            (header::AUTHORIZATION, format!("Basic {basic}")),
        ],
        body.into_bytes(),
    )
    .await?;
    if !status.is_success() {
        anyhow::bail!("Cloudflare rejected the authorization code ({status})");
    }
    let token: TokenResponse = serde_json::from_slice(&bytes)?;
    let publish_result = publish_records(state, domain_id, &token.access_token).await;
    revoke_token(client_id, client_secret, &token.access_token).await;
    publish_result
}

async fn publish_records(state: &AppState, domain_id: Uuid, token: &str) -> anyhow::Result<()> {
    let domain: String =
        sqlx::query_scalar("SELECT name FROM domains WHERE id=$1 AND status <> 'disabled'")
            .bind(domain_id)
            .fetch_one(&state.db)
            .await?;
    let mut available_zones = Vec::new();
    let mut page = 1;
    loop {
        let zones: CloudflareEnvelope<Vec<CloudflareZone>> = cloudflare_json(
            Method::GET,
            &format!("{CLOUDFLARE_API}/zones?per_page=50&page={page}"),
            token,
            None,
        )
        .await?;
        ensure_cloudflare_success(&zones)?;
        let total_pages = zones
            .result_info
            .as_ref()
            .and_then(|value| value.total_pages)
            .unwrap_or(1);
        available_zones.extend(zones.result);
        if page >= total_pages {
            break;
        }
        page += 1;
    }
    let zone = available_zones
        .into_iter()
        .filter(|zone| domain == zone.name || domain.ends_with(&format!(".{}", zone.name)))
        .max_by_key(|zone| zone.name.len())
        .ok_or_else(|| {
            anyhow::anyhow!("The authorized Cloudflare account does not contain {domain}")
        })?;
    let records = sqlx::query("SELECT record_type,name,value FROM domain_dns_records WHERE domain_id=$1 ORDER BY required_for_sending DESC,record_type,name")
        .bind(domain_id)
        .fetch_all(&state.db)
        .await?;
    for record in records {
        let stored_type: String = record.get("record_type");
        let record_type = if matches!(stored_type.as_str(), "SPF" | "DMARC") {
            "TXT"
        } else {
            stored_type.as_str()
        };
        let name: String = record.get("name");
        let value: String = record.get("value");
        create_record_if_missing(&zone.id, token, record_type, &name, &value, &stored_type).await?;
    }
    Ok(())
}

async fn create_record_if_missing(
    zone_id: &str,
    token: &str,
    record_type: &str,
    name: &str,
    value: &str,
    stored_type: &str,
) -> anyhow::Result<()> {
    let mut url = url::Url::parse(&format!("{CLOUDFLARE_API}/zones/{zone_id}/dns_records"))?;
    url.query_pairs_mut()
        .append_pair("name", name)
        .append_pair("per_page", "100");
    let existing: CloudflareEnvelope<Vec<CloudflareRecord>> =
        cloudflare_json(Method::GET, url.as_str(), token, None).await?;
    ensure_cloudflare_success(&existing)?;
    if existing.result.iter().any(|record| {
        record.record_type.eq_ignore_ascii_case(record_type)
            && record.name.eq_ignore_ascii_case(name)
            && record
                .content
                .trim_end_matches('.')
                .eq_ignore_ascii_case(value.trim_end_matches('.'))
            && (record_type != "MX" || record.priority == Some(10))
    }) {
        return Ok(());
    }
    let conflict = existing.result.iter().any(|record| match stored_type {
        "CNAME" => record.name.eq_ignore_ascii_case(name),
        "MX" => record.record_type == "MX" && record.name.eq_ignore_ascii_case(name),
        "SPF" => {
            record.record_type == "TXT"
                && record.name.eq_ignore_ascii_case(name)
                && record.content.to_ascii_lowercase().starts_with("v=spf1")
        }
        "DMARC" => {
            record.record_type == "TXT"
                && record.name.eq_ignore_ascii_case(name)
                && record.content.to_ascii_lowercase().starts_with("v=dmarc1")
        }
        _ => false,
    });
    if conflict {
        anyhow::bail!(
            "A conflicting {stored_type} record already exists at {name}; it was left unchanged"
        );
    }
    let payload = serde_json::to_vec(&CreateRecord {
        record_type,
        name,
        content: value,
        ttl: 1,
        proxied: (record_type == "CNAME").then_some(false),
        priority: (record_type == "MX").then_some(10),
        comment: "Managed by CrescentSphere Mailer",
    })?;
    let created: CloudflareEnvelope<serde_json::Value> = cloudflare_json(
        Method::POST,
        &format!("{CLOUDFLARE_API}/zones/{zone_id}/dns_records"),
        token,
        Some(payload),
    )
    .await?;
    ensure_cloudflare_success(&created)
}

fn ensure_cloudflare_success<T>(response: &CloudflareEnvelope<T>) -> anyhow::Result<()> {
    if response.success {
        Ok(())
    } else {
        let message = response
            .errors
            .first()
            .map(|error| error.message.as_str())
            .unwrap_or("Cloudflare API request failed");
        anyhow::bail!(message.to_owned())
    }
}

async fn cloudflare_json<T: serde::de::DeserializeOwned>(
    method: Method,
    url: &str,
    token: &str,
    body: Option<Vec<u8>>,
) -> anyhow::Result<T> {
    let mut headers = vec![(header::AUTHORIZATION, format!("Bearer {token}"))];
    if body.is_some() {
        headers.push((header::CONTENT_TYPE, "application/json".into()));
    }
    let (status, bytes) = http_request(method, url, &headers, body.unwrap_or_default()).await?;
    if !status.is_success() {
        anyhow::bail!("Cloudflare API returned {status}");
    }
    Ok(serde_json::from_slice(&bytes)?)
}

async fn revoke_token(client_id: &str, client_secret: &str, token: &str) {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("token", token)
        .finish();
    let basic = BASE64.encode(format!("{client_id}:{client_secret}"));
    if let Err(revoke_error) = http_request(
        Method::POST,
        CLOUDFLARE_REVOKE,
        &[
            (
                header::CONTENT_TYPE,
                "application/x-www-form-urlencoded".into(),
            ),
            (header::AUTHORIZATION, format!("Basic {basic}")),
        ],
        body.into_bytes(),
    )
    .await
    {
        tracing::warn!(error=%revoke_error, "unable to revoke one-time Cloudflare OAuth token");
    }
}

async fn http_request(
    method: Method,
    url: &str,
    headers: &[(header::HeaderName, String)],
    body: Vec<u8>,
) -> anyhow::Result<(StatusCode, Vec<u8>)> {
    let connector = HttpsConnectorBuilder::new()
        .with_native_roots()?
        .https_only()
        .enable_http1()
        .build();
    let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new()).build(connector);
    let mut builder = Request::builder().method(method).uri(url);
    for (name, value) in headers {
        builder = builder.header(name, value);
    }
    let request = builder.body(Full::new(Bytes::from(body)))?;
    let response =
        tokio::time::timeout(std::time::Duration::from_secs(15), client.request(request)).await??;
    let status = response.status();
    let bytes = response.into_body().collect().await?.to_bytes();
    Ok((status, bytes.to_vec()))
}

fn callback_url(state: &AppState) -> String {
    format!(
        "{}/api/v1/dns-automation/cloudflare/callback",
        state.console_origin.trim_end_matches('/')
    )
}

fn redirect_result(state: &AppState, result: &str) -> Response {
    let target = format!(
        "{}/#/domains?dns={result}",
        state.console_origin.trim_end_matches('/')
    );
    Redirect::to(&target).into_response()
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"code":code,"message":message}))).into_response()
}
