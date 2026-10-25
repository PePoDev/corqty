use jsonschema::{PatternOptions, Validator};
use serde_json::Value;

use crate::diagnostic::Diagnostic;

/// The only JSON Schema dialect Corqty accepts.
pub const DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

/// Compiles a user-supplied schema as Draft 2020-12. References to other documents are
/// never fetched, and patterns use the linear-time `regex` engine.
pub fn compile(schema: &Value) -> Result<Validator, String> {
    match schema.get("$schema") {
        None => {}
        Some(dialect) if dialect == DIALECT => {}
        Some(_) => return Err(format!("\"$schema\" must be absent or \"{DIALECT}\".")),
    }
    jsonschema::draft202012::options()
        .offline()
        .with_pattern_options(PatternOptions::regex())
        .build(schema)
        .map_err(|error| error.to_string())
}

/// Every Draft 2020-12 meta-schema error in `schema`, as (pointer into the schema, message).
#[must_use]
pub fn schema_errors(schema: &Value) -> Vec<(String, String)> {
    let mut errors: Vec<(String, String)> = jsonschema::draft202012::meta::validator()
        .iter_errors(schema)
        .map(|error| (error.instance_path().to_string(), error.to_string()))
        .collect();
    // A schema that is neither an object nor a boolean fails every branch the same way.
    errors.dedup();
    errors
}

/// Every error of `instance`, as (pointer into the instance, message). Messages say "value"
/// instead of repeating submitted data.
#[must_use]
pub fn instance_errors(validator: &Validator, instance: &Value) -> Vec<(String, String)> {
    validator
        .iter_errors(instance)
        .map(|error| {
            (
                error.instance_path().to_string(),
                error.masked().to_string(),
            )
        })
        .collect()
}

/// Compiles `schema`, or reports why it does not compile as `invalid_schema` diagnostics:
/// one per meta-schema error under `pointer`, otherwise the compile error at `pointer`.
pub fn compile_at(schema: &Value, pointer: &str) -> Result<Validator, Vec<Diagnostic>> {
    compile(schema).map_err(|message| {
        let errors = schema_errors(schema);
        if errors.is_empty() {
            return vec![Diagnostic::new("invalid_schema", pointer, message)];
        }
        errors
            .into_iter()
            .map(|(path, message)| {
                Diagnostic::new("invalid_schema", format!("{pointer}{path}"), message)
            })
            .collect()
    })
}
