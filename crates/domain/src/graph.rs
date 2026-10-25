use std::collections::{BTreeMap, HashMap, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::diagnostic::Diagnostic;

pub const GRAPH_SCHEMA_VERSION: u32 = 1;
// ponytail: fixed limits in M2; deployment configuration arrives in Milestone 5.
pub const MAX_NODES: usize = 500;
pub const MAX_EDGES: usize = 1000;
pub const DEFAULT_TIMEOUT_SECONDS: u32 = 900;
pub const MAX_TIMEOUT_SECONDS: u32 = 14_400;
pub const MAX_ATTEMPTS: u32 = 5;

/// A workflow graph as drafts, blueprints and published versions store it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphSnapshot {
    pub schema_version: u32,
    pub workflow_input_schema: Value,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub viewport: Viewport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphNode {
    pub id: String,
    pub definition: DefinitionRef,
    pub display_name: String,
    pub position: Position,
    pub config: Value,
    pub inputs: BTreeMap<String, InputBinding>,
    pub retry: RetryPolicy,
    pub timeout_seconds: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefinitionRef {
    pub key: String,
    pub version: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Viewport {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetryPolicy {
    /// `u32` rather than a bounded type so out-of-range values reach the validator.
    pub max_attempts: u32,
}

/// Where a node input gets its value when no edge feeds it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum InputBinding {
    Literal { value: Value },
    WorkflowInput { pointer: String },
    Secret { secret_id: Uuid },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphEdge {
    pub id: String,
    pub source: PortRef,
    pub target: PortRef,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortRef {
    pub node_id: String,
    pub port: String,
}

impl GraphSnapshot {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            schema_version: GRAPH_SCHEMA_VERSION,
            workflow_input_schema: json!({"type": "object", "properties": {}}),
            nodes: Vec::new(),
            edges: Vec::new(),
            viewport: Viewport {
                x: 0.0,
                y: 0.0,
                zoom: 1.0,
            },
        }
    }
}

/// Port data types are dotted lowercase names such as `core.string` or `aws.vpc-id`
/// (`^[a-z][a-z0-9-]*(\.[a-z][a-z0-9-]*)+$`).
#[must_use]
pub fn is_valid_data_type(data_type: &str) -> bool {
    data_type.contains('.')
        && data_type.split('.').all(|segment| {
            segment.starts_with(|character: char| character.is_ascii_lowercase())
                && segment.chars().all(|character| {
                    character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
                })
        })
}

/// Whether an output of type `source` may feed an input of type `target`.
#[must_use]
pub fn compatible(source: &str, target: &str) -> bool {
    source == target
        || (source == "core.integer" && target == "core.number")
        || target == "core.any"
}

/// Whether the `(source, target)` node id pairs contain a cycle, self edges included.
#[must_use]
pub fn has_cycle(edges: &[(&str, &str)]) -> bool {
    topological_order(&[], edges).is_none()
}

/// Kahn's algorithm over `node_ids` and every edge endpoint. Ready nodes are taken in the
/// order they were first listed (`node_ids` first, then edge endpoints). `None` on a cycle.
#[must_use]
pub fn topological_order(node_ids: &[&str], edges: &[(&str, &str)]) -> Option<Vec<String>> {
    let mut nodes = Vec::new();
    let mut incoming: HashMap<&str, usize> = HashMap::new();
    let endpoints = edges.iter().flat_map(|&(source, target)| [source, target]);
    for id in node_ids.iter().copied().chain(endpoints) {
        if incoming.insert(id, 0).is_none() {
            nodes.push(id);
        }
    }
    let mut outgoing: HashMap<&str, Vec<&str>> = HashMap::new();
    for &(source, target) in edges {
        outgoing.entry(source).or_default().push(target);
        *incoming.entry(target).or_default() += 1;
    }

    let mut ready: VecDeque<&str> = nodes
        .iter()
        .copied()
        .filter(|id| incoming[id] == 0)
        .collect();
    let mut order = Vec::with_capacity(nodes.len());
    while let Some(id) = ready.pop_front() {
        order.push(id.to_owned());
        for &target in outgoing.get(id).into_iter().flatten() {
            let count = incoming.entry(target).or_default();
            *count -= 1;
            if *count == 0 {
                ready.push_back(target);
            }
        }
    }
    (order.len() == nodes.len()).then_some(order)
}

/// The size limits every stored graph must respect, checked before anything else.
#[must_use]
pub fn check_limits(graph: &GraphSnapshot) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if graph.nodes.len() > MAX_NODES {
        diagnostics.push(Diagnostic::new(
            "too_many_nodes",
            "/nodes",
            format!("A graph can have at most {MAX_NODES} nodes."),
        ));
    }
    if graph.edges.len() > MAX_EDGES {
        diagnostics.push(Diagnostic::new(
            "too_many_edges",
            "/edges",
            format!("A graph can have at most {MAX_EDGES} edges."),
        ));
    }
    diagnostics
}
