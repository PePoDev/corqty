use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ServicePrincipal {
    pub id: Uuid,
    pub organization_id: Uuid,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ServiceTokenRecord {
    pub id: Uuid,
    pub name: String,
    pub scope: String,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

pub async fn authenticate_token(
    pool: &PgPool,
    hash: &[u8],
) -> Result<Option<ServicePrincipal>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM corqty.authenticate_service_token($1)")
        .bind(hash)
        .fetch_optional(pool)
        .await
}

pub async fn tokens(conn: &mut PgConnection) -> Result<Vec<ServiceTokenRecord>, sqlx::Error> {
    sqlx::query_as("SELECT id, name, scope, created_at, revoked_at FROM service_tokens ORDER BY created_at DESC, id DESC")
        .fetch_all(conn).await
}

pub async fn create_token(
    conn: &mut PgConnection,
    org: Uuid,
    user: Uuid,
    name: &str,
    hash: &[u8],
) -> Result<ServiceTokenRecord, sqlx::Error> {
    sqlx::query_as("INSERT INTO service_tokens(organization_id, created_by, name, token_hash) VALUES ($1,$2,$3,$4)
        RETURNING id, name, scope, created_at, revoked_at")
        .bind(org).bind(user).bind(name).bind(hash).fetch_one(conn).await
}

pub async fn revoke_token(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query(
        "UPDATE service_tokens SET revoked_at = coalesce(revoked_at, now()) WHERE id = $1",
    )
    .bind(id)
    .execute(conn)
    .await?
    .rows_affected()
        > 0)
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TriggerRecord {
    pub id: Uuid,
    #[serde(skip)]
    pub organization_id: Uuid,
    pub name: String,
    pub workflow_id: Uuid,
    #[serde(skip)]
    pub workflow_version_id: Uuid,
    pub version: i32,
    pub kind: String,
    pub input: Value,
    pub enabled: bool,
    pub cron: Option<String>,
    pub timezone: Option<String>,
    pub overlap_policy: String,
    pub next_scheduled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip)]
    pub webhook_ciphertext: Option<Vec<u8>>,
    #[serde(skip)]
    pub webhook_nonce: Option<Vec<u8>>,
    #[serde(skip)]
    pub webhook_key_version: Option<i32>,
}

pub async fn triggers(conn: &mut PgConnection) -> Result<Vec<TriggerRecord>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM triggers ORDER BY created_at DESC, id DESC")
        .fetch_all(conn)
        .await
}

pub async fn trigger(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<Option<TriggerRecord>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM triggers WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(conn)
        .await
}

pub async fn webhook(pool: &PgPool, id: Uuid) -> Result<Option<TriggerRecord>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM corqty.webhook_trigger($1)")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn due_triggers(
    conn: &mut PgConnection,
    now: DateTime<Utc>,
) -> Result<Vec<TriggerRecord>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM corqty.due_triggers($1)")
        .bind(now)
        .fetch_all(conn)
        .await
}

/// The caller must authenticate this actor before opening its tenant context. RLS
/// independently checks that the token/trigger remains enabled and belongs to this org.
pub async fn actor_context(
    conn: &mut PgConnection,
    org: Uuid,
    token: Option<Uuid>,
    trigger: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT set_config('corqty.user_id','',true), set_config('corqty.organization_id',$1,true),
        set_config('corqty.service_token_id',$2,true), set_config('corqty.trigger_id',$3,true)",
    )
    .bind(org.to_string())
    .bind(token.map(|id| id.to_string()).unwrap_or_default())
    .bind(trigger.map(|id| id.to_string()).unwrap_or_default())
    .execute(conn)
    .await?;
    Ok(())
}

pub enum Idempotency {
    Fresh,
    Replay(Uuid),
    Conflict,
}

/// The advisory lock serializes this exact key, including when no row exists yet.
/// It is held through run insertion and response recording in the same transaction.
pub async fn check_idempotency(
    conn: &mut PgConnection,
    org: Uuid,
    route: &str,
    actor: &str,
    key: &str,
    hash: &[u8],
) -> Result<Idempotency, sqlx::Error> {
    let lock = serde_json::json!([org, route, actor, key]).to_string();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(lock)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM run_idempotency WHERE organization_id=$1 AND route=$2 AND actor=$3 AND key=$4 AND expires_at <= now()")
        .bind(org).bind(route).bind(actor).bind(key).execute(&mut *conn).await?;
    let found: Option<(Uuid, Vec<u8>)> = sqlx::query_as("SELECT run_id, request_hash FROM run_idempotency WHERE organization_id=$1 AND route=$2 AND actor=$3 AND key=$4")
        .bind(org).bind(route).bind(actor).bind(key).fetch_optional(conn).await?;
    Ok(match found {
        None => Idempotency::Fresh,
        Some((id, previous)) if previous == hash => Idempotency::Replay(id),
        Some(_) => Idempotency::Conflict,
    })
}

pub async fn record_idempotency(
    conn: &mut PgConnection,
    org: Uuid,
    route: &str,
    actor: &str,
    key: &str,
    hash: &[u8],
    run: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO run_idempotency(organization_id,route,actor,key,request_hash,run_id) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(org).bind(route).bind(actor).bind(key).bind(hash).bind(run).execute(conn).await?;
    Ok(())
}

pub async fn audit_actor(
    conn: &mut PgConnection,
    org: Uuid,
    user: Option<Uuid>,
    action: &str,
    resource_type: &str,
    id: Uuid,
    metadata: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO audit_events(organization_id,actor_user_id,action,resource_type,resource_id,metadata) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(org).bind(user).bind(action).bind(resource_type).bind(id.to_string()).bind(metadata).execute(conn).await?;
    Ok(())
}
