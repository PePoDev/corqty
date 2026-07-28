use sqlx::Error;
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

    pub async fn membership_role(
        &self,
        user_id: Uuid,
        organization_id: Uuid,
    ) -> Result<Option<String>, Error> {
        let mut tx = begin_tenant(&self.pool, user_id, organization_id).await?;
        let role = sqlx::query_scalar(
            "SELECT role
             FROM memberships
             WHERE organization_id = $1 AND user_id = $2",
        )
        .bind(organization_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;
        tx.rollback().await?;
        Ok(role)
    }
}
