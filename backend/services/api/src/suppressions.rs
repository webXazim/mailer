use super::{
    activity::{failure, Page},
    api_keys, AppState,
};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Deserialize)]
struct Create {
    address: String,
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/suppressions", get(list).post(create))
        .route("/v1/suppressions/{id}", delete(remove))
}
async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Response {
    let (workspace, _) = match api_keys::access(&state, &headers, "suppressions:manage", true).await
    {
        Ok(v) => v,
        Err(r) => return r,
    };
    let (limit, offset) = page.bounds();
    match sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('id',id,'address',address,'reason',reason,'createdAt',created_at) FROM suppressions WHERE workspace_id=$1 ORDER BY created_at DESC,id DESC LIMIT $2 OFFSET $3").bind(workspace).bind(limit+1).bind(offset).fetch_all(&state.db).await {
        Ok(mut rows)=>{let more=rows.len()>limit as usize;rows.truncate(limit as usize);Json(json!({"data":rows,"hasMore":more,"nextOffset":if more {Some(offset+limit)} else {None}})).into_response()},
        Err(_)=>failure(StatusCode::SERVICE_UNAVAILABLE,"database_unavailable","Unable to load suppressions"),
    }
}
async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Create>,
) -> Response {
    let (workspace, _) = match api_keys::access(&state, &headers, "suppressions:manage", true).await
    {
        Ok(v) => v,
        Err(r) => return r,
    };
    let address = input.address.trim().to_lowercase();
    if address.len() > 254 || !address.contains('@') || address.chars().any(char::is_whitespace) {
        return failure(
            StatusCode::BAD_REQUEST,
            "invalid_address",
            "Enter an email address",
        );
    }
    match sqlx::query_scalar::<_,Uuid>("INSERT INTO suppressions(workspace_id,address,reason) VALUES($1,$2,'manual') ON CONFLICT(workspace_id,lower(address)) DO UPDATE SET address=EXCLUDED.address RETURNING id").bind(workspace).bind(address).fetch_one(&state.db).await {
        Ok(id)=>Json(json!({"data":{"id":id}})).into_response(),Err(_)=>failure(StatusCode::SERVICE_UNAVAILABLE,"database_unavailable","Unable to add suppression"),
    }
}
async fn remove(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let (workspace, _) = match api_keys::access(&state, &headers, "suppressions:manage", true).await
    {
        Ok(v) => v,
        Err(r) => return r,
    };
    match sqlx::query("DELETE FROM suppressions WHERE id=$1 AND workspace_id=$2")
        .bind(id)
        .bind(workspace)
        .execute(&state.db)
        .await
    {
        Ok(r) if r.rows_affected() == 1 => Json(json!({"data":{"removed":true}})).into_response(),
        Ok(_) => failure(
            StatusCode::NOT_FOUND,
            "suppression_not_found",
            "Suppression not found",
        ),
        Err(_) => failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "Unable to remove suppression",
        ),
    }
}
