use corqty_domain::rbac::Role;
use serde_json::json;
use sqlx::{Error, PgConnection};
use uuid::Uuid;

use crate::{models::OrganizationMembershipRecord, store::PgStore, tenant::begin_tenant};

impl PgStore {
    pub async fn create_organization(
        &self,
        user_id: Uuid,
        organization_id: Uuid,
        name: &str,
        slug: &str,
    ) -> Result<OrganizationMembershipRecord, Error> {
        let mut tx = begin_tenant(&self.pool, user_id, organization_id).await?;
        sqlx::query(
            "INSERT INTO organizations(id, name, slug, created_by)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(organization_id)
        .bind(name)
        .bind(slug)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO memberships(organization_id, user_id, role)
             VALUES ($1, $2, 'admin')",
        )
        .bind(organization_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
        crate::audit::insert_in_transaction(
            &mut tx,
            organization_id,
            user_id,
            "organization.created",
            "organization",
            &organization_id.to_string(),
            serde_json::json!({ "name": name }),
        )
        .await?;
        tx.commit().await?;
        Ok(OrganizationMembershipRecord {
            organization_id,
            name: name.to_owned(),
            slug: slug.to_owned(),
            role: "admin".to_owned(),
        })
    }
}

pub async fn member_role(
    conn: &mut PgConnection,
    organization_id: Uuid,
    user_id: Uuid,
) -> Result<Option<Role>, Error> {
    let role: Option<String> = sqlx::query_scalar(
        "SELECT role
         FROM memberships
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(user_id)
    .fetch_optional(conn)
    .await?;
    role.map(|role| role.parse().map_err(|error| Error::Decode(Box::new(error))))
        .transpose()
}

/// Whether the organization's admins have enabled the trusted shell executor.
pub async fn shell_enabled(conn: &mut PgConnection, organization_id: Uuid) -> Result<bool, Error> {
    sqlx::query_scalar("SELECT shell_enabled_at IS NOT NULL FROM organizations WHERE id = $1")
        .bind(organization_id)
        .fetch_one(conn)
        .await
}

/// Enables or disables the trusted shell executor and audits the change. Setting the value
/// the organization already has changes nothing and writes no audit event.
pub async fn set_shell_enabled(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    enabled: bool,
) -> Result<(), Error> {
    let changed = sqlx::query(
        "UPDATE organizations
         SET shell_enabled_at = CASE WHEN $2 THEN now() END
         WHERE id = $1 AND (shell_enabled_at IS NOT NULL) <> $2",
    )
    .bind(organization_id)
    .bind(enabled)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if changed == 1 {
        let action = if enabled {
            "organization.shell_enabled"
        } else {
            "organization.shell_disabled"
        };
        crate::audit::insert_in_transaction(
            conn,
            organization_id,
            actor_user_id,
            action,
            "organization",
            &organization_id.to_string(),
            json!({}),
        )
        .await?;
    }
    Ok(())
}
