use corqty_domain::{account::OrganizationName, slug::slugify};
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
    let slug = slugify(name.as_str(), "org", organization_id);
    Ok(state
        .store
        .create_organization(session.user_id, organization_id, name.as_str(), &slug)
        .await?)
}
