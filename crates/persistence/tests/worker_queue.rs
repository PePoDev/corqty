use std::collections::{BTreeMap, HashMap, HashSet};

use corqty_domain::{
    catalog::NewNodeDefinition,
    graph::GraphSnapshot,
    publish::canonical_checksum,
    run::{PlannedNode, plan_run},
};
use corqty_persistence::{
    catalog,
    runs::{self, NewRun},
    tenant::begin_tenant,
    worker_queue::{ClaimedJob, Job, LeaseStatus, NodeError, Outcome, WorkerDb},
    workflows,
};
use corqty_test_support::TestDatabase;
use serde_json::{Map, Value, json};
use serial_test::serial;
use sqlx::{Postgres, Transaction};
use tokio::task::JoinSet;
use uuid::Uuid;

/// An organization and its admin, who may edit and run workflows.
#[derive(Clone, Copy)]
struct Tenant {
    organization_id: Uuid,
    admin: Uuid,
}

/// Version 1 of a workflow, with the plan its runs start from.
struct Published {
    tenant: Tenant,
    workflow_id: Uuid,
    version_id: Uuid,
    plan: Vec<PlannedNode>,
}

/// Organizations A and B, each with an admin.
struct Fixture {
    database: TestDatabase,
    a: Tenant,
    b: Tenant,
}

impl Fixture {
    async fn new() -> Self {
        let database = TestDatabase::reset().await;
        let [org_a, org_b, admin_a, admin_b] = std::array::from_fn(|_| Uuid::new_v4());
        sqlx::query(
            "INSERT INTO users(id, email)
             SELECT id, id || '@example.com' FROM unnest($1::uuid[]) AS id",
        )
        .bind(vec![admin_a, admin_b])
        .execute(&database.owner)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO organizations(id, name, slug, created_by)
             VALUES ($1, 'A', 'a', $3), ($2, 'B', 'b', $4)",
        )
        .bind(org_a)
        .bind(org_b)
        .bind(admin_a)
        .bind(admin_b)
        .execute(&database.owner)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships(organization_id, user_id, role)
             VALUES ($1, $3, 'admin'), ($2, $4, 'admin')",
        )
        .bind(org_a)
        .bind(org_b)
        .bind(admin_a)
        .bind(admin_b)
        .execute(&database.owner)
        .await
        .unwrap();

        let a = Tenant {
            organization_id: org_a,
            admin: admin_a,
        };
        let b = Tenant {
            organization_id: org_b,
            admin: admin_b,
        };
        Self { database, a, b }
    }

    async fn tx(&self, tenant: Tenant) -> Transaction<'static, Postgres> {
        begin_tenant(&self.database.api, tenant.admin, tenant.organization_id)
            .await
            .unwrap()
    }

    /// The simulated infrastructure blueprint: vpc, then rds and deploy.
    async fn infrastructure(&self) -> GraphSnapshot {
        let id = "00000000-0000-7000-8000-000000000101".parse().unwrap();
        let mut tx = self.tx(self.a).await;
        let blueprint = catalog::get_blueprint(&mut tx, id).await.unwrap();
        tx.commit().await.unwrap();
        blueprint.unwrap().graph
    }

    /// Publishes `graph` as version 1 of a new workflow of the tenant.
    async fn publish(&self, tenant: Tenant, graph: &GraphSnapshot) -> Published {
        let (organization_id, admin) = (tenant.organization_id, tenant.admin);
        let slug = Uuid::new_v4().to_string();
        let mut tx = self.tx(tenant).await;
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
        tx.commit().await.unwrap();
        Published {
            tenant,
            workflow_id: workflow.id,
            version_id: version.id,
            plan: plan_run(graph, &definitions).unwrap(),
        }
    }

    /// Starts a run of the version with `nodes` as its plan and returns its id.
    async fn run(&self, published: &Published, nodes: &[PlannedNode]) -> Uuid {
        let tenant = published.tenant;
        let input = json!({"region": "eu-west-1"});
        let new = NewRun {
            organization_id: tenant.organization_id,
            actor_user_id: tenant.admin,
            workflow_id: published.workflow_id,
            workflow_version_id: published.version_id,
            version: 1,
            input: &input,
            nodes,
            rerun_of: None,
        };
        let mut tx = self.tx(tenant).await;
        let run = runs::create_run(&mut tx, &new).await.unwrap();
        tx.commit().await.unwrap();
        run.id
    }

    /// A new worker process that registered `executors`.
    async fn worker(&self, executors: &[&str]) -> WorkerDb {
        let worker = WorkerDb::new(self.database.worker.clone(), Uuid::now_v7());
        let executors: Vec<String> = executors.iter().map(|&key| key.to_owned()).collect();
        worker.register(&executors, 4).await.unwrap();
        worker
    }

    /// Claims every due node run the worker can take, by node id.
    async fn claim(&self, worker: &WorkerDb) -> BTreeMap<String, ClaimedJob> {
        let mut claimed = BTreeMap::new();
        for job in worker.claim(100, 30).await.unwrap() {
            let node_id: String = sqlx::query_scalar("SELECT node_id FROM node_runs WHERE id = $1")
                .bind(job.node_run_id)
                .fetch_one(&self.database.owner)
                .await
                .unwrap();
            claimed.insert(node_id, job);
        }
        claimed
    }

    /// The run's event payloads in sequence order.
    async fn events(&self, run_id: Uuid) -> Vec<Value> {
        sqlx::query_scalar("SELECT payload FROM run_events WHERE run_id = $1 ORDER BY seq")
            .bind(run_id)
            .fetch_all(&self.database.owner)
            .await
            .unwrap()
    }

    /// Each node of the run with its state, in plan order.
    async fn states(&self, run_id: Uuid) -> Vec<(String, String)> {
        sqlx::query_as("SELECT node_id, state FROM node_runs WHERE run_id = $1 ORDER BY id")
            .bind(run_id)
            .fetch_all(&self.database.owner)
            .await
            .unwrap()
    }

    /// Every node run, run and event, to show that a call changed nothing.
    async fn snapshot(&self) -> Value {
        sqlx::query_scalar(
            "SELECT jsonb_build_array(
                 (SELECT jsonb_agg(n ORDER BY n.id) FROM node_runs n),
                 (SELECT jsonb_agg(r ORDER BY r.id) FROM runs r),
                 (SELECT jsonb_agg(e ORDER BY e.run_id, e.seq) FROM run_events e)
             )",
        )
        .fetch_one(&self.database.owner)
        .await
        .unwrap()
    }

    /// The executors of the workers that claimed or heartbeated in the last minute, as the
    /// API sees them.
    async fn live_executors(&self) -> Vec<String> {
        sqlx::query_scalar("SELECT corqty.live_executors()")
            .fetch_one(&self.database.api)
            .await
            .unwrap()
    }
}

fn code(error: &sqlx::Error) -> Option<String> {
    let code = error.as_database_error().and_then(|error| error.code());
    code.map(String::from)
}

/// A node of a hand-written plan on the mock executor, with one attempt.
fn planned(node_id: &str, children: &[&str], pending_parents: i32) -> PlannedNode {
    PlannedNode {
        node_id: node_id.to_owned(),
        executor: "mock@1.0.0".to_owned(),
        max_attempts: 1,
        timeout_seconds: 60,
        children: children.iter().map(|&child| child.to_owned()).collect(),
        pending_parents,
    }
}

/// A `corqty.mock.task` node with up to `max_attempts` attempts.
fn task(id: &str, max_attempts: u32) -> Value {
    json!({
        "id": id,
        "definition": {"key": "corqty.mock.task", "version": 1},
        "displayName": id,
        "position": {"x": 0, "y": 0},
        "config": {"mode": "fixed", "output": {"result": id}, "delayMs": 0, "failureMessage": null},
        "inputs": {},
        "retry": {"maxAttempts": max_attempts},
        "timeoutSeconds": 60
    })
}

/// An edge from the result of `source` into the `port` input of `target`. Publish would
/// reject ports the definition lacks; a plan only reads which nodes an edge joins.
fn edge(source: &str, target: &str, port: &str) -> Value {
    json!({
        "id": format!("{source}-{target}-{port}"),
        "source": {"nodeId": source, "port": "result"},
        "target": {"nodeId": target, "port": port}
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

fn object(value: Value) -> Map<String, Value> {
    let Value::Object(object) = value else {
        panic!("not an object: {value}");
    };
    object
}

fn succeeded(output: Value) -> Outcome {
    Outcome::Succeeded {
        input: Map::new(),
        output: object(output),
    }
}

fn failure(code: &str, retryable: bool) -> NodeError {
    NodeError {
        code: code.to_owned(),
        message: format!("The mock failed with {code}."),
        retryable,
    }
}

fn failed(code: &str, retryable: bool) -> Outcome {
    Outcome::Failed {
        input: None,
        error: failure(code, retryable),
    }
}

fn run_state(state: &str, seq: i64) -> Value {
    json!({"type": "run.state", "state": state, "seq": seq})
}

fn node_state(node_id: &str, state: &str, attempt: i32, seq: i64) -> Value {
    json!({"type": "node.state", "nodeId": node_id, "state": state, "attempt": attempt, "seq": seq})
}

/// `event` with one more field.
fn with_field(mut event: Value, key: &str, value: Value) -> Value {
    event[key] = value;
    event
}

/// Starts the claimed attempt and completes it with `outcome`.
async fn execute(worker: &WorkerDb, job: &ClaimedJob, outcome: Outcome) {
    let (id, token) = (job.node_run_id, job.lease_token);
    assert!(worker.start(id, token).await.unwrap().is_some());
    assert!(worker.complete(id, token, &outcome).await.unwrap());
}

/// Makes every fenced call with one lease, in a worker's order, and reports which held:
/// (started, renewed, logged, completed).
async fn fenced_calls(worker: &WorkerDb, id: Uuid, token: i64) -> (bool, bool, bool, bool) {
    let started = worker.start(id, token).await.unwrap().is_some();
    let renewed = worker.heartbeat(&[(id, token)], 30).await.unwrap()[0].alive;
    let logged = worker
        .append_log(id, token, "stdout", "hello")
        .await
        .unwrap();
    let outcome = succeeded(json!({"vpcId": "vpc-1"}));
    let completed = worker.complete(id, token, &outcome).await.unwrap();
    (started, renewed, logged, completed)
}

#[tokio::test]
#[serial]
async fn concurrent_claimers_never_share_a_node_run() {
    let fixture = Fixture::new().await;
    let published = fixture
        .publish(fixture.a, &fixture.infrastructure().await)
        .await;
    let nodes: Vec<PlannedNode> = (0..40)
        .map(|index| planned(&format!("n{index}"), &[], 0))
        .collect();
    for _ in 0..5 {
        fixture.run(&published, &nodes).await;
    }

    // Eight workers claim at the same time, each on a connection of its own, until nothing
    // is left.
    let mut claimers = JoinSet::new();
    for _ in 0..8 {
        let worker = fixture.worker(&["mock@1.0.0"]).await;
        claimers.spawn(async move {
            let mut claimed = Vec::new();
            loop {
                let jobs = worker.claim(5, 30).await.unwrap();
                if jobs.is_empty() {
                    return claimed;
                }
                let worker_id = worker.worker_id();
                claimed.extend(jobs.into_iter().map(|job| (job.node_run_id, worker_id)));
            }
        });
    }
    let claimed: Vec<(Uuid, Uuid)> = claimers.join_all().await.concat();
    let distinct: HashSet<Uuid> = claimed.iter().map(|&(node_run, _)| node_run).collect();
    assert_eq!((claimed.len(), distinct.len()), (200, 200));

    // Each node run is leased, at its first attempt, to the worker that claimed it.
    let rows: Vec<(Uuid, String, Option<Uuid>, i32, i64)> =
        sqlx::query_as("SELECT id, state, leased_by, attempt, lease_token FROM node_runs")
            .fetch_all(&fixture.database.owner)
            .await
            .unwrap();
    let leases: HashMap<Uuid, (String, Option<Uuid>, i32, i64)> = rows
        .into_iter()
        .map(|(id, state, leased_by, attempt, token)| (id, (state, leased_by, attempt, token)))
        .collect();
    for (node_run, worker) in claimed {
        let expected = ("running".to_owned(), Some(worker), 1, 1);
        assert_eq!(leases[&node_run], expected);
    }
}

#[tokio::test]
#[serial]
async fn only_the_lease_holder_can_start_renew_log_or_complete_a_node_run() {
    let fixture = Fixture::new().await;
    let graph = fixture.infrastructure().await;
    let in_a = fixture.publish(fixture.a, &graph).await;
    let in_b = fixture.publish(fixture.b, &graph).await;
    fixture.run(&in_a, &in_a.plan).await;
    fixture.run(&in_b, &in_b.plan).await;
    // The holder claims A's vpc, which was queued first, and another worker claims B's.
    let holder = fixture.worker(&["mock@1.0.0"]).await;
    let other = fixture.worker(&["mock@1.0.0"]).await;
    let mine = holder.claim(1, 30).await.unwrap().remove(0);
    let theirs = other.claim(1, 30).await.unwrap().remove(0);
    assert_eq!(
        (mine.organization_id, theirs.organization_id),
        (fixture.a.organization_id, fixture.b.organization_id)
    );

    let before = fixture.snapshot().await;
    for (worker, id, token) in [
        // Another worker with the right token.
        (&other, mine.node_run_id, mine.lease_token),
        // The holder with an older or a newer token.
        (&holder, mine.node_run_id, mine.lease_token - 1),
        (&holder, mine.node_run_id, mine.lease_token + 1),
        // Another organization's node run, with the token it was leased under.
        (&holder, theirs.node_run_id, theirs.lease_token),
    ] {
        let calls = fenced_calls(worker, id, token).await;
        assert_eq!(calls, (false, false, false, false), "{id} {token}");
    }
    assert_eq!(fixture.snapshot().await, before);

    // The holder's lease works for every call, and completing the node ends it.
    let (id, token) = (mine.node_run_id, mine.lease_token);
    let calls = fenced_calls(&holder, id, token).await;
    assert_eq!(calls, (true, true, true, true));
    let calls = fenced_calls(&holder, id, token).await;
    assert_eq!(calls, (false, false, false, false));
}

#[tokio::test]
#[serial]
async fn parents_that_finish_together_release_their_child_once() {
    let fixture = Fixture::new().await;
    let published = fixture
        .publish(fixture.a, &fixture.infrastructure().await)
        .await;
    let mut nodes: Vec<PlannedNode> = ["p0", "p1", "p2", "p3"]
        .iter()
        .map(|parent| planned(parent, &["child"], 0))
        .collect();
    // No worker here runs the child's executor, so released children stay queued.
    nodes.push(PlannedNode {
        executor: "other@1.0.0".to_owned(),
        ..planned("child", &[], 4)
    });
    let worker = fixture.worker(&["mock@1.0.0"]).await;

    for _ in 0..50 {
        let run_id = fixture.run(&published, &nodes).await;
        let jobs = worker.claim(10, 30).await.unwrap();
        assert_eq!(jobs.len(), 4);
        let mut completions = JoinSet::new();
        for job in jobs {
            let worker = worker.clone();
            completions.spawn(async move {
                let outcome = succeeded(json!({}));
                let (id, token) = (job.node_run_id, job.lease_token);
                worker.complete(id, token, &outcome).await.unwrap()
            });
        }
        assert_eq!(completions.join_all().await, [true; 4]);

        let child: (String, i32) = sqlx::query_as(
            "SELECT state, pending_parents FROM node_runs WHERE run_id = $1 AND node_id = 'child'",
        )
        .bind(run_id)
        .fetch_one(&fixture.database.owner)
        .await
        .unwrap();
        assert_eq!(child, ("queued".to_owned(), 0));
        // Events 2 to 5 are the parents succeeding; the last one releases the child.
        let releases: Vec<Value> = fixture
            .events(run_id)
            .await
            .into_iter()
            .filter(|event| event["nodeId"] == "child")
            .collect();
        assert_eq!(releases, [node_state("child", "queued", 0, 6)]);
    }
}

#[tokio::test]
#[serial]
async fn a_child_waits_once_for_each_distinct_parent_and_the_run_succeeds() {
    let fixture = Fixture::new().await;
    // split feeds two ports of join, so join waits for two parents, not three edges.
    let graph = graph(
        &[task("split", 1), task("other", 1), task("join", 1)],
        &[
            edge("split", "join", "input"),
            edge("split", "join", "extra"),
            edge("other", "join", "more"),
        ],
    );
    let published = fixture.publish(fixture.a, &graph).await;
    let run_id = fixture.run(&published, &published.plan).await;
    let worker = fixture.worker(&["mock@1.0.0"]).await;
    let output = |node_id: &str| json!({"result": node_id});

    let roots = fixture.claim(&worker).await;
    assert_eq!(roots.keys().collect::<Vec<_>>(), ["other", "split"]);
    execute(&worker, &roots["split"], succeeded(output("split"))).await;
    // One parent is not enough.
    assert!(worker.claim(10, 30).await.unwrap().is_empty());
    execute(&worker, &roots["other"], succeeded(output("other"))).await;

    let join = fixture.claim(&worker).await.remove("join").unwrap();
    let job = worker.start(join.node_run_id, join.lease_token).await;
    let job = job.unwrap().unwrap();
    assert_eq!(
        job.upstream_outputs,
        HashMap::from([
            ("split".to_owned(), object(output("split"))),
            ("other".to_owned(), object(output("other"))),
        ])
    );
    let outcome = succeeded(output("join"));
    let completed = worker.complete(join.node_run_id, join.lease_token, &outcome);
    assert!(completed.await.unwrap());

    assert_eq!(
        fixture.events(run_id).await,
        [
            run_state("queued", 1),
            run_state("running", 2),
            node_state("split", "running", 1, 3),
            with_field(
                node_state("split", "succeeded", 1, 4),
                "output",
                output("split")
            ),
            node_state("other", "running", 1, 5),
            with_field(
                node_state("other", "succeeded", 1, 6),
                "output",
                output("other")
            ),
            node_state("join", "queued", 0, 7),
            node_state("join", "running", 1, 8),
            with_field(
                node_state("join", "succeeded", 1, 9),
                "output",
                output("join")
            ),
            run_state("succeeded", 10),
        ]
    );
    // Each node keeps its input and output, and no lease is left behind.
    let nodes: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object(
             'nodeId', node_id, 'input', input, 'output', output, 'error', error,
             'leased', leased_by IS NOT NULL OR lease_expires_at IS NOT NULL,
             'timed', started_at IS NOT NULL AND finished_at IS NOT NULL
         )
         FROM node_runs WHERE run_id = $1 ORDER BY id",
    )
    .bind(run_id)
    .fetch_all(&fixture.database.owner)
    .await
    .unwrap();
    let node = |node_id: &str| {
        json!({
            "nodeId": node_id, "input": {}, "output": output(node_id), "error": null,
            "leased": false, "timed": true
        })
    };
    assert_eq!(nodes, [node("split"), node("other"), node("join")]);
    let run: (String, bool, bool) = sqlx::query_as(
        "SELECT status, started_at IS NOT NULL, finished_at IS NOT NULL FROM runs WHERE id = $1",
    )
    .bind(run_id)
    .fetch_one(&fixture.database.owner)
    .await
    .unwrap();
    assert_eq!(run, ("succeeded".to_owned(), true, true));
}

#[tokio::test]
#[serial]
async fn a_failure_skips_only_its_descendants_and_fails_the_run() {
    let fixture = Fixture::new().await;
    // a -> b -> c, and u on its own. A failure the executor calls final is not retried,
    // although a has a second attempt.
    let graph = graph(
        &[task("a", 2), task("b", 1), task("c", 1), task("u", 1)],
        &[edge("a", "b", "input"), edge("b", "c", "input")],
    );
    let published = fixture.publish(fixture.a, &graph).await;
    let run_id = fixture.run(&published, &published.plan).await;
    let worker = fixture.worker(&["mock@1.0.0"]).await;

    let roots = fixture.claim(&worker).await;
    let (a, u) = (&roots["a"], &roots["u"]);
    let outcome = failed("mock_failure", false);
    assert!(
        worker
            .complete(a.node_run_id, a.lease_token, &outcome)
            .await
            .unwrap()
    );
    // The unrelated branch carries on.
    let state = |node_id: &str, state: &str| (node_id.to_owned(), state.to_owned());
    assert_eq!(
        fixture.states(run_id).await,
        [
            state("a", "failed"),
            state("u", "running"),
            state("b", "skipped"),
            state("c", "skipped"),
        ]
    );
    let outcome = succeeded(json!({"result": "u"}));
    assert!(
        worker
            .complete(u.node_run_id, u.lease_token, &outcome)
            .await
            .unwrap()
    );

    let error = json!(failure("mock_failure", false));
    assert_eq!(
        fixture.events(run_id).await,
        [
            run_state("queued", 1),
            with_field(node_state("a", "failed", 1, 2), "error", error),
            node_state("b", "skipped", 0, 3),
            node_state("c", "skipped", 0, 4),
            with_field(
                node_state("u", "succeeded", 1, 5),
                "output",
                json!({"result": "u"})
            ),
            run_state("failed", 6),
        ]
    );
}

#[tokio::test]
#[serial]
async fn retryable_failures_wait_with_backoff_until_the_attempts_run_out() {
    let fixture = Fixture::new().await;
    let published = fixture
        .publish(fixture.a, &graph(&[task("flaky", 2)], &[]))
        .await;
    let run_id = fixture.run(&published, &published.plan).await;
    let worker = fixture.worker(&["mock@1.0.0"]).await;
    let owner = &fixture.database.owner;
    let outcome = failed("mock_retryable_failure", true);
    let error = json!(failure("mock_retryable_failure", true));

    let first = fixture.claim(&worker).await.remove("flaky").unwrap();
    let completed = worker.complete(first.node_run_id, first.lease_token, &outcome);
    assert!(completed.await.unwrap());
    // Attempt 1 waits 2 s plus up to 20% jitter, counted from the completion that
    // scheduled the retry. The error stays, and the lease is gone.
    let (state, stored, leased_by, delay): (String, Value, Option<Uuid>, f64) = sqlx::query_as(
        "SELECT n.state, n.error, n.leased_by,
                extract(epoch FROM n.available_at - e.created_at)::float8
         FROM node_runs n
         JOIN run_events e ON e.run_id = n.run_id AND e.payload ->> 'state' = 'retry_wait'
         WHERE n.id = $1",
    )
    .bind(first.node_run_id)
    .fetch_one(owner)
    .await
    .unwrap();
    assert_eq!(
        (state.as_str(), &stored, leased_by),
        ("retry_wait", &error, None)
    );
    assert!((2.0..2.4).contains(&delay), "{delay}");
    assert!(worker.claim(10, 30).await.unwrap().is_empty());

    // Once due, the retry is claimed straight from retry_wait as attempt 2.
    sqlx::query("UPDATE node_runs SET available_at = now() WHERE id = $1")
        .bind(first.node_run_id)
        .execute(owner)
        .await
        .unwrap();
    let second = fixture.claim(&worker).await.remove("flaky").unwrap();
    assert_eq!(
        (second.node_run_id, second.attempt, second.lease_token),
        (first.node_run_id, 2, 2)
    );
    let completed = worker.complete(second.node_run_id, second.lease_token, &outcome);
    assert!(completed.await.unwrap());

    assert_eq!(
        fixture.events(run_id).await,
        [
            run_state("queued", 1),
            with_field(
                node_state("flaky", "retry_wait", 1, 2),
                "error",
                error.clone()
            ),
            with_field(node_state("flaky", "failed", 2, 3), "error", error),
            run_state("failed", 4),
        ]
    );
}

#[tokio::test]
#[serial]
async fn after_a_cancel_request_a_retryable_failure_cancels_the_node_and_the_run() {
    let fixture = Fixture::new().await;
    let graph = graph(
        &[task("long", 3), task("after", 1)],
        &[edge("long", "after", "input")],
    );
    let published = fixture.publish(fixture.a, &graph).await;
    let run_id = fixture.run(&published, &published.plan).await;
    let worker = fixture.worker(&["mock@1.0.0"]).await;
    let long = fixture.claim(&worker).await.remove("long").unwrap();
    let (id, token) = (long.node_run_id, long.lease_token);

    let tenant = fixture.a;
    let mut tx = fixture.tx(tenant).await;
    runs::cancel_run(&mut tx, tenant.organization_id, tenant.admin, run_id)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    // The worker learns about the request when it starts the attempt and on each heartbeat.
    let job = worker.start(id, token).await.unwrap().unwrap();
    assert!(job.cancel_requested);
    let leases = worker.heartbeat(&[(id, token)], 30).await.unwrap();
    let lease = LeaseStatus {
        node_run_id: id,
        alive: true,
        cancel_requested: true,
    };
    assert_eq!(leases, [lease]);

    // Two attempts are left, but a run that was asked to cancel is not retried.
    let outcome = failed("mock_retryable_failure", true);
    assert!(worker.complete(id, token, &outcome).await.unwrap());
    let error = json!(failure("mock_retryable_failure", true));
    assert_eq!(
        fixture.events(run_id).await,
        [
            run_state("queued", 1),
            node_state("after", "canceled", 0, 2),
            run_state("running", 3),
            node_state("long", "running", 1, 4),
            with_field(node_state("long", "canceled", 1, 5), "error", error),
            run_state("canceled", 6),
        ]
    );

    // Its nodes are not handed out even when they are queued again.
    sqlx::query("UPDATE node_runs SET state = 'queued', available_at = now() WHERE run_id = $1")
        .bind(run_id)
        .execute(&fixture.database.owner)
        .await
        .unwrap();
    assert!(worker.claim(10, 30).await.unwrap().is_empty());
}

#[tokio::test]
#[serial]
async fn heartbeats_renew_held_leases_and_report_lost_ones_in_order() {
    let fixture = Fixture::new().await;
    let published = fixture
        .publish(fixture.a, &fixture.infrastructure().await)
        .await;
    fixture.run(&published, &published.plan).await;
    let worker = fixture.worker(&["mock@1.0.0"]).await;
    let owner = &fixture.database.owner;
    let vpc = fixture.claim(&worker).await.remove("vpc").unwrap();
    sqlx::query("UPDATE worker_instances SET last_heartbeat_at = now() - interval '1 hour'")
        .execute(owner)
        .await
        .unwrap();

    let stranger = Uuid::now_v7();
    let leases = worker
        .heartbeat(&[(stranger, 1), (vpc.node_run_id, vpc.lease_token)], 120)
        .await
        .unwrap();
    let lease = |node_run_id, alive| LeaseStatus {
        node_run_id,
        alive,
        cancel_requested: false,
    };
    assert_eq!(
        leases,
        [lease(stranger, false), lease(vpc.node_run_id, true)]
    );
    // The 30 s lease of the claim now runs for 120 s, and the worker is seen alive.
    let (remaining, silence): (f64, f64) = sqlx::query_as(
        "SELECT extract(epoch FROM n.lease_expires_at - now())::float8,
                extract(epoch FROM now() - w.last_heartbeat_at)::float8
         FROM node_runs n JOIN worker_instances w ON w.id = n.leased_by
         WHERE n.id = $1",
    )
    .bind(vpc.node_run_id)
    .fetch_one(owner)
    .await
    .unwrap();
    assert!((119.0..=120.0).contains(&remaining), "{remaining}");
    assert!(silence < 60.0, "{silence}");
}

#[tokio::test]
#[serial]
async fn logs_stop_at_the_cap_on_a_character_boundary() {
    let fixture = Fixture::new().await;
    let published = fixture
        .publish(fixture.a, &fixture.infrastructure().await)
        .await;
    let run_id = fixture.run(&published, &published.plan).await;
    let worker = fixture.worker(&["mock@1.0.0"]).await;
    let owner = &fixture.database.owner;
    let vpc = fixture.claim(&worker).await.remove("vpc").unwrap();
    let (id, token) = (vpc.node_run_id, vpc.lease_token);
    assert!(worker.start(id, token).await.unwrap().is_some());

    assert!(
        worker
            .append_log(id, token, "stdout", "hello")
            .await
            .unwrap()
    );
    let logged: i64 = sqlx::query_scalar("SELECT log_bytes FROM node_runs WHERE id = $1")
        .bind(id)
        .fetch_one(owner)
        .await
        .unwrap();
    assert_eq!(logged, 5);
    // As if all but 5 bytes of the 10 MiB had been logged. "abc€" needs 6 bytes, and the
    // 5th would split the 3-byte "€", so only "abc" is stored.
    sqlx::query("UPDATE node_runs SET log_bytes = 10485760 - 5 WHERE id = $1")
        .bind(id)
        .execute(owner)
        .await
        .unwrap();
    assert!(
        worker
            .append_log(id, token, "stderr", "abc€")
            .await
            .unwrap()
    );
    // Later text is dropped, and the lease still holds.
    assert!(
        worker
            .append_log(id, token, "stdout", "more")
            .await
            .unwrap()
    );

    let log = |stream: &str, text: &str, seq: i64| json!({"type": "node.log", "nodeId": "vpc", "stream": stream, "text": text, "seq": seq});
    assert_eq!(
        fixture.events(run_id).await[3..],
        [
            log("stdout", "hello", 4),
            log("stderr", "abc", 5),
            json!({"type": "node.log_truncated", "nodeId": "vpc", "seq": 6}),
        ]
    );
    let stored: (i64, bool) =
        sqlx::query_as("SELECT log_bytes, log_truncated FROM node_runs WHERE id = $1")
            .bind(id)
            .fetch_one(owner)
            .await
            .unwrap();
    assert_eq!(stored, (10_485_760 - 2, true));

    // Logs have three streams.
    let error = worker
        .append_log(id, token, "stdin", "x")
        .await
        .unwrap_err();
    assert_eq!(code(&error).as_deref(), Some("22023"), "{error}");
}

#[tokio::test]
#[serial]
async fn workers_claim_only_the_executors_they_advertise() {
    let fixture = Fixture::new().await;
    let published = fixture
        .publish(fixture.a, &fixture.infrastructure().await)
        .await;
    fixture.run(&published, &published.plan).await;

    // A worker that never registered finds nothing, and neither does a shell-only worker.
    let unregistered = WorkerDb::new(fixture.database.worker.clone(), Uuid::now_v7());
    assert!(unregistered.claim(10, 30).await.unwrap().is_empty());
    let shell = fixture.worker(&["shell@1.0.0"]).await;
    assert!(shell.claim(10, 30).await.unwrap().is_empty());
    assert_eq!(fixture.live_executors().await, ["shell@1.0.0"]);

    // The API sees an executor as live while a worker that runs it was heard from in the
    // last minute. A claim with no free slot counts too.
    let mock = fixture.worker(&["mock@1.0.0"]).await;
    let both = ["mock@1.0.0", "shell@1.0.0"];
    assert_eq!(fixture.live_executors().await, both);
    sqlx::query(
        "UPDATE worker_instances SET last_heartbeat_at = now() - interval '61 seconds'
         WHERE id = $1",
    )
    .bind(mock.worker_id())
    .execute(&fixture.database.owner)
    .await
    .unwrap();
    assert_eq!(fixture.live_executors().await, ["shell@1.0.0"]);
    assert!(mock.claim(0, 30).await.unwrap().is_empty());
    assert_eq!(fixture.live_executors().await, both);

    let claimed = fixture.claim(&mock).await;
    assert_eq!(claimed.keys().collect::<Vec<_>>(), ["vpc"]);
}

#[tokio::test]
#[serial]
async fn each_job_carries_the_definition_of_its_own_organization() {
    let fixture = Fixture::new().await;
    let mut node = task("task", 1);
    node["definition"]["key"] = json!("acme.task");
    let graph = graph(&[node], &[]);
    let worker = fixture.worker(&["mock@1.0.0"]).await;

    // Both organizations define acme.task version 1, with different result types.
    for (tenant, result_type) in [(fixture.a, "string"), (fixture.b, "number")] {
        let definition: NewNodeDefinition = serde_json::from_value(json!({
            "key": "acme.task", "displayName": "Task", "category": "Testing",
            "inputs": [],
            "outputs": [{
                "name": "result", "dataType": format!("core.{result_type}"),
                "schema": {"type": result_type}
            }],
            "configSchema": {"type": "object"}, "uiSchema": {}, "defaults": {},
            "executor": {"key": "mock", "version": "1.0.0"}
        }))
        .unwrap();
        let (organization_id, admin) = (tenant.organization_id, tenant.admin);
        let mut tx = fixture.tx(tenant).await;
        catalog::insert_node_definition(&mut tx, organization_id, admin, &definition)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let published = fixture.publish(tenant, &graph).await;
        let run_id = fixture.run(&published, &published.plan).await;

        let claimed = worker.claim(10, 30).await.unwrap().remove(0);
        let job = worker.start(claimed.node_run_id, claimed.lease_token).await;
        let expected = Job {
            execution_id: claimed.node_run_id,
            attempt: 1,
            max_attempts: 1,
            timeout_seconds: 60,
            workflow_id: published.workflow_id,
            run_id,
            node_id: "task".to_owned(),
            cancel_requested: false,
            run_input: json!({"region": "eu-west-1"}),
            graph: graph.clone(),
            definition: definition.spec,
            upstream_outputs: HashMap::new(),
        };
        assert_eq!(job.unwrap(), Some(expected));
    }
}
