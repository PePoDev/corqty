use chrono::{DateTime, Utc};
use sqlx::Error;
use uuid::Uuid;

use crate::{
    models::{RegistrationRecord, SessionRecord},
    store::PgStore,
    tenant::begin_tenant,
};

pub struct NewRegistration {
    pub user_id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub organization_id: Uuid,
    pub organization_name: String,
    pub organization_slug: String,
}

pub struct NewSession {
    pub session_id: Uuid,
    pub token_hash: Vec<u8>,
    pub csrf_hash: Vec<u8>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct LoginRecord {
    pub user_id: Uuid,
    pub email: String,
    pub password_hash: String,
}

impl PgStore {
    pub async fn register(
        &self,
        registration: NewRegistration,
        session: NewSession,
    ) -> Result<RegistrationRecord, Error> {
        let mut tx = begin_tenant(
            &self.pool,
            registration.user_id,
            registration.organization_id,
        )
        .await?;

        sqlx::query("INSERT INTO users(id, email) VALUES ($1, $2)")
            .bind(registration.user_id)
            .bind(&registration.email)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO password_credentials(user_id, password_hash)
             VALUES ($1, $2)",
        )
        .bind(registration.user_id)
        .bind(&registration.password_hash)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO organizations(id, name, slug, created_by)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(registration.organization_id)
        .bind(&registration.organization_name)
        .bind(&registration.organization_slug)
        .bind(registration.user_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO memberships(organization_id, user_id, role)
             VALUES ($1, $2, 'admin')",
        )
        .bind(registration.organization_id)
        .bind(registration.user_id)
        .execute(&mut *tx)
        .await?;
        insert_session(&mut tx, registration.user_id, session).await?;
        tx.commit().await?;

        Ok(RegistrationRecord {
            user_id: registration.user_id,
            organization_id: registration.organization_id,
            role: "admin".to_owned(),
        })
    }

    pub async fn login_record(&self, email: &str) -> Result<Option<LoginRecord>, Error> {
        sqlx::query_as(
            "SELECT users.id AS user_id, users.email, password_credentials.password_hash
             FROM users
             JOIN password_credentials ON password_credentials.user_id = users.id
             WHERE users.email = $1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn create_session(&self, user_id: Uuid, session: NewSession) -> Result<(), Error> {
        let mut tx = self.pool.begin().await?;
        insert_session(&mut tx, user_id, session).await?;
        tx.commit().await
    }

    pub async fn session_by_token_hash(
        &self,
        token_hash: &[u8],
    ) -> Result<Option<SessionRecord>, Error> {
        sqlx::query_as(
            "SELECT sessions.id AS session_id, sessions.user_id, users.email,
                    sessions.csrf_hash, sessions.expires_at
             FROM sessions
             JOIN users ON users.id = sessions.user_id
             WHERE sessions.token_hash = $1
               AND sessions.expires_at > now()",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn delete_session(&self, session_id: Uuid) -> Result<(), Error> {
        sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_sessions(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<crate::models::SessionSummaryRecord>, Error> {
        sqlx::query_as(
            "SELECT id, created_at, last_seen_at, expires_at
             FROM sessions
             WHERE user_id = $1 AND expires_at > now()
             ORDER BY created_at DESC, id",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn change_password_and_revoke_other_sessions(
        &self,
        user_id: Uuid,
        current_session_id: Uuid,
        password_hash: &str,
    ) -> Result<(), Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE password_credentials
             SET password_hash = $2, updated_at = now()
             WHERE user_id = $1",
        )
        .bind(user_id)
        .bind(password_hash)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM sessions
             WHERE user_id = $1 AND id <> $2",
        )
        .bind(user_id)
        .bind(current_session_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }

    pub async fn revoke_user_session(
        &self,
        user_id: Uuid,
        session_id: Uuid,
    ) -> Result<bool, Error> {
        let affected = sqlx::query("DELETE FROM sessions WHERE id = $1 AND user_id = $2")
            .bind(session_id)
            .bind(user_id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(affected == 1)
    }
}

async fn insert_session(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    session: NewSession,
) -> Result<(), Error> {
    sqlx::query(
        "INSERT INTO sessions(id, user_id, token_hash, csrf_hash, expires_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(session.session_id)
    .bind(user_id)
    .bind(session.token_hash)
    .bind(session.csrf_hash)
    .bind(session.expires_at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
