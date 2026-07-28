use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use corqty_persistence::models::SessionRecord;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error::ApiError, state::AppState};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditQuery {
    before_id: Option<i64>,
    limit: Option<i64>,
}

#[derive(Serialize)]
pub struct AuditResponse {
    items: Vec<corqty_persistence::audit::AuditEventRecord>,
}

pub async fn list(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    Path(organization_id): Path<Uuid>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<AuditResponse>, ApiError> {
    let _role = state
        .store
        .membership_role(session.user_id, organization_id)
        .await
        .map_err(|_| ApiError::Internal)?
        .ok_or(ApiError::NotFound)?;
    let items = state
        .store
        .list_audit_events(
            session.user_id,
            organization_id,
            query.before_id,
            query.limit.unwrap_or(50),
        )
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(Json(AuditResponse { items }))
}
