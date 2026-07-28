use corqty_domain::account::OrganizationName;
use corqty_persistence::models::{OrganizationMembershipRecord, SessionRecord};
use uuid::Uuid;

use crate::{error::ApiError, state::AppState};

pub async fn create(
    state: &AppState,
    session: &SessionRecord,
    name: &str,
) -> Result<OrganizationMembershipRecord, ApiError> {
    let name = OrganizationName::parse(name)
        .map_err(|_| ApiError::Validation("invalid_organization_name"))?;
    let organization_id = Uuid::new_v4();
    let prefix = name
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
        .trim_matches('-')
        .to_owned();
    let slug = format!(
        "{}-{}",
        if prefix.is_empty() { "org" } else { &prefix },
        &organization_id.simple().to_string()[..8]
    );
    state
        .store
        .create_organization(session.user_id, organization_id, name.as_str(), &slug)
        .await
        .map_err(|_| ApiError::Internal)
}
