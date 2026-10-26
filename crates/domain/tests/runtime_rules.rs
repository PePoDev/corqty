use std::{collections::HashMap, fmt::Debug};

use corqty_domain::{
    catalog::NodeDefinitionSpec,
    graph::GraphSnapshot,
    run::NON_RETRYABLE,
    runtime::{
        MAX_LOG_BYTES_PER_NODE_RUN, MAX_NODE_INPUT_BYTES, MAX_NODE_OUTPUT_BYTES,
        MAX_RUN_INPUT_BYTES, NodeFailure, resolve_inputs, validate_output,
    },
};
use serde_json::{Map, Value, json};

/// A database that needs a VPC id (from an edge), a region, a size and a nullable label, and
/// takes optional notes (from an edge) and an optional zone. It reports an endpoint, a port
/// and free-form details.
fn spec() -> NodeDefinitionSpec {
    serde_json::from_value(json!({
        "inputs": [
            {
                "name": "vpcId",
                "dataType": "aws.vpc-id",
                "schema": {"type": "string", "pattern": "^vpc-"},
                "required": true
            },
            {
                "name": "region",
                "dataType": "core.string",
                "schema": {"type": "string", "minLength": 1},
                "required": true
            },
            {"name": "size", "dataType": "core.number", "schema": {"type": "number"}, "required": true},
            {
                "name": "label",
                "dataType": "core.string",
                "schema": {"type": ["string", "null"]},
                "required": true
            },
            {"name": "notes", "dataType": "core.any", "schema": {}},
            {"name": "zone", "dataType": "core.string", "schema": {"type": "string"}}
        ],
        "outputs": [
            {"name": "endpoint", "dataType": "core.string", "schema": {"type": "string"}},
            {"name": "port", "dataType": "core.integer", "schema": {"type": "integer", "minimum": 1}},
            {"name": "details", "dataType": "core.object", "schema": {"type": "object"}}
        ],
        "configSchema": {"type": "object"},
        "uiSchema": {},
        "defaults": {},
        "executor": {"key": "mock", "version": "1.0.0"}
    }))
    .unwrap()
}

/// The bindings of a node that resolves: a nested workflow input, a number literal and a
/// `null` literal for the nullable label.
fn bindings() -> Value {
    json!({
        "region": {"kind": "workflowInput", "pointer": "/network/region"},
        "size": {"kind": "literal", "value": 20},
        "label": {"kind": "literal", "value": null}
    })
}

/// Resolves the inputs of node `db`, whose `vpcId` and `notes` are fed by edges from the
/// `vpcId` and `tags` outputs of node `vpc`. `vpc_output` is `None` when `vpc` has no output.
fn resolve(
    bindings: Value,
    run_input: Value,
    vpc_output: Option<Value>,
) -> Result<Map<String, Value>, NodeFailure> {
    let node = |id: &str, key: &str, inputs: Value| {
        json!({
            "id": id,
            "definition": {"key": key, "version": 1},
            "displayName": id,
            "position": {"x": 0, "y": 0},
            "config": {},
            "inputs": inputs,
            "retry": {"maxAttempts": 1},
            "timeoutSeconds": 900
        })
    };
    let edge = |id: &str, output: &str, input: &str| {
        json!({
            "id": id,
            "source": {"nodeId": "vpc", "port": output},
            "target": {"nodeId": "db", "port": input}
        })
    };
    let graph: GraphSnapshot = serde_json::from_value(json!({
        "schemaVersion": 1,
        "workflowInputSchema": {"type": "object"},
        "nodes": [node("vpc", "acme.vpc", json!({})), node("db", "acme.db", bindings)],
        "edges": [edge("vpc-id", "vpcId", "vpcId"), edge("vpc-tags", "tags", "notes")],
        "viewport": {"x": 0, "y": 0, "zoom": 1}
    }))
    .unwrap();
    let upstream_outputs: HashMap<String, Map<String, Value>> = vpc_output
        .map(|output| ("vpc".to_owned(), serde_json::from_value(output).unwrap()))
        .into_iter()
        .collect();
    resolve_inputs(
        &graph.nodes[1],
        &spec(),
        &graph,
        &run_input,
        &upstream_outputs,
    )
}

fn object(value: Value) -> Map<String, Value> {
    serde_json::from_value(value).unwrap()
}

/// The failure in `result`, whose code is never retried.
fn failure<T: Debug>(result: Result<T, NodeFailure>) -> NodeFailure {
    let failure = result.unwrap_err();
    assert!(NON_RETRYABLE.contains(&failure.code), "{failure:?}");
    failure
}

#[test]
fn edges_literals_and_workflow_input_pointers_resolve_to_their_values() {
    let mut bindings = bindings();
    // An RFC 6901 escape: `~1` stands for `/`.
    bindings["zone"] = json!({"kind": "workflowInput", "pointer": "/placement~1zone"});
    let run_input = json!({"network": {"region": "eu-west-1"}, "placement/zone": "eu-west-1a"});
    let vpc_output = json!({"vpcId": "vpc-0a1", "tags": {"team": "data"}, "unused": 1});
    assert_eq!(
        resolve(bindings, run_input, Some(vpc_output)).unwrap(),
        object(json!({
            "vpcId": "vpc-0a1",
            "region": "eu-west-1",
            "size": 20,
            "label": null,
            "notes": {"team": "data"},
            "zone": "eu-west-1a"
        }))
    );
}

#[test]
fn optional_inputs_without_a_value_are_omitted_not_null() {
    let run_input = json!({"network": {"region": "eu-west-1"}});
    // `zone` is unbound and the edge into `notes` finds no `tags` output.
    let inputs = resolve(
        bindings(),
        run_input.clone(),
        Some(json!({"vpcId": "vpc-0a1"})),
    )
    .unwrap();
    assert_eq!(inputs.get("notes"), None);
    assert_eq!(inputs.get("zone"), None);

    // A pointer to a property the run input lacks.
    let mut bindings = bindings();
    bindings["zone"] = json!({"kind": "workflowInput", "pointer": "/zone"});
    let inputs = resolve(bindings, run_input, Some(json!({"vpcId": "vpc-0a1"}))).unwrap();
    assert_eq!(inputs.get("zone"), None);
    assert_eq!(inputs.len(), 4);
}

#[test]
fn null_passes_only_where_the_port_schema_allows_it() {
    let vpc_output = || Some(json!({"vpcId": "vpc-0a1"}));
    let run_input = json!({"network": {"region": "eu-west-1"}});
    let inputs = resolve(bindings(), run_input.clone(), vpc_output()).unwrap();
    assert_eq!(inputs.get("label"), Some(&Value::Null));

    let mut bindings_with_null_size = bindings();
    bindings_with_null_size["size"]["value"] = Value::Null;
    let failed = failure(resolve(bindings_with_null_size, run_input, vpc_output()));
    assert_eq!(failed.code, "input_invalid");

    // A null in the run input is a value, not a missing one.
    let null_region = json!({"network": {"region": null}});
    let failed = failure(resolve(bindings(), null_region, vpc_output()));
    assert_eq!(failed.code, "input_invalid");
}

#[test]
fn required_inputs_without_a_value_fail_as_input_missing() {
    let run_input = json!({"network": {"region": "eu-west-1"}});
    let vpc_output = || Some(json!({"vpcId": "vpc-0a1"}));
    let mut unbound_size = bindings();
    unbound_size.as_object_mut().unwrap().remove("size");
    for (bindings, run_input, vpc_output, port) in [
        // The pointer finds nothing in the run input.
        (bindings(), json!({"network": {}}), vpc_output(), "region"),
        // The upstream output lacks the port the edge reads.
        (bindings(), run_input.clone(), Some(json!({})), "vpcId"),
        // The upstream node has no output at all.
        (bindings(), run_input.clone(), None, "vpcId"),
        // Nothing binds the port.
        (unbound_size, run_input, vpc_output(), "size"),
    ] {
        let failed = failure(resolve(bindings, run_input, vpc_output));
        assert_eq!(failed.code, "input_missing", "{port}");
        assert!(failed.message.contains(port), "{failed:?}");
    }
}

#[test]
fn values_that_break_the_port_schema_fail_as_input_invalid() {
    let run_input = json!({"network": {"region": "eu-west-1"}});
    // An upstream value is checked against the target port's full schema.
    let failed = failure(resolve(
        bindings(),
        run_input.clone(),
        Some(json!({"vpcId": "subnet-0a1"})),
    ));
    assert_eq!(failed.code, "input_invalid");
    assert!(failed.message.contains("/vpcId"), "{failed:?}");
    // The message never repeats the value.
    assert!(!failed.message.contains("subnet-0a1"), "{failed:?}");

    // No coercion: the string "20" is not a number.
    let mut string_size = bindings();
    string_size["size"]["value"] = json!("20");
    let failed = failure(resolve(
        string_size,
        run_input,
        Some(json!({"vpcId": "vpc-0a1"})),
    ));
    assert_eq!(failed.code, "input_invalid");
    assert!(failed.message.contains("/size"), "{failed:?}");
}

#[test]
fn resolved_inputs_over_one_mebibyte_fail_as_input_too_large() {
    let run_input = json!({"network": {"region": "eu-west-1"}});
    let with_notes = |notes: String| {
        resolve(
            bindings(),
            run_input.clone(),
            Some(json!({"vpcId": "vpc-0a1", "tags": notes})),
        )
    };
    let empty = serde_json::to_string(&with_notes(String::new()).unwrap()).unwrap();
    let fits = "a".repeat(MAX_NODE_INPUT_BYTES - empty.len());
    let inputs = with_notes(fits).unwrap();
    assert_eq!(
        serde_json::to_string(&inputs).unwrap().len(),
        MAX_NODE_INPUT_BYTES
    );

    let too_long = "a".repeat(MAX_NODE_INPUT_BYTES - empty.len() + 1);
    assert_eq!(failure(with_notes(too_long)).code, "input_too_large");
}

#[test]
fn secret_bindings_fail_as_secret_unavailable() {
    let mut bindings = bindings();
    bindings["zone"] =
        json!({"kind": "secret", "secretId": "0198c2a0-0000-7000-8000-000000000001"});
    let run_input = json!({"network": {"region": "eu-west-1"}});
    let failed = failure(resolve(
        bindings,
        run_input,
        Some(json!({"vpcId": "vpc-0a1"})),
    ));
    assert_eq!(failed.code, "secret_unavailable");
    assert!(failed.message.contains("zone"), "{failed:?}");
}

#[test]
fn declared_outputs_that_match_their_schemas_pass() {
    for output in [
        json!({"endpoint": "db.internal", "port": 5432, "details": {"engine": "postgres"}}),
        // Outputs are not required: a missing one only fails a required input it feeds.
        json!({"endpoint": "db.internal"}),
        json!({}),
    ] {
        assert_eq!(
            validate_output(&spec(), &object(output.clone())),
            Ok(()),
            "{output}"
        );
    }
}

#[test]
fn undeclared_keys_and_schema_violations_fail_as_output_invalid() {
    for (output, mention) in [
        (json!({"endpoint": "db.internal", "extra": 1}), "extra"),
        (json!({"port": 0}), "/port"),
        // No coercion: the string "5432" is not an integer.
        (json!({"port": "5432"}), "/port"),
    ] {
        let failed = failure(validate_output(&spec(), &object(output)));
        assert_eq!(failed.code, "output_invalid", "{mention}");
        assert!(failed.message.contains(mention), "{failed:?}");
    }
}

#[test]
fn strings_with_nul_characters_fail_as_output_invalid() {
    // PostgreSQL cannot store U+0000 in jsonb, in a value or in a key, at any depth.
    for output in [
        json!({"endpoint": "db\u{0}internal"}),
        json!({"details": {"tags": ["ok", "\u{0}"]}}),
        json!({"details": {"key\u{0}": 1}}),
    ] {
        let failed = failure(validate_output(&spec(), &object(output.clone())));
        assert_eq!(failed.code, "output_invalid", "{output}");
        assert!(failed.message.contains("U+0000"), "{failed:?}");
    }
}

#[test]
fn outputs_over_one_mebibyte_fail_as_output_too_large() {
    let output = |endpoint: String| object(json!({"endpoint": endpoint}));
    let empty = serde_json::to_string(&output(String::new())).unwrap();
    let fits = output("a".repeat(MAX_NODE_OUTPUT_BYTES - empty.len()));
    assert_eq!(
        serde_json::to_string(&fits).unwrap().len(),
        MAX_NODE_OUTPUT_BYTES
    );
    assert_eq!(validate_output(&spec(), &fits), Ok(()));

    let too_large = output("a".repeat(MAX_NODE_OUTPUT_BYTES - empty.len() + 1));
    assert_eq!(
        failure(validate_output(&spec(), &too_large)).code,
        "output_too_large"
    );
}

#[test]
fn limits_match_the_spec() {
    // Spec §15: 1 MiB run input, resolved node input and node output; 10 MiB of logs per node run.
    assert_eq!(MAX_RUN_INPUT_BYTES, 1_048_576);
    assert_eq!(MAX_NODE_INPUT_BYTES, 1_048_576);
    assert_eq!(MAX_NODE_OUTPUT_BYTES, 1_048_576);
    assert_eq!(MAX_LOG_BYTES_PER_NODE_RUN, 10_485_760);
}
