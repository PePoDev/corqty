use std::collections::HashSet;

use chrono::{DateTime, Utc};
use corqty_domain::secrets::EncryptedSecret;
use serde::Serialize;
use serde_json::json;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::audit;

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SecretRecord {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct BoundSecretRecord {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub key_version: i32,
}

impl BoundSecretRecord {
    #[must_use]
    pub fn encrypted(&self) -> EncryptedSecret {
        EncryptedSecret {
            ciphertext: self.ciphertext.clone(),
            nonce: self.nonce.clone(),
            key_version: self.key_version,
        }
    }
}

pub async fn list(conn: &mut PgConnection) -> Result<Vec<SecretRecord>, sqlx::Error> {
    sqlx::query_as("SELECT id, name, created_at, updated_at FROM secrets ORDER BY name, id")
        .fetch_all(conn)
        .await
}

/// Call while validating a publish/run in its tenant transaction. The advisory lock is
/// shared with reference writes/deletion and survives until commit, closing the check/use race.
pub async fn ids(conn: &mut PgConnection) -> Result<HashSet<Uuid>, sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended(corqty.current_organization_id()::text, 874251))")
        .execute(&mut *conn).await?;
    Ok(sqlx::query_scalar::<_, Uuid>("SELECT id FROM secrets")
        .fetch_all(conn)
        .await?
        .into_iter()
        .collect())
}

pub struct SecretWrite<'a> {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub actor: Uuid,
    pub name: &'a str,
    pub encrypted: &'a EncryptedSecret,
}

pub async fn create(
    conn: &mut PgConnection,
    write: &SecretWrite<'_>,
) -> Result<SecretRecord, sqlx::Error> {
    let record = sqlx::query_as(
        "INSERT INTO secrets(id, organization_id, name, ciphertext, nonce, key_version)
         VALUES ($1,$2,$3,$4,$5,$6) RETURNING id, name, created_at, updated_at",
    )
    .bind(write.id)
    .bind(write.organization_id)
    .bind(write.name)
    .bind(&write.encrypted.ciphertext)
    .bind(&write.encrypted.nonce)
    .bind(write.encrypted.key_version)
    .fetch_one(&mut *conn)
    .await?;
    audit::insert_in_transaction(
        conn,
        write.organization_id,
        write.actor,
        "secret.created",
        "secret",
        &write.id.to_string(),
        json!({}),
    )
    .await?;
    Ok(record)
}

pub async fn update(
    conn: &mut PgConnection,
    write: &SecretWrite<'_>,
) -> Result<Option<SecretRecord>, sqlx::Error> {
    let record = sqlx::query_as(
        "UPDATE secrets SET name=$2, ciphertext=$3, nonce=$4, key_version=$5, updated_at=now()
         WHERE id=$1 RETURNING id, name, created_at, updated_at",
    )
    .bind(write.id)
    .bind(write.name)
    .bind(&write.encrypted.ciphertext)
    .bind(&write.encrypted.nonce)
    .bind(write.encrypted.key_version)
    .fetch_optional(&mut *conn)
    .await?;
    if record.is_some() {
        audit::insert_in_transaction(
            conn,
            write.organization_id,
            write.actor,
            "secret.updated",
            "secret",
            &write.id.to_string(),
            json!({}),
        )
        .await?;
    }
    Ok(record)
}

pub async fn remove(
    conn: &mut PgConnection,
    org: Uuid,
    actor: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 874251))")
        .bind(org)
        .execute(&mut *conn)
        .await?;
    let removed = sqlx::query("DELETE FROM secrets WHERE id=$1")
        .bind(id)
        .execute(&mut *conn)
        .await?
        .rows_affected()
        == 1;
    if removed {
        audit::insert_in_transaction(
            conn,
            org,
            actor,
            "secret.deleted",
            "secret",
            &id.to_string(),
            json!({}),
        )
        .await?;
    }
    Ok(removed)
}
