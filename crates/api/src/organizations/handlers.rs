use axum::{Extension, Json, extract::State, http::StatusCode};
use corqty_persistence::models::{OrganizationMembershipRecord, SessionRecord};
use serde::Deserialize;

use crate::{error::ApiError, extract::ApiJson, organizations::service, state::AppState};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateOrganization {
    name: String,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiJson(request): ApiJson<CreateOrganization>,
) -> Result<(StatusCode, Json<OrganizationMembershipRecord>), ApiError> {
    Ok((
        StatusCode::CREATED,
        Json(service::create(&state, &session, &request.name).await?),
    ))
}
