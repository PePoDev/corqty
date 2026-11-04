use serde::Serialize;
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AuditEventRecord {
    pub id: i64,
    pub actor_user_id: Option<Uuid>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub metadata: Value,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
}

pub async fn insert_in_transaction(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    metadata: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_events(
            organization_id, actor_user_id, action,
            resource_type, resource_id, metadata
         ) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(organization_id)
    .bind(actor_user_id)
    .bind(action)
    .bind(resource_type)
    .bind(resource_id)
    .bind(metadata)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn insert_user_event_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    metadata: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT corqty.audit_current_user_memberships($1, $2, $3, $4)")
        .bind(action)
        .bind(resource_type)
        .bind(resource_id)
        .bind(metadata)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub async fn list_audit_events(
    conn: &mut PgConnection,
    organization_id: Uuid,
    before_id: Option<i64>,
    limit: i64,
) -> Result<Vec<AuditEventRecord>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, actor_user_id, action, resource_type,
                resource_id, metadata, occurred_at
         FROM audit_events
         WHERE organization_id = $1
           AND ($2::bigint IS NULL OR id < $2)
         ORDER BY id DESC
         LIMIT $3",
    )
    .bind(organization_id)
    .bind(before_id)
    .bind(limit.clamp(1, 100))
    .fetch_all(conn)
    .await
}
