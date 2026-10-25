use chrono::{DateTime, Utc};
use corqty_domain::{
    catalog::{NewNodeDefinition, NodeDefinitionSpec},
    graph::GraphSnapshot,
    publish::Definitions,
};
use serde::Serialize;
use serde_json::json;
use sqlx::{PgConnection, types::Json};
use uuid::Uuid;

/// One node definition version. The spec fields sit at the top level of its JSON.
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct NodeDefinitionRecord {
    pub id: Uuid,
    pub scope: String,
    pub key: String,
    pub version: i32,
    pub display_name: String,
    pub description: String,
    pub category: String,
    #[serde(flatten)]
    #[sqlx(json)]
    pub spec: NodeDefinitionSpec,
    pub deprecated: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintRecord {
    pub id: Uuid,
    pub scope: String,
    pub key: String,
    pub version: i32,
    pub display_name: String,
    pub description: String,
    #[sqlx(json)]
    pub graph: GraphSnapshot,
    pub created_at: DateTime<Utc>,
}

const DEFINITION: &str = "id, scope, key, version, display_name, description, category, spec,
     deprecated_at IS NOT NULL AS deprecated, created_at";
const BLUEPRINT: &str = "id, scope, key, version, display_name, description, graph, created_at";

/// The system catalog and the current organization's definitions, by key and version.
pub async fn list_node_definitions(
    conn: &mut PgConnection,
) -> Result<Vec<NodeDefinitionRecord>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {DEFINITION} FROM node_definitions ORDER BY key, version"
    ))
    .fetch_all(conn)
    .await
}

/// Stores the next version of the key in the organization. A concurrent insert of the same
/// key fails with a unique violation on `node_definitions_key_version`.
pub async fn insert_node_definition(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    definition: &NewNodeDefinition,
) -> Result<NodeDefinitionRecord, sqlx::Error> {
    let record: NodeDefinitionRecord = sqlx::query_as(&format!(
        "INSERT INTO node_definitions(
            organization_id, scope, key, version, display_name,
            description, category, spec, created_by
         )
         SELECT $1, 'organization', $2, COALESCE(MAX(version), 0) + 1, $3, $4, $5, $6, $7
         FROM node_definitions
         WHERE organization_id = $1 AND key = $2
         RETURNING {DEFINITION}"
    ))
    .bind(organization_id)
    .bind(&definition.key)
    .bind(&definition.display_name)
    .bind(&definition.description)
    .bind(&definition.category)
    .bind(Json(&definition.spec))
    .bind(actor_user_id)
    .fetch_one(&mut *conn)
    .await?;
    audit_definition(
        conn,
        organization_id,
        actor_user_id,
        "node_definition.created",
        &record,
    )
    .await?;
    Ok(record)
}

/// Deprecates one of the organization's own definitions. System and foreign ids give None.
pub async fn deprecate_node_definition(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    id: Uuid,
) -> Result<Option<NodeDefinitionRecord>, sqlx::Error> {
    let record: Option<NodeDefinitionRecord> = sqlx::query_as(&format!(
        "UPDATE node_definitions
         SET deprecated_at = COALESCE(deprecated_at, now())
         WHERE id = $1 AND organization_id = $2
         RETURNING {DEFINITION}"
    ))
    .bind(id)
    .bind(organization_id)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(record) = &record {
        audit_definition(
            conn,
            organization_id,
            actor_user_id,
            "node_definition.deprecated",
            record,
        )
        .await?;
    }
    Ok(record)
}

/// Every definition a graph in the current organization may use, deprecated ones included.
pub async fn definitions_map(conn: &mut PgConnection) -> Result<Definitions, sqlx::Error> {
    let rows: Vec<(String, i32, Json<NodeDefinitionSpec>)> =
        sqlx::query_as("SELECT key, version, spec FROM node_definitions")
            .fetch_all(conn)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(key, version, Json(spec))| ((key, version), spec))
        .collect())
}

/// The system blueprints and the current organization's, by key and version.
pub async fn list_blueprints(conn: &mut PgConnection) -> Result<Vec<BlueprintRecord>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {BLUEPRINT} FROM blueprints ORDER BY key, version"
    ))
    .fetch_all(conn)
    .await
}

pub async fn get_blueprint(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<Option<BlueprintRecord>, sqlx::Error> {
    sqlx::query_as(&format!("SELECT {BLUEPRINT} FROM blueprints WHERE id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// Stores the next version of the key in the organization, like `insert_node_definition`.
pub async fn insert_blueprint(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    key: &str,
    display_name: &str,
    description: &str,
    graph: &GraphSnapshot,
) -> Result<BlueprintRecord, sqlx::Error> {
    let record: BlueprintRecord = sqlx::query_as(&format!(
        "INSERT INTO blueprints(
            organization_id, scope, key, version, display_name, description, graph, created_by
         )
         SELECT $1, 'organization', $2, COALESCE(MAX(version), 0) + 1, $3, $4, $5, $6
         FROM blueprints
         WHERE organization_id = $1 AND key = $2
         RETURNING {BLUEPRINT}"
    ))
    .bind(organization_id)
    .bind(key)
    .bind(display_name)
    .bind(description)
    .bind(Json(graph))
    .bind(actor_user_id)
    .fetch_one(&mut *conn)
    .await?;
    crate::audit::insert_in_transaction(
        conn,
        organization_id,
        actor_user_id,
        "blueprint.created",
        "blueprint",
        &record.id.to_string(),
        json!({ "key": record.key, "version": record.version }),
    )
    .await?;
    Ok(record)
}

async fn audit_definition(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    action: &str,
    record: &NodeDefinitionRecord,
) -> Result<(), sqlx::Error> {
    crate::audit::insert_in_transaction(
        conn,
        organization_id,
        actor_user_id,
        action,
        "node_definition",
        &record.id.to_string(),
        json!({ "key": record.key, "version": record.version }),
    )
    .await
}
