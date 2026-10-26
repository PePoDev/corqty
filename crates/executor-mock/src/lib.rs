#![forbid(unsafe_code)]

//! The deterministic mock executor `mock@1.0.0` (spec §10.3). It powers both starter
//! blueprints and the scheduler tests: it waits, then returns a fixed output or echoes its
//! inputs, or fails the way its config says.

use std::time::Duration;

use corqty_domain::diagnostic::Diagnostic;
use corqty_domain::schema::{compile, instance_errors};
use corqty_plugin_sdk::{
    Capability, ExecuteRequest, ExecutionContext, Executor, ExecutorError, ExecutorMetadata,
    HOST_CONTRACT, LogChunk, LogStream,
};
use futures_util::future::BoxFuture;
use serde::Deserialize;
use serde_json::{Map, Value, json};

pub struct MockExecutor;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    mode: Mode,
    output: Map<String, Value>,
    delay_ms: u64,
    failure_message: Option<String>,
    #[serde(default)]
    fail_attempts: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Fixed,
    Echo,
}

impl Executor for MockExecutor {
    fn metadata(&self) -> ExecutorMetadata {
        ExecutorMetadata {
            key: "mock",
            version: "1.0.0",
            host_contract: HOST_CONTRACT,
            capabilities: vec![Capability::Log],
            timeout_retryable: false,
        }
    }

    /// The seeded mock config schema plus the optional `failAttempts`, which only definitions
    /// with their own config schema can set.
    fn validate(&self, config: &Value) -> Vec<Diagnostic> {
        let schema = json!({
            "type": "object",
            "properties": {
                "mode": {"enum": ["fixed", "echo"]},
                "output": {"type": "object"},
                "delayMs": {"type": "integer", "minimum": 0, "maximum": 60_000},
                "failureMessage": {"type": ["string", "null"], "maxLength": 500},
                "failAttempts": {"type": "integer", "minimum": 0},
            },
            "required": ["mode", "output", "delayMs"],
            "additionalProperties": false,
        });
        let validator = compile(&schema).expect("the mock config schema compiles");
        instance_errors(&validator, config)
            .into_iter()
            .map(|(pointer, message)| Diagnostic::new("invalid_config", pointer, message))
            .collect()
    }

    /// Waits `delayMs`, unless the token fires first. Then a `failureMessage` fails the
    /// attempt for good, attempts up to `failAttempts` fail retryably, and the rest succeed
    /// with the fixed `output` or with `{"result": <inputs>}`.
    fn execute<'a>(
        &'a self,
        request: ExecuteRequest,
        context: &'a ExecutionContext,
    ) -> BoxFuture<'a, Result<Map<String, Value>, ExecutorError>> {
        Box::pin(async move {
            // ponytail: execute reads config leniently; validate is the strict check, and
            // publish has already applied the definition's config schema.
            let config: Config = serde_json::from_value(request.config).map_err(|error| {
                failure(
                    "mock_config_invalid",
                    format!("The mock cannot read its config: {error}."),
                    false,
                )
            })?;
            let text = format!("Simulating {} ms of work.", config.delay_ms);
            let chunk = LogChunk {
                stream: LogStream::System,
                text,
            };
            let _ = context.logs.send(chunk).await;
            tokio::select! {
                // A token that has already fired wins over a zero delay.
                biased;
                () = context.cancel.cancelled() => {
                    return Err(failure("canceled", "The attempt was canceled.".to_owned(), false));
                }
                () = tokio::time::sleep(Duration::from_millis(config.delay_ms)) => {}
            }
            if let Some(message) = config.failure_message {
                return Err(failure("mock_failure", message, false));
            }
            if request.attempt <= config.fail_attempts {
                let message = format!(
                    "Attempt {} fails because failAttempts is {}.",
                    request.attempt, config.fail_attempts
                );
                return Err(failure("mock_retryable_failure", message, true));
            }
            Ok(match config.mode {
                Mode::Fixed => config.output,
                Mode::Echo => Map::from_iter([("result".to_owned(), request.inputs.into())]),
            })
        })
    }
}

fn failure(code: &str, message: String, retryable: bool) -> ExecutorError {
    ExecutorError {
        code: code.to_owned(),
        message,
        retryable,
    }
}
