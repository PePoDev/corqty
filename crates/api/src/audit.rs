use axum::{
    Extension, Json,
    extract::{Query, State},
};
use corqty_domain::rbac::Capability;
use corqty_persistence::models::SessionRecord;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{authz::authorize, error::ApiError, extract::ApiPath, state::AppState};

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
    ApiPath(organization_id): ApiPath<Uuid>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<AuditResponse>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let items = corqty_persistence::audit::list_audit_events(
        &mut access.tx,
        organization_id,
        query.before_id,
        query.limit.unwrap_or(50),
    )
    .await?;
    access.commit().await?;
    Ok(Json(AuditResponse { items }))
}
