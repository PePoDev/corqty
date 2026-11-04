use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PluginRecord {
    pub id: Uuid,
    pub key: String,
    pub version: String,
    pub host_contract: String,
    pub manifest: Value,
    pub checksum: String,
    pub enabled: bool,
    pub approved: bool,
    pub created_at: DateTime<Utc>,
}
#[derive(sqlx::FromRow)]
pub struct RuntimePayload {
    pub manifest: Value,
    pub component: Vec<u8>,
    pub capabilities: Value,
}

const FIELDS:&str="p.id,p.key,p.version,p.host_contract,p.manifest,p.checksum,p.enabled,p.created_at,
    (NOT EXISTS (SELECT FROM jsonb_array_elements(p.manifest->'capabilities') c WHERE c->>'type'<>'log')
     OR EXISTS (SELECT FROM plugin_approvals a WHERE a.plugin_id=p.id AND a.organization_id=p.organization_id
       AND a.checksum=p.checksum AND a.manifest=p.manifest AND a.capabilities=p.manifest->'capabilities')) AS approved";

pub async fn list(conn: &mut PgConnection) -> Result<Vec<PluginRecord>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {FIELDS} FROM plugin_versions p ORDER BY p.key,p.created_at DESC"
    ))
    .fetch_all(conn)
    .await
}

/// A transaction-stable availability snapshot without loading any component bytes.
pub async fn enabled_identities(
    conn: &mut PgConnection,
) -> Result<HashSet<(String, String)>, sqlx::Error> {
    lock_catalog(conn).await?;
    Ok(list(conn)
        .await?
        .into_iter()
        .filter(|plugin| plugin.enabled && plugin.approved)
        .map(|plugin| (plugin.key, plugin.version))
        .collect())
}
pub async fn get(conn: &mut PgConnection, id: Uuid) -> Result<Option<PluginRecord>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {FIELDS} FROM plugin_versions p WHERE p.id=$1"
    ))
    .bind(id)
    .fetch_optional(conn)
    .await
}
pub async fn available(
    conn: &mut PgConnection,
    key: &str,
    version: &str,
) -> Result<Option<RuntimePayload>, sqlx::Error> {
    lock_catalog(conn).await?;
    sqlx::query_as("SELECT p.manifest,p.component,coalesce(a.capabilities,'[]'::jsonb) AS capabilities
        FROM plugin_versions p LEFT JOIN plugin_approvals a ON a.plugin_id=p.id AND a.organization_id=p.organization_id
          AND a.checksum=p.checksum AND a.manifest=p.manifest
        WHERE p.key=$1 AND p.version=$2 AND p.enabled
          AND (NOT EXISTS (SELECT FROM jsonb_array_elements(p.manifest->'capabilities') c WHERE c->>'type'<>'log')
               OR a.capabilities=p.manifest->'capabilities')")
        .bind(key).bind(version).fetch_optional(conn).await
}
pub struct NewPlugin<'a> {
    pub org: Uuid,
    pub actor: Uuid,
    pub key: &'a str,
    pub version: &'a str,
    pub host_contract: &'a str,
    pub manifest: &'a Value,
    pub checksum: &'a str,
    pub component: &'a [u8],
}
pub async fn insert(
    conn: &mut PgConnection,
    new: NewPlugin<'_>,
) -> Result<PluginRecord, sqlx::Error> {
    let id:Uuid=sqlx::query_scalar("INSERT INTO plugin_versions(organization_id,installed_by,key,version,host_contract,manifest,checksum,component) VALUES($1,$2,$3,$4,$5,$6,$7,$8) RETURNING id")
        .bind(new.org).bind(new.actor).bind(new.key).bind(new.version).bind(new.host_contract).bind(new.manifest).bind(new.checksum).bind(new.component).fetch_one(&mut *conn).await?;
    crate::audit::insert_in_transaction(
        conn,
        new.org,
        new.actor,
        "plugin.installed",
        "plugin",
        &id.to_string(),
        json!({"key":new.key,"version":new.version,"checksum":new.checksum}),
    )
    .await?;
    Ok(get(conn, id).await?.expect("inserted plugin"))
}
pub async fn approve(
    conn: &mut PgConnection,
    org: Uuid,
    actor: Uuid,
    record: &PluginRecord,
    capabilities: &Value,
) -> Result<PluginRecord, sqlx::Error> {
    sqlx::query("INSERT INTO plugin_approvals(organization_id,plugin_id,checksum,manifest,capabilities,approved_by) VALUES($1,$2,$3,$4,$5,$6)
      ON CONFLICT(plugin_id) DO UPDATE SET checksum=excluded.checksum,manifest=excluded.manifest,capabilities=excluded.capabilities,approved_by=excluded.approved_by,approved_at=now()")
      .bind(org).bind(record.id).bind(&record.checksum).bind(&record.manifest).bind(capabilities).bind(actor).execute(&mut *conn).await?;
    crate::audit::insert_in_transaction(
        conn,
        org,
        actor,
        "plugin.approved",
        "plugin",
        &record.id.to_string(),
        json!({"checksum":record.checksum,"capabilities":capabilities}),
    )
    .await?;
    Ok(get(conn, record.id).await?.expect("approved plugin"))
}
pub async fn set_enabled(
    conn: &mut PgConnection,
    org: Uuid,
    actor: Uuid,
    id: Uuid,
    enabled: bool,
) -> Result<Option<PluginRecord>, sqlx::Error> {
    lock_catalog(conn).await?;
    let result = sqlx::query("UPDATE plugin_versions SET enabled=$2 WHERE id=$1 AND enabled<>$2")
        .bind(id)
        .bind(enabled)
        .execute(&mut *conn)
        .await?;
    if result.rows_affected() > 0 {
        crate::audit::insert_in_transaction(
            conn,
            org,
            actor,
            if enabled {
                "plugin.enabled"
            } else {
                "plugin.disabled"
            },
            "plugin",
            &id.to_string(),
            json!({}),
        )
        .await?;
    }
    get(conn, id).await
}

async fn lock_catalog(conn: &mut PgConnection) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended(corqty.current_organization_id()::text,874252))").execute(conn).await?;
    Ok(())
}
