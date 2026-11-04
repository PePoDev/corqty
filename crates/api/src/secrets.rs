use axum::{Extension, Json, extract::State, http::StatusCode};
use corqty_domain::{rbac::Capability, secrets::MAX_SECRET_BYTES};
use corqty_persistence::{
    models::SessionRecord,
    secrets::{self, SecretRecord, SecretWrite},
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    authz::authorize,
    catalog::Items,
    error::ApiError,
    extract::{ApiJson, ApiPath},
    state::AppState,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretBody {
    name: String,
    value: String,
}

impl SecretBody {
    fn validate(&self) -> Result<(), ApiError> {
        if !(1..=100).contains(&self.name.trim().chars().count()) || self.name.contains('\0') {
            return Err(ApiError::Validation("invalid_secret_name"));
        }
        if self.value.is_empty() || self.value.len() > MAX_SECRET_BYTES || self.value.contains('\0')
        {
            return Err(ApiError::Validation("invalid_secret_value"));
        }
        Ok(())
    }
}

pub async fn list(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(org): ApiPath<Uuid>,
) -> Result<Json<Items<SecretRecord>>, ApiError> {
    let mut access = authorize(&state, &session, org, Capability::EditWorkflows).await?;
    let items = secrets::list(&mut access.tx).await?;
    access.commit().await?;
    Ok(Json(Items { items }))
}

pub async fn create(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(org): ApiPath<Uuid>,
    ApiJson(body): ApiJson<SecretBody>,
) -> Result<(StatusCode, Json<SecretRecord>), ApiError> {
    let mut access = authorize(&state, &session, org, Capability::ManageSecrets).await?;
    body.validate()?;
    let id = Uuid::now_v7();
    let encrypted = state
        .config
        .master_key
        .encrypt(org, id, &body.value)
        .map_err(ApiError::internal)?;
    let record = secrets::create(
        &mut access.tx,
        &SecretWrite {
            id,
            organization_id: org,
            actor: access.user_id,
            name: body.name.trim(),
            encrypted: &encrypted,
        },
    )
    .await
    .map_err(map_error)?;
    access.commit().await?;
    Ok((StatusCode::CREATED, Json(record)))
}

pub async fn update(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((org, id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(body): ApiJson<SecretBody>,
) -> Result<Json<SecretRecord>, ApiError> {
    let mut access = authorize(&state, &session, org, Capability::ManageSecrets).await?;
    body.validate()?;
    let encrypted = state
        .config
        .master_key
        .encrypt(org, id, &body.value)
        .map_err(ApiError::internal)?;
    let record = secrets::update(
        &mut access.tx,
        &SecretWrite {
            id,
            organization_id: org,
            actor: access.user_id,
            name: body.name.trim(),
            encrypted: &encrypted,
        },
    )
    .await
    .map_err(map_error)?
    .ok_or(ApiError::NotFound)?;
    access.commit().await?;
    Ok(Json(record))
}

pub async fn remove(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((org, id)): ApiPath<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let mut access = authorize(&state, &session, org, Capability::ManageSecrets).await?;
    if !secrets::remove(&mut access.tx, org, access.user_id, id)
        .await
        .map_err(map_error)?
    {
        return Err(ApiError::NotFound);
    }
    access.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

fn map_error(error: sqlx::Error) -> ApiError {
    match error.as_database_error() {
        Some(database) if database.message() == "secret_in_use" => {
            ApiError::Conflict("secret_in_use")
        }
        Some(database) if database.constraint() == Some("secrets_org_name") => {
            ApiError::Conflict("secret_name_conflict")
        }
        _ => error.into(),
    }
}
