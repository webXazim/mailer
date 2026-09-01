use super::{api_keys, AppState};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

#[derive(Deserialize, Default)]
pub(crate) struct Page {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub environment: Option<String>,
}
impl Page {
    pub fn bounds(&self) -> (i64, i64) {
        (
            self.limit.unwrap_or(50).clamp(1, 100),
            self.offset.unwrap_or(0).clamp(0, 1_000_000),
        )
    }
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/emails", get(list))
        .route("/v1/emails/{id}", get(retrieve))
}
pub(crate) fn failure(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"code":code,"message":message}))).into_response()
}
async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Response {
    let (workspace, key_env) = match api_keys::access(&state, &headers, "emails:read", false).await
    {
        Ok(v) => v,
        Err(r) => return r,
    };
    if page
        .environment
        .as_deref()
        .is_some_and(|v| !matches!(v, "test" | "production"))
    {
        return failure(
            StatusCode::BAD_REQUEST,
            "invalid_environment",
            "Choose test or production",
        );
    }
    if key_env.is_some() && page.environment.is_some() && key_env != page.environment {
        return failure(
            StatusCode::FORBIDDEN,
            "environment_mismatch",
            "API key cannot read another environment",
        );
    }
    let environment = key_env.or(page.environment.clone());
    let (limit, offset) = page.bounds();
    let rows=sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('id',e.id,'environment',e.environment,'from',e.sender,'subject',e.subject,'status',e.status,'acceptedAt',e.accepted_at,'sentAt',e.sent_at,'completedAt',e.completed_at,'lastError',e.last_error,'metadata',e.metadata,'recipients',COALESCE((SELECT jsonb_agg(jsonb_build_object('address',r.address,'type',r.recipient_type,'status',r.status) ORDER BY r.address) FROM email_recipients r WHERE r.email_id=e.id),'[]'::jsonb)) FROM emails e WHERE e.workspace_id=$1 AND ($2::text IS NULL OR e.environment=$2) ORDER BY e.accepted_at DESC,e.id DESC LIMIT $3 OFFSET $4")
        .bind(workspace).bind(environment).bind(limit+1).bind(offset).fetch_all(&state.db).await;
    match rows {
        Ok(mut rows) => {
            let more = rows.len() > limit as usize;
            rows.truncate(limit as usize);
            Json(json!({"data":rows,"hasMore":more,"nextOffset":if more {Some(offset+limit)} else {None}})).into_response()
        }
        Err(_) => failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "Unable to load emails",
        ),
    }
}
async fn retrieve(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let (workspace, environment) =
        match api_keys::access(&state, &headers, "emails:read", false).await {
            Ok(v) => v,
            Err(r) => return r,
        };
    let row=match sqlx::query("SELECT * FROM emails WHERE id=$1 AND workspace_id=$2 AND ($3::text IS NULL OR environment=$3)").bind(id).bind(workspace).bind(environment).fetch_optional(&state.db).await {
        Ok(Some(row))=>row,Ok(None)=>return failure(StatusCode::NOT_FOUND,"email_not_found","Email not found"),Err(_)=>return failure(StatusCode::SERVICE_UNAVAILABLE,"database_unavailable","Unable to load email"),
    };
    let recipients=sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('address',address,'type',recipient_type,'status',status) FROM email_recipients WHERE email_id=$1 ORDER BY address").bind(id).fetch_all(&state.db).await;
    let events=sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('id',id,'type','email.'||event_type,'recipient',recipient,'occurredAt',occurred_at,'data',payload) FROM delivery_events WHERE email_id=$1 ORDER BY occurred_at DESC LIMIT 100").bind(id).fetch_all(&state.db).await;
    let (Ok(recipients), Ok(events)) = (recipients, events) else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "Unable to load activity",
        );
    };
    let mut content = json!({"text":row.get::<Option<String>,_>("text_body"),"html":row.get::<Option<String>,_>("html_body"),"attachments":[]});
    let mut content_available = row
        .get::<Option<chrono::DateTime<chrono::Utc>>, _>("content_deleted_at")
        .is_none();
    if let (Some(store), Some(key), Some(checksum)) = (
        &state.object_store,
        row.get::<Option<String>, _>("raw_object_key"),
        row.get::<Option<Vec<u8>>, _>("content_checksum"),
    ) {
        match store.get_verified(&key, &checksum).await {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(value) => content = value,
                Err(_) => content_available = false,
            },
            Err(_) => content_available = false,
        }
    }
    // Metadata only; do not embed attachment bytes in activity responses.
    if let Some(attachments) = content
        .get_mut("attachments")
        .and_then(|v| v.as_array_mut())
    {
        for item in attachments {
            if let Some(map) = item.as_object_mut() {
                map.remove("content");
            }
        }
    }
    Json(json!({"data":{"id":id,"environment":row.get::<String,_>("environment"),"from":row.get::<String,_>("sender"),"subject":row.get::<String,_>("subject"),"status":row.get::<String,_>("status"),"acceptedAt":row.get::<chrono::DateTime<chrono::Utc>,_>("accepted_at"),"sentAt":row.get::<Option<chrono::DateTime<chrono::Utc>>,_>("sent_at"),"completedAt":row.get::<Option<chrono::DateTime<chrono::Utc>>,_>("completed_at"),"lastError":row.get::<Option<String>,_>("last_error"),"providerMessageId":row.get::<Option<String>,_>("provider_message_id"),"metadata":row.get::<Value,_>("metadata"),"recipients":recipients,"events":events,"content":content,"contentAvailable":content_available}})).into_response()
}
