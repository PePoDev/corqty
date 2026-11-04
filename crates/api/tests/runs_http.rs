mod support;

use std::pin::pin;

use axum::{
    body::to_bytes,
    http::{Method, StatusCode},
    response::Response,
};
use corqty_domain::runtime::MAX_RUN_INPUT_BYTES;
use corqty_persistence::worker_queue::WorkerDb;
use serde_json::{Map, Value, json};
use serial_test::serial;
use support::AuthFixture;
use uuid::Uuid;

const INFRASTRUCTURE_BLUEPRINT_ID: &str = "00000000-0000-7000-8000-000000000101";
const INPUT: &str = r#"{"input":{"region":"eu-west-1"}}"#;
const NO_WORKER: &str = "waiting_for_compatible_worker";

/// Asserts the status and returns the JSON body. A run repeats its input, up to 1 MiB.
async fn body(response: Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    let body = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// Registers `<role>@example.com` and adds it to the admin's organization with that role.
async fn member_of(admin: &AuthFixture, role: &str) -> AuthFixture {
    let invitations = format!("/api/v1/orgs/{}/invitations", admin.organization_id);
    let email = format!("{role}@example.com");
    let request = json!({ "email": email, "role": role }).to_string();
    let invitation = body(
        admin.post_json(&invitations, &request).await,
        StatusCode::CREATED,
    )
    .await;
    let member = admin.register_another(&email, "Personal").await;
    let accept = json!({ "token": invitation["token"] }).to_string();
    let accepted = member
        .post_json("/api/v1/invitations/accept", &accept)
        .await;
    assert_eq!(accepted.status(), StatusCode::OK);
    member
}

/// A workflow of the organization whose version 1 is the simulated-infrastructure blueprint
/// with the blueprint's own node ids: vpc, then rds and deploy, which wait for vpc. Returns
/// the workflow's URL.
async fn infrastructure_workflow(member: &AuthFixture, organization_id: Uuid) -> String {
    let org = format!("/api/v1/orgs/{organization_id}");
    let blueprints = body(
        member.get(&format!("{org}/blueprints")).await,
        StatusCode::OK,
    )
    .await;
    let blueprint = blueprints["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|blueprint| blueprint["id"] == INFRASTRUCTURE_BLUEPRINT_ID)
        .unwrap();
    let created = body(
        member
            .post_json(&format!("{org}/workflows"), r#"{"name":"Provision"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let workflow = format!("{org}/workflows/{}", created["id"].as_str().unwrap());
    let draft = json!({ "revision": 1, "graph": blueprint["graph"] }).to_string();
    let saved = member
        .request_json(Method::PUT, &format!("{workflow}/draft"), &draft)
        .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let published = member
        .post_json(&format!("{workflow}/publish"), r#"{"revision":2}"#)
        .await;
    assert_eq!(published.status(), StatusCode::CREATED);
    workflow
}

fn run_id(run: &Value) -> Uuid {
    run["id"].as_str().unwrap().parse().unwrap()
}

/// The URL of a run of the admin's organization.
fn run_path(admin: &AuthFixture, run: &Value) -> String {
    format!(
        "/api/v1/orgs/{}/runs/{}",
        admin.organization_id,
        run_id(run)
    )
}

/// The RunSummary fields of a Run.
fn summary(run: &Value) -> Value {
    let fields = [
        "id",
        "workflowId",
        "version",
        "status",
        "triggerSource",
        "createdAt",
        "startedAt",
        "finishedAt",
        "cancelRequested",
    ];
    let summary: Map<String, Value> = fields
        .into_iter()
        .map(|field| (field.to_owned(), run[field].clone()))
        .collect();
    Value::Object(summary)
}

/// A node run that no worker has claimed yet.
fn unclaimed(node_id: &str, state: &str, queue_reason: Option<&str>) -> Value {
    json!({
        "nodeId": node_id, "state": state, "attempt": 0, "maxAttempts": 1,
        "executor": "mock@1.0.0", "queueReason": queue_reason,
        "input": null, "output": null, "error": null, "startedAt": null, "finishedAt": null,
    })
}

/// Each node's queue reason, in plan order.
async fn queue_reasons(admin: &AuthFixture, run: &Value) -> Vec<Value> {
    let run = body(admin.get(&run_path(admin, run)).await, StatusCode::OK).await;
    run["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["queueReason"].clone())
        .collect()
}

/// The run's events in seq order.
async fn events(admin: &AuthFixture, run: &Value) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM run_events WHERE run_id = $1 ORDER BY seq")
        .bind(run_id(run))
        .fetch_all(&admin.owner)
        .await
        .unwrap()
}

/// The admin's organization's run audit events, newest first, as
/// [actorUserId, action, resourceId, metadata].
async fn run_audit(admin: &AuthFixture) -> Vec<Value> {
    let path = format!("/api/v1/orgs/{}/audit-events", admin.organization_id);
    let events = body(admin.get(&path).await, StatusCode::OK).await;
    events["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["resourceType"] == "run")
        .map(|event| {
            json!([
                event["actorUserId"],
                event["action"],
                event["resourceId"],
                event["metadata"],
            ])
        })
        .collect()
}

#[tokio::test]
#[serial]
async fn runners_start_the_published_version_with_its_plan() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let runner = member_of(&admin, "runner").await;
    let workflow = infrastructure_workflow(&admin, admin.organization_id).await;
    let version = body(
        admin.get(&format!("{workflow}/versions/1")).await,
        StatusCode::OK,
    )
    .await;
    let runs = format!("{workflow}/runs");

    let run = body(runner.post_json(&runs, INPUT).await, StatusCode::CREATED).await;
    assert!(run["createdAt"].is_string());
    assert_eq!(
        run,
        json!({
            "id": run["id"],
            "workflowId": workflow.rsplit('/').next().unwrap(),
            "version": 1,
            "status": "queued",
            "triggerSource": "manual",
            "createdAt": run["createdAt"],
            "startedAt": null,
            "finishedAt": null,
            "cancelRequested": false,
            "workflowName": "Provision",
            "input": { "region": "eu-west-1" },
            "lastEventSeq": 1,
            "graph": version["graph"],
            "nodes": [
                unclaimed("vpc", "queued", Some(NO_WORKER)),
                unclaimed("rds", "blocked", None),
                unclaimed("deploy", "blocked", None),
            ],
        })
    );
    // Only the run is evented at creation; the snapshot holds the node states.
    assert_eq!(
        events(&admin, &run).await,
        [json!({ "type": "run.state", "state": "queued", "seq": 1 })]
    );
    assert_eq!(
        body(runner.get(&run_path(&admin, &run)).await, StatusCode::OK).await,
        run
    );

    // The list holds summaries, newest first.
    let second = body(
        admin
            .post_json(&runs, r#"{"input":{"region":"us-east-1"}}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(
        body(runner.get(&runs).await, StatusCode::OK).await,
        json!({ "items": [summary(&second), summary(&run)] })
    );
}

#[tokio::test]
#[serial]
async fn runs_start_only_from_a_published_version() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let org = format!("/api/v1/orgs/{}", admin.organization_id);
    let created = body(
        admin
            .post_json(&format!("{org}/workflows"), r#"{"name":"Draft only"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let draft_only = format!("{org}/workflows/{}", created["id"].as_str().unwrap());
    let unpublished = body(
        admin.post_json(&format!("{draft_only}/runs"), "{}").await,
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(unpublished["code"], "workflow_not_published");

    // A version that does not exist is not found, whether the workflow is published or not,
    // and so is a workflow that does not exist.
    let workflow = infrastructure_workflow(&admin, admin.organization_id).await;
    let unknown = format!("{org}/workflows/{}", Uuid::new_v4());
    for (path, request) in [
        (
            &workflow,
            r#"{"input":{"region":"eu-west-1"},"version":99}"#,
        ),
        (&draft_only, r#"{"version":1}"#),
        (&unknown, INPUT),
    ] {
        let response = admin.post_json(&format!("{path}/runs"), request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path} {request}");
    }
    let missing = admin.get(&format!("{unknown}/runs")).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    // Nothing was started.
    assert_eq!(
        body(admin.get(&format!("{workflow}/runs")).await, StatusCode::OK).await,
        json!({ "items": [] })
    );
    assert_eq!(run_audit(&admin).await, Vec::<Value>::new());
}

#[tokio::test]
#[serial]
async fn run_inputs_must_match_the_schema_and_fit_in_one_mebibyte() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let workflow = infrastructure_workflow(&admin, admin.organization_id).await;
    let runs = format!("{workflow}/runs");

    let invalid = body(
        admin.post_json(&runs, r#"{"input":{"region":5}}"#).await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(
        invalid,
        json!({
            "type": "about:blank",
            "title": "Validation failed",
            "status": 422,
            "code": "run_input_invalid",
            "diagnostics": [{
                "code": "invalid_input",
                "pointer": "/region",
                "message": "value is not of type \"string\"",
            }],
        })
    );
    // A missing input is the empty object, which lacks the required region.
    let missing = body(
        admin.post_json(&runs, "{}").await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(missing["code"], "run_input_invalid");
    assert_eq!(
        missing["diagnostics"],
        json!([{
            "code": "invalid_input",
            "pointer": "",
            "message": "\"region\" is a required property",
        }])
    );

    // The limit applies to the input's serialized JSON: exactly 1 MiB passes, a byte more
    // does not.
    let room = MAX_RUN_INPUT_BYTES - r#"{"region":""}"#.len();
    let request = |length: usize| json!({ "input": { "region": "a".repeat(length) } }).to_string();
    let too_large = body(
        admin.post_json(&runs, &request(room + 1)).await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(too_large["code"], "run_input_too_large");
    let largest = body(
        admin.post_json(&runs, &request(room)).await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(largest["input"]["region"].as_str().unwrap().len(), room);

    // An input the database cannot store, and one that is not an object.
    let nul = body(
        admin
            .post_json(&runs, r#"{"input":{"region":"eu\u0000west"}}"#)
            .await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(nul["code"], "unsupported_character");
    let list = body(
        admin.post_json(&runs, r#"{"input":["eu-west-1"]}"#).await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(list["code"], "invalid_json");

    // Only the 1 MiB run was started.
    assert_eq!(
        body(admin.get(&runs).await, StatusCode::OK).await,
        json!({ "items": [summary(&largest)] })
    );
}

#[tokio::test]
#[serial]
async fn reruns_copy_the_input_and_default_to_the_source_version() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let runner = member_of(&admin, "runner").await;
    let workflow = infrastructure_workflow(&admin, admin.organization_id).await;
    let first = body(
        runner.post_json(&format!("{workflow}/runs"), INPUT).await,
        StatusCode::CREATED,
    )
    .await;

    // Version 2 also requires a replica count, which the first run's input lacks.
    let stored = body(admin.get(&workflow).await, StatusCode::OK).await;
    let mut graph = stored["draft"]["graph"].clone();
    graph["workflowInputSchema"]["properties"]["replicas"] = json!({ "type": "integer" });
    graph["workflowInputSchema"]["required"] = json!(["region", "replicas"]);
    let save = json!({ "revision": 2, "graph": graph }).to_string();
    let saved = admin
        .request_json(Method::PUT, &format!("{workflow}/draft"), &save)
        .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let published = admin
        .post_json(&format!("{workflow}/publish"), r#"{"revision":3}"#)
        .await;
    assert_eq!(published.status(), StatusCode::CREATED);

    let rerun_path = format!("{}/rerun", run_path(&admin, &first));
    let rerun = body(
        runner.post_json(&rerun_path, "{}").await,
        StatusCode::CREATED,
    )
    .await;
    assert_ne!(rerun["id"], first["id"]);
    assert_eq!(
        (&rerun["version"], &rerun["status"], &rerun["input"]),
        (&json!(1), &json!("queued"), &first["input"])
    );
    assert_eq!(
        (&rerun["graph"], &rerun["nodes"]),
        (&first["graph"], &first["nodes"])
    );
    // On version 2 the copied input is checked against version 2's schema.
    let invalid = body(
        runner.post_json(&rerun_path, r#"{"version":2}"#).await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(
        (&invalid["code"], &invalid["diagnostics"]),
        (
            &json!("run_input_invalid"),
            &json!([{
                "code": "invalid_input",
                "pointer": "",
                "message": "\"replicas\" is a required property",
            }])
        )
    );
    let unknown = runner.post_json(&rerun_path, r#"{"version":99}"#).await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    // A new run without a version starts the published version, now 2.
    let latest = body(
        runner
            .post_json(
                &format!("{workflow}/runs"),
                r#"{"input":{"region":"eu-west-1","replicas":2}}"#,
            )
            .await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(latest["version"], 2);

    // Each new run audited how it started, with ids and the version only.
    assert_eq!(
        run_audit(&admin).await,
        [
            json!([
                runner.user_id,
                "run.started",
                latest["id"],
                { "runId": latest["id"], "version": 2 },
            ]),
            json!([
                runner.user_id,
                "run.rerun",
                rerun["id"],
                { "runId": rerun["id"], "sourceRunId": first["id"], "version": 1 },
            ]),
            json!([
                runner.user_id,
                "run.started",
                first["id"],
                { "runId": first["id"], "version": 1 },
            ]),
        ]
    );
}

#[tokio::test]
#[serial]
async fn cancel_ends_a_queued_run_once() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let runner = member_of(&admin, "runner").await;
    let workflow = infrastructure_workflow(&admin, admin.organization_id).await;
    let run = body(
        runner.post_json(&format!("{workflow}/runs"), INPUT).await,
        StatusCode::CREATED,
    )
    .await;
    let cancel = format!("{}/cancel", run_path(&admin, &run));

    // The web app sends a cancel without a body or a content type.
    let canceled = body(runner.post(&cancel).await, StatusCode::OK).await;
    assert!(canceled["finishedAt"].is_string());
    let mut expected = summary(&run);
    expected["status"] = json!("canceled");
    expected["finishedAt"] = canceled["finishedAt"].clone();
    expected["cancelRequested"] = json!(true);
    assert_eq!(canceled, expected);

    // Every node was waiting, so every node is canceled and the run is over.
    let stored = body(runner.get(&run_path(&admin, &run)).await, StatusCode::OK).await;
    assert_eq!(summary(&stored), canceled);
    let nodes: Vec<Value> = stored["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| {
            json!([
                node["nodeId"],
                node["state"],
                node["queueReason"],
                node["finishedAt"].is_string()
            ])
        })
        .collect();
    assert_eq!(
        nodes,
        [
            json!(["vpc", "canceled", null, true]),
            json!(["rds", "canceled", null, true]),
            json!(["deploy", "canceled", null, true]),
        ]
    );
    let node_canceled = |node_id: &str, seq: i64| {
        json!({
            "type": "node.state", "nodeId": node_id, "state": "canceled", "attempt": 0,
            "seq": seq,
        })
    };
    assert_eq!(
        events(&admin, &run).await,
        [
            json!({ "type": "run.state", "state": "queued", "seq": 1 }),
            node_canceled("vpc", 2),
            node_canceled("rds", 3),
            node_canceled("deploy", 4),
            json!({ "type": "run.state", "state": "canceled", "seq": 5 }),
        ]
    );
    assert_eq!(stored["lastEventSeq"], 5);

    // A finished run cannot be canceled again, and only the first cancel was audited.
    let again = body(runner.post(&cancel).await, StatusCode::CONFLICT).await;
    assert_eq!(again["code"], "run_finished");
    assert_eq!(
        run_audit(&admin).await,
        [
            json!([
                runner.user_id,
                "run.canceled",
                run["id"],
                { "runId": run["id"] },
            ]),
            json!([
                runner.user_id,
                "run.started",
                run["id"],
                { "runId": run["id"], "version": 1 },
            ]),
        ]
    );
}

#[tokio::test]
#[serial]
async fn a_cancel_that_deadlocks_is_tried_again() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let workflow = infrastructure_workflow(&admin, admin.organization_id).await;
    let run = body(
        admin.post_json(&format!("{workflow}/runs"), INPUT).await,
        StatusCode::CREATED,
    )
    .await;

    // Another transaction locks a node of the run and then the run itself, the reverse of the
    // order every writer of a run follows. The cancel, which waits first, loses the deadlock.
    let mut other = admin.owner.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM node_runs WHERE run_id = $1 AND node_id = 'vpc' FOR UPDATE")
        .bind(run_id(&run))
        .execute(&mut *other)
        .await
        .unwrap();
    let cancel = format!("{}/cancel", run_path(&admin, &run));
    let mut response = pin!(admin.post(&cancel));
    let waiting =
        "SELECT EXISTS (SELECT 1 FROM pg_locks WHERE locktype = 'transactionid' AND NOT granted)";
    tokio::select! {
        response = &mut response => panic!("the cancel did not wait: {}", response.status()),
        () = async {
            while !sqlx::query_scalar::<_, bool>(waiting).fetch_one(&admin.owner).await.unwrap() {}
        } => {}
    }
    let deadlock = async {
        sqlx::query("SELECT 1 FROM runs WHERE id = $1 FOR UPDATE")
            .bind(run_id(&run))
            .execute(&mut *other)
            .await
            .unwrap();
        other.commit().await.unwrap();
    };
    let (response, ()) = tokio::join!(response, deadlock);

    let canceled = body(response, StatusCode::OK).await;
    assert_eq!(canceled["status"], "canceled");
    // The failed attempt rolled back completely: one set of events and one audit row.
    assert_eq!(events(&admin, &run).await.len(), 5);
    assert_eq!(
        run_audit(&admin).await,
        [
            json!([
                admin.user_id,
                "run.canceled",
                run["id"],
                { "runId": run["id"] },
            ]),
            json!([
                admin.user_id,
                "run.started",
                run["id"],
                { "runId": run["id"], "version": 1 },
            ]),
        ]
    );
}

#[tokio::test]
#[serial]
async fn queued_nodes_wait_for_a_worker_that_runs_their_executor() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let workflow = infrastructure_workflow(&admin, admin.organization_id).await;
    let run = body(
        admin.post_json(&format!("{workflow}/runs"), INPUT).await,
        StatusCode::CREATED,
    )
    .await;
    // deploy waits for its retry the way vpc waits for its first attempt. rds waits for vpc,
    // not for a worker.
    sqlx::query(
        "UPDATE node_runs SET state = 'retry_wait' WHERE run_id = $1 AND node_id = 'deploy'",
    )
    .bind(run_id(&run))
    .execute(&admin.owner)
    .await
    .unwrap();
    let waiting = [json!(NO_WORKER), Value::Null, json!(NO_WORKER)];
    assert_eq!(queue_reasons(&admin, &run).await, waiting);

    // A worker without the mock executor changes nothing; one with it is enough.
    let shell = WorkerDb::new(admin.worker.clone(), Uuid::now_v7());
    shell
        .register(&["shell@1.0.0".to_owned()], 4)
        .await
        .unwrap();
    assert_eq!(queue_reasons(&admin, &run).await, waiting);
    let mock = WorkerDb::new(admin.worker.clone(), Uuid::now_v7());
    mock.register(&["mock@1.0.0".to_owned()], 4).await.unwrap();
    assert_eq!(
        queue_reasons(&admin, &run).await,
        [Value::Null, Value::Null, Value::Null]
    );

    // A worker that has not been heard from for a minute no longer counts.
    sqlx::query("UPDATE worker_instances SET last_heartbeat_at = now() - interval '61 seconds'")
        .execute(&admin.owner)
        .await
        .unwrap();
    assert_eq!(queue_reasons(&admin, &run).await, waiting);
}

#[tokio::test]
#[serial]
async fn run_routes_do_not_reach_another_organizations_runs() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let beta = body(
        admin
            .post_json("/api/v1/organizations", r#"{"name":"Beta"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let beta: Uuid = beta["organizationId"].as_str().unwrap().parse().unwrap();
    let beta_workflow = infrastructure_workflow(&admin, beta).await;
    let beta_runs = format!("{beta_workflow}/runs");
    let beta_run = body(
        admin.post_json(&beta_runs, INPUT).await,
        StatusCode::CREATED,
    )
    .await;

    // Through Acme's URLs, Beta's workflow and run are not found, just like unknown ids.
    let acme = format!("/api/v1/orgs/{}", admin.organization_id);
    let foreign = (
        beta_workflow.rsplit('/').next().unwrap().to_owned(),
        run_id(&beta_run).to_string(),
    );
    let unknown = (Uuid::new_v4().to_string(), Uuid::new_v4().to_string());
    for (workflow, run) in [foreign, unknown] {
        for (method, path, request) in [
            (Method::GET, format!("{acme}/workflows/{workflow}/runs"), ""),
            (
                Method::POST,
                format!("{acme}/workflows/{workflow}/runs"),
                INPUT,
            ),
            (Method::GET, format!("{acme}/runs/{run}"), ""),
            (Method::POST, format!("{acme}/runs/{run}/cancel"), ""),
            (Method::POST, format!("{acme}/runs/{run}/rerun"), "{}"),
        ] {
            let response = admin.request_json(method.clone(), &path, request).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {path}");
        }
    }

    // Beta's run was neither canceled nor rerun, and Acme audited nothing.
    let beta_run_path = format!("/api/v1/orgs/{beta}/runs/{}", run_id(&beta_run));
    assert_eq!(
        body(admin.get(&beta_run_path).await, StatusCode::OK).await,
        beta_run
    );
    assert_eq!(
        body(admin.get(&beta_runs).await, StatusCode::OK).await,
        json!({ "items": [summary(&beta_run)] })
    );
    assert_eq!(run_audit(&admin).await, Vec::<Value>::new());
}
