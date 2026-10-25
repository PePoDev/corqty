use std::collections::{HashMap, HashSet};

use jsonschema::Validator;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    catalog::{BUILTIN_EXECUTORS, NodeDefinitionSpec, PortSpec, fits, validate_compiled_config},
    diagnostic::{Diagnostic, MAX_DIAGNOSTICS, capped, pointer_token},
    graph::{
        GRAPH_SCHEMA_VERSION, GraphSnapshot, InputBinding, MAX_ATTEMPTS, MAX_TIMEOUT_SECONDS,
        check_limits, compatible, has_cycle,
    },
    schema::{compile, compile_at, instance_errors},
};

/// The node definitions a graph may use, by (key, version): the system catalog plus the
/// organization's own.
pub type Definitions = HashMap<(String, i32), NodeDefinitionSpec>;

/// Every reason `graph` cannot be published, sorted by (pointer, code) and capped at
/// `MAX_DIAGNOSTICS`. Empty means publishable. An unknown definition or port skips the rules
/// that depend on it, so nothing cascades.
#[must_use]
pub fn validate_graph(graph: &GraphSnapshot, definitions: &Definitions) -> Vec<Diagnostic> {
    let mut diagnostics = check_limits(graph);
    if !diagnostics.is_empty() {
        // Nothing else is checked, so the work below stays bounded by the limits.
        return diagnostics;
    }
    if graph.schema_version != GRAPH_SCHEMA_VERSION {
        diagnostics.push(Diagnostic::new(
            "unsupported_schema_version",
            "/schemaVersion",
            format!("The graph schema version must be {GRAPH_SCHEMA_VERSION}."),
        ));
    }
    if let Err(errors) = compile_at(&graph.workflow_input_schema, "/workflowInputSchema") {
        diagnostics.extend(errors);
    }
    if graph.workflow_input_schema.get("type") != Some(&json!("object")) {
        diagnostics.push(Diagnostic::new(
            "invalid_schema",
            "/workflowInputSchema",
            "The workflow input schema must have \"type\": \"object\".",
        ));
    }

    // Each node id's definition, None when it is unknown. The first node with an id wins.
    let mut specs: HashMap<&str, Option<&NodeDefinitionSpec>> = HashMap::new();
    for node in &graph.nodes {
        let reference = (node.definition.key.clone(), node.definition.version);
        specs
            .entry(node.id.as_str())
            .or_insert_with(|| definitions.get(&reference));
    }

    let mut edge_ids = HashSet::new();
    // (node id, input port) pairs that an edge feeds.
    let mut fed = HashSet::new();
    // Edges between two different known nodes, for the cycle check.
    let (mut links, mut link_edges) = (Vec::new(), Vec::new());
    for (index, edge) in graph.edges.iter().enumerate() {
        if diagnostics.len() > MAX_DIAGNOSTICS {
            break;
        }
        let pointer = format!("/edges/{index}");
        if !edge_ids.insert(edge.id.as_str()) {
            diagnostics.push(Diagnostic::new(
                "duplicate_edge_id",
                format!("{pointer}/id"),
                "Edge ids must be unique.",
            ));
        }
        let source = specs.get(edge.source.node_id.as_str()).copied();
        let target = specs.get(edge.target.node_id.as_str()).copied();
        for (end, known) in [("source", source.is_some()), ("target", target.is_some())] {
            if !known {
                diagnostics.push(Diagnostic::new(
                    "unknown_node",
                    format!("{pointer}/{end}"),
                    "The edge must connect nodes of this graph.",
                ));
            }
        }
        if source.is_some() && target.is_some() {
            if edge.source.node_id == edge.target.node_id {
                diagnostics.push(Diagnostic::new(
                    "self_edge",
                    pointer.clone(),
                    "An edge cannot connect a node to itself.",
                ));
            } else {
                links.push((edge.source.node_id.as_str(), edge.target.node_id.as_str()));
                link_edges.push(index);
            }
        }

        let output = source.flatten().and_then(|spec| {
            let pointer = format!("{pointer}/source");
            find_port(
                &spec.outputs,
                "output",
                &edge.source.port,
                pointer,
                &mut diagnostics,
            )
        });
        let input = target.flatten().and_then(|spec| {
            let pointer = format!("{pointer}/target");
            find_port(
                &spec.inputs,
                "input",
                &edge.target.port,
                pointer,
                &mut diagnostics,
            )
        });
        if let Some(input) = input
            && !fed.insert((edge.target.node_id.as_str(), input.name.as_str()))
        {
            diagnostics.push(Diagnostic::new(
                "multiply_bound_input",
                pointer.clone(),
                "Another edge already feeds this input.",
            ));
        }
        if let (Some(output), Some(input)) = (output, input)
            && !compatible(&output.data_type, &input.data_type)
        {
            diagnostics.push(Diagnostic::new(
                "incompatible_ports",
                pointer,
                format!("{} cannot feed {}.", output.data_type, input.data_type),
            ));
        }
    }
    if has_cycle(&links) {
        // The first edge that closes a cycle ends the shortest cyclic prefix of `links`:
        // links[..low] has no cycle and links[..high] has one.
        let (mut low, mut high) = (0, links.len());
        while high - low > 1 {
            let middle = (low + high) / 2;
            if has_cycle(&links[..middle]) {
                high = middle;
            } else {
                low = middle;
            }
        }
        diagnostics.push(Diagnostic::new(
            "cycle",
            format!("/edges/{}", link_edges[high - 1]),
            "This edge closes a cycle.",
        ));
    }

    // Each definition's config schema, and each port schema a literal needs, compiles at most
    // once, however many nodes use it.
    let mut config_validators: HashMap<(&str, i32), Option<Validator>> = HashMap::new();
    let mut port_validators: HashMap<(&str, i32, &str), Option<Validator>> = HashMap::new();
    let mut node_ids = HashSet::new();
    for (index, node) in graph.nodes.iter().enumerate() {
        if diagnostics.len() > MAX_DIAGNOSTICS {
            break;
        }
        let pointer = format!("/nodes/{index}");
        if !node_ids.insert(node.id.as_str()) {
            diagnostics.push(Diagnostic::new(
                "duplicate_node_id",
                format!("{pointer}/id"),
                "Node ids must be unique.",
            ));
        }
        if !fits(&node.display_name, 100) {
            diagnostics.push(Diagnostic::new(
                "invalid_display_name",
                format!("{pointer}/displayName"),
                "A display name must have 1 to 100 characters.",
            ));
        }
        if !(1..=MAX_ATTEMPTS).contains(&node.retry.max_attempts) {
            diagnostics.push(Diagnostic::new(
                "invalid_retry",
                format!("{pointer}/retry/maxAttempts"),
                format!("A node can run 1 to {MAX_ATTEMPTS} times."),
            ));
        }
        if !(1..=MAX_TIMEOUT_SECONDS).contains(&node.timeout_seconds) {
            diagnostics.push(Diagnostic::new(
                "invalid_timeout",
                format!("{pointer}/timeoutSeconds"),
                format!("A timeout must be 1 to {MAX_TIMEOUT_SECONDS} seconds."),
            ));
        }
        let reference = (node.definition.key.clone(), node.definition.version);
        let Some(spec) = definitions.get(&reference) else {
            diagnostics.push(Diagnostic::new(
                "unknown_definition",
                format!("{pointer}/definition"),
                "The node definition does not exist.",
            ));
            continue;
        };
        let executor = (spec.executor.key.as_str(), spec.executor.version.as_str());
        if !BUILTIN_EXECUTORS.contains(&executor) {
            diagnostics.push(Diagnostic::new(
                "unknown_executor",
                format!("{pointer}/definition"),
                "The node definition's executor is not available.",
            ));
        }
        let (key, version) = (node.definition.key.as_str(), node.definition.version);
        let validator = config_validators
            .entry((key, version))
            .or_insert_with(|| compile(&spec.config_schema).ok());
        diagnostics.extend(validate_compiled_config(
            spec,
            validator.as_ref(),
            &node.config,
            &format!("{pointer}/config"),
        ));

        for (name, binding) in &node.inputs {
            if diagnostics.len() > MAX_DIAGNOSTICS {
                break;
            }
            let at = format!("{pointer}/inputs/{}", pointer_token(name));
            let Some(port) = find_port(&spec.inputs, "input", name, at.clone(), &mut diagnostics)
            else {
                continue;
            };
            if fed.contains(&(node.id.as_str(), name.as_str())) {
                diagnostics.push(Diagnostic::new(
                    "multiply_bound_input",
                    at.clone(),
                    "An edge already feeds this input.",
                ));
            }
            match binding {
                InputBinding::Literal { value } => match port_validators
                    .entry((key, version, port.name.as_str()))
                    .or_insert_with(|| compile(&port.schema).ok())
                {
                    Some(validator) => {
                        for (path, message) in instance_errors(validator, value) {
                            let pointer = format!("{at}/value{path}");
                            diagnostics.push(Diagnostic::new("invalid_literal", pointer, message));
                        }
                    }
                    None => diagnostics.push(Diagnostic::new(
                        "invalid_literal",
                        format!("{at}/value"),
                        "The input port's schema does not compile.",
                    )),
                },
                InputBinding::WorkflowInput { pointer } => {
                    if !resolves(&graph.workflow_input_schema, pointer) {
                        diagnostics.push(Diagnostic::new(
                            "invalid_pointer",
                            format!("{at}/pointer"),
                            "The pointer must name a property of the workflow input schema.",
                        ));
                    }
                }
                // ponytail: no secrets until M4; replace with an org-scoped lookup then.
                InputBinding::Secret { .. } => diagnostics.push(Diagnostic::new(
                    "secret_not_found",
                    format!("{at}/secretId"),
                    "The secret does not exist.",
                )),
            }
        }
        for port in spec.inputs.iter().filter(|port| port.required) {
            let bound = node.inputs.contains_key(&port.name)
                || fed.contains(&(node.id.as_str(), port.name.as_str()));
            if !bound {
                diagnostics.push(Diagnostic::new(
                    "missing_required_input",
                    format!("{pointer}/inputs/{}", pointer_token(&port.name)),
                    "This input is required: connect an edge or add a binding.",
                ));
            }
        }
    }

    diagnostics.sort_by(|a, b| (&a.pointer, &a.code).cmp(&(&b.pointer, &b.code)));
    capped(diagnostics)
}

/// The lowercase hex SHA-256 of the graph as JSON with sorted object keys. The same graph
/// always gets the same checksum, whatever key order it was submitted with.
#[must_use]
pub fn canonical_checksum(graph: &GraphSnapshot) -> String {
    // Going through `Value` sorts every object's keys; serializing the struct would not.
    let value = serde_json::to_value(graph).expect("a graph always serializes");
    format!("{:x}", Sha256::digest(value.to_string()))
}

/// The port called `name` among `ports`, or an `unknown_port` diagnostic at `pointer`.
fn find_port<'a>(
    ports: &'a [PortSpec],
    side: &str,
    name: &str,
    pointer: String,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<&'a PortSpec> {
    let port = ports.iter().find(|port| port.name == name);
    if port.is_none() {
        diagnostics.push(Diagnostic::new(
            "unknown_port",
            pointer,
            format!("The node's definition has no {side} port with this name."),
        ));
    }
    port
}

/// Whether `pointer` is an RFC 6901 pointer whose tokens walk `properties` down `schema`
/// (`/a/b` names `properties.a.properties.b`).
fn resolves(schema: &Value, pointer: &str) -> bool {
    let Some(tokens) = pointer.strip_prefix('/') else {
        return pointer.is_empty();
    };
    tokens
        .split('/')
        .try_fold(schema, |schema, token| {
            // `~` only starts the escapes `~0` and `~1`.
            if token
                .split('~')
                .skip(1)
                .any(|rest| !rest.starts_with(['0', '1']))
            {
                return None;
            }
            let token = token.replace("~1", "/").replace("~0", "~");
            schema.get("properties")?.get(token.as_str())
        })
        .is_some()
}
