use corqty_domain::{
    catalog::{
        FILE_TEXT_MAX_BYTES, MAX_PORTS, NewNodeDefinition, validate_blueprint_metadata,
        validate_config, validate_new_definition,
    },
    diagnostic::{Diagnostic, MAX_DIAGNOSTICS},
    schema::{DIALECT, MAX_SCHEMA_BYTES, compile, instance_errors, schema_errors},
};
use serde_json::{Value, json};

/// An organization definition that passes every rule, with a config field for each widget
/// that has extra rules.
fn definition() -> Value {
    json!({
        "key": "acme.create-vpc",
        "displayName": "Create VPC",
        "category": "Infrastructure",
        "inputs": [{
            "name": "region",
            "dataType": "core.string",
            "schema": {"type": "string", "minLength": 1},
            "required": true
        }],
        "outputs": [{
            "name": "vpcId",
            "dataType": "aws.vpc-id",
            "schema": {"type": "string", "pattern": "^vpc-"}
        }],
        "configSchema": {
            "type": "object",
            "properties": {
                "mode": {"type": "string", "enum": ["fixed", "echo"]},
                "delayMs": {"type": "integer", "minimum": 0},
                "script": {"type": "string"},
                "token": {"type": ["object", "null"]}
            },
            "required": ["mode"],
            "additionalProperties": false
        },
        "uiSchema": {
            "mode": {"widget": "select", "label": "Mode"},
            "delayMs": {"widget": "number"},
            "script": {"widget": "fileText", "help": "Uploaded as text."},
            "token": {"widget": "secretReference"}
        },
        "defaults": {"mode": "fixed", "delayMs": 500, "token": null},
        "executor": {"key": "mock", "version": "1.0.0"}
    })
}

/// `definition()` with `value` set at `pointer` (inserted when the parent is an array).
fn with(pointer: &str, value: Value) -> NewNodeDefinition {
    let mut body = definition();
    let (parent, token) = pointer.rsplit_once('/').unwrap();
    match body.pointer_mut(parent).unwrap() {
        Value::Array(items) => items.insert(token.parse().unwrap(), value),
        object => object[token.replace("~1", "/")] = value,
    }
    serde_json::from_value(body).unwrap()
}

fn text(length: usize) -> Value {
    json!("x".repeat(length))
}

/// A string schema whose JSON has exactly `bytes` bytes.
fn schema_of(bytes: usize) -> Value {
    let padding = bytes
        - json!({"type": "string", "description": ""})
            .to_string()
            .len();
    json!({"type": "string", "description": "x".repeat(padding)})
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<(&str, &str)> {
    diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.pointer.as_str()))
        .collect()
}

#[test]
fn a_well_formed_definition_has_no_diagnostics() {
    let definition: NewNodeDefinition = serde_json::from_value(definition()).unwrap();
    assert_eq!(definition.description, "");
    let diagnostics = validate_new_definition(&definition);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    for (pointer, value) in [
        ("/key", json!(format!("acme.{}", "x".repeat(95)))),
        ("/displayName", text(100)),
        ("/description", text(1000)),
        ("/category", text(60)),
        // Properties may be named like keywords: their values are schemas, not strings.
        ("/configSchema/properties/$ref", json!({"type": "string"})),
        (
            "/configSchema/properties/$schema",
            json!({"type": "string"}),
        ),
        ("/outputs/0/schema", schema_of(MAX_SCHEMA_BYTES)),
        ("/executor/key", json!("shell")),
    ] {
        let diagnostics = validate_new_definition(&with(pointer, value));
        assert!(diagnostics.is_empty(), "{pointer}: {diagnostics:?}");
    }
}

#[test]
fn each_definition_rule_reports_its_code_at_its_pointer() {
    for (pointer, value, code, at) in [
        ("/key", json!("Acme.Thing"), "invalid_key", "/key"),
        ("/key", json!("acme..thing"), "invalid_key", "/key"),
        (
            "/key",
            json!(format!("acme.{}", "x".repeat(96))),
            "invalid_key",
            "/key",
        ),
        ("/key", json!("corqty.custom"), "reserved_key", "/key"),
        (
            "/displayName",
            json!("   "),
            "invalid_display_name",
            "/displayName",
        ),
        (
            "/displayName",
            text(101),
            "invalid_display_name",
            "/displayName",
        ),
        (
            "/description",
            text(1001),
            "invalid_description",
            "/description",
        ),
        ("/category", json!(""), "invalid_category", "/category"),
        ("/category", text(61), "invalid_category", "/category"),
        (
            "/inputs/0/name",
            json!("1region"),
            "invalid_port_name",
            "/inputs/0/name",
        ),
        (
            "/outputs/0/name",
            json!("vpc-id"),
            "invalid_port_name",
            "/outputs/0/name",
        ),
        (
            "/inputs/1",
            json!({"name": "region", "dataType": "core.any", "schema": {}}),
            "duplicate_port",
            "/inputs/1/name",
        ),
        (
            "/outputs/0/dataType",
            json!("VpcId"),
            "invalid_data_type",
            "/outputs/0/dataType",
        ),
        (
            "/inputs/0/schema/type",
            json!("strin"),
            "invalid_schema",
            "/inputs/0/schema/type",
        ),
        (
            "/outputs/0/schema/pattern",
            json!("^(?=vpc-)"),
            "invalid_schema",
            "/outputs/0/schema",
        ),
        (
            "/outputs/0/schema/$ref",
            json!("https://example.com/vpc.json"),
            "invalid_schema",
            "/outputs/0/schema",
        ),
        (
            "/outputs/0/schema/$schema",
            json!("http://json-schema.org/draft-07/schema#"),
            "invalid_schema",
            "/outputs/0/schema",
        ),
        (
            "/inputs/0/schema",
            json!({"$defs": {"region": {"type": "string"}}, "$ref": "#/$defs/region"}),
            "invalid_schema",
            "/inputs/0/schema",
        ),
        (
            "/outputs/0/schema/$dynamicRef",
            json!("#"),
            "invalid_schema",
            "/outputs/0/schema",
        ),
        (
            "/outputs/0/schema/$recursiveRef",
            json!("#"),
            "invalid_schema",
            "/outputs/0/schema",
        ),
        (
            "/configSchema/properties/script",
            json!({
                "$id": "https://example.com/script",
                "$schema": "http://json-schema.org/draft-07/schema#",
                "type": "string"
            }),
            "invalid_schema",
            "/configSchema",
        ),
        (
            "/outputs/0/schema",
            schema_of(MAX_SCHEMA_BYTES + 1),
            "invalid_schema",
            "/outputs/0/schema",
        ),
        (
            "/configSchema/type",
            json!(["object", "null"]),
            "invalid_schema",
            "/configSchema",
        ),
        (
            "/uiSchema/colour",
            json!({"widget": "text"}),
            "unknown_config_field",
            "/uiSchema/colour",
        ),
        (
            "/uiSchema/a~1b",
            json!({"widget": "text"}),
            "unknown_config_field",
            "/uiSchema/a~1b",
        ),
        (
            "/uiSchema/mode/widget",
            json!("dropdown"),
            "unknown_widget",
            "/uiSchema/mode/widget",
        ),
        (
            "/uiSchema/script/widget",
            json!("secretReference"),
            "invalid_ui_schema",
            "/uiSchema/script",
        ),
        (
            "/defaults/delayMs",
            json!(-1),
            "invalid_defaults",
            "/defaults/delayMs",
        ),
        (
            "/executor/key",
            json!("python"),
            "unknown_executor",
            "/executor",
        ),
        (
            "/executor/version",
            json!("2.0.0"),
            "unknown_executor",
            "/executor",
        ),
    ] {
        let diagnostics = validate_new_definition(&with(pointer, value));
        assert_eq!(codes(&diagnostics), [(code, at)], "{pointer}");
    }
}

#[test]
fn each_side_has_at_most_fifty_ports() {
    let ports = |count: usize| -> Value {
        (0..count)
            .map(|index| json!({"name": format!("p{index}"), "dataType": "core.any", "schema": {}}))
            .collect()
    };
    assert!(validate_new_definition(&with("/inputs", ports(MAX_PORTS))).is_empty());
    for side in ["inputs", "outputs"] {
        let pointer = format!("/{side}");
        let diagnostics = validate_new_definition(&with(&pointer, ports(MAX_PORTS + 1)));
        assert_eq!(codes(&diagnostics), [("too_many_ports", pointer.as_str())]);
    }
}

#[test]
fn a_definition_reports_at_most_two_hundred_diagnostics() {
    let fields: serde_json::Map<String, Value> = (0..2 * MAX_DIAGNOSTICS)
        .map(|index| (format!("f{index}"), json!({"widget": "text"})))
        .collect();
    let diagnostics = validate_new_definition(&with("/uiSchema", Value::Object(fields)));
    assert_eq!(diagnostics.len(), MAX_DIAGNOSTICS + 1);
    assert_eq!(
        codes(&diagnostics[MAX_DIAGNOSTICS..]),
        [("too_many_diagnostics", "")]
    );
}

#[test]
fn blueprint_metadata_follows_the_definition_rules() {
    assert!(validate_blueprint_metadata("acme.data-flow", "Data flow", "").is_empty());
    let diagnostics = validate_blueprint_metadata("corqty.flow", " ", &"x".repeat(1001));
    assert_eq!(
        codes(&diagnostics),
        [
            ("reserved_key", "/key"),
            ("invalid_display_name", "/displayName"),
            ("invalid_description", "/description"),
        ]
    );
}

#[test]
fn config_follows_the_schema_and_the_widget_value_contracts() {
    let spec = serde_json::from_value::<NewNodeDefinition>(definition())
        .unwrap()
        .spec;
    let check = |config: Value| validate_config(&spec, &config, "/nodes/0/config");

    let script = "x".repeat(FILE_TEXT_MAX_BYTES);
    let diagnostics = check(json!({"mode": "echo", "script": script, "token": null}));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    let diagnostics =
        check(json!({"mode": "loud", "delayMs": "soon", "token": "hunter2", "extra": 1}));
    let mut found = codes(&diagnostics);
    found.sort_unstable();
    assert_eq!(
        found,
        [
            ("invalid_config", "/nodes/0/config"),
            ("invalid_config", "/nodes/0/config/delayMs"),
            ("invalid_config", "/nodes/0/config/mode"),
            ("invalid_config", "/nodes/0/config/token"),
            ("invalid_secret_reference", "/nodes/0/config/token"),
        ]
    );
    for diagnostic in &diagnostics {
        for submitted in ["loud", "soon", "hunter2"] {
            assert!(!diagnostic.message.contains(submitted), "{diagnostic:?}");
        }
    }

    let script = "x".repeat(FILE_TEXT_MAX_BYTES + 1);
    assert_eq!(
        codes(&check(json!({"mode": "fixed", "script": script}))),
        [("file_text_too_large", "/nodes/0/config/script")]
    );
    let secret = json!({"kind": "secret", "secretId": "00000000-0000-0000-0000-000000000007"});
    assert_eq!(
        codes(&check(json!({"mode": "fixed", "token": secret}))),
        [("secret_not_found", "/nodes/0/config/token/secretId")]
    );
    let malformed = json!({"kind": "secret", "secretId": "not-a-uuid"});
    assert_eq!(
        codes(&check(json!({"mode": "fixed", "token": malformed}))),
        [("invalid_secret_reference", "/nodes/0/config/token")]
    );
}

#[test]
fn schemas_compile_offline_as_draft_2020_12_only() {
    assert!(compile(&json!({"$schema": DIALECT, "type": "string"})).is_ok());
    // Even a local reference can make validation exponential, so none is accepted.
    let local_ref =
        json!({"$schema": DIALECT, "$defs": {"id": {"type": "string"}}, "$ref": "#/$defs/id"});
    assert_eq!(
        compile(&local_ref).err().as_deref(),
        Some("Schema references are not supported.")
    );
    assert!(compile(&json!({"$schema": "http://json-schema.org/draft-07/schema#"})).is_err());
    let nested_draft_07 = json!({"properties": {"a": {
        "$id": "https://example.com/a",
        "$schema": "http://json-schema.org/draft-07/schema#"
    }}});
    assert_eq!(
        compile(&nested_draft_07).err().as_deref(),
        Some("Only JSON Schema Draft 2020-12 is supported.")
    );
    assert!(compile(&json!({"$ref": "file:///etc/passwd"})).is_err());

    assert_eq!(
        schema_errors(&json!(5)),
        [(
            String::new(),
            r#"5 is not of types "boolean", "object""#.to_owned()
        )]
    );
    let validator = compile(&json!({"type": "integer"})).unwrap();
    assert_eq!(
        instance_errors(&validator, &json!("hunter2")),
        [(
            String::new(),
            r#"value is not of type "integer""#.to_owned()
        )]
    );
}
