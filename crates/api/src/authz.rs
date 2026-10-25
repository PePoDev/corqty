use corqty_domain::rbac::{Capability, Role};
use corqty_persistence::{models::SessionRecord, organizations::member_role};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{error::ApiError, state::AppState};

/// An open tenant transaction for a member whose role was read inside it.
/// Handlers do all database work through `&mut access.tx` and finish with
/// `access.commit()`; dropping it rolls back.
pub struct OrgAccess {
    pub tx: Transaction<'static, Postgres>,
    pub organization_id: Uuid,
    pub user_id: Uuid,
    pub role: Role,
}

/// Non-members (including requests naming another organization) get 404;
/// members lacking the capability get 403.
pub async fn authorize(
    state: &AppState,
    session: &SessionRecord,
    organization_id: Uuid,
    capability: Capability,
) -> Result<OrgAccess, ApiError> {
    let mut tx = state
        .store
        .begin_tenant(session.user_id, organization_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    let role = member_role(&mut tx, organization_id, session.user_id)
        .await
        .map_err(|_| ApiError::Internal)?
        .ok_or(ApiError::NotFound)?;
    if !role.allows(capability) {
        return Err(ApiError::Forbidden);
    }
    Ok(OrgAccess {
        tx,
        organization_id,
        user_id: session.user_id,
        role,
    })
}

impl OrgAccess {
    /// The final-admin trigger is deferred, so it can only fail here.
    pub async fn commit(self) -> Result<(), ApiError> {
        self.tx.commit().await.map_err(|error| {
            if error
                .as_database_error()
                .is_some_and(|database| database.message() == "final_admin_required")
            {
                ApiError::Conflict("final_admin_required")
            } else {
                ApiError::Internal
            }
        })
    }
}
