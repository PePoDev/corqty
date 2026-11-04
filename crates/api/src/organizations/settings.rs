use axum::{Extension, Json, extract::State};
use corqty_domain::rbac::Capability;
use corqty_persistence::{models::SessionRecord, organizations};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    authz::{OrgAccess, authorize},
    error::ApiError,
    extract::{ApiJson, ApiPath},
    state::AppState,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Whether the deployment offers the shell executor; see `ApiConfig`.
    shell_available: bool,
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
    Ok(Json(Settings {
        shell_available: state.config.shell_executor_available,
        shell_enabled,
    }))
}

/// Allows or refuses shell nodes in new publishes and runs. Runs that already exist keep
/// their shell nodes either way. Only a deployment that offers the shell executor lets an
/// admin enable it; disabling always works.
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
    if request.enabled && !state.config.shell_executor_available {
        return Err(ApiError::Conflict("shell_unavailable"));
    }
    organizations::set_shell_enabled(
        &mut access.tx,
        organization_id,
        access.user_id,
        request.enabled,
    )
    .await?;
    access.commit().await?;
    Ok(Json(Settings {
        shell_available: state.config.shell_executor_available,
        shell_enabled: request.enabled,
    }))
}

/// Whether the organization may publish and run shell nodes: the deployment offers the shell
/// executor and an admin of the organization enabled it.
pub async fn shell_allowed(state: &AppState, access: &mut OrgAccess) -> Result<bool, sqlx::Error> {
    Ok(state.config.shell_executor_available
        && organizations::shell_enabled(&mut access.tx, access.organization_id).await?)
}
