use chrono::{Duration, Utc};
use corqty_domain::account::{NormalizedEmail, OrganizationName, validate_password};
use corqty_persistence::{
    auth::{NewRegistration, NewSession},
    models::{OrganizationMembershipRecord, SessionRecord},
};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    auth::crypto::{PasswordEngine, SecretToken},
    error::ApiError,
    state::AppState,
};

pub struct IssuedSession {
    pub session_token: String,
    pub csrf_token: String,
}

#[derive(Serialize)]
pub struct UserView {
    pub id: Uuid,
    pub email: String,
}

#[derive(Serialize)]
pub struct MeView {
    pub user: UserView,
    pub organizations: Vec<OrganizationMembershipRecord>,
}

pub async fn register(
    state: &AppState,
    email: &str,
    password: &str,
    organization_name: &str,
) -> Result<IssuedSession, ApiError> {
    let email = NormalizedEmail::parse(email).map_err(|_| ApiError::Validation("invalid_email"))?;
    validate_password(password).map_err(|_| ApiError::Validation("invalid_password"))?;
    let organization_name = OrganizationName::parse(organization_name)
        .map_err(|_| ApiError::Validation("invalid_organization_name"))?;
    let password_owned = password.to_owned();
    let password_hash = tokio::task::spawn_blocking(move || PasswordEngine::hash(&password_owned))
        .await
        .map_err(|_| ApiError::Internal)??;
    let session_token = SecretToken::generate()?;
    let csrf_token = SecretToken::generate()?;
    let user_id = Uuid::new_v4();
    let organization_id = Uuid::new_v4();
    let slug_prefix: String = organization_name
        .as_str()
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let organization_slug = format!(
        "{}-{}",
        if slug_prefix.is_empty() {
            "org"
        } else {
            &slug_prefix
        },
        &organization_id.simple().to_string()[..8]
    );

    state
        .store
        .register(
            NewRegistration {
                user_id,
                email: email.as_str().to_owned(),
                password_hash,
                organization_id,
                organization_name: organization_name.as_str().to_owned(),
                organization_slug,
            },
            NewSession {
                session_id: Uuid::new_v4(),
                token_hash: session_token.hash,
                csrf_hash: csrf_token.hash,
                expires_at: Utc::now() + Duration::days(state.config.session_days),
            },
        )
        .await
        .map_err(map_database_error)?;

    Ok(IssuedSession {
        session_token: session_token.plaintext,
        csrf_token: csrf_token.plaintext,
    })
}

pub async fn login(
    state: &AppState,
    email: &str,
    password: &str,
) -> Result<IssuedSession, ApiError> {
    let record = match NormalizedEmail::parse(email) {
        Ok(email) => state
            .store
            .login_record(email.as_str())
            .await
            .map_err(|_| ApiError::Internal)?,
        Err(_) => None,
    };
    let password_owned = password.to_owned();
    let password_hash = match &record {
        Some(record) => record.password_hash.clone(),
        None => state.dummy_password_hash.to_string(),
    };
    let valid = tokio::task::spawn_blocking(move || {
        PasswordEngine::verify(&password_owned, &password_hash)
    })
    .await
    .map_err(|_| ApiError::Internal)?;
    let record = record.filter(|_| valid).ok_or(ApiError::Unauthorized)?;

    let session_token = SecretToken::generate()?;
    let csrf_token = SecretToken::generate()?;
    state
        .store
        .create_session(
            record.user_id,
            NewSession {
                session_id: Uuid::new_v4(),
                token_hash: session_token.hash,
                csrf_hash: csrf_token.hash,
                expires_at: Utc::now() + Duration::days(state.config.session_days),
            },
        )
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(IssuedSession {
        session_token: session_token.plaintext,
        csrf_token: csrf_token.plaintext,
    })
}

pub async fn me(state: &AppState, session: &SessionRecord) -> Result<MeView, ApiError> {
    let organizations = state
        .store
        .organizations_for_user(session.user_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(MeView {
        user: UserView {
            id: session.user_id,
            email: session.email.clone(),
        },
        organizations,
    })
}

pub async fn change_password(
    state: &AppState,
    session: &SessionRecord,
    current_password: &str,
    new_password: &str,
) -> Result<(), ApiError> {
    validate_password(new_password).map_err(|_| ApiError::Validation("invalid_password"))?;
    let login = state
        .store
        .login_record(&session.email)
        .await
        .map_err(|_| ApiError::Internal)?
        .ok_or(ApiError::Unauthorized)?;
    let current_owned = current_password.to_owned();
    let current_hash = login.password_hash;
    let valid =
        tokio::task::spawn_blocking(move || PasswordEngine::verify(&current_owned, &current_hash))
            .await
            .map_err(|_| ApiError::Internal)?;
    if !valid {
        return Err(ApiError::Unauthorized);
    }
    let new_owned = new_password.to_owned();
    let new_hash = tokio::task::spawn_blocking(move || PasswordEngine::hash(&new_owned))
        .await
        .map_err(|_| ApiError::Internal)??;
    state
        .store
        .change_password_and_revoke_other_sessions(session.user_id, session.session_id, &new_hash)
        .await
        .map_err(|_| ApiError::Internal)
}

fn map_database_error(error: sqlx::Error) -> ApiError {
    if error
        .as_database_error()
        .is_some_and(|database| database.code().as_deref() == Some("23505"))
    {
        ApiError::Conflict("email_already_registered")
    } else {
        ApiError::Internal
    }
}
