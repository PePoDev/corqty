//! The worker side of the run queue. The worker role has no table privileges: each call is
//! one `corqty` definer function, and every call after the claim is fenced by the worker id
//! and the lease token of the claim.

use std::collections::HashMap;

use corqty_domain::{catalog::NodeDefinitionSpec, graph::GraphSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::{PgPool, types::Json};
use uuid::Uuid;

/// A node run leased to this worker. Its id and lease token identify it in every later call.
#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct ClaimedJob {
    pub node_run_id: Uuid,
    pub run_id: Uuid,
    pub organization_id: Uuid,
    /// `"<key>@<version>"`, as the worker advertised it.
    pub executor: String,
    pub attempt: i32,
    pub lease_token: i64,
}

/// What a started attempt needs to resolve its inputs and run.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    /// The node run id, the same on every attempt.
    pub execution_id: Uuid,
    pub attempt: i32,
    pub max_attempts: i32,
    pub timeout_seconds: i32,
    pub workflow_id: Uuid,
    pub run_id: Uuid,
    pub node_id: String,
    pub cancel_requested: bool,
    pub run_input: Value,
    /// The pinned version's graph.
    pub graph: GraphSnapshot,
    /// The spec of the node's definition: a system one or the run's organization's own.
    pub definition: NodeDefinitionSpec,
    /// The output of each parent, by node id.
    pub upstream_outputs: HashMap<String, Map<String, Value>>,
}

/// One answer of a heartbeat. A lease that is not alive is gone: the worker stops the attempt
/// and reports nothing more about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct LeaseStatus {
    pub node_run_id: Uuid,
    pub alive: bool,
    pub cancel_requested: bool,
}

/// How an attempt ended.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    Succeeded {
        input: Map<String, Value>,
        output: Map<String, Value>,
    },
    /// `input` is None when the inputs could not be resolved.
    Failed {
        #[serde(skip_serializing_if = "Option::is_none")]
        input: Option<Map<String, Value>>,
        error: NodeError,
    },
    Canceled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NodeError {
    pub code: String,
    pub message: String,
    /// Whether another attempt may succeed. The database retries only while attempts are
    /// left and the run was not asked to cancel.
    pub retryable: bool,
}

/// One worker process's handle on the queue. The worker id keys its leases and its
/// `worker_instances` row.
#[derive(Clone, Debug)]
pub struct WorkerDb {
    pool: PgPool,
    worker_id: Uuid,
}

impl WorkerDb {
    #[must_use]
    pub const fn new(pool: PgPool, worker_id: Uuid) -> Self {
        Self { pool, worker_id }
    }

    #[must_use]
    pub const fn worker_id(&self) -> Uuid {
        self.worker_id
    }

    /// Records the executors this worker runs, as `"<key>@<version>"`, and its capacity.
    pub async fn register(&self, executors: &[String], capacity: i32) -> Result<(), sqlx::Error> {
        // Host contracts stay empty until plugins are loaded at run time.
        sqlx::query("SELECT corqty.register_worker($1, $2, '{}', $3)")
            .bind(self.worker_id)
            .bind(executors)
            .bind(capacity)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Leases up to `limit` due node runs of the registered executors for `lease_seconds`.
    /// Every call, `limit` 0 included, also counts as the worker's heartbeat.
    pub async fn claim(
        &self,
        limit: i32,
        lease_seconds: i32,
    ) -> Result<Vec<ClaimedJob>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM corqty.claim_node_runs($1, $2, $3)")
            .bind(self.worker_id)
            .bind(limit)
            .bind(lease_seconds)
            .fetch_all(&self.pool)
            .await
    }

    /// Starts a claimed attempt and returns its job, or None when the lease is gone.
    pub async fn start(
        &self,
        node_run_id: Uuid,
        lease_token: i64,
    ) -> Result<Option<Job>, sqlx::Error> {
        let job: Option<Json<Job>> = sqlx::query_scalar("SELECT corqty.start_node_run($1, $2, $3)")
            .bind(self.worker_id)
            .bind(node_run_id)
            .bind(lease_token)
            .fetch_one(&self.pool)
            .await?;
        Ok(job.map(|Json(job)| job))
    }

    /// Extends the `(node run id, lease token)` leases the worker still holds to
    /// `lease_seconds` from now. One status per lease, in order.
    pub async fn heartbeat(
        &self,
        leases: &[(Uuid, i64)],
        lease_seconds: i32,
    ) -> Result<Vec<LeaseStatus>, sqlx::Error> {
        let (node_run_ids, lease_tokens): (Vec<Uuid>, Vec<i64>) = leases.iter().copied().unzip();
        sqlx::query_as("SELECT * FROM corqty.heartbeat($1, $2, $3, $4)")
            .bind(self.worker_id)
            .bind(node_run_ids)
            .bind(lease_tokens)
            .bind(lease_seconds)
            .fetch_all(&self.pool)
            .await
    }

    /// Appends log text from `stream` (`stdout`, `stderr` or `system`). False only when the
    /// lease is gone: text past the node run's cap is dropped and still returns true.
    pub async fn append_log(
        &self,
        node_run_id: Uuid,
        lease_token: i64,
        stream: &str,
        text: &str,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar("SELECT corqty.append_node_log($1, $2, $3, $4, $5)")
            .bind(self.worker_id)
            .bind(node_run_id)
            .bind(lease_token)
            .bind(stream)
            .bind(text)
            .fetch_one(&self.pool)
            .await
    }

    /// Records how the attempt ended. False when the lease is gone.
    pub async fn complete(
        &self,
        node_run_id: Uuid,
        lease_token: i64,
        outcome: &Outcome,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar("SELECT corqty.complete_node_run($1, $2, $3, $4)")
            .bind(self.worker_id)
            .bind(node_run_id)
            .bind(lease_token)
            .bind(Json(outcome))
            .fetch_one(&self.pool)
            .await
    }
}
