use chrono::{DateTime, Utc};
use corqty_domain::{graph::GraphSnapshot, run::PlannedNode};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, types::Json};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum CancelError {
    #[error("run was not found")]
    NotFound,
    #[error("run has already finished")]
    Finished,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// A run of a published version, with the plan its nodes start from.
pub struct NewRun<'a> {
    pub organization_id: Uuid,
    pub actor_user_id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_version_id: Uuid,
    pub version: i32,
    pub input: &'a Value,
    pub nodes: &'a [PlannedNode],
    /// The run this one repeats.
    pub rerun_of: Option<Uuid>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RunSummaryRecord {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub version: i32,
    pub status: String,
    pub trigger_source: String,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub cancel_requested: bool,
}

/// A run with its pinned graph and its nodes in plan order.
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub summary: RunSummaryRecord,
    pub workflow_name: String,
    pub input: Value,
    pub last_event_seq: i64,
    #[sqlx(json)]
    pub graph: GraphSnapshot,
    #[sqlx(skip)]
    pub nodes: Vec<NodeRunRecord>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct NodeRunRecord {
    pub node_id: String,
    pub state: String,
    pub attempt: i32,
    pub max_attempts: i32,
    pub executor: String,
    /// `waiting_for_compatible_worker` while the node waits for a worker and no live worker
    /// runs its executor. Derived on read, never stored.
    pub queue_reason: Option<String>,
    pub input: Option<Value>,
    pub output: Option<Value>,
    pub error: Option<Value>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct RunEventRecord {
    pub seq: i64,
    pub payload: Value,
}

const RUN_SUMMARY: &str = "r.id, r.workflow_id, r.version, r.status, r.trigger_source,
     r.created_at, r.started_at, r.finished_at,
     r.cancel_requested_at IS NOT NULL AS cancel_requested";

/// Creates a queued run: roots start queued, and every other node waits blocked for its
/// distinct parents. Its first event is `run.state queued`; the snapshot holds the node
/// states. A plan without nodes has nothing to wait for and settles at once.
pub async fn create_run(
    conn: &mut PgConnection,
    run: &NewRun<'_>,
) -> Result<RunSummaryRecord, sqlx::Error> {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO runs(
            organization_id, workflow_id, workflow_version_id, version, input, rerun_of,
            created_by
         ) VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id",
    )
    .bind(run.organization_id)
    .bind(run.workflow_id)
    .bind(run.workflow_version_id)
    .bind(run.version)
    .bind(Json(run.input))
    .bind(run.rerun_of)
    .bind(run.actor_user_id)
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO node_runs(
            organization_id, run_id, node_id, executor, state,
            max_attempts, timeout_seconds, pending_parents, children
         )
         SELECT $1, $2, node_id, executor,
                CASE WHEN pending_parents = 0 THEN 'queued' ELSE 'blocked' END,
                max_attempts, timeout_seconds, pending_parents, children
         FROM jsonb_to_recordset($3) AS node(
             node_id text, executor text, max_attempts integer, timeout_seconds integer,
             pending_parents integer, children text[]
         )",
    )
    .bind(run.organization_id)
    .bind(id)
    .bind(Json(run.nodes))
    .execute(&mut *conn)
    .await?;
    append_run_event(conn, id, &json!({"type": "run.state", "state": "queued"})).await?;
    settle_run(conn, id).await?;
    let (action, metadata) = match run.rerun_of {
        None => ("run.started", json!({"runId": id, "version": run.version})),
        Some(source) => (
            "run.rerun",
            json!({"runId": id, "sourceRunId": source, "version": run.version}),
        ),
    };
    audit(
        conn,
        run.organization_id,
        run.actor_user_id,
        action,
        id,
        metadata,
    )
    .await?;
    summary(conn, id).await
}

/// The workflow's runs, newest first.
// ponytail: the 50 newest, without paging.
pub async fn list_runs(
    conn: &mut PgConnection,
    workflow_id: Uuid,
) -> Result<Vec<RunSummaryRecord>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {RUN_SUMMARY} FROM runs r
         WHERE r.workflow_id = $1
         ORDER BY r.created_at DESC, r.id DESC
         LIMIT 50"
    ))
    .bind(workflow_id)
    .fetch_all(conn)
    .await
}

/// Reads the run before its nodes, never the other way round: the node states are then at
/// least as new as `last_event_seq`, so a client that applies the later events stays current.
/// A queued or retrying node whose executor no worker has advertised within the last minute
/// gets a queue reason.
pub async fn get_run(
    conn: &mut PgConnection,
    run_id: Uuid,
) -> Result<Option<RunRecord>, sqlx::Error> {
    let run: Option<RunRecord> = sqlx::query_as(&format!(
        "SELECT {RUN_SUMMARY}, w.name AS workflow_name, r.input, r.last_event_seq, v.graph
         FROM runs r
         JOIN workflows w ON w.id = r.workflow_id
         JOIN workflow_versions v ON v.id = r.workflow_version_id
         WHERE r.id = $1"
    ))
    .bind(run_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(mut run) = run else {
        return Ok(None);
    };
    run.nodes = sqlx::query_as(
        "SELECT n.node_id, n.state, n.attempt, n.max_attempts, n.executor,
                CASE WHEN n.state IN ('queued', 'retry_wait')
                          AND NOT n.executor = ANY (live.executors)
                     THEN 'waiting_for_compatible_worker'
                END AS queue_reason,
                n.input, n.output, n.error, n.started_at, n.finished_at
         FROM node_runs n, corqty.live_executors() AS live(executors)
         WHERE n.run_id = $1
         ORDER BY n.id",
    )
    .bind(run_id)
    .fetch_all(conn)
    .await?;
    Ok(Some(run))
}

/// Requests the cancellation of an unfinished run. Its blocked, queued and retry_wait nodes
/// are canceled now; running nodes are left to their workers, which see the request on their
/// next heartbeat. The run settles as canceled once no node can change.
pub async fn cancel_run(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    run_id: Uuid,
) -> Result<RunSummaryRecord, CancelError> {
    // The runs row first, then the node rows in one statement ordered by id: every writer of
    // a run locks in this order, so a cancel cannot deadlock with a completing worker.
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM runs WHERE id = $1 FOR UPDATE")
            .bind(run_id)
            .fetch_optional(&mut *conn)
            .await?;
    match status.as_deref() {
        None => return Err(CancelError::NotFound),
        Some("queued" | "running") => {}
        Some(_) => return Err(CancelError::Finished),
    }
    sqlx::query(
        "WITH canceled AS (
            UPDATE node_runs AS n
            SET state = 'canceled', finished_at = now()
            FROM (
                SELECT id FROM node_runs
                WHERE run_id = $1 AND state IN ('blocked', 'queued', 'retry_wait')
                ORDER BY id
                FOR UPDATE
            ) AS waiting
            WHERE n.id = waiting.id
            RETURNING n.id, n.node_id, n.attempt
         )
         SELECT corqty.append_run_event($1, jsonb_build_object(
             'type', 'node.state', 'nodeId', node_id, 'state', 'canceled', 'attempt', attempt
         ))
         FROM canceled
         ORDER BY id",
    )
    .bind(run_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE runs SET cancel_requested_at = coalesce(cancel_requested_at, now())
         WHERE id = $1",
    )
    .bind(run_id)
    .execute(&mut *conn)
    .await?;
    settle_run(conn, run_id).await?;
    let metadata = json!({"runId": run_id});
    audit(
        conn,
        organization_id,
        actor_user_id,
        "run.canceled",
        run_id,
        metadata,
    )
    .await?;
    Ok(summary(conn, run_id).await?)
}

/// Up to `limit` of the run's events after seq `after`, in order.
pub async fn events_after(
    conn: &mut PgConnection,
    run_id: Uuid,
    after: i64,
    limit: i64,
) -> Result<Vec<RunEventRecord>, sqlx::Error> {
    sqlx::query_as(
        "SELECT seq, payload FROM run_events
         WHERE run_id = $1 AND seq > $2
         ORDER BY seq
         LIMIT $3",
    )
    .bind(run_id)
    .bind(after)
    .bind(limit)
    .fetch_all(conn)
    .await
}

/// Appends an event through `corqty.append_run_event`, which numbers it. None means the run
/// is not visible to the caller.
pub async fn append_run_event(
    conn: &mut PgConnection,
    run_id: Uuid,
    payload: &Value,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar("SELECT corqty.append_run_event($1, $2)")
        .bind(run_id)
        .bind(Json(payload))
        .fetch_one(conn)
        .await
}

/// Finishes the run through `corqty.settle_run` if none of its nodes can change any more.
pub async fn settle_run(conn: &mut PgConnection, run_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT corqty.settle_run($1)")
        .bind(run_id)
        .execute(conn)
        .await?;
    Ok(())
}

async fn summary(conn: &mut PgConnection, run_id: Uuid) -> Result<RunSummaryRecord, sqlx::Error> {
    sqlx::query_as(&format!("SELECT {RUN_SUMMARY} FROM runs r WHERE r.id = $1"))
        .bind(run_id)
        .fetch_one(conn)
        .await
}

async fn audit(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    action: &str,
    run_id: Uuid,
    metadata: Value,
) -> Result<(), sqlx::Error> {
    crate::audit::insert_in_transaction(
        conn,
        organization_id,
        actor_user_id,
        action,
        "run",
        &run_id.to_string(),
        metadata,
    )
    .await
}
