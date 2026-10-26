use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::{
    catalog::{NodeDefinitionSpec, PortSpec},
    graph::{GraphNode, GraphSnapshot, InputBinding},
    schema::{compile, instance_errors},
};

// ponytail: fixed limits until Milestone 5 makes them deployment configuration.
/// The largest run input, as serialized JSON.
pub const MAX_RUN_INPUT_BYTES: usize = 1_048_576;
/// The largest inputs object one node gets, as serialized JSON.
pub const MAX_NODE_INPUT_BYTES: usize = 1_048_576;
/// The largest output object one node may return, as serialized JSON.
pub const MAX_NODE_OUTPUT_BYTES: usize = 1_048_576;
/// The most log text kept for one node run; one `node.log_truncated` event marks the cut.
pub const MAX_LOG_BYTES_PER_NODE_RUN: usize = 10_485_760;

/// Why a node run fails around its executor. Every code is in `run::NON_RETRYABLE`, and the
/// message never repeats a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeFailure {
    pub code: &'static str,
    pub message: String,
}

impl NodeFailure {
    fn new(code: &'static str, message: String) -> Self {
        Self { code, message }
    }
}

/// The inputs object `node`'s executor gets. Each input port of `spec` takes the output its
/// edge reads from `upstream_outputs`, else its binding: a literal, or a JSON Pointer into
/// `run_input`. A port without a value is left out when optional; null is a value. Every
/// value must match its port's schema as it is, without coercion.
pub fn resolve_inputs(
    node: &GraphNode,
    spec: &NodeDefinitionSpec,
    graph: &GraphSnapshot,
    run_input: &Value,
    upstream_outputs: &HashMap<String, Map<String, Value>>,
) -> Result<Map<String, Value>, NodeFailure> {
    let mut inputs = Map::new();
    for port in &spec.inputs {
        let edge = graph
            .edges
            .iter()
            .find(|edge| edge.target.node_id == node.id && edge.target.port == port.name);
        let value = match (edge, node.inputs.get(&port.name)) {
            (Some(edge), _) => upstream_outputs
                .get(&edge.source.node_id)
                .and_then(|output| output.get(&edge.source.port)),
            (None, Some(InputBinding::Literal { value })) => Some(value),
            (None, Some(InputBinding::WorkflowInput { pointer })) => run_input.pointer(pointer),
            // ponytail: no secrets until M4, and publish rejects secret bindings until then.
            (None, Some(InputBinding::Secret { .. })) => {
                return Err(NodeFailure::new(
                    "secret_unavailable",
                    format!(
                        "The input \"{}\" needs a secret, which workers cannot read yet.",
                        port.name
                    ),
                ));
            }
            (None, None) => None,
        };
        match value {
            Some(value) => {
                inputs.insert(port.name.clone(), value.clone());
            }
            None if port.required => {
                return Err(NodeFailure::new(
                    "input_missing",
                    format!("The required input \"{}\" has no value.", port.name),
                ));
            }
            None => {}
        }
    }
    // Before validation, so validating costs at most one limit's worth of JSON.
    if json_bytes(&inputs) > MAX_NODE_INPUT_BYTES {
        return Err(NodeFailure::new(
            "input_too_large",
            format!("The inputs have more than {MAX_NODE_INPUT_BYTES} bytes of JSON."),
        ));
    }
    let problem = spec
        .inputs
        .iter()
        .find_map(|port| schema_problem("input", port, inputs.get(&port.name)?));
    match problem {
        Some(problem) => Err(NodeFailure::new("input_invalid", problem)),
        None => Ok(inputs),
    }
}

/// Checks what an executor returned: at most `MAX_NODE_OUTPUT_BYTES` of JSON, only declared
/// output ports, no U+0000 anywhere (jsonb cannot store it), and each value matching its
/// port's schema. Declared outputs may be missing.
pub fn validate_output(
    spec: &NodeDefinitionSpec,
    output: &Map<String, Value>,
) -> Result<(), NodeFailure> {
    if json_bytes(output) > MAX_NODE_OUTPUT_BYTES {
        return Err(NodeFailure::new(
            "output_too_large",
            format!("The output has more than {MAX_NODE_OUTPUT_BYTES} bytes of JSON."),
        ));
    }
    if let Some(name) = output
        .keys()
        .find(|name| !spec.outputs.iter().any(|port| &port.name == *name))
    {
        return Err(NodeFailure::new(
            "output_invalid",
            format!(
                "The node definition has no output \"{}\".",
                name.replace('\0', "\u{fffd}")
            ),
        ));
    }
    // Declared names never hold U+0000; nested keys and strings might.
    if output.values().any(has_nul) {
        return Err(NodeFailure::new(
            "output_invalid",
            "The output contains the character U+0000.".to_owned(),
        ));
    }
    let problem = spec
        .outputs
        .iter()
        .find_map(|port| schema_problem("output", port, output.get(&port.name)?));
    match problem {
        Some(problem) => Err(NodeFailure::new("output_invalid", problem)),
        None => Ok(()),
    }
}

/// The first way `value` breaks `port`'s schema, located by a pointer that starts at the
/// port's name (port names need no escaping). The message masks the value.
fn schema_problem(side: &str, port: &PortSpec, value: &Value) -> Option<String> {
    let Ok(validator) = compile(&port.schema) else {
        return Some(format!(
            "The schema of the {side} /{} does not compile.",
            port.name
        ));
    };
    let (path, message) = instance_errors(&validator, value).into_iter().next()?;
    Some(format!(
        "The {side} at /{}{path} does not match its schema: {message}.",
        port.name
    ))
}

fn json_bytes(object: &Map<String, Value>) -> usize {
    serde_json::to_vec(object)
        .expect("a JSON object always serializes")
        .len()
}

fn has_nul(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains('\0'),
        Value::Array(items) => items.iter().any(has_nul),
        Value::Object(object) => object
            .iter()
            .any(|(key, value)| key.contains('\0') || has_nul(value)),
        _ => false,
    }
}
