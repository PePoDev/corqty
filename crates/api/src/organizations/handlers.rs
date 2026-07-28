use axum::{Extension, Json, extract::State, http::StatusCode};
use corqty_persistence::models::{OrganizationMembershipRecord, SessionRecord};
use serde::Deserialize;

use crate::{error::ApiError, organizations::service, state::AppState};

#[derive(Deserialize)]
pub struct CreateOrganization {
    name: String,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    Json(request): Json<CreateOrganization>,
) -> Result<(StatusCode, Json<OrganizationMembershipRecord>), ApiError> {
    Ok((
        StatusCode::CREATED,
        Json(service::create(&state, &session, &request.name).await?),
    ))
}
