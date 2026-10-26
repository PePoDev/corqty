use std::time::Duration;

use corqty_domain::{
    publish::canonical_checksum,
    run::{NodeState, PlannedNode, RunStatus, aggregate, plan_run},
};
use corqty_persistence::{
    catalog,
    runs::{self, CancelError, NewRun},
    tenant::begin_tenant,
    workflows,
};
use corqty_test_support::{API_URL, OWNER_URL, TestDatabase};
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::{Connection, PgConnection, Postgres, Transaction, postgres::PgListener};
use tokio::task::JoinSet;
use uuid::Uuid;

/// Organization A with one member per role and a workflow whose version 1 is the simulated
/// infrastructure blueprint, and organization B with its own admin.
struct Fixture {
    database: TestDatabase,
    org_a: Uuid,
    org_b: Uuid,
    admin: Uuid,
    editor: Uuid,
    runner: Uuid,
    viewer: Uuid,
    admin_b: Uuid,
    workflow_id: Uuid,
    version_id: Uuid,
    /// Version 1's plan: vpc, then rds and deploy.
    plan: Vec<PlannedNode>,
}

impl Fixture {
    async fn new() -> Self {
        let database = TestDatabase::reset().await;
        let [org_a, org_b, admin, editor, runner, viewer, admin_b] =
            std::array::from_fn(|_| Uuid::new_v4());
        sqlx::query(
            "INSERT INTO users(id, email)
             SELECT id, id || '@example.com' FROM unnest($1::uuid[]) AS id",
        )
        .bind(vec![admin, editor, runner, viewer, admin_b])
        .execute(&database.owner)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO organizations(id, name, slug, created_by)
             VALUES ($1, 'A', 'a', $3), ($2, 'B', 'b', $4)",
        )
        .bind(org_a)
        .bind(org_b)
        .bind(admin)
        .bind(admin_b)
        .execute(&database.owner)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships(organization_id, user_id, role)
             VALUES ($1, $3, 'admin'), ($1, $4, 'editor'), ($1, $5, 'runner'),
                    ($1, $6, 'viewer'), ($2, $7, 'admin')",
        )
        .bind(org_a)
        .bind(org_b)
        .bind(admin)
        .bind(editor)
        .bind(runner)
        .bind(viewer)
        .bind(admin_b)
        .execute(&database.owner)
        .await
        .unwrap();

        let blueprint_id = "00000000-0000-7000-8000-000000000101".parse().unwrap();
        let mut tx = begin_tenant(&database.api, editor, org_a).await.unwrap();
        let blueprint = catalog::get_blueprint(&mut tx, blueprint_id).await.unwrap();
        let graph = blueprint.unwrap().graph;
        let workflow = workflows::insert_workflow(
            &mut tx,
            org_a,
            editor,
            "Provision",
            "provision",
            &graph,
            None,
        )
        .await
        .unwrap();
        let checksum = canonical_checksum(&graph);
        let version =
            workflows::insert_version(&mut tx, org_a, editor, workflow.id, 1, &graph, &checksum)
                .await
                .unwrap();
        let definitions = catalog::definitions_map(&mut tx).await.unwrap();
        tx.commit().await.unwrap();

        Self {
            database,
            org_a,
            org_b,
            admin,
            editor,
            runner,
            viewer,
            admin_b,
            workflow_id: workflow.id,
            version_id: version.id,
            plan: plan_run(&graph, &definitions).unwrap(),
        }
    }

    async fn tx(&self, user: Uuid, organization: Uuid) -> Transaction<'static, Postgres> {
        begin_tenant(&self.database.api, user, organization)
            .await
            .unwrap()
    }

    /// Runs `sql` as `user` in `organization` and rolls it back. Returns the rows it touched,
    /// or None when PostgreSQL refused it with 42501 (a privilege or row-level security check).
    async fn attempt(&self, user: Uuid, organization: Uuid, sql: &str) -> Option<u64> {
        let mut tx = self.tx(user, organization).await;
        let result = sqlx::query(sql).execute(&mut *tx).await;
        tx.rollback().await.unwrap();
        match result {
            Ok(done) => Some(done.rows_affected()),
            Err(error) => {
                assert_eq!(code(&error).as_deref(), Some("42501"), "{sql}: {error}");
                None
            }
        }
    }

    /// A manual run of version 1 in organization A.
    fn run<'a>(&self, actor: Uuid, input: &'a Value, nodes: &'a [PlannedNode]) -> NewRun<'a> {
        NewRun {
            organization_id: self.org_a,
            actor_user_id: actor,
            workflow_id: self.workflow_id,
            workflow_version_id: self.version_id,
            version: 1,
            input,
            nodes,
            rerun_of: None,
        }
    }

    /// Creates a run of `nodes` as the runner, commits it, and returns its id.
    async fn create(&self, nodes: &[PlannedNode]) -> Uuid {
        let input = json!({"region": "eu-west-1"});
        let mut tx = self.tx(self.runner, self.org_a).await;
        let run = runs::create_run(&mut tx, &self.run(self.runner, &input, nodes))
            .await
            .unwrap();
        tx.commit().await.unwrap();
        run.id
    }
}

fn code(error: &sqlx::Error) -> Option<String> {
    let code = error.as_database_error().and_then(|error| error.code());
    code.map(String::from)
}

/// A node without parents, so it starts queued.
fn root(node_id: &str) -> PlannedNode {
    PlannedNode {
        node_id: node_id.to_owned(),
        executor: "mock@1.0.0".to_owned(),
        max_attempts: 1,
        timeout_seconds: 900,
        children: Vec::new(),
        pending_parents: 0,
    }
}

fn run_state(state: &str, seq: i64) -> Value {
    json!({"type": "run.state", "state": state, "seq": seq})
}

fn node_canceled(node_id: &str, seq: i64) -> Value {
    json!({"type": "node.state", "nodeId": node_id, "state": "canceled", "attempt": 0, "seq": seq})
}

/// The run's event payloads in sequence order, read as the owner.
async fn events(database: &TestDatabase, run_id: Uuid) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM run_events WHERE run_id = $1 ORDER BY seq")
        .bind(run_id)
        .fetch_all(&database.owner)
        .await
        .unwrap()
}

/// Each node of the run with its state, in plan order.
async fn node_states(database: &TestDatabase, run_id: Uuid) -> Vec<(String, String)> {
    sqlx::query_as("SELECT node_id, state FROM node_runs WHERE run_id = $1 ORDER BY id")
        .bind(run_id)
        .fetch_all(&database.owner)
        .await
        .unwrap()
}

/// Every run audit row in order: (action, resource type, resource id, metadata).
async fn run_audit(database: &TestDatabase) -> Vec<(String, String, String, Value)> {
    sqlx::query_as(
        "SELECT action, resource_type, resource_id, metadata FROM audit_events
         WHERE action LIKE 'run.%'
         ORDER BY id",
    )
    .fetch_all(&database.owner)
    .await
    .unwrap()
}

/// An expected run audit row. The resource is the run.
fn audit(action: &str, run_id: Uuid, metadata: Value) -> (String, String, String, Value) {
    let action = action.to_owned();
    (action, "run".to_owned(), run_id.to_string(), metadata)
}

#[tokio::test]
#[serial]
async fn runners_create_runs_from_the_plan_and_viewers_cannot() {
    let fixture = Fixture::new().await;
    let (a, input) = (fixture.org_a, json!({"region": "eu-west-1"}));
    for (user, allowed) in [
        (fixture.admin, true),
        (fixture.editor, true),
        (fixture.viewer, false),
    ] {
        let mut tx = fixture.tx(user, a).await;
        let created = runs::create_run(&mut tx, &fixture.run(user, &input, &fixture.plan)).await;
        tx.rollback().await.unwrap();
        match created {
            Ok(_) => assert!(allowed, "{user}"),
            Err(error) => {
                assert!(!allowed, "{user}: {error}");
                assert_eq!(code(&error).as_deref(), Some("42501"));
            }
        }
    }

    let mut tx = fixture.tx(fixture.runner, a).await;
    let new = fixture.run(fixture.runner, &input, &fixture.plan);
    let first = runs::create_run(&mut tx, &new).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        (
            first.workflow_id,
            first.version,
            first.status.as_str(),
            first.trigger_source.as_str()
        ),
        (fixture.workflow_id, 1, "queued", "manual")
    );
    assert_eq!(
        (first.started_at, first.finished_at, first.cancel_requested),
        (None, None, false)
    );
    // Roots start queued; every other node waits for its distinct parents.
    let stored: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object(
             'nodeId', node_id, 'state', state, 'executor', executor, 'attempt', attempt,
             'maxAttempts', max_attempts, 'timeoutSeconds', timeout_seconds,
             'pendingParents', pending_parents, 'children', children
         )
         FROM node_runs WHERE run_id = $1 ORDER BY id",
    )
    .bind(first.id)
    .fetch_all(&fixture.database.owner)
    .await
    .unwrap();
    let row = |node_id: &str, state: &str, pending_parents: i32, children: &[&str]| {
        json!({
            "nodeId": node_id, "state": state, "executor": "mock@1.0.0", "attempt": 0,
            "maxAttempts": 1, "timeoutSeconds": 900, "pendingParents": pending_parents,
            "children": children
        })
    };
    assert_eq!(
        stored,
        [
            row("vpc", "queued", 0, &["deploy", "rds"]),
            row("rds", "blocked", 1, &[]),
            row("deploy", "blocked", 1, &[]),
        ]
    );
    // Node states are in the snapshot, so creation records only the run's own state.
    assert_eq!(
        events(&fixture.database, first.id).await,
        [run_state("queued", 1)]
    );

    let mut tx = fixture.tx(fixture.admin, a).await;
    let rerun = NewRun {
        rerun_of: Some(first.id),
        ..fixture.run(fixture.admin, &input, &fixture.plan)
    };
    let second = runs::create_run(&mut tx, &rerun).await.unwrap();
    tx.commit().await.unwrap();
    let rerun_of: Option<Uuid> = sqlx::query_scalar("SELECT rerun_of FROM runs WHERE id = $1")
        .bind(second.id)
        .fetch_one(&fixture.database.owner)
        .await
        .unwrap();
    assert_eq!(rerun_of, Some(first.id));

    // A plan without nodes has nothing to wait for, so its run succeeds at once.
    let mut tx = fixture.tx(fixture.runner, a).await;
    let new = fixture.run(fixture.runner, &input, &[]);
    let empty = runs::create_run(&mut tx, &new).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        (empty.status.as_str(), empty.finished_at.is_some()),
        ("succeeded", true)
    );
    assert_eq!(
        events(&fixture.database, empty.id).await,
        [run_state("queued", 1), run_state("succeeded", 2)]
    );

    let mut tx = fixture.tx(fixture.viewer, a).await;
    let listed = runs::list_runs(&mut tx, fixture.workflow_id).await.unwrap();
    let listed: Vec<Uuid> = listed.into_iter().map(|run| run.id).collect();
    assert_eq!(listed, [empty.id, second.id, first.id]);
    let stored = runs::get_run(&mut tx, first.id).await.unwrap().unwrap();
    let version = workflows::get_version(&mut tx, fixture.workflow_id, 1);
    let version = version.await.unwrap().unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        (
            stored.summary.id,
            stored.workflow_name.as_str(),
            &stored.input,
            stored.last_event_seq,
            &stored.graph
        ),
        (first.id, "Provision", &input, 1, &version.graph)
    );
    // Every field is present, with null for a missing value.
    let node = |node_id: &str, state: &str| {
        json!({
            "nodeId": node_id, "state": state, "attempt": 0, "maxAttempts": 1,
            "executor": "mock@1.0.0", "input": null, "output": null, "error": null,
            "startedAt": null, "finishedAt": null
        })
    };
    assert_eq!(
        serde_json::to_value(&stored.nodes).unwrap(),
        json!([
            node("vpc", "queued"),
            node("rds", "blocked"),
            node("deploy", "blocked")
        ])
    );

    assert_eq!(
        run_audit(&fixture.database).await,
        [
            audit(
                "run.started",
                first.id,
                json!({"runId": first.id, "version": 1})
            ),
            audit(
                "run.rerun",
                second.id,
                json!({"runId": second.id, "sourceRunId": first.id, "version": 1})
            ),
            audit(
                "run.started",
                empty.id,
                json!({"runId": empty.id, "version": 1})
            ),
        ]
    );
}

#[tokio::test]
#[serial]
async fn runs_node_runs_and_events_stay_inside_their_organization() {
    let fixture = Fixture::new().await;
    let (a, workflow, version) = (fixture.org_a, fixture.workflow_id, fixture.version_id);
    let run = fixture.create(&fixture.plan).await;

    // Organization B finds nothing through the repository and changes nothing.
    let (b, admin_b) = (fixture.org_b, fixture.admin_b);
    let mut tx = fixture.tx(admin_b, b).await;
    assert!(runs::list_runs(&mut tx, workflow).await.unwrap().is_empty());
    assert!(runs::get_run(&mut tx, run).await.unwrap().is_none());
    assert!(
        runs::events_after(&mut tx, run, 0, 500)
            .await
            .unwrap()
            .is_empty()
    );
    let event = json!({"type": "run.state", "state": "running"});
    let appended = runs::append_run_event(&mut tx, run, &event).await.unwrap();
    assert_eq!(appended, None);
    let canceled = runs::cancel_run(&mut tx, b, admin_b, run).await;
    assert!(
        matches!(canceled, Err(CancelError::NotFound)),
        "{canceled:?}"
    );
    tx.commit().await.unwrap();
    assert_eq!(
        events(&fixture.database, run).await,
        [run_state("queued", 1)]
    );

    for (table, rows) in [("runs", 1), ("node_runs", 3), ("run_events", 1)] {
        let sql = format!("SELECT FROM {table} WHERE organization_id = '{a}'");
        let viewer = fixture.attempt(fixture.viewer, a, &sql).await;
        assert_eq!(viewer, Some(rows), "{sql}");
        let other = fixture.attempt(admin_b, b, &sql).await;
        assert_eq!(other, Some(0), "{sql}");
    }

    // (statement, rows for A's admin, editor and runner, rows for A's viewer and B's admin)
    let writes = [
        (
            format!(
                "INSERT INTO runs(organization_id, workflow_id, workflow_version_id, version, input)
                 VALUES ('{a}', '{workflow}', '{version}', 1, '{{}}')"
            ),
            Some(1),
            None,
        ),
        (
            format!(
                "INSERT INTO node_runs(
                     organization_id, run_id, node_id, executor, state,
                     max_attempts, timeout_seconds, pending_parents
                 ) VALUES ('{a}', '{run}', 'extra', 'mock@1.0.0', 'queued', 1, 900, 0)"
            ),
            Some(1),
            None,
        ),
        (
            format!(
                "INSERT INTO run_events(organization_id, run_id, seq, payload)
                 VALUES ('{a}', '{run}', 2, '{{}}')"
            ),
            Some(1),
            None,
        ),
        ("UPDATE runs SET input = input".to_owned(), Some(1), Some(0)),
        (
            "UPDATE node_runs SET attempt = attempt".to_owned(),
            Some(3),
            Some(0),
        ),
    ];
    for (sql, runners, others) in &writes {
        for (user, organization, expected) in [
            (fixture.admin, a, runners),
            (fixture.editor, a, runners),
            (fixture.runner, a, runners),
            (fixture.viewer, a, others),
            (admin_b, b, others),
        ] {
            let rows = fixture.attempt(user, organization, sql).await;
            assert_eq!(rows, *expected, "{sql}");
        }
    }

    // Events are append-only, and nothing about a run is ever deleted.
    for sql in [
        "UPDATE run_events SET payload = payload",
        "DELETE FROM runs",
        "DELETE FROM node_runs",
        "DELETE FROM run_events",
    ] {
        assert_eq!(fixture.attempt(fixture.admin, a, sql).await, None, "{sql}");
    }
}

#[tokio::test]
#[serial]
async fn concurrent_appends_number_the_events_of_a_run_without_gaps() {
    let fixture = Fixture::new().await;
    let run_id = fixture.create(&fixture.plan).await;

    // Eight connections of their own, so that the appends really overlap.
    let mut writers = JoinSet::new();
    for writer in 0..8 {
        writers.spawn(async move {
            let mut conn = PgConnection::connect(OWNER_URL).await.unwrap();
            for line in 0..25 {
                let text = format!("writer {writer} line {line}");
                let event =
                    json!({"type": "node.log", "nodeId": "vpc", "stream": "stdout", "text": text});
                let seq = runs::append_run_event(&mut conn, run_id, &event).await;
                assert!(seq.unwrap().is_some());
            }
        });
    }
    writers.join_all().await;

    let stored: Vec<(i64, Value)> =
        sqlx::query_as("SELECT seq, payload FROM run_events WHERE run_id = $1 ORDER BY seq")
            .bind(run_id)
            .fetch_all(&fixture.database.owner)
            .await
            .unwrap();
    let seqs: Vec<i64> = stored.iter().map(|(seq, _)| *seq).collect();
    assert_eq!(seqs, (1..=201).collect::<Vec<_>>());
    assert!(stored.iter().all(|(seq, payload)| payload["seq"] == *seq));
    let last: i64 = sqlx::query_scalar("SELECT last_event_seq FROM runs WHERE id = $1")
        .bind(run_id)
        .fetch_one(&fixture.database.owner)
        .await
        .unwrap();
    assert_eq!(last, 201);
}

#[tokio::test]
#[serial]
async fn appended_events_drop_only_their_own_null_fields() {
    let fixture = Fixture::new().await;
    let run_id = fixture.create(&fixture.plan).await;

    // A null that belongs to a node's output is data and stays.
    let mut tx = fixture.tx(fixture.runner, fixture.org_a).await;
    let event = json!({
        "type": "node.state", "nodeId": "vpc", "state": "succeeded", "attempt": 1,
        "error": null, "output": {"vpcId": "vpc-1", "tags": null}, "seq": 99
    });
    let seq = runs::append_run_event(&mut tx, run_id, &event)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(seq, Some(2));
    assert_eq!(
        events(&fixture.database, run_id).await[1],
        json!({
            "type": "node.state", "nodeId": "vpc", "state": "succeeded", "attempt": 1,
            "output": {"vpcId": "vpc-1", "tags": null}, "seq": 2
        })
    );
}

#[tokio::test]
#[serial]
async fn each_event_notifies_listeners_with_its_run_id() {
    let fixture = Fixture::new().await;
    // The API listens as itself; LISTEN needs no privilege.
    let mut listener = PgListener::connect(API_URL).await.unwrap();
    listener.listen("corqty_run_events").await.unwrap();

    let run_id = fixture.create(&fixture.plan).await;

    let notification = tokio::time::timeout(Duration::from_secs(5), listener.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (notification.channel(), notification.payload()),
        ("corqty_run_events", run_id.to_string().as_str())
    );
}

#[tokio::test]
#[serial]
async fn settling_gives_a_finished_run_the_status_the_domain_aggregates() {
    use NodeState::{Blocked, Canceled, Failed, Queued, RetryWait, Running, Skipped, Succeeded};
    let fixture = Fixture::new().await;
    let owner = &fixture.database.owner;
    let cases: [(&[NodeState], bool); 9] = [
        (&[Succeeded, Running], false),
        (&[Queued, Blocked], true),
        (&[RetryWait, Canceled], true),
        (&[Succeeded, Succeeded], false),
        (&[Succeeded, Succeeded], true),
        (&[Succeeded, Canceled, Skipped], false),
        (&[Failed, Skipped], false),
        (&[Succeeded, Failed, Canceled, Skipped], true),
        (&[], false),
    ];
    for (states, cancel_requested) in cases {
        let nodes: Vec<_> = (0..states.len())
            .map(|index| root(&format!("n{index}")))
            .collect();
        let run_id = fixture.create(&nodes).await;
        let node_ids: Vec<&str> = nodes.iter().map(|node| node.node_id.as_str()).collect();
        let texts: Vec<&str> = states.iter().map(|state| state.as_str()).collect();
        sqlx::query(
            "UPDATE node_runs n SET state = s.state
             FROM unnest($2::text[], $3::text[]) AS s(node_id, state)
             WHERE n.run_id = $1 AND n.node_id = s.node_id",
        )
        .bind(run_id)
        .bind(&node_ids)
        .bind(&texts)
        .execute(owner)
        .await
        .unwrap();
        sqlx::query("UPDATE runs SET cancel_requested_at = now() WHERE id = $1 AND $2")
            .bind(run_id)
            .bind(cancel_requested)
            .execute(owner)
            .await
            .unwrap();
        // The second call finds the run settled and leaves it alone.
        for _ in 0..2 {
            let mut tx = fixture.tx(fixture.runner, fixture.org_a).await;
            runs::settle_run(&mut tx, run_id).await.unwrap();
            tx.commit().await.unwrap();
        }

        let expected = aggregate(states, cancel_requested);
        let stored: (String, bool) =
            sqlx::query_as("SELECT status, finished_at IS NOT NULL FROM runs WHERE id = $1")
                .bind(run_id)
                .fetch_one(owner)
                .await
                .unwrap();
        let status = expected.map_or("queued", RunStatus::as_str);
        let case = format!("{states:?} with cancel requested {cancel_requested}");
        assert_eq!(stored, (status.to_owned(), expected.is_some()), "{case}");
        let mut log = vec![run_state("queued", 1)];
        log.extend(expected.map(|status| run_state(status.as_str(), 2)));
        assert_eq!(events(&fixture.database, run_id).await, log, "{case}");
    }
}

#[tokio::test]
#[serial]
async fn canceling_cancels_the_waiting_nodes_and_settles_the_run_once_nothing_runs() {
    let fixture = Fixture::new().await;
    let (a, runner, owner) = (fixture.org_a, fixture.runner, &fixture.database.owner);
    let waiting = fixture.create(&fixture.plan).await;
    let started = fixture.create(&fixture.plan).await;
    // As a worker would leave it: the second run's vpc is running.
    sqlx::query(
        "UPDATE node_runs SET state = 'running', attempt = 1, started_at = now()
         WHERE run_id = $1 AND node_id = 'vpc'",
    )
    .bind(started)
    .execute(owner)
    .await
    .unwrap();
    sqlx::query("UPDATE runs SET status = 'running', started_at = now() WHERE id = $1")
        .bind(started)
        .execute(owner)
        .await
        .unwrap();

    // A viewer cannot lock the run, and organization B cannot see it.
    for (user, organization) in [(fixture.viewer, a), (fixture.admin_b, fixture.org_b)] {
        let mut tx = fixture.tx(user, organization).await;
        let canceled = runs::cancel_run(&mut tx, organization, user, waiting).await;
        tx.rollback().await.unwrap();
        assert!(
            matches!(canceled, Err(CancelError::NotFound)),
            "{canceled:?}"
        );
    }

    let mut tx = fixture.tx(runner, a).await;
    let canceled = runs::cancel_run(&mut tx, a, runner, waiting).await.unwrap();
    let requested = runs::cancel_run(&mut tx, a, runner, started).await.unwrap();
    tx.commit().await.unwrap();
    let summary = |run: &runs::RunSummaryRecord| {
        let finished = run.finished_at.is_some();
        (run.status.clone(), run.cancel_requested, finished)
    };
    assert_eq!(summary(&canceled), ("canceled".to_owned(), true, true));
    assert_eq!(
        events(&fixture.database, waiting).await,
        [
            run_state("queued", 1),
            node_canceled("vpc", 2),
            node_canceled("rds", 3),
            node_canceled("deploy", 4),
            run_state("canceled", 5),
        ]
    );
    // The running node is left to its worker, so the run is not settled yet.
    assert_eq!(summary(&requested), ("running".to_owned(), true, false));
    let state = |node_id: &str, state: &str| (node_id.to_owned(), state.to_owned());
    assert_eq!(
        node_states(&fixture.database, started).await,
        [
            state("vpc", "running"),
            state("rds", "canceled"),
            state("deploy", "canceled")
        ]
    );
    assert_eq!(
        events(&fixture.database, started).await,
        [
            run_state("queued", 1),
            node_canceled("rds", 2),
            node_canceled("deploy", 3),
        ]
    );

    // Once the worker has stopped the node, the run settles as canceled, and a finished
    // run cannot be canceled again.
    sqlx::query("UPDATE node_runs SET state = 'canceled' WHERE run_id = $1 AND node_id = 'vpc'")
        .bind(started)
        .execute(owner)
        .await
        .unwrap();
    let mut tx = fixture.tx(runner, a).await;
    runs::settle_run(&mut tx, started).await.unwrap();
    let again = runs::cancel_run(&mut tx, a, runner, waiting).await;
    tx.commit().await.unwrap();
    assert!(matches!(again, Err(CancelError::Finished)), "{again:?}");
    let log = events(&fixture.database, started).await;
    assert_eq!(log.last(), Some(&run_state("canceled", 4)));

    let mut tx = fixture.tx(fixture.viewer, a).await;
    let page = runs::events_after(&mut tx, waiting, 1, 2).await.unwrap();
    tx.commit().await.unwrap();
    let page: Vec<_> = page
        .into_iter()
        .map(|event| (event.seq, event.payload))
        .collect();
    assert_eq!(
        page,
        [(2, node_canceled("vpc", 2)), (3, node_canceled("rds", 3))]
    );

    assert_eq!(
        run_audit(&fixture.database).await[2..],
        [
            audit("run.canceled", waiting, json!({"runId": waiting})),
            audit("run.canceled", started, json!({"runId": started})),
        ]
    );
}

#[tokio::test]
#[serial]
async fn worker_instances_stay_hidden_and_the_worker_role_reaches_no_run_data() {
    let fixture = Fixture::new().await;
    let run_id = fixture.create(&fixture.plan).await;
    sqlx::query(
        "INSERT INTO worker_instances(id, executors, capacity)
         VALUES ($1, ARRAY['mock@1.0.0'], 4)",
    )
    .bind(Uuid::now_v7())
    .execute(&fixture.database.owner)
    .await
    .unwrap();

    // The API learns about workers only through definer functions.
    let sql = "SELECT FROM worker_instances";
    for (user, organization) in [
        (fixture.admin, fixture.org_a),
        (fixture.admin_b, fixture.org_b),
    ] {
        assert_eq!(fixture.attempt(user, organization, sql).await, None);
    }

    // The worker has no table grants, and the shared run functions belong to the API.
    for sql in [
        "SELECT FROM runs".to_owned(),
        "SELECT FROM worker_instances".to_owned(),
        format!("SELECT corqty.append_run_event('{run_id}', '{{}}')"),
        format!("SELECT corqty.settle_run('{run_id}')"),
    ] {
        let error = sqlx::query(&sql)
            .execute(&fixture.database.worker)
            .await
            .unwrap_err();
        assert_eq!(code(&error).as_deref(), Some("42501"), "{sql}: {error}");
    }
}
