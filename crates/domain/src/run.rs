use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::{
    graph::{GraphNode, GraphSnapshot, topological_order},
    publish::Definitions,
};

/// Error codes that are never retried, whatever the executor says: a bad input or output, a
/// missing secret or executor, or a denied permission fails the same way on every attempt.
pub const NON_RETRYABLE: &[&str] = &[
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
];

/// Where one node of one run stands. Stored as the `node_runs.state` text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    Blocked,
    Queued,
    Running,
    RetryWait,
    Succeeded,
    Failed,
    Canceled,
    Skipped,
}

/// Where a whole run stands. Stored as the `runs.status` text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
}

impl NodeState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::RetryWait => "retry_wait",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
            Self::Skipped => "skipped",
        }
    }

    /// Whether a node may move from `self` to `to`: spec §11.1, plus `retry_wait` to
    /// `canceled` (a cancel stops a pending retry) and to `running` (a due retry is claimed
    /// directly). The SQL functions perform only these transitions.
    #[must_use]
    pub const fn can_transition(self, to: Self) -> bool {
        matches!(
            (self, to),
            (Self::Blocked, Self::Queued | Self::Canceled | Self::Skipped)
                | (Self::Queued, Self::Running | Self::Canceled)
                | (
                    Self::Running,
                    Self::Succeeded | Self::Failed | Self::RetryWait | Self::Canceled
                )
                | (
                    Self::RetryWait,
                    Self::Queued | Self::Running | Self::Canceled
                )
        )
    }

    /// Whether the node is done: no transition leaves these states.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Canceled | Self::Skipped
        )
    }
}

impl RunStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }
}

/// The status a run ends with, or `None` while any node can still change. A failure wins over
/// a cancel request; skipped nodes only sit below a failed or canceled node, so they decide
/// nothing. `corqty.settle_run` applies the same rule in the database.
#[must_use]
pub fn aggregate(states: &[NodeState], cancel_requested: bool) -> Option<RunStatus> {
    if !states.iter().all(|state| state.is_terminal()) {
        return None;
    }
    Some(if states.contains(&NodeState::Failed) {
        RunStatus::Failed
    } else if cancel_requested || states.contains(&NodeState::Canceled) {
        RunStatus::Canceled
    } else {
        RunStatus::Succeeded
    })
}

/// Whether a failed attempt may run again: the executor decides, except for the codes in
/// `NON_RETRYABLE`.
#[must_use]
pub fn is_retryable(code: &str, executor_says: bool) -> bool {
    executor_says && !NON_RETRYABLE.contains(&code)
}

/// The `node_runs` row a node starts a run with. It serializes with the column names, so a
/// whole plan can be inserted from one JSON array.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlannedNode {
    pub node_id: String,
    /// The definition's executor as `"<key>@<version>"`, the form workers advertise.
    pub executor: String,
    pub max_attempts: i32,
    pub timeout_seconds: i32,
    /// The distinct nodes this node's edges feed, sorted.
    pub children: Vec<String>,
    /// The number of distinct nodes that feed this one. Each decrements it once when it
    /// succeeds, so two edges from one parent must count once. Roots start at 0, queued.
    pub pending_parents: i32,
}

/// The nodes a run of `graph` starts with, in `topological_order`. `None` when the graph has a
/// cycle, an edge to a node it lacks or a node whose definition is unknown, which publish
/// validation rules out.
#[must_use]
pub fn plan_run(graph: &GraphSnapshot, definitions: &Definitions) -> Option<Vec<PlannedNode>> {
    let links: Vec<(&str, &str)> = graph
        .edges
        .iter()
        .map(|edge| (edge.source.node_id.as_str(), edge.target.node_id.as_str()))
        .collect();
    let node_ids: Vec<&str> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
    let order = topological_order(&node_ids, &links)?;

    let nodes: HashMap<&str, &GraphNode> = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut children: HashMap<&str, Vec<String>> = HashMap::new();
    let mut pending_parents: HashMap<&str, i32> = HashMap::new();
    // Sorted and distinct, so each child is listed once, in order, and counts each parent once.
    for (source, target) in links.into_iter().collect::<BTreeSet<_>>() {
        children.entry(source).or_default().push(target.to_owned());
        *pending_parents.entry(target).or_default() += 1;
    }
    order
        .into_iter()
        .map(|id| {
            let node = nodes.get(id.as_str())?;
            let reference = (node.definition.key.clone(), node.definition.version);
            let executor = &definitions.get(&reference)?.executor;
            Some(PlannedNode {
                executor: format!("{}@{}", executor.key, executor.version),
                max_attempts: i32::try_from(node.retry.max_attempts).ok()?,
                timeout_seconds: i32::try_from(node.timeout_seconds).ok()?,
                children: children.remove(id.as_str()).unwrap_or_default(),
                pending_parents: pending_parents.get(id.as_str()).copied().unwrap_or(0),
                node_id: id,
            })
        })
        .collect()
}
