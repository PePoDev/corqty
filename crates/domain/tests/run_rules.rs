use corqty_domain::{
    graph::GraphSnapshot,
    publish::Definitions,
    run::{NON_RETRYABLE, NodeState, PlannedNode, RunStatus, aggregate, is_retryable, plan_run},
};
use serde_json::{Value, json};

const STATES: [NodeState; 8] = [
    NodeState::Blocked,
    NodeState::Queued,
    NodeState::Running,
    NodeState::RetryWait,
    NodeState::Succeeded,
    NodeState::Failed,
    NodeState::Canceled,
    NodeState::Skipped,
];

/// Definitions that only differ in their executor, the one field `plan_run` reads.
fn definitions(keys: &[(&str, &str)]) -> Definitions {
    keys.iter()
        .map(|&(key, executor)| {
            let (executor_key, executor_version) = executor.split_once('@').unwrap();
            let spec = json!({
                "inputs": [],
                "outputs": [],
                "configSchema": {"type": "object"},
                "uiSchema": {},
                "defaults": {},
                "executor": {"key": executor_key, "version": executor_version}
            });
            ((key.to_owned(), 1), serde_json::from_value(spec).unwrap())
        })
        .collect()
}

/// A node using version 1 of the definition `key`.
fn node(id: &str, key: &str, max_attempts: u32, timeout_seconds: u32) -> Value {
    json!({
        "id": id,
        "definition": {"key": key, "version": 1},
        "displayName": id,
        "position": {"x": 0, "y": 0},
        "config": {},
        "inputs": {},
        "retry": {"maxAttempts": max_attempts},
        "timeoutSeconds": timeout_seconds
    })
}

/// An edge between two `node.port` ends.
fn edge(id: &str, source: &str, target: &str) -> Value {
    let end = |end: &str| {
        let (node, port) = end.split_once('.').unwrap();
        json!({"nodeId": node, "port": port})
    };
    json!({"id": id, "source": end(source), "target": end(target)})
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

fn planned(
    node_id: &str,
    executor: &str,
    max_attempts: i32,
    timeout_seconds: i32,
    children: &[&str],
    pending_parents: i32,
) -> PlannedNode {
    PlannedNode {
        node_id: node_id.to_owned(),
        executor: executor.to_owned(),
        max_attempts,
        timeout_seconds,
        children: children.iter().map(|&child| child.to_owned()).collect(),
        pending_parents,
    }
}

#[test]
fn node_transitions_follow_the_run_lifecycle() {
    use NodeState::{Blocked, Canceled, Failed, Queued, RetryWait, Running, Skipped, Succeeded};
    // Spec §11.1, plus retry_wait -> canceled (cancel stops a pending retry) and
    // retry_wait -> running (a due retry is claimed directly).
    let allowed = [
        (Blocked, Queued),
        (Blocked, Canceled),
        (Blocked, Skipped),
        (Queued, Running),
        (Queued, Canceled),
        (Running, Succeeded),
        (Running, Failed),
        (Running, RetryWait),
        (Running, Canceled),
        (RetryWait, Queued),
        (RetryWait, Running),
        (RetryWait, Canceled),
    ];
    for from in STATES {
        for to in STATES {
            assert_eq!(
                from.can_transition(to),
                allowed.contains(&(from, to)),
                "{from:?} -> {to:?}"
            );
        }
    }
}

#[test]
fn succeeded_failed_canceled_and_skipped_are_terminal() {
    for state in STATES {
        let terminal = matches!(
            state,
            NodeState::Succeeded | NodeState::Failed | NodeState::Canceled | NodeState::Skipped
        );
        assert_eq!(state.is_terminal(), terminal, "{state:?}");
        // Nothing leaves a terminal state.
        if terminal {
            assert!(
                STATES.iter().all(|&to| !state.can_transition(to)),
                "{state:?}"
            );
        }
    }
}

#[test]
fn states_and_statuses_are_snake_case() {
    for (state, text) in STATES.into_iter().zip([
        "blocked",
        "queued",
        "running",
        "retry_wait",
        "succeeded",
        "failed",
        "canceled",
        "skipped",
    ]) {
        assert_eq!(state.as_str(), text);
        assert_eq!(serde_json::to_value(state).unwrap(), json!(text));
        assert_eq!(
            serde_json::from_value::<NodeState>(json!(text)).unwrap(),
            state
        );
    }
    for (status, text) in [
        (RunStatus::Queued, "queued"),
        (RunStatus::Running, "running"),
        (RunStatus::Succeeded, "succeeded"),
        (RunStatus::Failed, "failed"),
        (RunStatus::Canceled, "canceled"),
    ] {
        assert_eq!(status.as_str(), text);
        assert_eq!(serde_json::to_value(status).unwrap(), json!(text));
        assert_eq!(
            serde_json::from_value::<RunStatus>(json!(text)).unwrap(),
            status
        );
    }
}

#[test]
fn runs_settle_only_once_every_node_is_terminal() {
    use NodeState::{Canceled, Failed, Skipped, Succeeded};
    for state in STATES.into_iter().filter(|state| !state.is_terminal()) {
        assert_eq!(aggregate(&[Failed, state], true), None, "{state:?}");
    }
    for (states, cancel_requested, status) in [
        // A failure wins over a cancel request and over canceled nodes.
        (&[Failed, Canceled, Succeeded][..], true, RunStatus::Failed),
        // Nodes skipped below a failure leave the run failed.
        (&[Succeeded, Failed, Skipped], false, RunStatus::Failed),
        (&[Succeeded, Succeeded], true, RunStatus::Canceled),
        (&[Succeeded, Canceled, Skipped], false, RunStatus::Canceled),
        (&[Succeeded, Succeeded], false, RunStatus::Succeeded),
        // A graph without nodes has nothing left to run.
        (&[], false, RunStatus::Succeeded),
    ] {
        assert_eq!(
            aggregate(states, cancel_requested),
            Some(status),
            "{states:?}, cancel requested: {cancel_requested}"
        );
    }
}

#[test]
fn listed_codes_are_never_retried_and_others_follow_the_executor() {
    assert_eq!(
        NON_RETRYABLE,
        [
            "input_missing",
            "input_invalid",
            "input_too_large",
            "output_invalid",
            "output_too_large",
            "secret_unavailable",
            "secret_output_detected",
            "executor_missing",
            "permission_denied",
            "capability_denied",
        ]
    );
    for code in NON_RETRYABLE {
        assert!(!is_retryable(code, true), "{code}");
        assert!(!is_retryable(code, false), "{code}");
    }
    for code in [
        "timeout",
        "lease_expired",
        "mock_retryable_failure",
        "shell_exit",
    ] {
        assert!(is_retryable(code, true), "{code}");
        assert!(!is_retryable(code, false), "{code}");
    }
}

#[test]
fn seeded_blueprints_plan_their_roots_queued_and_children_by_name() {
    let definitions = definitions(&[
        ("corqty.simulated.create-vpc", "mock@1.0.0"),
        ("corqty.simulated.create-rds", "mock@1.0.0"),
        ("corqty.simulated.deploy-service", "mock@1.0.0"),
        ("corqty.mock.normalize-payload", "mock@1.0.0"),
        ("corqty.mock.produce-summary", "mock@1.0.0"),
    ]);
    // The two blueprints of migration 202610250001, reduced to what `plan_run` reads.
    let infrastructure = graph(
        &[
            node("vpc", "corqty.simulated.create-vpc", 1, 900),
            node("rds", "corqty.simulated.create-rds", 1, 900),
            node("deploy", "corqty.simulated.deploy-service", 1, 900),
        ],
        &[
            edge("vpc-rds", "vpc.vpcId", "rds.vpcId"),
            edge("vpc-deploy", "vpc.vpcId", "deploy.vpcId"),
        ],
    );
    assert_eq!(
        plan_run(&infrastructure, &definitions),
        Some(vec![
            planned("vpc", "mock@1.0.0", 1, 900, &["deploy", "rds"], 0),
            planned("rds", "mock@1.0.0", 1, 900, &[], 1),
            planned("deploy", "mock@1.0.0", 1, 900, &[], 1),
        ])
    );

    let data_processing = graph(
        &[
            node("normalize", "corqty.mock.normalize-payload", 1, 900),
            node("summary", "corqty.mock.produce-summary", 1, 900),
        ],
        &[edge(
            "normalize-summary",
            "normalize.normalized",
            "summary.normalized",
        )],
    );
    assert_eq!(
        plan_run(&data_processing, &definitions),
        Some(vec![
            planned("normalize", "mock@1.0.0", 1, 900, &["summary"], 0),
            planned("summary", "mock@1.0.0", 1, 900, &[], 1),
        ])
    );
}

#[test]
fn a_parent_counts_once_however_many_of_its_edges_reach_a_child() {
    let definitions = definitions(&[("acme.split", "mock@1.0.0"), ("acme.merge", "shell@1.0.0")]);
    // The child is listed first; the plan still starts with its parents.
    let graph = graph(
        &[
            node("merge", "acme.merge", 3, 60),
            node("split", "acme.split", 2, 30),
            node("extra", "acme.split", 1, 900),
        ],
        &[
            edge("left", "split.left", "merge.left"),
            edge("right", "split.right", "merge.right"),
            edge("extra", "extra.left", "merge.extra"),
        ],
    );
    assert_eq!(
        plan_run(&graph, &definitions),
        Some(vec![
            planned("split", "mock@1.0.0", 2, 30, &["merge"], 0),
            planned("extra", "mock@1.0.0", 1, 900, &["merge"], 0),
            planned("merge", "shell@1.0.0", 3, 60, &[], 2),
        ])
    );
}

#[test]
fn graphs_that_cannot_publish_have_no_plan() {
    let definitions = definitions(&[("acme.split", "mock@1.0.0")]);
    let cycle = graph(
        &[
            node("a", "acme.split", 1, 900),
            node("b", "acme.split", 1, 900),
        ],
        &[edge("ab", "a.out", "b.in"), edge("ba", "b.out", "a.in")],
    );
    assert_eq!(plan_run(&cycle, &definitions), None);

    let unknown_definition = graph(&[node("a", "acme.unknown", 1, 900)], &[]);
    assert_eq!(plan_run(&unknown_definition, &definitions), None);
}
