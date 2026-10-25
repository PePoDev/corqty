use corqty_domain::{
    catalog::FILE_TEXT_MAX_BYTES,
    diagnostic::Diagnostic,
    graph::GraphSnapshot,
    publish::{Definitions, canonical_checksum, validate_graph},
};
use serde_json::{Value, json};

/// A VPC that also reports its subnet count, and a database that needs the VPC id, a size
/// and a nullable label.
fn definitions() -> Definitions {
    let vpc = json!({
        "inputs": [
            {
                "name": "region",
                "dataType": "core.string",
                "schema": {"type": "string", "minLength": 1},
                "required": true
            },
            {"name": "after", "dataType": "core.any", "schema": {}}
        ],
        "outputs": [
            {"name": "vpcId", "dataType": "aws.vpc-id", "schema": {"type": "string", "pattern": "^vpc-"}},
            {"name": "subnets", "dataType": "core.integer", "schema": {"type": "integer"}}
        ],
        "configSchema": {
            "type": "object",
            "properties": {"script": {"type": "string"}, "token": {"type": ["object", "null"]}},
            "additionalProperties": false
        },
        "uiSchema": {"script": {"widget": "fileText"}, "token": {"widget": "secretReference"}},
        "defaults": {},
        "executor": {"key": "mock", "version": "1.0.0"}
    });
    let database = json!({
        "inputs": [
            {
                "name": "vpcId",
                "dataType": "aws.vpc-id",
                "schema": {"type": "string", "pattern": "^vpc-"},
                "required": true
            },
            {"name": "size", "dataType": "core.number", "schema": {"type": "number"}, "required": true},
            {
                "name": "label",
                "dataType": "core.string",
                "schema": {"type": ["string", "null"]},
                "required": true
            },
            {"name": "notes", "dataType": "core.any", "schema": {}}
        ],
        "outputs": [{"name": "endpoint", "dataType": "core.string", "schema": {"type": "string"}}],
        "configSchema": {"type": "object"},
        "uiSchema": {},
        "defaults": {},
        "executor": {"key": "mock", "version": "1.0.0"}
    });
    Definitions::from([
        (
            ("acme.vpc".to_owned(), 1),
            serde_json::from_value(vpc).unwrap(),
        ),
        (
            ("acme.db".to_owned(), 1),
            serde_json::from_value(database).unwrap(),
        ),
    ])
}

/// A publishable graph. It reads a nested workflow input, feeds an integer output into a
/// number input, binds a nullable input to `null` and leaves the optional `notes` unbound.
fn graph() -> Value {
    json!({
        "schemaVersion": 1,
        "workflowInputSchema": {
            "type": "object",
            "properties": {"network": {"type": "object", "properties": {"region": {"type": "string"}}}}
        },
        "nodes": [
            {
                "id": "vpc",
                "definition": {"key": "acme.vpc", "version": 1},
                "displayName": "VPC",
                "position": {"x": 0, "y": 0},
                "config": {"script": "echo", "token": null},
                "inputs": {"region": {"kind": "workflowInput", "pointer": "/network/region"}},
                "retry": {"maxAttempts": 1},
                "timeoutSeconds": 900
            },
            {
                "id": "db",
                "definition": {"key": "acme.db", "version": 1},
                "displayName": "Database",
                "position": {"x": 300, "y": 0},
                "config": {},
                "inputs": {"label": {"kind": "literal", "value": null}},
                "retry": {"maxAttempts": 5},
                "timeoutSeconds": 14400
            }
        ],
        "edges": [edge("vpc-db", "vpc.vpcId", "db.vpcId"), edge("subnets-size", "vpc.subnets", "db.size")],
        "viewport": {"x": 0, "y": 0, "zoom": 1}
    })
}

/// A valid VPC node that takes its region from a literal.
fn node(id: &str) -> Value {
    json!({
        "id": id,
        "definition": {"key": "acme.vpc", "version": 1},
        "displayName": "VPC",
        "position": {"x": 0, "y": 0},
        "config": {},
        "inputs": {"region": {"kind": "literal", "value": "eu-west-1"}},
        "retry": {"maxAttempts": 1},
        "timeoutSeconds": 900
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

/// `graph()` with each value set at its pointer (inserted when the parent is an array).
fn with(changes: &[(&str, Value)]) -> GraphSnapshot {
    let mut body = graph();
    for (pointer, value) in changes {
        let (parent, token) = pointer.rsplit_once('/').unwrap();
        match body.pointer_mut(parent).unwrap() {
            Value::Array(items) => items.insert(token.parse().unwrap(), value.clone()),
            object => object[token.replace("~1", "/")] = value.clone(),
        }
    }
    serde_json::from_value(body).unwrap()
}

fn text(length: usize) -> Value {
    json!("x".repeat(length))
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<(&str, &str)> {
    diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.pointer.as_str()))
        .collect()
}

#[test]
fn a_publishable_graph_has_no_diagnostics() {
    let diagnostics = validate_graph(&with(&[]), &definitions());
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    for (pointer, value) in [
        ("/nodes/0/displayName", text(100)),
        ("/nodes/0/timeoutSeconds", json!(1)),
        (
            "/nodes/1/inputs/notes",
            json!({"kind": "literal", "value": null}),
        ),
    ] {
        let diagnostics = validate_graph(&with(&[(pointer, value)]), &definitions());
        assert!(diagnostics.is_empty(), "{pointer}: {diagnostics:?}");
    }
}

#[test]
fn each_publish_rule_reports_its_code_at_its_pointer() {
    let secret = json!({"kind": "secret", "secretId": "00000000-0000-0000-0000-000000000007"});
    for (pointer, value, code, at) in [
        (
            "/schemaVersion",
            json!(2),
            "unsupported_schema_version",
            "/schemaVersion",
        ),
        (
            "/workflowInputSchema/type",
            json!("array"),
            "invalid_schema",
            "/workflowInputSchema",
        ),
        (
            "/workflowInputSchema/properties/network/type",
            json!("objekt"),
            "invalid_schema",
            "/workflowInputSchema/properties/network/type",
        ),
        ("/nodes/2", node("vpc"), "duplicate_node_id", "/nodes/2/id"),
        (
            "/edges/2",
            edge("vpc-db", "vpc.vpcId", "db.notes"),
            "duplicate_edge_id",
            "/edges/2/id",
        ),
        (
            "/nodes/0/displayName",
            json!(" "),
            "invalid_display_name",
            "/nodes/0/displayName",
        ),
        (
            "/nodes/0/displayName",
            text(101),
            "invalid_display_name",
            "/nodes/0/displayName",
        ),
        (
            "/nodes/1/definition/version",
            json!(2),
            "unknown_definition",
            "/nodes/1/definition",
        ),
        (
            "/edges/0/source/nodeId",
            json!("nat"),
            "unknown_node",
            "/edges/0/source",
        ),
        (
            "/edges/2",
            edge("gateway", "vpc.vpcId", "gateway.vpcId"),
            "unknown_node",
            "/edges/2/target",
        ),
        (
            "/edges/0/source/port",
            json!("region"),
            "unknown_port",
            "/edges/0/source",
        ),
        (
            "/edges/2",
            edge("endpoint", "vpc.vpcId", "db.endpoint"),
            "unknown_port",
            "/edges/2/target",
        ),
        (
            "/nodes/1/inputs/a~1b",
            json!({"kind": "literal", "value": 1}),
            "unknown_port",
            "/nodes/1/inputs/a~1b",
        ),
        (
            "/edges/2",
            edge("loop", "vpc.vpcId", "vpc.after"),
            "self_edge",
            "/edges/2",
        ),
        (
            "/edges/2",
            edge("back", "db.endpoint", "vpc.after"),
            "cycle",
            "/edges/2",
        ),
        (
            "/edges/1/source/port",
            json!("vpcId"),
            "incompatible_ports",
            "/edges/1",
        ),
        (
            "/edges/2",
            edge("again", "vpc.vpcId", "db.vpcId"),
            "multiply_bound_input",
            "/edges/2",
        ),
        (
            "/nodes/1/inputs/vpcId",
            json!({"kind": "literal", "value": "vpc-1"}),
            "multiply_bound_input",
            "/nodes/1/inputs/vpcId",
        ),
        (
            "/nodes/0/inputs",
            json!({}),
            "missing_required_input",
            "/nodes/0/inputs/region",
        ),
        (
            "/nodes/0/inputs/region",
            json!({"kind": "literal", "value": null}),
            "invalid_literal",
            "/nodes/0/inputs/region/value",
        ),
        (
            "/nodes/1/inputs/label/value",
            json!(5),
            "invalid_literal",
            "/nodes/1/inputs/label/value",
        ),
        (
            "/nodes/0/inputs/region/pointer",
            json!("/network/zone"),
            "invalid_pointer",
            "/nodes/0/inputs/region/pointer",
        ),
        (
            "/nodes/0/inputs/region/pointer",
            json!("/region"),
            "invalid_pointer",
            "/nodes/0/inputs/region/pointer",
        ),
        (
            "/nodes/0/inputs/region/pointer",
            json!("network/region"),
            "invalid_pointer",
            "/nodes/0/inputs/region/pointer",
        ),
        (
            "/nodes/0/inputs/region/pointer",
            json!("/network/~2region"),
            "invalid_pointer",
            "/nodes/0/inputs/region/pointer",
        ),
        (
            "/nodes/0/config/extra",
            json!(1),
            "invalid_config",
            "/nodes/0/config",
        ),
        (
            "/nodes/0/config/script",
            text(FILE_TEXT_MAX_BYTES + 1),
            "file_text_too_large",
            "/nodes/0/config/script",
        ),
        (
            "/nodes/0/config/token",
            json!({"kind": "literal", "value": "hunter2"}),
            "invalid_secret_reference",
            "/nodes/0/config/token",
        ),
        (
            "/nodes/0/config/token",
            secret.clone(),
            "secret_not_found",
            "/nodes/0/config/token/secretId",
        ),
        (
            "/nodes/0/inputs/region",
            secret,
            "secret_not_found",
            "/nodes/0/inputs/region/secretId",
        ),
        (
            "/nodes/0/retry/maxAttempts",
            json!(0),
            "invalid_retry",
            "/nodes/0/retry/maxAttempts",
        ),
        (
            "/nodes/0/retry/maxAttempts",
            json!(6),
            "invalid_retry",
            "/nodes/0/retry/maxAttempts",
        ),
        (
            "/nodes/0/timeoutSeconds",
            json!(0),
            "invalid_timeout",
            "/nodes/0/timeoutSeconds",
        ),
        (
            "/nodes/0/timeoutSeconds",
            json!(14_401),
            "invalid_timeout",
            "/nodes/0/timeoutSeconds",
        ),
    ] {
        let diagnostics = validate_graph(&with(&[(pointer, value)]), &definitions());
        assert_eq!(codes(&diagnostics), [(code, at)], "{pointer}");
    }
}

#[test]
fn an_executor_that_is_not_built_in_is_reported_on_the_node() {
    let mut definitions = definitions();
    let database = definitions.get_mut(&("acme.db".to_owned(), 1)).unwrap();
    database.executor.version = "2.0.0".to_owned();
    assert_eq!(
        codes(&validate_graph(&with(&[]), &definitions)),
        [("unknown_executor", "/nodes/1/definition")]
    );
}

#[test]
fn a_graph_over_the_limits_gets_only_the_limit_diagnostics() {
    let nodes: Vec<Value> = (0..501).map(|index| node(&format!("n{index}"))).collect();
    let graph = with(&[("/nodes", json!(nodes)), ("/schemaVersion", json!(2))]);
    assert_eq!(
        codes(&validate_graph(&graph, &definitions())),
        [("too_many_nodes", "/nodes")]
    );

    let edges: Vec<Value> = (0..1001)
        .map(|index| edge(&format!("e{index}"), "vpc.vpcId", "db.notes"))
        .collect();
    let graph = with(&[("/edges", json!(edges))]);
    assert_eq!(
        codes(&validate_graph(&graph, &definitions())),
        [("too_many_edges", "/edges")]
    );
}

#[test]
fn only_the_first_edge_that_closes_a_cycle_is_reported() {
    // Edge 3 lies on the cycle too, but edge 2 already closed it.
    let graph = with(&[
        ("/edges/2", edge("back", "db.endpoint", "vpc.after")),
        ("/edges/3", edge("notes", "vpc.vpcId", "db.notes")),
    ]);
    assert_eq!(
        codes(&validate_graph(&graph, &definitions())),
        [("cycle", "/edges/2")]
    );
}

#[test]
fn diagnostics_are_sorted_by_pointer_then_code() {
    let graph = with(&[
        ("/schemaVersion", json!(2)),
        ("/nodes/0/retry/maxAttempts", json!(0)),
        ("/edges/2", edge("loop", "vpc.subnets", "vpc.region")),
    ]);
    assert_eq!(
        codes(&validate_graph(&graph, &definitions())),
        [
            ("incompatible_ports", "/edges/2"),
            ("self_edge", "/edges/2"),
            ("multiply_bound_input", "/nodes/0/inputs/region"),
            ("invalid_retry", "/nodes/0/retry/maxAttempts"),
            ("unsupported_schema_version", "/schemaVersion"),
        ]
    );
}

#[test]
fn checksums_ignore_key_order_but_cover_the_viewport() {
    let ordered: GraphSnapshot = serde_json::from_str(
        r#"{"schemaVersion":1,"workflowInputSchema":{"type":"object","properties":{"a":{"type":"string"},"b":{}}},
 "nodes":[{"id":"n","definition":{"key":"acme.vpc","version":1},"displayName":"VPC","position":{"x":1.5,"y":-2},
   "config":{"script":"echo","token":null},
   "inputs":{"after":{"kind":"literal","value":{"z":1,"a":2}},"region":{"kind":"workflowInput","pointer":"/a"}},
   "retry":{"maxAttempts":1},"timeoutSeconds":900}],
 "edges":[],"viewport":{"x":0,"y":0,"zoom":1}}"#,
    )
    .unwrap();
    let reordered: GraphSnapshot = serde_json::from_str(
        r#"{"viewport":{"zoom":1,"y":0,"x":0},"edges":[],
 "nodes":[{"timeoutSeconds":900,"retry":{"maxAttempts":1},
   "inputs":{"region":{"pointer":"/a","kind":"workflowInput"},"after":{"value":{"a":2,"z":1},"kind":"literal"}},
   "config":{"token":null,"script":"echo"},
   "position":{"y":-2,"x":1.5},"displayName":"VPC","definition":{"version":1,"key":"acme.vpc"},"id":"n"}],
 "workflowInputSchema":{"properties":{"b":{},"a":{"type":"string"}},"type":"object"},"schemaVersion":1}"#,
    )
    .unwrap();

    let checksum = canonical_checksum(&ordered);
    assert_eq!(canonical_checksum(&reordered), checksum);
    assert_eq!(checksum.len(), 64);
    assert!(
        checksum
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    );

    let mut moved = ordered;
    moved.viewport.zoom = 1.25;
    assert_ne!(canonical_checksum(&moved), checksum);
}

#[test]
fn the_empty_graph_has_a_pinned_checksum() {
    // SHA-256 of {"edges":[],"nodes":[],"schemaVersion":1,"viewport":{"x":0.0,"y":0.0,"zoom":1.0},
    // "workflowInputSchema":{"properties":{},"type":"object"}} without the line break.
    assert_eq!(
        canonical_checksum(&GraphSnapshot::empty()),
        "0a4920bf2f3421535f9d6cadfab4be1b98d494db660d00049d0ac36224d67406"
    );
}

#[test]
fn serde_json_serializes_object_keys_in_sorted_order() {
    // The checksum relies on this: serde_json's `preserve_order` feature must stay off.
    assert_eq!(
        serde_json::to_string(&json!({"z": 1, "a": 2})).unwrap(),
        r#"{"a":2,"z":1}"#
    );
}
