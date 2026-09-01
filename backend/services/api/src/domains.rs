use super::AppState;
use aws_sdk_sesv2::error::ProvideErrorMetadata;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use trust_dns_resolver::{
    config::{ResolverConfig, ResolverOpts},
    TokioAsyncResolver,
};
use uuid::Uuid;

#[derive(Deserialize)]
struct AddDomainRequest {
    domain: String,
}

#[derive(Serialize)]
struct DomainView {
    id: Uuid,
    domain: String,
    status: String,
    verified_at: Option<String>,
    created_at: String,
    records: Vec<RecordView>,
}

#[derive(Serialize)]
struct RecordView {
    record_type: String,
    name: String,
    value: String,
    status: String,
    last_checked_at: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/domains", get(list_domains).post(add_domain))
        .route("/v1/domains/{id}", get(get_domain).delete(delete_domain))
        .route("/v1/domains/{id}/verify", post(verify_domain))
}

async fn add_domain(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<AddDomainRequest>,
) -> Response {
    let workspace_id = match workspace_id(&state, &headers, true).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(domain) = normalize_domain(&input.domain) else {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_domain",
            "Enter a valid domain name",
        );
    };
    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "database_unavailable",
                "Unable to add domain",
            )
        }
    };
    if sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("domain:{domain}"))
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "Unable to reserve domain",
        );
    }
    match sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM domains WHERE lower(name) = $1 AND status <> 'disabled'",
    )
    .bind(&domain)
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(Some(_)) => {
            return error(
                StatusCode::CONFLICT,
                "domain_exists",
                "This domain is already connected to CrescentSphere Mailer",
            )
        }
        Ok(None) => {}
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to check domain availability",
            )
        }
    }
    let Some(ses) = &state.ses else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "domain_provider_unavailable",
            "Domain verification is not configured in this environment",
        );
    };
    let dkim_tokens = match ses
        .create_email_identity()
        .email_identity(&domain)
        .send()
        .await
    {
        Ok(identity) => identity
            .dkim_attributes()
            .map(|v| v.tokens().to_vec())
            .unwrap_or_default(),
        Err(error_value)
            if error_value.as_service_error().and_then(|v| v.code())
                == Some("AlreadyExistsException") =>
        {
            // Reconcile an earlier successful provider call whose DB commit failed.
            match ses
                .get_email_identity()
                .email_identity(&domain)
                .send()
                .await
            {
                Ok(identity) => identity
                    .dkim_attributes()
                    .map(|v| v.tokens().to_vec())
                    .unwrap_or_default(),
                Err(_) => {
                    return error(
                        StatusCode::BAD_GATEWAY,
                        "provider_error",
                        "Unable to reconcile sending identity",
                    )
                }
            }
        }
        Err(_) => {
            return error(
                StatusCode::BAD_GATEWAY,
                "provider_error",
                "Unable to create sending identity",
            )
        }
    };
    if dkim_tokens.is_empty() {
        return error(
            StatusCode::BAD_GATEWAY,
            "provider_error",
            "The provider did not return DKIM records",
        );
    }
    let mail_from = format!("bounce.{domain}");
    if let Err(provider_error) = ses
        .put_email_identity_mail_from_attributes()
        .email_identity(&domain)
        .mail_from_domain(&mail_from)
        .send()
        .await
    {
        tracing::error!(error = %provider_error, domain = %domain, "SES MAIL FROM configuration failed");
        return error(
            StatusCode::BAD_GATEWAY,
            "provider_error",
            "Unable to configure the sending identity",
        );
    }
    let row = match sqlx::query(
        "INSERT INTO domains (workspace_id, name) VALUES ($1, $2) RETURNING id, status, created_at",
    )
    .bind(workspace_id)
    .bind(&domain)
    .fetch_one(&mut *tx)
    .await
    {
        Ok(value) => value,
        Err(db_error) if db_error.to_string().contains("duplicate key") => {
            return error(
                StatusCode::CONFLICT,
                "domain_exists",
                "This domain is already connected to the workspace",
            )
        }
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to add domain",
            )
        }
    };
    let domain_id: Uuid = row.get("id");
    let mut records = dns_records(&domain, &state.aws_region, &dkim_tokens);
    records.push((
        "TXT".into(),
        format!("_mailer-verification.{domain}"),
        format!("mailer-verification={domain_id}"),
        true,
    ));
    for (record_type, name, value, required) in &records {
        if sqlx::query("INSERT INTO domain_dns_records (domain_id, record_type, name, value, required_for_sending) VALUES ($1, $2, $3, $4, $5)")
            .bind(domain_id).bind(record_type).bind(name).bind(value).bind(required).execute(&mut *tx).await.is_err() {
            return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to create DNS records");
        }
    }
    if tx.commit().await.is_err() {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Unable to add domain",
        );
    }
    let view = DomainView {
        id: domain_id,
        domain,
        status: row.get("status"),
        verified_at: None,
        created_at: row
            .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
            .to_rfc3339(),
        records: records
            .into_iter()
            .map(|(record_type, name, value, _)| RecordView {
                record_type,
                name,
                value,
                status: "pending".into(),
                last_checked_at: None,
            })
            .collect(),
    };
    (StatusCode::CREATED, Json(json!({"data": view}))).into_response()
}

async fn list_domains(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let workspace_id = match workspace_id(&state, &headers, false).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let rows = match sqlx::query("SELECT id, name, status, verified_at, created_at FROM domains WHERE workspace_id = $1 AND status <> 'disabled' ORDER BY created_at DESC").bind(workspace_id).fetch_all(&state.db).await { Ok(value) => value, Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to list domains") };
    let mut domains = Vec::with_capacity(rows.len());
    for row in rows {
        match domain_view(&state, row).await {
            Ok(view) => domains.push(view),
            Err(_) => {
                return error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "Unable to load domains",
                )
            }
        }
    }
    Json(json!({"data": domains})).into_response()
}

async fn get_domain(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let workspace_id = match workspace_id(&state, &headers, false).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let row = match sqlx::query("SELECT id, name, status, verified_at, created_at FROM domains WHERE id = $1 AND workspace_id = $2 AND status <> 'disabled'").bind(id).bind(workspace_id).fetch_optional(&state.db).await { Ok(Some(value)) => value, Ok(None) => return error(StatusCode::NOT_FOUND, "domain_not_found", "Domain was not found"), Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to load domain") };
    match domain_view(&state, row).await {
        Ok(view) => Json(json!({"data": view})).into_response(),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Unable to load domain",
        ),
    }
}

async fn delete_domain(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let workspace_id = match workspace_id(&state, &headers, true).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    // Disabling Mailer must not delete an SES identity shared with another application.
    match sqlx::query("UPDATE domains SET status = 'disabled', updated_at = now() WHERE id = $1 AND workspace_id = $2 AND status <> 'disabled'").bind(id).bind(workspace_id).execute(&state.db).await { Ok(result) if result.rows_affected() == 1 => Json(json!({"data": {"disabled": true}})).into_response(), Ok(_) => error(StatusCode::NOT_FOUND, "domain_not_found", "Domain was not found"), Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to remove domain") }
}

async fn verify_domain(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let workspace_id = match workspace_id(&state, &headers, true).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let row = match sqlx::query(
        "SELECT id, name FROM domains WHERE id = $1 AND workspace_id = $2 AND status <> 'disabled'",
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
                "domain_not_found",
                "Domain was not found",
            )
        }
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Unable to verify domain",
            )
        }
    };
    let name: String = row.get("name");
    let Some(ses) = &state.ses else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "domain_provider_unavailable",
            "Domain verification is not configured in this environment",
        );
    };
    let provider_verified = match ses.get_email_identity().email_identity(&name).send().await {
        Ok(value) => value.verified_for_sending_status(),
        Err(provider_error) => {
            tracing::error!(error = %provider_error, domain = %name, "SES identity status failed");
            return error(
                StatusCode::BAD_GATEWAY,
                "provider_error",
                "Unable to check the sending identity",
            );
        }
    };
    let resolver = TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default());
    let expected = match sqlx::query("SELECT id, record_type, name, value, required_for_sending FROM domain_dns_records WHERE domain_id = $1").bind(id).fetch_all(&state.db).await { Ok(value) => value, Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "Unable to verify DNS records") };
    let mut required_dns_verified = true;
    for record in expected {
        let record_id: Uuid = record.get("id");
        let record_type: String = record.get("record_type");
        let record_name: String = record.get("name");
        let record_value: String = record.get("value");
        let required: bool = record.get("required_for_sending");
        let found = dns_record_exists(&resolver, &record_type, &record_name, &record_value).await;
        let status = if found { "verified" } else { "pending" };
        if required && !found {
            required_dns_verified = false;
        }
        let _ = sqlx::query(
            "UPDATE domain_dns_records SET status = $1, last_checked_at = now() WHERE id = $2",
        )
        .bind(status)
        .bind(record_id)
        .execute(&state.db)
        .await;
    }
    let verified = provider_verified && required_dns_verified;
    let domain_status = if verified { "verified" } else { "pending" };
    let provider_status = if provider_verified {
        "verified"
    } else {
        "pending"
    };
    if sqlx::query("UPDATE domains SET status = $1, provider_status = $2, verified_at = CASE WHEN $1 = 'verified' THEN now() ELSE NULL END, updated_at = now() WHERE id = $3").bind(domain_status).bind(provider_status).bind(id).execute(&state.db).await.is_err() { return error(StatusCode::SERVICE_UNAVAILABLE,"database_unavailable","Unable to store verification result"); }
    Json(json!({"data": {"domain": name, "status": domain_status, "verified": verified, "providerVerified": provider_verified, "requiredDnsVerified": required_dns_verified}})).into_response()
}

async fn domain_view(
    state: &AppState,
    row: sqlx::postgres::PgRow,
) -> Result<DomainView, sqlx::Error> {
    let id: Uuid = row.get("id");
    let records = sqlx::query("SELECT record_type, name, value, status, last_checked_at FROM domain_dns_records WHERE domain_id = $1 ORDER BY record_type, name").bind(id).fetch_all(&state.db).await?;
    Ok(DomainView {
        id,
        domain: row.get("name"),
        status: row.get("status"),
        verified_at: row
            .get::<Option<chrono::DateTime<chrono::Utc>>, _>("verified_at")
            .map(|value| value.to_rfc3339()),
        created_at: row
            .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
            .to_rfc3339(),
        records: records
            .into_iter()
            .map(|record| RecordView {
                record_type: record.get("record_type"),
                name: record.get("name"),
                value: record.get("value"),
                status: record.get("status"),
                last_checked_at: record
                    .get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_checked_at")
                    .map(|value| value.to_rfc3339()),
            })
            .collect(),
    })
}

async fn dns_record_exists(
    resolver: &TokioAsyncResolver,
    record_type: &str,
    name: &str,
    expected: &str,
) -> bool {
    match record_type {
        "TXT" | "SPF" | "DMARC" => resolver.txt_lookup(name).await.ok().is_some_and(|lookup| {
            lookup.iter().any(|txt| {
                txt.txt_data()
                    .iter()
                    .map(|part| String::from_utf8_lossy(part))
                    .collect::<String>()
                    .trim_matches('"')
                    == expected.trim_matches('"')
            })
        }),
        "CNAME" => resolver
            .lookup(name, trust_dns_resolver::proto::rr::RecordType::CNAME)
            .await
            .ok()
            .is_some_and(|lookup| {
                lookup.iter().any(|record| {
                    record
                        .to_string()
                        .trim_end_matches('.')
                        .eq_ignore_ascii_case(expected.trim_end_matches('.'))
                })
            }),
        "MX" => resolver.mx_lookup(name).await.ok().is_some_and(|lookup| {
            lookup.iter().any(|record| {
                record
                    .exchange()
                    .to_utf8()
                    .trim_end_matches('.')
                    .eq_ignore_ascii_case(expected.trim_end_matches('.'))
            })
        }),
        _ => true,
    }
}

fn dns_records(
    domain: &str,
    region: &str,
    dkim_tokens: &[String],
) -> Vec<(String, String, String, bool)> {
    let mut records: Vec<_> = dkim_tokens
        .iter()
        .map(|token| {
            (
                "CNAME".into(),
                format!("{token}._domainkey.{domain}"),
                format!("{token}.dkim.amazonses.com"),
                true,
            )
        })
        .collect();
    records.push((
        "MX".into(),
        format!("bounce.{domain}"),
        format!("feedback-smtp.{region}.amazonses.com"),
        true,
    ));
    records.push((
        "SPF".into(),
        format!("bounce.{domain}"),
        "v=spf1 include:amazonses.com ~all".into(),
        true,
    ));
    records.push((
        "DMARC".into(),
        format!("_dmarc.{domain}"),
        "v=DMARC1; p=none".into(),
        false,
    ));
    records
}

fn normalize_domain(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('.').to_ascii_lowercase();
    if value.is_empty()
        || value.len() > 253
        || value.contains("..")
        || value.split('.').count() < 2
        || value.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                || label.starts_with('-')
                || label.ends_with('-')
        })
    {
        None
    } else {
        Some(value)
    }
}

async fn workspace_id(
    state: &AppState,
    headers: &HeaderMap,
    require_admin: bool,
) -> Result<Uuid, Response> {
    super::api_keys::access(
        state,
        headers,
        if require_admin {
            "domains:write"
        } else {
            "domains:read"
        },
        require_admin,
    )
    .await
    .map(|v| v.0)
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"code": code, "message": message}))).into_response()
}

#[cfg(test)]
mod tests {
    use super::normalize_domain;

    #[test]
    fn normalizes_valid_domains() {
        assert_eq!(
            normalize_domain(" Mail.Example.COM. "),
            Some("mail.example.com".into())
        );
    }

    #[test]
    fn rejects_invalid_domains() {
        for domain in [
            "localhost",
            "-mail.example.com",
            "mail..example.com",
            "mail_example.com",
        ] {
            assert_eq!(normalize_domain(domain), None, "{domain} must be rejected");
        }
    }
}
