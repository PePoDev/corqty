#![forbid(unsafe_code)]

//! The worker (spec §10.2). It claims the node runs whose executor it has, runs each attempt
//! under a lease that it keeps renewing, and reports the attempt's logs and outcome. It
//! reaches the database only through the worker role's `corqty` functions.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use chrono::{TimeDelta, Utc};
use corqty_domain::run::is_retryable;
use corqty_domain::runtime::{
    MAX_LOG_BYTES_PER_NODE_RUN, NodeFailure, resolve_inputs, validate_output,
};
use corqty_persistence::worker_queue::{ClaimedJob, Job, NodeError, Outcome, WorkerDb};
use corqty_plugin_host::Registry;
use corqty_plugin_sdk::{
    ExecuteRequest, ExecutionContext, Executor, ExecutorError, LogChunk, LogStream,
};
use serde_json::{Map, Value};
use sqlx::PgPool;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::{Instant, interval_at, sleep_until, timeout};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// A running attempt's logs go to the database at least this often...
const LOG_FLUSH_INTERVAL: Duration = Duration::from_millis(500);
/// ...and as soon as this much text waits.
const LOG_FLUSH_BYTES: usize = 8 * 1024;
/// How many log chunks an executor may send before it waits for the worker.
const LOG_CHANNEL_CAPACITY: usize = 64;
/// A worker counts as alive while it has claimed this recently.
const ALIVE_MAX_AGE: Duration = Duration::from_secs(10);

/// How a worker runs. `Default` holds the production values.
#[derive(Clone, Debug)]
pub struct WorkerConfig {
    /// Keys the worker's leases and its `worker_instances` row: new for every process.
    pub worker_id: Uuid,
    /// How many attempts run at once.
    pub capacity: i32,
    /// How long a lease lasts without renewal.
    pub lease_seconds: i32,
    /// How often a running attempt renews its lease and checks for a cancel request.
    pub heartbeat: Duration,
    /// How often the worker claims work. Every claim, even with no free slot, shows the
    /// worker alive.
    pub poll: Duration,
    /// How long running attempts may go on finishing after shutdown begins.
    pub drain: Duration,
    /// How long an executor has to return once its attempt is canceled. Then it is dropped.
    pub cancel_grace: Duration,
    /// Where each attempt gets a scratch directory of its own.
    pub scratch_root: PathBuf,
    /// Touched after every successful claim, so that a healthcheck can tell the worker still
    /// reaches the database.
    pub alive_file: PathBuf,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            worker_id: Uuid::now_v7(),
            capacity: 4,
            lease_seconds: 30,
            heartbeat: Duration::from_secs(10),
            poll: Duration::from_millis(500),
            drain: Duration::from_secs(25),
            cancel_grace: Duration::from_secs(5),
            scratch_root: std::env::temp_dir(),
            alive_file: std::env::temp_dir().join("corqty-worker.alive"),
        }
    }
}

/// Registers the worker's executors, then claims and runs attempts until `shutdown` is
/// cancelled. It then stops claiming and gives running attempts `config.drain` to finish.
/// Attempts still running after that are stopped and never reported: their leases expire and
/// lease recovery retries them. Only a failed registration is returned; later database
/// errors are logged and tried again.
pub async fn run(
    config: WorkerConfig,
    registry: Registry,
    pool: PgPool,
    shutdown: CancellationToken,
) -> Result<(), sqlx::Error> {
    let db = WorkerDb::new(pool, config.worker_id);
    let executors = registry.advertised();
    retrying(|| db.register(&executors, config.capacity)).await?;
    let worker = Arc::new(Worker {
        config,
        registry,
        db,
        stop: CancellationToken::new(),
    });
    let mut attempts = JoinSet::new();
    let mut poll = tokio::time::interval(worker.config.poll);
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            _ = poll.tick() => {}
        }
        while attempts.try_join_next().is_some() {}
        let running = i32::try_from(attempts.len()).unwrap_or(i32::MAX);
        let free = worker.config.capacity - running;
        match retrying(|| worker.db.claim(free, worker.config.lease_seconds)).await {
            Ok(claimed) => {
                touch(&worker.config.alive_file);
                for job in claimed {
                    attempts.spawn(Arc::clone(&worker).attempt(job));
                }
            }
            Err(error) => tracing::warn!(error = %error, "claiming node runs failed"),
        }
    }

    tracing::info!(
        running = attempts.len(),
        "shutting down: finishing running attempts"
    );
    let drain = async { while attempts.join_next().await.is_some() {} };
    if timeout(worker.config.drain, drain).await.is_err() {
        tracing::warn!(
            running = attempts.len(),
            "the drain is over: stopping running attempts without reporting them"
        );
        worker.stop.cancel();
        while attempts.join_next().await.is_some() {}
    }
    Ok(())
}

/// Checks that a worker touched `alive_file` within the last 10 s, as it does after every
/// successful claim. The `healthcheck` subcommand runs this.
pub fn check_alive(alive_file: &Path) -> Result<(), String> {
    let modified = std::fs::metadata(alive_file)
        .and_then(|metadata| metadata.modified())
        .map_err(|error| format!("{}: {error}", alive_file.display()))?;
    // A file from the future, after the clock went back, counts as fresh.
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default();
    if age > ALIVE_MAX_AGE {
        return Err(format!(
            "{} is {} s old",
            alive_file.display(),
            age.as_secs()
        ));
    }
    Ok(())
}

/// Creates the file, or sets its modification time to now.
fn touch(path: &Path) {
    let touched = std::fs::File::create(path).and_then(|file| file.set_modified(SystemTime::now()));
    if let Err(error) = touched {
        tracing::warn!(error = %error, "touching the alive file failed");
    }
}

struct Worker {
    config: WorkerConfig,
    registry: Registry,
    db: WorkerDb,
    /// Fired when the drain is over: running attempts stop and report nothing.
    stop: CancellationToken,
}

/// How an attempt's executor ended.
enum Ended {
    Returned(Result<Map<String, Value>, ExecutorError>),
    /// The attempt reached its timeout.
    Deadline,
    /// The run was asked to cancel.
    Canceled,
    /// The lease is gone, or the worker is shutting down: nothing more is reported.
    Abandoned,
}

impl Worker {
    /// Starts a claimed attempt, runs it and reports its logs and outcome, unless the lease
    /// is gone or the worker shuts down first.
    async fn attempt(self: Arc<Self>, claimed: ClaimedJob) {
        let lease = (claimed.node_run_id, claimed.lease_token);
        let job = match retrying(|| self.db.start(lease.0, lease.1)).await {
            Ok(Some(job)) => job,
            // Lease recovery has taken the node run back.
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(error = %error, "starting a node run failed");
                return;
            }
        };
        let mut logs = Logs {
            db: &self.db,
            lease,
            pending: Vec::new(),
            taken: 0,
        };
        let outcome = if job.cancel_requested {
            Some(Outcome::Canceled)
        } else {
            self.execute(&claimed.executor, &job, &mut logs).await
        };
        let Some(outcome) = outcome else { return };
        // The logs go first: completing ends the lease that appending needs.
        if !logs.flush().await {
            return;
        }
        if let Err(error) = retrying(|| self.db.complete(lease.0, lease.1, &outcome)).await {
            tracing::warn!(error = %error, "completing a node run failed");
        }
    }

    /// Resolves the inputs and runs the executor in a new scratch directory. None when the
    /// outcome must not be reported.
    async fn execute(&self, executor: &str, job: &Job, logs: &mut Logs<'_>) -> Option<Outcome> {
        let found = executor
            .split_once('@')
            .and_then(|(key, version)| self.registry.find(key, version));
        let Some(found) = found else {
            let message = format!("This worker has no executor {executor}.");
            return Some(failed(None, "executor_missing", &message, false));
        };
        let node = job.graph.nodes.iter().find(|node| node.id == job.node_id);
        let node = node.expect("the pinned graph holds every node of its run");
        let resolved = resolve_inputs(
            node,
            &job.definition,
            &job.graph,
            &job.run_input,
            &job.upstream_outputs,
        );
        let inputs = match resolved {
            Ok(inputs) => inputs,
            Err(NodeFailure { code, message }) => return Some(failed(None, code, &message, false)),
        };
        let scratch = tempfile::Builder::new()
            .prefix("corqty-node-")
            .tempdir_in(&self.config.scratch_root);
        let scratch = match scratch {
            Ok(scratch) => scratch,
            Err(error) => {
                let message = format!("The worker could not create a scratch directory: {error}.");
                return Some(failed(Some(inputs), "scratch_unavailable", &message, true));
            }
        };
        let limit = Duration::from_secs(u64::from(job.timeout_seconds.unsigned_abs()));
        let request = ExecuteRequest {
            execution_id: job.execution_id,
            attempt: job.attempt.unsigned_abs(),
            workflow_id: job.workflow_id,
            run_id: job.run_id,
            node_id: job.node_id.clone(),
            inputs: inputs.clone(),
            config: node.config.clone(),
            deadline: Utc::now() + TimeDelta::seconds(i64::from(job.timeout_seconds)),
        };
        let ended = self
            .supervise(
                found.as_ref(),
                request,
                scratch.path().to_owned(),
                limit,
                logs,
            )
            .await;
        // The executor has returned or been dropped, so nothing uses the directory now.
        if let Ok(Err(error)) = tokio::task::spawn_blocking(move || scratch.close()).await {
            tracing::warn!(error = %error, "removing a scratch directory failed");
        }

        Some(match ended {
            Ended::Returned(Ok(output)) => match validate_output(&job.definition, &output) {
                Ok(()) => Outcome::Succeeded {
                    input: inputs,
                    output,
                },
                Err(NodeFailure { code, message }) => failed(Some(inputs), code, &message, false),
            },
            Ended::Returned(Err(error)) => {
                let retryable = is_retryable(&error.code, error.retryable);
                failed(Some(inputs), &error.code, &error.message, retryable)
            }
            Ended::Deadline => {
                let message = format!(
                    "The attempt did not finish within its timeout of {} s.",
                    job.timeout_seconds
                );
                let retryable = found.metadata().timeout_retryable;
                failed(Some(inputs), "timeout", &message, retryable)
            }
            Ended::Canceled => Outcome::Canceled,
            Ended::Abandoned => return None,
        })
    }

    /// Runs the executor while it takes the executor's logs and renews the lease. On the
    /// deadline, a cancel request, a lost lease or the end of the drain, it fires the
    /// attempt's token, gives the executor `cancel_grace` to return, and then drops it. The
    /// executor's own answer is ignored then.
    async fn supervise(
        &self,
        executor: &dyn Executor,
        request: ExecuteRequest,
        scratch_dir: PathBuf,
        limit: Duration,
        logs: &mut Logs<'_>,
    ) -> Ended {
        let (sender, mut receiver) = mpsc::channel(LOG_CHANNEL_CAPACITY);
        let context = ExecutionContext {
            cancel: CancellationToken::new(),
            scratch_dir,
            logs: sender,
        };
        let mut execution = executor.execute(request, &context);
        let start = Instant::now();
        let mut deadline = std::pin::pin!(sleep_until(start + limit));
        let period = self.config.heartbeat;
        let mut heartbeat = interval_at(start + period, period);
        let mut flush = interval_at(start + LOG_FLUSH_INTERVAL, LOG_FLUSH_INTERVAL);
        let leases = [logs.lease];
        let ended = loop {
            tokio::select! {
                result = &mut execution => {
                    while let Ok(chunk) = receiver.try_recv() {
                        logs.push(chunk);
                    }
                    return Ended::Returned(result);
                }
                Some(chunk) = receiver.recv() => {
                    logs.push(chunk);
                    if logs.is_full() && !logs.flush().await {
                        break Ended::Abandoned;
                    }
                }
                _ = flush.tick() => {
                    if !logs.flush().await {
                        break Ended::Abandoned;
                    }
                }
                _ = heartbeat.tick() => {
                    let lease_seconds = self.config.lease_seconds;
                    match retrying(|| self.db.heartbeat(&leases, lease_seconds)).await {
                        Ok(statuses) if statuses.iter().any(|status| !status.alive) => {
                            break Ended::Abandoned;
                        }
                        Ok(statuses) if statuses.iter().any(|status| status.cancel_requested) => {
                            break Ended::Canceled;
                        }
                        Ok(_) => {}
                        Err(error) => tracing::warn!(error = %error, "renewing a lease failed"),
                    }
                }
                () = &mut deadline => break Ended::Deadline,
                () = self.stop.cancelled() => break Ended::Abandoned,
            }
        };
        context.cancel.cancel();
        // Logs are still taken meanwhile, so the executor never waits on a full channel.
        let grace = async {
            loop {
                tokio::select! {
                    _ = &mut execution => break,
                    Some(chunk) = receiver.recv() => logs.push(chunk),
                }
            }
        };
        let _ = timeout(self.config.cancel_grace, grace).await;
        ended
    }
}

/// One attempt's log text on its way to `append_node_log`.
struct Logs<'a> {
    db: &'a WorkerDb,
    lease: (Uuid, i64),
    /// Text not sent yet, in order: one entry per run of chunks from the same stream.
    pending: Vec<(LogStream, String)>,
    /// How many bytes the executor has sent.
    taken: usize,
}

impl Logs<'_> {
    /// Keeps a chunk for the next flush until the attempt has sent more than the log cap,
    /// and drops it after that. The chunk that passes the cap is kept whole, so the database
    /// sees the text go on, cuts it at the cap and records one `node.log_truncated`.
    fn push(&mut self, chunk: LogChunk) {
        if self.taken > MAX_LOG_BYTES_PER_NODE_RUN {
            return;
        }
        self.taken += chunk.text.len();
        let text = printable(&chunk.text);
        match self.pending.last_mut() {
            Some((stream, pending)) if *stream == chunk.stream => pending.push_str(&text),
            _ => self.pending.push((chunk.stream, text)),
        }
    }

    fn is_full(&self) -> bool {
        let waiting: usize = self.pending.iter().map(|(_, text)| text.len()).sum();
        waiting >= LOG_FLUSH_BYTES
    }

    /// Sends the pending text. False when the lease is gone.
    async fn flush(&mut self) -> bool {
        let (node_run_id, lease_token) = self.lease;
        for (stream, text) in std::mem::take(&mut self.pending) {
            let stream = stream.as_str();
            match retrying(|| self.db.append_log(node_run_id, lease_token, stream, &text)).await {
                Ok(true) => {}
                Ok(false) => return false,
                // ponytail: text that a database error rejected is dropped, not sent again.
                Err(error) => tracing::warn!(error = %error, "appending a node log failed"),
            }
        }
        true
    }
}

fn failed(
    input: Option<Map<String, Value>>,
    code: &str,
    message: &str,
    retryable: bool,
) -> Outcome {
    let error = NodeError {
        code: code.to_owned(),
        message: printable(message),
        retryable,
    };
    Outcome::Failed { input, error }
}

/// `text` with U+0000, which PostgreSQL text and jsonb cannot hold, replaced by U+FFFD.
fn printable(text: &str) -> String {
    text.replace('\0', "\u{fffd}")
}

/// Makes a database call, and makes it again up to 3 times when PostgreSQL aborted it for a
/// deadlock (40P01) or a serialization failure (40001).
async fn retrying<T, F>(mut call: impl FnMut() -> F) -> Result<T, sqlx::Error>
where
    F: Future<Output = Result<T, sqlx::Error>>,
{
    let mut retries = 0;
    loop {
        match call().await {
            Err(error) if retries < 3 && is_transient(&error) => retries += 1,
            result => return result,
        }
    }
}

fn is_transient(error: &sqlx::Error) -> bool {
    let code = error.as_database_error().and_then(|error| error.code());
    code.is_some_and(|code| code == "40P01" || code == "40001")
}
