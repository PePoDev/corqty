#![forbid(unsafe_code)]

//! The logical contract between the worker and an executor (spec §10.1). Milestone 3 compiles
//! executors into the worker; Milestone 4 runtime plugins implement the same operations.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use corqty_domain::diagnostic::Diagnostic;
use futures_util::future::BoxFuture;
use serde_json::{Map, Value};
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// The host contract version this worker implements.
pub const HOST_CONTRACT: &str = "1";

/// A host operation an executor declares it needs (spec §10.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Capability {
    Log,
    SecretRead,
    ScratchFs,
    Network { host: String, port: u16 },
    CommandExec { executables: Vec<String> },
}

/// Who an executor is and what it needs from the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutorMetadata {
    pub key: &'static str,
    pub version: &'static str,
    pub host_contract: &'static str,
    pub capabilities: Vec<Capability>,
    /// Whether an attempt that reaches its deadline may be retried. The worker enforces
    /// deadlines, so this flag classifies a timeout, not the executor's own error.
    pub timeout_retryable: bool,
}

/// One attempt of one node run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecuteRequest {
    /// The node run id. It stays the same on every attempt, so it is the idempotency key.
    pub execution_id: Uuid,
    pub attempt: u32,
    pub workflow_id: Uuid,
    pub run_id: Uuid,
    pub node_id: String,
    /// The resolved inputs, already validated against the input port schemas.
    pub inputs: Map<String, Value>,
    pub config: Value,
    /// When the worker cancels this attempt. Informational: executors watch only `cancel`.
    pub deadline: DateTime<Utc>,
}

/// What the host lends an executor for one attempt.
#[derive(Debug)]
pub struct ExecutionContext {
    /// Fired by the worker on a deadline, a user cancel or shutdown. The executor then returns
    /// an `ExecutorError` with code `canceled` promptly.
    pub cancel: CancellationToken,
    /// A directory the worker creates for this attempt only and deletes afterwards.
    pub scratch_dir: PathBuf,
    /// A bounded channel: `send(..).await` waits while the worker catches up. Executors ignore
    /// a closed channel.
    pub logs: Sender<LogChunk>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
    System,
}

impl LogStream {
    /// The stream's name in `append_node_log` and in `node.log` events.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::System => "system",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogChunk {
    pub stream: LogStream,
    pub text: String,
}

/// Why an attempt failed. `retryable` is the executor's opinion; the codes in
/// `corqty_domain::run::NON_RETRYABLE` are never retried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutorError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

/// An executor compiled into the worker.
pub trait Executor: Send + Sync {
    fn metadata(&self) -> ExecutorMetadata;

    /// Problems in a node's `config`, located by JSON Pointer. The Milestone 3 worker never
    /// calls it, because publish has already checked config against the definition's schema.
    fn validate(&self, config: &Value) -> Vec<Diagnostic>;

    /// Runs one attempt and returns the output object, one entry per output port.
    fn execute<'a>(
        &'a self,
        request: ExecuteRequest,
        context: &'a ExecutionContext,
    ) -> BoxFuture<'a, Result<Map<String, Value>, ExecutorError>>;
}
