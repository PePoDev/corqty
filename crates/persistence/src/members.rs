use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    store::PgStore,
    tenant::{begin_tenant, begin_user},
};

#[derive(Debug, thiserror::Error)]
pub enum MemberMutationError {
    #[error("the final admin cannot be changed")]
    FinalAdmin,
    #[error("member was not found")]
    NotFound,
    #[error(transparent)]
    Database(sqlx::Error),
}

impl From<sqlx::Error> for MemberMutationError {
    fn from(error: sqlx::Error) -> Self {
        if error
            .as_database_error()
            .is_some_and(|database| database.message() == "final_admin_required")
        {
            Self::FinalAdmin
        } else {
            Self::Database(error)
        }
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MemberRecord {
    pub user_id: Uuid,
    pub email: String,
    pub role: String,
}

impl PgStore {
    pub async fn create_invitation(
        &self,
        actor_user_id: Uuid,
        organization_id: Uuid,
        invitation_id: Uuid,
        email: &str,
        role: &str,
        token_hash: &[u8],
        expires_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let mut tx = begin_tenant(&self.pool, actor_user_id, organization_id).await?;
        sqlx::query(
            "INSERT INTO invitations(
                id, organization_id, email, role,
                token_hash, invited_by, expires_at
             ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(invitation_id)
        .bind(organization_id)
        .bind(email)
        .bind(role)
        .bind(token_hash)
        .bind(actor_user_id)
        .bind(expires_at)
        .execute(&mut *tx)
        .await?;
        crate::audit::insert_in_transaction(
            &mut tx,
            organization_id,
            actor_user_id,
            "invitation.created",
            "invitation",
            &invitation_id.to_string(),
            serde_json::json!({ "email": email, "role": role }),
        )
        .await?;
        tx.commit().await
    }

    pub async fn accept_invitation(
        &self,
        user_id: Uuid,
        token_hash: &[u8],
    ) -> Result<(Uuid, String), sqlx::Error> {
        let mut tx = begin_user(&self.pool, user_id).await?;
        let accepted = sqlx::query_as(
            "SELECT organization_id, accepted_role
             FROM corqty.accept_invitation($1)",
        )
        .bind(token_hash)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(accepted)
    }

    pub async fn list_members(
        &self,
        actor_user_id: Uuid,
        organization_id: Uuid,
    ) -> Result<Vec<MemberRecord>, sqlx::Error> {
        let mut tx = begin_tenant(&self.pool, actor_user_id, organization_id).await?;
        let members = sqlx::query_as(
            "SELECT users.id AS user_id, users.email, memberships.role
             FROM memberships
             JOIN users ON users.id = memberships.user_id
             WHERE memberships.organization_id = $1
             ORDER BY users.email, users.id",
        )
        .bind(organization_id)
        .fetch_all(&mut *tx)
        .await?;
        tx.rollback().await?;
        Ok(members)
    }

    pub async fn change_member_role(
        &self,
        actor_user_id: Uuid,
        organization_id: Uuid,
        target_user_id: Uuid,
        new_role: &str,
    ) -> Result<(), MemberMutationError> {
        let mut tx = begin_tenant(&self.pool, actor_user_id, organization_id).await?;
        lock_organization(&mut tx, organization_id).await?;
        let current_role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM memberships
             WHERE organization_id = $1 AND user_id = $2
             FOR UPDATE",
        )
        .bind(organization_id)
        .bind(target_user_id)
        .fetch_optional(&mut *tx)
        .await?;
        let current_role = current_role.ok_or(MemberMutationError::NotFound)?;
        if current_role == "admin"
            && new_role != "admin"
            && admin_count(&mut tx, organization_id).await? == 1
        {
            return Err(MemberMutationError::FinalAdmin);
        }
        sqlx::query(
            "UPDATE memberships SET role = $3
             WHERE organization_id = $1 AND user_id = $2",
        )
        .bind(organization_id)
        .bind(target_user_id)
        .bind(new_role)
        .execute(&mut *tx)
        .await?;
        crate::audit::insert_in_transaction(
            &mut tx,
            organization_id,
            actor_user_id,
            "membership.role_changed",
            "membership",
            &target_user_id.to_string(),
            serde_json::json!({ "role": new_role }),
        )
        .await?;
        tx.commit().await.map_err(MemberMutationError::from)
    }

    pub async fn remove_member(
        &self,
        actor_user_id: Uuid,
        organization_id: Uuid,
        target_user_id: Uuid,
    ) -> Result<(), MemberMutationError> {
        let mut tx = begin_tenant(&self.pool, actor_user_id, organization_id).await?;
        lock_organization(&mut tx, organization_id).await?;
        let current_role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM memberships
             WHERE organization_id = $1 AND user_id = $2
             FOR UPDATE",
        )
        .bind(organization_id)
        .bind(target_user_id)
        .fetch_optional(&mut *tx)
        .await?;
        let current_role = current_role.ok_or(MemberMutationError::NotFound)?;
        if current_role == "admin" && admin_count(&mut tx, organization_id).await? == 1 {
            return Err(MemberMutationError::FinalAdmin);
        }
        crate::audit::insert_in_transaction(
            &mut tx,
            organization_id,
            actor_user_id,
            "membership.removed",
            "membership",
            &target_user_id.to_string(),
            serde_json::json!({}),
        )
        .await?;
        sqlx::query(
            "DELETE FROM memberships
             WHERE organization_id = $1 AND user_id = $2",
        )
        .bind(organization_id)
        .bind(target_user_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await.map_err(MemberMutationError::from)
    }
}

async fn lock_organization(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    organization_id: Uuid,
) -> Result<(), sqlx::Error> {
    let _: Uuid = sqlx::query_scalar("SELECT id FROM organizations WHERE id = $1 FOR UPDATE")
        .bind(organization_id)
        .fetch_one(&mut **tx)
        .await?;
    Ok(())
}

async fn admin_count(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    organization_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM memberships
         WHERE organization_id = $1 AND role = 'admin'",
    )
    .bind(organization_id)
    .fetch_one(&mut **tx)
    .await
}
