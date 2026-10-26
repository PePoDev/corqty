use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use corqty_domain::{
    diagnostic::Diagnostic, graph::GraphSnapshot, publish::canonical_checksum, run::plan_run,
};
use corqty_executor_mock::MockExecutor;
use corqty_executor_shell::ShellExecutor;
use corqty_persistence::{
    catalog,
    runs::{self, NewRun},
    tenant::begin_tenant,
    worker_queue::recover_expired_leases,
    workflows,
};
use corqty_plugin_host::Registry;
use corqty_plugin_sdk::{
    ExecuteRequest, ExecutionContext, Executor, ExecutorError, ExecutorMetadata, LogChunk,
    LogStream,
};
use corqty_test_support::TestDatabase;
use corqty_worker::{WorkerConfig, check_alive, run};
use futures_util::future::BoxFuture;
use serde_json::{Map, Value, json};
use serial_test::serial;
use sqlx::{Postgres, Transaction};
use tempfile::TempDir;
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep, timeout};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const INFRASTRUCTURE: &str = "00000000-0000-7000-8000-000000000101";
const DATA_PROCESSING: &str = "00000000-0000-7000-8000-000000000102";

/// The mock executor the recording one delegates to.
static MOCK: MockExecutor = MockExecutor;

/// An organization with an admin who runs the workflows, and the root under which the
/// workers create scratch directories.
struct Fixture {
    database: TestDatabase,
    organization_id: Uuid,
    admin: Uuid,
    scratch_root: TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let database = TestDatabase::reset().await;
        let (organization_id, admin) = (Uuid::new_v4(), Uuid::new_v4());
        sqlx::query("INSERT INTO users(id, email) VALUES ($1, 'admin@example.com')")
            .bind(admin)
            .execute(&database.owner)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO organizations(id, name, slug, created_by) VALUES ($1, 'Acme', 'acme', $2)",
        )
        .bind(organization_id)
        .bind(admin)
        .execute(&database.owner)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships(organization_id, user_id, role) VALUES ($1, $2, 'admin')",
        )
        .bind(organization_id)
        .bind(admin)
        .execute(&database.owner)
        .await
        .unwrap();
        Self {
            database,
            organization_id,
            admin,
            scratch_root: TempDir::new().unwrap(),
        }
    }

    async fn tx(&self) -> Transaction<'static, Postgres> {
        begin_tenant(&self.database.api, self.admin, self.organization_id)
            .await
            .unwrap()
    }

    async fn blueprint(&self, id: &str) -> GraphSnapshot {
        let mut tx = self.tx().await;
        let blueprint = catalog::get_blueprint(&mut tx, id.parse().unwrap()).await;
        tx.commit().await.unwrap();
        blueprint.unwrap().unwrap().graph
    }

    /// Publishes `graph` as version 1 of a new workflow, without publish validation, and
    /// starts a run of it with `input`.
    async fn run(&self, graph: &GraphSnapshot, input: Value) -> Uuid {
        let (organization_id, admin) = (self.organization_id, self.admin);
        let slug = Uuid::new_v4().to_string();
        let mut tx = self.tx().await;
        let workflow =
            workflows::insert_workflow(&mut tx, organization_id, admin, "Flow", &slug, graph, None)
                .await
                .unwrap();
        let checksum = canonical_checksum(graph);
        let version = workflows::insert_version(
            &mut tx,
            organization_id,
            admin,
            workflow.id,
            1,
            graph,
            &checksum,
        )
        .await
        .unwrap();
        let definitions = catalog::definitions_map(&mut tx).await.unwrap();
        let nodes = plan_run(graph, &definitions).unwrap();
        let new = NewRun {
            organization_id,
            actor_user_id: admin,
            workflow_id: workflow.id,
            workflow_version_id: version.id,
            version: 1,
            input: &input,
            nodes: &nodes,
            rerun_of: None,
        };
        let run = runs::create_run(&mut tx, &new).await.unwrap();
        tx.commit().await.unwrap();
        run.id
    }

    /// Short timings for tests: a 3 s lease renewed every 200 ms, a claim every 50 ms, no
    /// drain and a 1 s cancel grace.
    fn config(&self, capacity: i32) -> WorkerConfig {
        WorkerConfig {
            capacity,
            lease_seconds: 3,
            heartbeat: Duration::from_millis(200),
            poll: Duration::from_millis(50),
            drain: Duration::ZERO,
            cancel_grace: Duration::from_secs(1),
            scratch_root: self.scratch_root.path().to_owned(),
            ..WorkerConfig::default()
        }
    }

    /// Starts a worker with `executors` on the worker role's pool.
    fn start(&self, config: WorkerConfig, executors: Vec<Arc<dyn Executor>>) -> Running {
        let id = config.worker_id;
        let registry = Registry::new(executors).unwrap();
        let shutdown = CancellationToken::new();
        let pool = self.database.worker.clone();
        let task = tokio::spawn(run(config, registry, pool, shutdown.clone()));
        Running { id, shutdown, task }
    }

    async fn status(&self, run: Uuid) -> String {
        sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
            .bind(run)
            .fetch_one(&self.database.owner)
            .await
            .unwrap()
    }

    /// Waits for the run to finish and returns its status.
    async fn finished(&self, run: Uuid) -> String {
        eventually(|| async move {
            let status = self.status(run).await;
            (!matches!(status.as_str(), "queued" | "running")).then_some(status)
        })
        .await
    }

    /// Waits until a worker has started an attempt of the node.
    async fn started(&self, run: Uuid, node_id: &str) {
        eventually(|| async move {
            let started: bool = sqlx::query_scalar(
                "SELECT state = 'running' AND started_at IS NOT NULL
                 FROM node_runs WHERE run_id = $1 AND node_id = $2",
            )
            .bind(run)
            .bind(node_id)
            .fetch_one(&self.database.owner)
            .await
            .unwrap();
            started.then_some(())
        })
        .await;
    }

    /// Each node's state, attempt, input, output and error, by node id.
    async fn nodes(&self, run: Uuid) -> Value {
        sqlx::query_scalar(
            "SELECT jsonb_object_agg(node_id, jsonb_build_object(
                 'state', state, 'attempt', attempt, 'input', input, 'output', output,
                 'error', error
             ))
             FROM node_runs WHERE run_id = $1",
        )
        .bind(run)
        .fetch_one(&self.database.owner)
        .await
        .unwrap()
    }

    /// The run's event payloads in sequence order.
    async fn events(&self, run: Uuid) -> Vec<Value> {
        sqlx::query_scalar("SELECT payload FROM run_events WHERE run_id = $1 ORDER BY seq")
            .bind(run)
            .fetch_all(&self.database.owner)
            .await
            .unwrap()
    }

    /// How many entries the scratch root holds.
    fn scratch_entries(&self) -> usize {
        std::fs::read_dir(self.scratch_root.path()).unwrap().count()
    }
}

/// A worker running in the background.
struct Running {
    id: Uuid,
    shutdown: CancellationToken,
    task: JoinHandle<Result<(), sqlx::Error>>,
}

impl Running {
    /// Shuts the worker down and waits for it to return.
    async fn stop(self) {
        self.shutdown.cancel();
        let stopped = timeout(Duration::from_secs(5), self.task).await;
        stopped
            .expect("the worker stops within 5 s")
            .unwrap()
            .unwrap();
    }
}

/// What the executor saw of one attempt.
struct Seen {
    execution_id: Uuid,
    attempt: u32,
    scratch_dir: PathBuf,
    scratch_existed: bool,
}

/// The mock executor that records each attempt. A config `log` text is first written to
/// stdout `repeat` times (once by default); jsonb cannot hold U+0000, so the config spells it
/// U+2400.
#[derive(Clone, Default)]
struct Recording {
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Recording {
    fn seen(&self) -> Vec<Seen> {
        std::mem::take(&mut *self.seen.lock().unwrap())
    }
}

impl Executor for Recording {
    fn metadata(&self) -> ExecutorMetadata {
        MOCK.metadata()
    }

    fn validate(&self, config: &Value) -> Vec<Diagnostic> {
        MOCK.validate(config)
    }

    fn execute<'a>(
        &'a self,
        request: ExecuteRequest,
        context: &'a ExecutionContext,
    ) -> BoxFuture<'a, Result<Map<String, Value>, ExecutorError>> {
        self.seen.lock().unwrap().push(Seen {
            execution_id: request.execution_id,
            attempt: request.attempt,
            scratch_dir: context.scratch_dir.clone(),
            scratch_existed: context.scratch_dir.is_dir(),
        });
        Box::pin(async move {
            if let Some(text) = request.config["log"].as_str() {
                for _ in 0..request.config["repeat"].as_u64().unwrap_or(1) {
                    let text = text.replace('\u{2400}', "\0");
                    let chunk = LogChunk {
                        stream: LogStream::Stdout,
                        text,
                    };
                    let _ = context.logs.send(chunk).await;
                }
            }
            MOCK.execute(request, context).await
        })
    }
}

fn mock() -> Vec<Arc<dyn Executor>> {
    vec![Arc::new(MockExecutor)]
}

/// Polls `check` every 20 ms until it returns a value, for up to 10 s.
async fn eventually<T, F>(mut check: impl FnMut() -> F) -> T
where
    F: Future<Output = Option<T>>,
{
    let polling = async {
        loop {
            if let Some(value) = check().await {
                return value;
            }
            sleep(Duration::from_millis(20)).await;
        }
    };
    timeout(Duration::from_secs(10), polling)
        .await
        .expect("the condition holds within 10 s")
}

/// Sets the modification time of the file at `path`.
fn set_modified(path: &Path, time: SystemTime) {
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(time).unwrap();
}

/// `base` with the top-level fields of `changes` replaced.
fn merge(mut base: Value, changes: Value) -> Value {
    let Value::Object(changes) = changes else {
        panic!("not an object: {changes}");
    };
    base.as_object_mut().unwrap().extend(changes);
    base
}

/// A `corqty.mock.task` node with one attempt whose mock returns `{"result": <id>}` at once,
/// with `config` replacing fields of that mock config.
fn task(id: &str, config: Value) -> Value {
    let mock = json!({
        "mode": "fixed",
        "output": {"result": id},
        "delayMs": 0,
        "failureMessage": null
    });
    json!({
        "id": id,
        "definition": {"key": "corqty.mock.task", "version": 1},
        "displayName": id,
        "position": {"x": 0, "y": 0},
        "config": merge(mock, config),
        "inputs": {},
        "retry": {"maxAttempts": 1},
        "timeoutSeconds": 60
    })
}

/// An edge from the result of `source` into the input of `target`.
fn edge(source: &str, target: &str) -> Value {
    json!({
        "id": format!("{source}-{target}"),
        "source": {"nodeId": source, "port": "result"},
        "target": {"nodeId": target, "port": "input"}
    })
}

fn graph(nodes: &[Value], edges: &[Value]) -> GraphSnapshot {
    serde_json::from_value(json!({
        "schemaVersion": 1,
        "workflowInputSchema": {"type": "object"},
        "nodes": nodes,
        "edges": edges,
        "viewport": {"x": 0, "y": 0, "zoom": 1}
    }))
    .unwrap()
}

/// What happened to one node, in event order: `<state> <attempt>` for a state change,
/// `<stream>: <text>` for log text, and the event type otherwise.
fn timeline(events: &[Value], node_id: &str) -> Vec<String> {
    let text = |event: &Value, key: &str| event[key].as_str().unwrap().to_owned();
    events
        .iter()
        .filter(|event| event["nodeId"] == node_id)
        .map(|event| match event["type"].as_str().unwrap() {
            "node.state" => format!("{} {}", text(event, "state"), event["attempt"]),
            "node.log" => format!("{}: {}", text(event, "stream"), text(event, "text")),
            other => other.to_owned(),
        })
        .collect()
}

/// The state changes in a timeline, without its log lines.
fn changes(timeline: &[String]) -> Vec<&str> {
    timeline
        .iter()
        .map(String::as_str)
        .filter(|entry| !entry.contains(": "))
        .collect()
}

#[tokio::test]
#[serial]
async fn both_starter_blueprints_run_to_completion() {
    let fixture = Fixture::new().await;
    let infrastructure = fixture.blueprint(INFRASTRUCTURE).await;
    let infrastructure = fixture
        .run(&infrastructure, json!({"region": "eu-west-1"}))
        .await;
    let data = fixture.blueprint(DATA_PROCESSING).await;
    let data = fixture.run(&data, json!({"payload": {"a": 1}})).await;

    let worker = fixture.start(fixture.config(4), mock());
    assert_eq!(fixture.finished(infrastructure).await, "succeeded");
    assert_eq!(fixture.finished(data).await, "succeeded");
    worker.stop().await;

    // vpc's output reaches the inputs of rds and deploy.
    assert_eq!(
        fixture.nodes(infrastructure).await,
        json!({
            "vpc": {
                "state": "succeeded", "attempt": 1, "input": {"region": "eu-west-1"},
                "output": {"vpcId": "vpc-0simulated"}, "error": null
            },
            "rds": {
                "state": "succeeded", "attempt": 1, "input": {"vpcId": "vpc-0simulated"},
                "output": {"endpoint": "db.simulated.internal:5432"}, "error": null
            },
            "deploy": {
                "state": "succeeded", "attempt": 1, "input": {"vpcId": "vpc-0simulated"},
                "output": {"url": "https://service.simulated.internal"}, "error": null
            }
        })
    );
    assert_eq!(
        fixture.nodes(data).await,
        json!({
            "normalize": {
                "state": "succeeded", "attempt": 1, "input": {"payload": {"a": 1}},
                "output": {"normalized": {"records": 3}}, "error": null
            },
            "summary": {
                "state": "succeeded", "attempt": 1, "input": {"normalized": {"records": 3}},
                "output": {"summary": "3 records processed"}, "error": null
            }
        })
    );

    // Each attempt's log line is flushed before the attempt completes.
    let events = fixture.events(infrastructure).await;
    let log = "system: Simulating 500 ms of work.";
    assert_eq!(timeline(&events, "vpc"), ["running 1", log, "succeeded 1"]);
    for node_id in ["rds", "deploy"] {
        let expected = ["queued 0", "running 1", log, "succeeded 1"];
        assert_eq!(timeline(&events, node_id), expected);
    }
    assert_eq!(
        events.first(),
        Some(&json!({"type": "run.state", "state": "queued", "seq": 1}))
    );
    assert_eq!(events.last().unwrap()["state"], "succeeded");
    assert_eq!(fixture.scratch_entries(), 0);
}

#[tokio::test]
#[serial]
async fn three_workers_with_one_slot_each_run_the_branches_in_parallel() {
    let fixture = Fixture::new().await;
    let graph = fixture.blueprint(INFRASTRUCTURE).await;
    let run = fixture.run(&graph, json!({"region": "eu-west-1"})).await;

    let workers: Vec<Running> = (0..3)
        .map(|_| fixture.start(fixture.config(1), mock()))
        .collect();
    // rds and deploy run at the same time, so two different workers hold their leases.
    let owner = &fixture.database.owner;
    let leased_by: Vec<Uuid> = eventually(|| async move {
        let leased_by: Vec<Uuid> = sqlx::query_scalar(
            "SELECT leased_by FROM node_runs
             WHERE run_id = $1 AND node_id IN ('rds', 'deploy') AND state = 'running'",
        )
        .bind(run)
        .fetch_all(owner)
        .await
        .unwrap();
        (leased_by.len() == 2).then_some(leased_by)
    })
    .await;
    assert_ne!(leased_by[0], leased_by[1]);
    assert!(
        leased_by
            .iter()
            .all(|id| workers.iter().any(|worker| worker.id == *id))
    );
    assert_eq!(fixture.finished(run).await, "succeeded");
    for worker in workers {
        worker.stop().await;
    }

    let overlap: bool = sqlx::query_scalar(
        "SELECT rds.started_at < deploy.finished_at AND deploy.started_at < rds.finished_at
         FROM node_runs rds JOIN node_runs deploy USING (run_id)
         WHERE rds.run_id = $1 AND rds.node_id = 'rds' AND deploy.node_id = 'deploy'",
    )
    .bind(run)
    .fetch_one(&fixture.database.owner)
    .await
    .unwrap();
    assert!(overlap);
    // Each branch was released once and ran once.
    let events = fixture.events(run).await;
    for node_id in ["rds", "deploy"] {
        let entries = timeline(&events, node_id);
        assert_eq!(changes(&entries), ["queued 0", "running 1", "succeeded 1"]);
    }
}

#[tokio::test]
#[serial]
async fn a_failed_node_skips_only_its_descendants() {
    let fixture = Fixture::new().await;
    let broken = task("broken", json!({"failureMessage": "The mock broke."}));
    let nodes = [broken, task("child", json!({})), task("other", json!({}))];
    let run = fixture
        .run(&graph(&nodes, &[edge("broken", "child")]), json!({}))
        .await;

    let worker = fixture.start(fixture.config(4), mock());
    assert_eq!(fixture.finished(run).await, "failed");
    worker.stop().await;

    let error = json!({"code": "mock_failure", "message": "The mock broke.", "retryable": false});
    assert_eq!(
        fixture.nodes(run).await,
        json!({
            "broken": {"state": "failed", "attempt": 1, "input": {}, "output": null, "error": error},
            "child": {"state": "skipped", "attempt": 0, "input": null, "output": null, "error": null},
            "other": {
                "state": "succeeded", "attempt": 1, "input": {}, "output": {"result": "other"},
                "error": null
            }
        })
    );
}

#[tokio::test]
#[serial]
async fn a_retryable_failure_runs_again_with_the_same_execution_id_and_a_new_scratch_directory() {
    let fixture = Fixture::new().await;
    let flaky = merge(
        task("flaky", json!({"failAttempts": 1})),
        json!({"retry": {"maxAttempts": 2}}),
    );
    let run = fixture.run(&graph(&[flaky], &[]), json!({})).await;

    let recording = Recording::default();
    let worker = fixture.start(fixture.config(1), vec![Arc::new(recording.clone())]);
    // The retry waits 2.0 to 2.4 s.
    assert_eq!(fixture.finished(run).await, "succeeded");
    worker.stop().await;

    let node_run_id: Uuid = sqlx::query_scalar("SELECT id FROM node_runs WHERE run_id = $1")
        .bind(run)
        .fetch_one(&fixture.database.owner)
        .await
        .unwrap();
    let seen = recording.seen();
    let attempts: Vec<(Uuid, u32, bool)> = seen
        .iter()
        .map(|seen| (seen.execution_id, seen.attempt, seen.scratch_existed))
        .collect();
    assert_eq!(attempts, [(node_run_id, 1, true), (node_run_id, 2, true)]);
    assert_ne!(seen[0].scratch_dir, seen[1].scratch_dir);
    for seen in &seen {
        assert!(seen.scratch_dir.starts_with(fixture.scratch_root.path()));
        assert!(!seen.scratch_dir.exists());
    }
    assert_eq!(fixture.scratch_entries(), 0);

    let events = fixture.events(run).await;
    let log = "system: Simulating 0 ms of work.";
    let expected = [
        "running 1",
        log,
        "retry_wait 1",
        "running 2",
        log,
        "succeeded 2",
    ];
    assert_eq!(timeline(&events, "flaky"), expected);
    let retry = events.iter().find(|event| event["state"] == "retry_wait");
    assert_eq!(
        retry.unwrap()["error"],
        json!({
            "code": "mock_retryable_failure",
            "message": "Attempt 1 fails because failAttempts is 1.",
            "retryable": true
        })
    );
}

#[tokio::test]
#[serial]
async fn echo_returns_the_resolved_inputs_as_the_result() {
    let fixture = Fixture::new().await;
    let echo = merge(
        task("echo", json!({"mode": "echo", "output": {}})),
        json!({"inputs": {"input": {"kind": "literal", "value": {"name": "corqty"}}}}),
    );
    let run = fixture.run(&graph(&[echo], &[]), json!({})).await;

    let worker = fixture.start(fixture.config(1), mock());
    assert_eq!(fixture.finished(run).await, "succeeded");
    worker.stop().await;

    assert_eq!(
        fixture.nodes(run).await,
        json!({
            "echo": {
                "state": "succeeded", "attempt": 1, "input": {"input": {"name": "corqty"}},
                "output": {"result": {"input": {"name": "corqty"}}}, "error": null
            }
        })
    );
}

#[tokio::test]
#[serial]
async fn a_cancel_request_stops_the_running_attempt() {
    let fixture = Fixture::new().await;
    let slow = task("slow", json!({"delayMs": 10_000}));
    let run = fixture.run(&graph(&[slow], &[]), json!({})).await;

    let worker = fixture.start(fixture.config(1), mock());
    fixture.started(run, "slow").await;
    let canceled = Instant::now();
    let mut tx = fixture.tx().await;
    runs::cancel_run(&mut tx, fixture.organization_id, fixture.admin, run)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    // The next heartbeat reports the request, and the mock returns as soon as it is told.
    assert_eq!(fixture.finished(run).await, "canceled");
    assert!(canceled.elapsed() < Duration::from_secs(2));
    worker.stop().await;

    assert_eq!(
        fixture.nodes(run).await,
        json!({
            "slow": {"state": "canceled", "attempt": 1, "input": null, "output": null, "error": null}
        })
    );
    let events = fixture.events(run).await;
    let expected = [
        "running 1",
        "system: Simulating 10000 ms of work.",
        "canceled 1",
    ];
    assert_eq!(timeline(&events, "slow"), expected);
    assert_eq!(fixture.scratch_entries(), 0);
}

#[tokio::test]
#[serial]
async fn an_attempt_past_its_deadline_fails_with_timeout() {
    let fixture = Fixture::new().await;
    let slow = merge(
        task("slow", json!({"delayMs": 3000})),
        json!({"retry": {"maxAttempts": 2}, "timeoutSeconds": 1}),
    );
    let run = fixture.run(&graph(&[slow], &[]), json!({})).await;

    let worker = fixture.start(fixture.config(1), mock());
    fixture.started(run, "slow").await;
    let started = Instant::now();
    assert_eq!(fixture.finished(run).await, "failed");
    assert!(started.elapsed() < Duration::from_secs(2));
    worker.stop().await;

    // The mock does not retry timeouts, although an attempt is left.
    let error = json!({
        "code": "timeout",
        "message": "The attempt did not finish within its timeout of 1 s.",
        "retryable": false
    });
    assert_eq!(
        fixture.nodes(run).await,
        json!({
            "slow": {"state": "failed", "attempt": 1, "input": {}, "output": null, "error": error}
        })
    );
    let events = fixture.events(run).await;
    let expected = [
        "running 1",
        "system: Simulating 3000 ms of work.",
        "failed 1",
    ];
    assert_eq!(timeline(&events, "slow"), expected);
}

#[tokio::test]
#[serial]
async fn an_output_that_breaks_its_schema_fails_without_retry() {
    let fixture = Fixture::new().await;
    let vpc = merge(
        task("vpc", json!({"output": {"vpcId": "subnet-1"}})),
        json!({
            "definition": {"key": "corqty.simulated.create-vpc", "version": 1},
            "inputs": {"region": {"kind": "literal", "value": "eu-west-1"}},
            "retry": {"maxAttempts": 3}
        }),
    );
    let run = fixture.run(&graph(&[vpc], &[]), json!({})).await;

    let worker = fixture.start(fixture.config(1), mock());
    assert_eq!(fixture.finished(run).await, "failed");
    worker.stop().await;

    let error = json!({
        "code": "output_invalid",
        "message": "The output at /vpcId does not match its schema: value does not match \"^vpc-\".",
        "retryable": false
    });
    assert_eq!(
        fixture.nodes(run).await,
        json!({
            "vpc": {
                "state": "failed", "attempt": 1, "input": {"region": "eu-west-1"},
                "output": null, "error": error
            }
        })
    );
}

#[tokio::test]
#[serial]
async fn a_worker_that_shuts_down_leaves_its_attempt_to_lease_recovery() {
    let fixture = Fixture::new().await;
    let slow = merge(
        task("slow", json!({"delayMs": 2000})),
        json!({"retry": {"maxAttempts": 2}}),
    );
    let run = fixture.run(&graph(&[slow], &[]), json!({})).await;

    let first = fixture.start(fixture.config(1), mock());
    let first_id = first.id;
    fixture.started(run, "slow").await;
    // With no drain, the worker stops the attempt at once and reports nothing about it.
    let stopping = Instant::now();
    first.stop().await;
    assert!(stopping.elapsed() < Duration::from_secs(1));
    let lease: (String, Option<Uuid>, bool) = sqlx::query_as(
        "SELECT state, leased_by, lease_expires_at > now() FROM node_runs WHERE run_id = $1",
    )
    .bind(run)
    .fetch_one(&fixture.database.owner)
    .await
    .unwrap();
    assert_eq!(lease, ("running".to_owned(), Some(first_id), true));
    let events = fixture.events(run).await;
    let reported: Vec<&Value> = events
        .iter()
        .filter(|event| event["type"] != "node.log")
        .collect();
    assert_eq!(
        reported,
        [
            &json!({"type": "run.state", "state": "queued", "seq": 1}),
            &json!({"type": "run.state", "state": "running", "seq": 2}),
            &json!({"type": "node.state", "nodeId": "slow", "state": "running", "attempt": 1, "seq": 3}),
        ]
    );
    assert_eq!(fixture.scratch_entries(), 0);

    // The lease expires, recovery schedules a retry, and another worker runs it.
    let owner = &fixture.database.owner;
    sqlx::query(
        "UPDATE node_runs SET lease_expires_at = now() - interval '1 second' WHERE run_id = $1",
    )
    .bind(run)
    .execute(owner)
    .await
    .unwrap();
    assert_eq!(
        recover_expired_leases(&fixture.database.api).await.unwrap(),
        1
    );
    // Due at once instead of after the backoff.
    sqlx::query("UPDATE node_runs SET available_at = now() WHERE run_id = $1")
        .bind(run)
        .execute(owner)
        .await
        .unwrap();
    let second = fixture.start(fixture.config(1), mock());
    assert_eq!(fixture.finished(run).await, "succeeded");
    second.stop().await;

    assert_eq!(
        fixture.nodes(run).await,
        json!({
            "slow": {
                "state": "succeeded", "attempt": 2, "input": {}, "output": {"result": "slow"},
                "error": null
            }
        })
    );
    let events = fixture.events(run).await;
    let entries = timeline(&events, "slow");
    let expected = ["running 1", "retry_wait 1", "running 2", "succeeded 2"];
    assert_eq!(changes(&entries), expected);
    let retry = events.iter().find(|event| event["state"] == "retry_wait");
    assert_eq!(retry.unwrap()["error"]["code"], "lease_expired");
    assert_eq!(fixture.scratch_entries(), 0);
}

#[tokio::test]
#[serial]
async fn a_worker_without_the_executor_leaves_the_node_queued() {
    let fixture = Fixture::new().await;
    let run = fixture
        .run(&graph(&[task("task", json!({}))], &[]), json!({}))
        .await;

    let worker = fixture.start(fixture.config(1), vec![Arc::new(ShellExecutor)]);
    // The worker is alive and claims every 50 ms, but only shell nodes.
    let api = &fixture.database.api;
    eventually(|| async move {
        let live: Vec<String> = sqlx::query_scalar("SELECT corqty.live_executors()")
            .fetch_one(api)
            .await
            .unwrap();
        (live == ["shell@1.0.0"]).then_some(())
    })
    .await;
    sleep(Duration::from_millis(500)).await;
    worker.stop().await;

    assert_eq!(
        fixture.nodes(run).await,
        json!({
            "task": {"state": "queued", "attempt": 0, "input": null, "output": null, "error": null}
        })
    );
    assert_eq!(
        fixture.events(run).await,
        [json!({"type": "run.state", "state": "queued", "seq": 1})]
    );
}

#[tokio::test]
#[serial]
async fn logs_reach_the_events_in_order_with_nul_replaced() {
    let fixture = Fixture::new().await;
    let chatty = task("chatty", json!({"log": "first\u{2400}line\n"}));
    let run = fixture.run(&graph(&[chatty], &[]), json!({})).await;

    let worker = fixture.start(fixture.config(1), vec![Arc::new(Recording::default())]);
    assert_eq!(fixture.finished(run).await, "succeeded");
    worker.stop().await;

    let events = fixture.events(run).await;
    let expected = [
        "running 1",
        "stdout: first\u{fffd}line\n",
        "system: Simulating 0 ms of work.",
        "succeeded 1",
    ];
    assert_eq!(timeline(&events, "chatty"), expected);
}

#[tokio::test]
#[serial]
async fn a_flood_of_logs_is_cut_at_the_cap_with_one_truncation_event() {
    let fixture = Fixture::new().await;
    // 161 chunks of 64 KiB: one chunk more than the 10 MiB cap.
    let flood = task("flood", json!({"log": "a".repeat(65_536), "repeat": 161}));
    let run = fixture.run(&graph(&[flood], &[]), json!({})).await;

    let worker = fixture.start(fixture.config(1), vec![Arc::new(Recording::default())]);
    assert_eq!(fixture.finished(run).await, "succeeded");
    worker.stop().await;

    let logged: (i64, bool) =
        sqlx::query_as("SELECT log_bytes, log_truncated FROM node_runs WHERE run_id = $1")
            .bind(run)
            .fetch_one(&fixture.database.owner)
            .await
            .unwrap();
    assert_eq!(logged, (10_485_760, true));
    let events = fixture.events(run).await;
    let text: usize = events
        .iter()
        .filter(|event| event["type"] == "node.log")
        .map(|event| event["text"].as_str().unwrap().len())
        .sum();
    assert_eq!(text, 10_485_760);
    let truncations = events
        .iter()
        .filter(|event| event["type"] == "node.log_truncated")
        .count();
    assert_eq!(truncations, 1);
}

#[tokio::test]
#[serial]
async fn every_successful_claim_touches_the_alive_file() {
    let fixture = Fixture::new().await;
    let directory = TempDir::new().unwrap();
    let config = WorkerConfig {
        alive_file: directory.path().join("corqty-worker.alive"),
        ..fixture.config(1)
    };
    let alive_file = config.alive_file.clone();
    let path = alive_file.as_path();
    let worker = fixture.start(config, mock());
    // A worker with nothing to run still claims every 50 ms, and each claim touches the file.
    eventually(|| async move { check_alive(path).ok() }).await;
    let stale = SystemTime::now() - Duration::from_secs(60);
    set_modified(path, stale);
    eventually(|| async move { check_alive(path).ok() }).await;

    // Without its database the worker's claims fail, so the file goes stale.
    fixture.database.worker.close().await;
    sleep(Duration::from_millis(200)).await;
    set_modified(path, stale);
    sleep(Duration::from_millis(500)).await;
    assert!(check_alive(path).is_err());
    worker.stop().await;
}
