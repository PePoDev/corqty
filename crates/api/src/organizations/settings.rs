use axum::{Extension, Json, extract::State};
use corqty_domain::rbac::Capability;
use corqty_persistence::{models::SessionRecord, organizations};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    authz::authorize,
    error::ApiError,
    extract::{ApiJson, ApiPath},
    state::AppState,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    shell_enabled: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShellSetting {
    enabled: bool,
}

pub async fn get_settings(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(organization_id): ApiPath<Uuid>,
) -> Result<Json<Settings>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let shell_enabled = organizations::shell_enabled(&mut access.tx, organization_id).await?;
    access.commit().await?;
    Ok(Json(Settings { shell_enabled }))
}

/// Allows or refuses shell nodes in new publishes and runs. Runs that already exist keep
/// their shell nodes either way.
pub async fn set_shell(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(organization_id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<ShellSetting>,
) -> Result<Json<Settings>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::EnableShellExecutor,
    )
    .await?;
    organizations::set_shell_enabled(
        &mut access.tx,
        organization_id,
        access.user_id,
        request.enabled,
    )
    .await?;
    access.commit().await?;
    Ok(Json(Settings {
        shell_enabled: request.enabled,
    }))
}
