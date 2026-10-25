use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    diagnostic::{Diagnostic, pointer_token},
    graph::{InputBinding, is_valid_data_type},
    schema::{compile, compile_at, instance_errors},
};

/// The form widgets a `uiSchema` field can name. The web app renders each one.
pub const WIDGETS: [&str; 10] = [
    "text",
    "textarea",
    "number",
    "boolean",
    "select",
    "multiSelect",
    "secretReference",
    "code",
    "keyValue",
    "fileText",
];
// ponytail: a static compiled registry; Milestone 3 adds executors that workers advertise.
pub const BUILTIN_EXECUTORS: &[(&str, &str)] = &[("mock", "1.0.0")];
pub const FILE_TEXT_MAX_BYTES: usize = 262_144;

/// What a node definition version does: its typed ports, its configuration form and the
/// executor that runs it. Stored as the `spec` column.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeDefinitionSpec {
    pub inputs: Vec<PortSpec>,
    pub outputs: Vec<PortSpec>,
    pub config_schema: Value,
    pub ui_schema: BTreeMap<String, UiField>,
    pub defaults: Value,
    pub executor: ExecutorRef,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortSpec {
    pub name: String,
    pub data_type: String,
    pub schema: Value,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiField {
    pub widget: String,
    pub label: Option<String>,
    pub help: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutorRef {
    pub key: String,
    pub version: String,
}

/// The body of `POST /node-definitions`. serde cannot deny unknown fields next to a
/// flattened one, so unknown top-level fields are ignored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewNodeDefinition {
    pub key: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    pub category: String,
    #[serde(flatten)]
    pub spec: NodeDefinitionSpec,
}

/// Every problem in a new organization definition, with pointers into the request body.
#[must_use]
pub fn validate_new_definition(definition: &NewNodeDefinition) -> Vec<Diagnostic> {
    let mut diagnostics = validate_blueprint_metadata(
        &definition.key,
        &definition.display_name,
        &definition.description,
    );
    if !fits(&definition.category, 60) {
        diagnostics.push(Diagnostic::new(
            "invalid_category",
            "/category",
            "A category must have 1 to 60 characters.",
        ));
    }
    diagnostics.extend(validate_spec(&definition.spec));
    diagnostics
}

/// The key, display name and description rules that organization definitions and blueprints share.
#[must_use]
pub fn validate_blueprint_metadata(
    key: &str,
    display_name: &str,
    description: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    // `^[a-z0-9]+([.-][a-z0-9]+)*$`, at most 100 characters.
    let valid_key = key.len() <= 100
        && key.split(['.', '-']).all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        });
    if !valid_key {
        diagnostics.push(Diagnostic::new(
            "invalid_key",
            "/key",
            "A key is lowercase letters and digits joined by single dots or hyphens, at most 100 characters.",
        ));
    }
    if key.starts_with("corqty.") {
        diagnostics.push(Diagnostic::new(
            "reserved_key",
            "/key",
            "Keys starting with \"corqty.\" belong to the system catalog.",
        ));
    }
    if !fits(display_name, 100) {
        diagnostics.push(Diagnostic::new(
            "invalid_display_name",
            "/displayName",
            "A display name must have 1 to 100 characters.",
        ));
    }
    if description.chars().count() > 1000 {
        diagnostics.push(Diagnostic::new(
            "invalid_description",
            "/description",
            "A description can have at most 1000 characters.",
        ));
    }
    diagnostics
}

/// Every problem in a definition's ports, schemas, form and executor. Pointers are relative
/// to the definition, whose top level holds the spec fields.
#[must_use]
pub fn validate_spec(spec: &NodeDefinitionSpec) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (side, ports) in [("inputs", &spec.inputs), ("outputs", &spec.outputs)] {
        let mut names = HashSet::new();
        for (index, port) in ports.iter().enumerate() {
            let pointer = format!("/{side}/{index}");
            let valid_name = port
                .name
                .starts_with(|character: char| character.is_ascii_alphabetic())
                && port
                    .name
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_');
            if !valid_name {
                diagnostics.push(Diagnostic::new(
                    "invalid_port_name",
                    format!("{pointer}/name"),
                    "A port name starts with a letter and has only letters, digits and underscores.",
                ));
            }
            if !names.insert(port.name.as_str()) {
                diagnostics.push(Diagnostic::new(
                    "duplicate_port",
                    format!("{pointer}/name"),
                    format!("Port names must be unique among the {side}."),
                ));
            }
            if !is_valid_data_type(&port.data_type) {
                diagnostics.push(Diagnostic::new(
                    "invalid_data_type",
                    format!("{pointer}/dataType"),
                    "A data type is a dotted lowercase name such as core.string.",
                ));
            }
            if let Err(errors) = compile_at(&port.schema, &format!("{pointer}/schema")) {
                diagnostics.extend(errors);
            }
        }
    }

    match compile_at(&spec.config_schema, "/configSchema") {
        Ok(validator) => {
            for (path, message) in instance_errors(&validator, &spec.defaults) {
                let pointer = format!("/defaults{path}");
                diagnostics.push(Diagnostic::new("invalid_defaults", pointer, message));
            }
        }
        Err(errors) => diagnostics.extend(errors),
    }
    if spec.config_schema.get("type") != Some(&json!("object")) {
        diagnostics.push(Diagnostic::new(
            "invalid_schema",
            "/configSchema",
            "The config schema must have \"type\": \"object\".",
        ));
    }

    let properties = spec.config_schema.get("properties");
    for (field, ui) in &spec.ui_schema {
        let pointer = format!("/uiSchema/{}", pointer_token(field));
        if !WIDGETS.contains(&ui.widget.as_str()) {
            diagnostics.push(Diagnostic::new(
                "unknown_widget",
                format!("{pointer}/widget"),
                format!("The widget must be one of: {}.", WIDGETS.join(", ")),
            ));
        }
        let property = properties.and_then(|properties| properties.get(field));
        let kind = property.and_then(|property| property.get("type"));
        if property.is_none() {
            diagnostics.push(Diagnostic::new(
                "unknown_config_field",
                pointer,
                "Every uiSchema field must be a property of the config schema.",
            ));
        } else if ui.widget == "secretReference"
            && kind != Some(&json!("object"))
            && kind != Some(&json!(["object", "null"]))
        {
            diagnostics.push(Diagnostic::new(
                "invalid_ui_schema",
                pointer,
                "A secretReference field must have type \"object\" or [\"object\", \"null\"].",
            ));
        }
    }

    let executor = (spec.executor.key.as_str(), spec.executor.version.as_str());
    if !BUILTIN_EXECUTORS.contains(&executor) {
        diagnostics.push(Diagnostic::new(
            "unknown_executor",
            "/executor",
            "The executor must be mock 1.0.0.",
        ));
    }
    diagnostics
}

/// Every problem in a node's `config`, with pointers under `pointer_prefix`.
#[must_use]
pub fn validate_config(
    spec: &NodeDefinitionSpec,
    config: &Value,
    pointer_prefix: &str,
) -> Vec<Diagnostic> {
    let Ok(validator) = compile(&spec.config_schema) else {
        return vec![Diagnostic::new(
            "invalid_config",
            pointer_prefix,
            "The node definition's config schema does not compile.",
        )];
    };
    let mut diagnostics: Vec<Diagnostic> = instance_errors(&validator, config)
        .into_iter()
        .map(|(path, message)| {
            Diagnostic::new("invalid_config", format!("{pointer_prefix}{path}"), message)
        })
        .collect();
    for (field, ui) in &spec.ui_schema {
        let Some(value) = config.get(field) else {
            continue;
        };
        let pointer = format!("{pointer_prefix}/{}", pointer_token(field));
        match ui.widget.as_str() {
            "fileText"
                if value
                    .as_str()
                    .is_some_and(|text| text.len() > FILE_TEXT_MAX_BYTES) =>
            {
                diagnostics.push(Diagnostic::new(
                    "file_text_too_large",
                    pointer,
                    format!("A file can have at most {FILE_TEXT_MAX_BYTES} bytes."),
                ));
            }
            "secretReference" if !value.is_null() => {
                diagnostics.push(match InputBinding::deserialize(value) {
                    // ponytail: no secrets until M4; replace with an org-scoped lookup then.
                    Ok(InputBinding::Secret { .. }) => Diagnostic::new(
                        "secret_not_found",
                        format!("{pointer}/secretId"),
                        "The secret does not exist.",
                    ),
                    _ => Diagnostic::new(
                        "invalid_secret_reference",
                        pointer,
                        "A secret reference is {\"kind\": \"secret\", \"secretId\": \"<uuid>\"}.",
                    ),
                });
            }
            _ => {}
        }
    }
    diagnostics
}

/// Not blank and at most `max` characters, so the value always fits its database column.
pub(crate) fn fits(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.chars().count() <= max
}
