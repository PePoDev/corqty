use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::{DateTime, Duration, Utc};
use corqty_domain::{
    account::NormalizedEmail,
    rbac::{Capability, Role},
};
use corqty_persistence::{
    members::{self, MemberMutationError, MemberRecord, NewInvitation},
    models::SessionRecord,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{auth::crypto::SecretToken, authz::authorize, error::ApiError, state::AppState};

#[derive(Deserialize)]
pub struct InvitationRequest {
    email: String,
    role: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvitationResponse {
    token: String,
    invite_url: String,
    expires_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct AcceptInvitationRequest {
    token: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptInvitationResponse {
    organization_id: Uuid,
    role: String,
}

#[derive(Deserialize)]
pub struct ChangeRoleRequest {
    role: String,
}

pub async fn create_invitation(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    Path(organization_id): Path<Uuid>,
    Json(request): Json<InvitationRequest>,
) -> Result<(StatusCode, Json<InvitationResponse>), ApiError> {
    let mut access =
        authorize(&state, &session, organization_id, Capability::ManageMembers).await?;
    let email = NormalizedEmail::parse(&request.email)
        .map_err(|_| ApiError::Validation("invalid_email"))?;
    let role = request
        .role
        .parse::<Role>()
        .map_err(|_| ApiError::Validation("invalid_role"))?;
    let token = SecretToken::generate()?;
    let expires_at = Utc::now() + Duration::days(7);
    members::create_invitation(
        &mut access.tx,
        NewInvitation {
            actor_user_id: access.user_id,
            organization_id,
            invitation_id: Uuid::new_v4(),
            email: email.as_str(),
            role: role.as_str(),
            token_hash: &token.hash,
            expires_at,
        },
    )
    .await
    .map_err(|_| ApiError::Internal)?;
    access.commit().await?;
    let base = state.config.public_web_url.trim_end_matches('/');
    let invite_url = format!("{base}/invitations/accept#token={}", token.plaintext);
    Ok((
        StatusCode::CREATED,
        Json(InvitationResponse {
            token: token.plaintext,
            invite_url,
            expires_at,
        }),
    ))
}

pub async fn accept_invitation(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    Json(request): Json<AcceptInvitationRequest>,
) -> Result<Json<AcceptInvitationResponse>, ApiError> {
    let result = state
        .store
        .accept_invitation(session.user_id, &SecretToken::hash(&request.token))
        .await
        .map_err(map_invitation_error)?;
    Ok(Json(AcceptInvitationResponse {
        organization_id: result.0,
        role: result.1,
    }))
}

pub async fn list_members(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    Path(organization_id): Path<Uuid>,
) -> Result<Json<Vec<MemberRecord>>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let members = members::list_members(&mut access.tx, organization_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    access.commit().await?;
    Ok(Json(members))
}

pub async fn change_role(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    Path((organization_id, target_user_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<ChangeRoleRequest>,
) -> Result<StatusCode, ApiError> {
    let mut access =
        authorize(&state, &session, organization_id, Capability::ManageMembers).await?;
    let role = request
        .role
        .parse::<Role>()
        .map_err(|_| ApiError::Validation("invalid_role"))?;
    members::change_member_role(
        &mut access.tx,
        access.user_id,
        organization_id,
        target_user_id,
        role.as_str(),
    )
    .await
    .map_err(map_member_error)?;
    access.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_member(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    Path((organization_id, target_user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let mut access =
        authorize(&state, &session, organization_id, Capability::ManageMembers).await?;
    members::remove_member(
        &mut access.tx,
        access.user_id,
        organization_id,
        target_user_id,
    )
    .await
    .map_err(map_member_error)?;
    access.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

fn map_member_error(error: MemberMutationError) -> ApiError {
    match error {
        MemberMutationError::FinalAdmin => ApiError::Conflict("final_admin_required"),
        MemberMutationError::NotFound => ApiError::NotFound,
        MemberMutationError::Database(_) => ApiError::Internal,
    }
}

fn map_invitation_error(error: sqlx::Error) -> ApiError {
    match error.as_database_error().map(|database| database.message()) {
        Some("invalid_invitation") => ApiError::Validation("invalid_invitation"),
        Some("final_admin_required") => ApiError::Conflict("final_admin_required"),
        _ => ApiError::Internal,
    }
}
