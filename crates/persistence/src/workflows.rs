use chrono::{DateTime, Utc};
use corqty_domain::graph::GraphSnapshot;
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, types::Json};
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSummaryRecord {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub draft_revision: i64,
    pub published_version: Option<i32>,
    pub published_draft_revision: Option<i64>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRecord {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub published_version: Option<i32>,
    pub published_draft_revision: Option<i64>,
    #[sqlx(flatten)]
    pub draft: DraftRecord,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct DraftRecord {
    pub revision: i64,
    #[sqlx(json)]
    pub graph: GraphSnapshot,
    pub updated_at: DateTime<Utc>,
}

/// The draft that a publish reads while it holds the workflow's row locks.
#[derive(Debug, sqlx::FromRow)]
pub struct LockedWorkflow {
    pub revision: i64,
    #[sqlx(json)]
    pub graph: GraphSnapshot,
    pub published_version: Option<i32>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct VersionSummaryRecord {
    pub id: Uuid,
    pub version: i32,
    pub checksum: String,
    pub draft_revision: i64,
    pub published_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct VersionRecord {
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub summary: VersionSummaryRecord,
    #[sqlx(json)]
    pub graph: GraphSnapshot,
}

/// Each workflow with its draft and, through `published_version`, its published version.
const WORKFLOWS: &str = "workflows w
     JOIN workflow_drafts d ON d.workflow_id = w.id
     LEFT JOIN workflow_versions v ON v.workflow_id = w.id AND v.version = w.published_version";
const VERSION_SUMMARY: &str = "id, version, checksum, draft_revision, published_at";

/// The current organization's workflows, most recently changed first.
pub async fn list_workflows(
    conn: &mut PgConnection,
) -> Result<Vec<WorkflowSummaryRecord>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT w.id, w.name, w.slug, d.revision AS draft_revision, w.published_version,
                v.draft_revision AS published_draft_revision, w.updated_at
         FROM {WORKFLOWS}
         ORDER BY w.updated_at DESC, w.id DESC"
    ))
    .fetch_all(conn)
    .await
}

pub async fn get_workflow(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<Option<WorkflowRecord>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT w.id, w.name, w.slug, w.published_version,
                v.draft_revision AS published_draft_revision,
                d.revision, d.graph, d.updated_at
         FROM {WORKFLOWS}
         WHERE w.id = $1"
    ))
    .bind(id)
    .fetch_optional(conn)
    .await
}

/// Creates a workflow whose draft starts at revision 1 with `graph`.
pub async fn insert_workflow(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    name: &str,
    slug: &str,
    graph: &GraphSnapshot,
    blueprint_id: Option<Uuid>,
) -> Result<WorkflowRecord, sqlx::Error> {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO workflows(organization_id, name, slug, created_by)
         VALUES ($1, $2, $3, $4)
         RETURNING id",
    )
    .bind(organization_id)
    .bind(name)
    .bind(slug)
    .bind(actor_user_id)
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO workflow_drafts(workflow_id, organization_id, graph, revision, updated_by)
         VALUES ($1, $2, $3, 1, $4)",
    )
    .bind(id)
    .bind(organization_id)
    .bind(Json(graph))
    .bind(actor_user_id)
    .execute(&mut *conn)
    .await?;
    let metadata = blueprint_id.map_or_else(|| json!({}), |id| json!({ "blueprintId": id }));
    audit(
        conn,
        organization_id,
        actor_user_id,
        "workflow.created",
        id,
        metadata,
    )
    .await?;
    get_workflow(conn, id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

/// Replaces the draft if it is still at `expected_revision` and returns the new revision.
/// None means the revision moved on or the workflow is not visible; the caller then rolls
/// back. The workflow row is locked before the draft row, in `lock_workflow`'s order, so a
/// save and a publish of one workflow cannot deadlock.
pub async fn save_draft(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    workflow_id: Uuid,
    expected_revision: i64,
    graph: &GraphSnapshot,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query("UPDATE workflows SET updated_at = now() WHERE id = $1")
        .bind(workflow_id)
        .execute(&mut *conn)
        .await?;
    let revision: Option<i64> = sqlx::query_scalar(
        "UPDATE workflow_drafts
         SET graph = $3, revision = revision + 1, updated_by = $4, updated_at = now()
         WHERE workflow_id = $1 AND revision = $2
         RETURNING revision",
    )
    .bind(workflow_id)
    .bind(expected_revision)
    .bind(Json(graph))
    .bind(actor_user_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(revision) = revision else {
        return Ok(None);
    };
    let metadata = json!({ "revision": revision });
    audit(
        conn,
        organization_id,
        actor_user_id,
        "workflow.draft_saved",
        workflow_id,
        metadata,
    )
    .await?;
    Ok(Some(revision))
}

pub async fn workflow_exists(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM workflows WHERE id = $1)")
        .bind(id)
        .fetch_one(conn)
        .await
}

/// Locks the workflow and its draft until the transaction ends, so publishes and saves of
/// one workflow run one at a time.
pub async fn lock_workflow(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<Option<LockedWorkflow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT d.revision, d.graph, w.published_version
         FROM workflows w
         JOIN workflow_drafts d ON d.workflow_id = w.id
         WHERE w.id = $1
         FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(conn)
    .await
}

pub async fn version_exists_for_revision(
    conn: &mut PgConnection,
    workflow_id: Uuid,
    revision: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM workflow_versions WHERE workflow_id = $1 AND draft_revision = $2
         )",
    )
    .bind(workflow_id)
    .bind(revision)
    .fetch_one(conn)
    .await
}

/// Stores `graph` as the workflow's next version and makes it the published one. Callers
/// hold `lock_workflow`'s locks. A second version of one draft revision fails with a unique
/// violation on `workflow_versions_draft_revision`.
pub async fn insert_version(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    workflow_id: Uuid,
    draft_revision: i64,
    graph: &GraphSnapshot,
    checksum: &str,
) -> Result<VersionSummaryRecord, sqlx::Error> {
    let version: VersionSummaryRecord = sqlx::query_as(&format!(
        "INSERT INTO workflow_versions(
            organization_id, workflow_id, version, graph, checksum, draft_revision, published_by
         )
         SELECT $1, $2, COALESCE(MAX(version), 0) + 1, $3, $4, $5, $6
         FROM workflow_versions
         WHERE workflow_id = $2
         RETURNING {VERSION_SUMMARY}"
    ))
    .bind(organization_id)
    .bind(workflow_id)
    .bind(Json(graph))
    .bind(checksum)
    .bind(draft_revision)
    .bind(actor_user_id)
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query("UPDATE workflows SET published_version = $2, updated_at = now() WHERE id = $1")
        .bind(workflow_id)
        .bind(version.version)
        .execute(&mut *conn)
        .await?;
    let metadata = json!({
        "version": version.version,
        "draftRevision": draft_revision,
        "checksum": checksum,
    });
    audit(
        conn,
        organization_id,
        actor_user_id,
        "workflow.published",
        workflow_id,
        metadata,
    )
    .await?;
    Ok(version)
}

/// The workflow's versions, newest first.
pub async fn list_versions(
    conn: &mut PgConnection,
    workflow_id: Uuid,
) -> Result<Vec<VersionSummaryRecord>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {VERSION_SUMMARY} FROM workflow_versions
         WHERE workflow_id = $1
         ORDER BY version DESC"
    ))
    .bind(workflow_id)
    .fetch_all(conn)
    .await
}

pub async fn get_version(
    conn: &mut PgConnection,
    workflow_id: Uuid,
    version: i32,
) -> Result<Option<VersionRecord>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {VERSION_SUMMARY}, graph FROM workflow_versions
         WHERE workflow_id = $1 AND version = $2"
    ))
    .bind(workflow_id)
    .bind(version)
    .fetch_optional(conn)
    .await
}

async fn audit(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    action: &str,
    workflow_id: Uuid,
    metadata: Value,
) -> Result<(), sqlx::Error> {
    crate::audit::insert_in_transaction(
        conn,
        organization_id,
        actor_user_id,
        action,
        "workflow",
        &workflow_id.to_string(),
        metadata,
    )
    .await
}
