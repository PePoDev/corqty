use jsonschema::{PatternOptions, Validator};
use serde_json::Value;

use crate::diagnostic::{Diagnostic, MAX_DIAGNOSTICS};

/// The only JSON Schema dialect Corqty accepts.
pub const DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";
/// The largest schema, as JSON, that Corqty compiles. It bounds what one validation costs.
pub const MAX_SCHEMA_BYTES: usize = 16_384;

/// Compiles a user-supplied schema as Draft 2020-12. A schema must be self-contained and
/// small: references are refused, because even local ones can make validation exponential.
/// Patterns use the linear-time `regex` engine.
pub fn compile(schema: &Value) -> Result<Validator, String> {
    if schema.to_string().len() > MAX_SCHEMA_BYTES {
        return Err(format!(
            "A schema can have at most {MAX_SCHEMA_BYTES} bytes."
        ));
    }
    check_keywords(schema)?;
    jsonschema::draft202012::options()
        .offline()
        .with_pattern_options(PatternOptions::regex())
        .build(schema)
        .map_err(|error| error.to_string())
}

/// Refuses reference keywords and any dialect but `DIALECT`, at every depth. Only string
/// values count: a property merely named `$ref` or `$schema` holds a schema object.
fn check_keywords(value: &Value) -> Result<(), String> {
    match value {
        Value::Object(object) => object.iter().try_for_each(|(key, value)| {
            match (key.as_str(), value) {
                ("$ref" | "$dynamicRef" | "$recursiveRef", Value::String(_)) => {
                    return Err("Schema references are not supported.".to_owned());
                }
                ("$schema", Value::String(dialect)) if dialect != DIALECT => {
                    return Err("Only JSON Schema Draft 2020-12 is supported.".to_owned());
                }
                _ => {}
            }
            check_keywords(value)
        }),
        Value::Array(items) => items.iter().try_for_each(check_keywords),
        _ => Ok(()),
    }
}

/// The first `MAX_DIAGNOSTICS` Draft 2020-12 meta-schema errors in `schema`, as (pointer
/// into the schema, message).
#[must_use]
pub fn schema_errors(schema: &Value) -> Vec<(String, String)> {
    let mut errors: Vec<(String, String)> = jsonschema::draft202012::meta::validator()
        .iter_errors(schema)
        .take(MAX_DIAGNOSTICS)
        .map(|error| (error.instance_path().to_string(), error.to_string()))
        .collect();
    // A schema that is neither an object nor a boolean fails every branch the same way.
    errors.dedup();
    errors
}

/// The first `MAX_DIAGNOSTICS` errors of `instance`, as (pointer into the instance, message).
/// Messages say "value" instead of repeating submitted data.
#[must_use]
pub fn instance_errors(validator: &Validator, instance: &Value) -> Vec<(String, String)> {
    validator
        .iter_errors(instance)
        .take(MAX_DIAGNOSTICS)
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
