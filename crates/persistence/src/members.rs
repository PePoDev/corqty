use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{store::PgStore, tenant::begin_user};

#[derive(Debug, thiserror::Error)]
pub enum MemberMutationError {
    #[error("the final admin cannot be changed")]
    FinalAdmin,
    #[error("member was not found")]
    NotFound,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MemberRecord {
    pub user_id: Uuid,
    pub email: String,
    pub role: String,
}

pub struct NewInvitation<'a> {
    pub actor_user_id: Uuid,
    pub organization_id: Uuid,
    pub invitation_id: Uuid,
    pub email: &'a str,
    pub role: &'a str,
    pub token_hash: &'a [u8],
    pub expires_at: DateTime<Utc>,
}

pub async fn create_invitation(
    conn: &mut PgConnection,
    invitation: NewInvitation<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO invitations(
            id, organization_id, email, role,
            token_hash, invited_by, expires_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(invitation.invitation_id)
    .bind(invitation.organization_id)
    .bind(invitation.email)
    .bind(invitation.role)
    .bind(invitation.token_hash)
    .bind(invitation.actor_user_id)
    .bind(invitation.expires_at)
    .execute(&mut *conn)
    .await?;
    crate::audit::insert_in_transaction(
        conn,
        invitation.organization_id,
        invitation.actor_user_id,
        "invitation.created",
        "invitation",
        &invitation.invitation_id.to_string(),
        serde_json::json!({
            "email": invitation.email,
            "role": invitation.role,
        }),
    )
    .await
}

impl PgStore {
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
}

pub async fn list_members(
    conn: &mut PgConnection,
    organization_id: Uuid,
) -> Result<Vec<MemberRecord>, sqlx::Error> {
    sqlx::query_as(
        "SELECT users.id AS user_id, users.email, memberships.role
         FROM memberships
         JOIN users ON users.id = memberships.user_id
         WHERE memberships.organization_id = $1
         ORDER BY users.email, users.id",
    )
    .bind(organization_id)
    .fetch_all(conn)
    .await
}

pub async fn change_member_role(
    conn: &mut PgConnection,
    actor_user_id: Uuid,
    organization_id: Uuid,
    target_user_id: Uuid,
    new_role: &str,
) -> Result<(), MemberMutationError> {
    lock_organization(conn, organization_id).await?;
    let current_role: Option<String> = sqlx::query_scalar(
        "SELECT role FROM memberships
         WHERE organization_id = $1 AND user_id = $2
         FOR UPDATE",
    )
    .bind(organization_id)
    .bind(target_user_id)
    .fetch_optional(&mut *conn)
    .await?;
    let current_role = current_role.ok_or(MemberMutationError::NotFound)?;
    if current_role == "admin"
        && new_role != "admin"
        && admin_count(conn, organization_id).await? == 1
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
    .execute(&mut *conn)
    .await?;
    crate::audit::insert_in_transaction(
        conn,
        organization_id,
        actor_user_id,
        "membership.role_changed",
        "membership",
        &target_user_id.to_string(),
        serde_json::json!({ "role": new_role }),
    )
    .await?;
    Ok(())
}

pub async fn remove_member(
    conn: &mut PgConnection,
    actor_user_id: Uuid,
    organization_id: Uuid,
    target_user_id: Uuid,
) -> Result<(), MemberMutationError> {
    lock_organization(conn, organization_id).await?;
    let current_role: Option<String> = sqlx::query_scalar(
        "SELECT role FROM memberships
         WHERE organization_id = $1 AND user_id = $2
         FOR UPDATE",
    )
    .bind(organization_id)
    .bind(target_user_id)
    .fetch_optional(&mut *conn)
    .await?;
    let current_role = current_role.ok_or(MemberMutationError::NotFound)?;
    if current_role == "admin" && admin_count(conn, organization_id).await? == 1 {
        return Err(MemberMutationError::FinalAdmin);
    }
    crate::audit::insert_in_transaction(
        conn,
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
    .execute(conn)
    .await?;
    Ok(())
}

async fn lock_organization(
    conn: &mut PgConnection,
    organization_id: Uuid,
) -> Result<(), sqlx::Error> {
    let _: Uuid = sqlx::query_scalar("SELECT id FROM organizations WHERE id = $1 FOR UPDATE")
        .bind(organization_id)
        .fetch_one(conn)
        .await?;
    Ok(())
}

async fn admin_count(conn: &mut PgConnection, organization_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM memberships
         WHERE organization_id = $1 AND role = 'admin'",
    )
    .bind(organization_id)
    .fetch_one(conn)
    .await
}
