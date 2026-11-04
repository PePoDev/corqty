use corqty_domain::{
    diagnostic::Diagnostic,
    graph::{
        DefinitionRef, GraphEdge, GraphSnapshot, InputBinding, MAX_EDGES, MAX_NODES, PortRef,
        Position, Viewport, check_limits, compatible, has_cycle, is_valid_data_type,
        topological_order,
    },
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

/// Shared with the web app's tests, so both sides agree on these rules.
const RULES: &str = include_str!("fixtures/graph-rules.json");

/// A draft graph as the web app sends it.
const WIRE_EXAMPLE: &str = r#"{"schemaVersion":1,"workflowInputSchema":{"type":"object","properties":{"region":{"type":"string"}},"required":["region"]},
 "nodes":[{"id":"vpc","definition":{"key":"corqty.simulated.create-vpc","version":1},"displayName":"Create VPC (simulated)",
   "position":{"x":0,"y":0},"config":{"mode":"fixed","output":{"vpcId":"vpc-0simulated"},"delayMs":500,"failureMessage":null},
   "inputs":{"region":{"kind":"workflowInput","pointer":"/region"}},"retry":{"maxAttempts":1},"timeoutSeconds":900}],
 "edges":[{"id":"vpc-rds","source":{"nodeId":"vpc","port":"vpcId"},"target":{"nodeId":"rds","port":"vpcId"}}],
 "viewport":{"x":0,"y":0,"zoom":1}}"#;

#[derive(Deserialize)]
struct Rules {
    compatibility: Vec<CompatibilityCase>,
    cycles: Vec<CycleCase>,
}

#[derive(Deserialize)]
struct CompatibilityCase {
    source: String,
    target: String,
    compatible: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CycleCase {
    name: String,
    edges: Vec<(String, String)>,
    has_cycle: bool,
}

fn rules() -> Rules {
    serde_json::from_str(RULES).unwrap()
}

#[test]
fn compatibility_follows_the_shared_fixture() {
    for case in rules().compatibility {
        assert!(is_valid_data_type(&case.source), "{}", case.source);
        assert!(is_valid_data_type(&case.target), "{}", case.target);
        assert_eq!(
            compatible(&case.source, &case.target),
            case.compatible,
            "{} -> {}",
            case.source,
            case.target
        );
    }
}

#[test]
fn cycle_detection_follows_the_shared_fixture() {
    for case in rules().cycles {
        let edges: Vec<(&str, &str)> = case
            .edges
            .iter()
            .map(|(source, target)| (source.as_str(), target.as_str()))
            .collect();
        assert_eq!(has_cycle(&edges), case.has_cycle, "{}", case.name);
    }
}

#[test]
fn data_types_are_dotted_lowercase_names() {
    for valid in ["core.string", "aws.vpc-id", "acme.billing.invoice-2"] {
        assert!(is_valid_data_type(valid), "{valid}");
    }
    for invalid in [
        "string",
        "Core.string",
        "core.",
        ".core",
        "core..any",
        "core.1st",
        "core.any!",
    ] {
        assert!(!is_valid_data_type(invalid), "{invalid}");
    }
}

#[test]
fn topological_order_lists_sources_first_and_refuses_cycles() {
    let diamond = [("a", "b"), ("a", "c"), ("b", "d"), ("c", "d")];
    assert_eq!(
        topological_order(&["d", "c", "b", "a", "e"], &diamond),
        Some(["a", "e", "b", "c", "d"].map(str::to_owned).to_vec())
    );
    assert_eq!(
        topological_order(&["a", "b"], &[("a", "b"), ("b", "a")]),
        None
    );
}

#[test]
fn graph_snapshot_round_trips_the_wire_example() {
    let wire: serde_json::Value = serde_json::from_str(WIRE_EXAMPLE).unwrap();
    let graph: GraphSnapshot = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        graph.nodes[0].definition,
        DefinitionRef {
            key: "corqty.simulated.create-vpc".to_owned(),
            version: 1,
        }
    );
    assert_eq!(
        graph.nodes[0].inputs["region"],
        InputBinding::WorkflowInput {
            pointer: "/region".to_owned(),
        }
    );
    assert_eq!(
        graph.edges,
        [GraphEdge {
            id: "vpc-rds".to_owned(),
            source: PortRef {
                node_id: "vpc".to_owned(),
                port: "vpcId".to_owned(),
            },
            target: PortRef {
                node_id: "rds".to_owned(),
                port: "vpcId".to_owned(),
            },
        }]
    );
    assert_eq!(graph.nodes[0].position, Position { x: 0.0, y: 0.0 });
    assert_eq!(
        graph.viewport,
        Viewport {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
        }
    );

    // Floats serialize as 0.0 rather than 0, so those two fields were compared above.
    let mut written = serde_json::to_value(&graph).unwrap();
    let mut expected = wire.clone();
    for json in [&mut written, &mut expected] {
        json["nodes"][0]["position"].take();
        json["viewport"].take();
    }
    assert_eq!(written, expected);

    let mut unknown_field = wire;
    unknown_field["nodes"][0]["color"] = json!("red");
    assert!(serde_json::from_value::<GraphSnapshot>(unknown_field).is_err());
}

#[test]
fn secret_bindings_serialize_their_id_as_secret_id() {
    let binding = InputBinding::Secret {
        secret_id: Uuid::from_u128(7),
    };
    let wire = json!({"kind": "secret", "secretId": "00000000-0000-0000-0000-000000000007"});
    assert_eq!(serde_json::to_value(&binding).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<InputBinding>(wire).unwrap(),
        binding
    );
    assert!(
        serde_json::from_value::<InputBinding>(json!({
            "kind": "secret",
            "secret_id": "00000000-0000-0000-0000-000000000007",
        }))
        .is_err()
    );
}

#[test]
fn empty_graph_has_an_object_input_schema_and_a_unit_viewport() {
    assert_eq!(
        serde_json::to_value(GraphSnapshot::empty()).unwrap(),
        json!({
            "schemaVersion": 1,
            "workflowInputSchema": {"type": "object", "properties": {}},
            "nodes": [],
            "edges": [],
            "viewport": {"x": 0.0, "y": 0.0, "zoom": 1.0},
        })
    );
}

#[test]
fn limits_allow_500_nodes_and_1000_edges() {
    let example: GraphSnapshot = serde_json::from_str(WIRE_EXAMPLE).unwrap();
    let mut graph = GraphSnapshot::empty();
    graph.nodes = vec![example.nodes[0].clone(); MAX_NODES];
    graph.edges = vec![example.edges[0].clone(); MAX_EDGES];
    assert!(check_limits(&graph).is_empty());

    graph.nodes.push(example.nodes[0].clone());
    graph.edges.push(example.edges[0].clone());
    assert_eq!(
        check_limits(&graph),
        [
            Diagnostic::new(
                "too_many_nodes",
                "/nodes",
                "A graph can have at most 500 nodes."
            ),
            Diagnostic::new(
                "too_many_edges",
                "/edges",
                "A graph can have at most 1000 edges."
            ),
        ]
    );
}
