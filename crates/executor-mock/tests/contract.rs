use std::time::Duration;

use chrono::Utc;
use corqty_executor_mock::MockExecutor;
use corqty_plugin_sdk::{
    Capability, ExecuteRequest, ExecutionContext, Executor, ExecutorError, ExecutorMetadata,
    HOST_CONTRACT, LogChunk,
};
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;
use tokio::time::{Instant, timeout};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("expected an object, got {other}"),
    }
}

/// The seeded create-vpc config (fixed output, no delay, no failure) with `changes` applied.
fn config(changes: Value) -> Value {
    let mut config = object(json!({
        "mode": "fixed",
        "output": {"vpcId": "vpc-0simulated"},
        "delayMs": 0,
        "failureMessage": null,
    }));
    config.extend(object(changes));
    Value::Object(config)
}

fn request(attempt: u32, config: Value) -> ExecuteRequest {
    ExecuteRequest {
        execution_id: Uuid::now_v7(),
        attempt,
        workflow_id: Uuid::now_v7(),
        run_id: Uuid::now_v7(),
        node_id: "task".to_owned(),
        inputs: object(json!({"input": {"a": 1}})),
        config,
        deadline: Utc::now(),
    }
}

/// A context whose log receiver stays open, as the worker's does.
fn context() -> (ExecutionContext, mpsc::Receiver<LogChunk>) {
    let (logs, received) = mpsc::channel(64);
    let context = ExecutionContext {
        cancel: CancellationToken::new(),
        scratch_dir: std::env::temp_dir(),
        logs,
    };
    (context, received)
}

async fn execute(attempt: u32, config: Value) -> Result<Map<String, Value>, ExecutorError> {
    let (context, _received) = context();
    MockExecutor
        .execute(request(attempt, config), &context)
        .await
}

/// What `validate` reports for `config`, as sorted "code at pointer" lines.
fn problems(config: &Value) -> Vec<String> {
    let mut problems: Vec<String> = MockExecutor
        .validate(config)
        .into_iter()
        .map(|diagnostic| format!("{} at {}", diagnostic.code, diagnostic.pointer))
        .collect();
    problems.sort();
    problems
}

#[test]
fn the_metadata_names_mock_1_0_0_with_only_the_log_capability() {
    assert_eq!(
        MockExecutor.metadata(),
        ExecutorMetadata {
            key: "mock",
            version: "1.0.0",
            host_contract: HOST_CONTRACT,
            capabilities: vec![Capability::Log],
            timeout_retryable: false,
        }
    );
}

#[tokio::test]
async fn fixed_mode_returns_the_configured_output_and_logs_the_delay() {
    let (context, mut received) = context();

    let output = MockExecutor
        .execute(request(1, config(json!({"delayMs": 20}))), &context)
        .await
        .expect("the attempt succeeds");
    drop(context);
    let mut chunks = Vec::new();
    while let Some(chunk) = received.recv().await {
        chunks.push((chunk.stream.as_str(), chunk.text));
    }

    assert_eq!(output, object(json!({"vpcId": "vpc-0simulated"})));
    assert_eq!(chunks, [("system", "Simulating 20 ms of work.".to_owned())]);
}

#[tokio::test]
async fn echo_mode_returns_the_resolved_inputs_under_result() {
    let output = execute(1, config(json!({"mode": "echo"})))
        .await
        .expect("the attempt succeeds");

    assert_eq!(output, object(json!({"result": {"input": {"a": 1}}})));
}

#[tokio::test]
async fn the_attempt_waits_for_the_configured_delay() {
    let started = Instant::now();

    execute(1, config(json!({"delayMs": 200})))
        .await
        .expect("the attempt succeeds");

    assert!(started.elapsed() >= Duration::from_millis(200));
}

#[tokio::test]
async fn a_cancel_during_a_long_delay_returns_canceled_within_a_second() {
    let (context, _received) = context();
    let cancel = context.cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        cancel.cancel();
    });

    let error = timeout(
        Duration::from_secs(1),
        MockExecutor.execute(request(1, config(json!({"delayMs": 10_000}))), &context),
    )
    .await
    .expect("the attempt returns within 1 s")
    .expect_err("a canceled attempt fails");

    assert_eq!(
        error,
        ExecutorError {
            code: "canceled".to_owned(),
            message: "The attempt was canceled.".to_owned(),
            retryable: false,
        }
    );
}

#[tokio::test]
async fn a_token_canceled_before_the_attempt_wins_over_a_zero_delay() {
    let (context, _received) = context();
    context.cancel.cancel();

    let error = MockExecutor
        .execute(request(1, config(json!({}))), &context)
        .await
        .expect_err("a canceled attempt fails");

    assert_eq!(error.code, "canceled");
}

#[tokio::test]
async fn a_failure_message_fails_every_attempt_without_retry() {
    for attempt in [1, 2] {
        let error = execute(
            attempt,
            config(json!({"failureMessage": "Simulated outage", "failAttempts": 1})),
        )
        .await
        .expect_err("the attempt fails");

        assert_eq!(
            error,
            ExecutorError {
                code: "mock_failure".to_owned(),
                message: "Simulated outage".to_owned(),
                retryable: false,
            }
        );
    }
}

#[tokio::test]
async fn fail_attempts_fail_retryably_and_then_succeed() {
    let config = config(json!({"failAttempts": 1}));

    let error = execute(1, config.clone())
        .await
        .expect_err("attempt 1 fails");
    let output = execute(2, config).await.expect("attempt 2 succeeds");

    assert_eq!(
        error,
        ExecutorError {
            code: "mock_retryable_failure".to_owned(),
            message: "Attempt 1 fails because failAttempts is 1.".to_owned(),
            retryable: true,
        }
    );
    assert_eq!(output, object(json!({"vpcId": "vpc-0simulated"})));
}

#[tokio::test]
async fn a_config_the_mock_cannot_read_fails_without_retry() {
    let error = execute(1, config(json!({"mode": "loop"})))
        .await
        .expect_err("the attempt fails");

    assert_eq!(
        (error.code.as_str(), error.retryable),
        ("mock_config_invalid", false)
    );
}

#[test]
fn validate_accepts_the_seeded_configs_and_fail_attempts() {
    for changes in [
        json!({}),
        json!({"mode": "echo", "delayMs": 60_000, "failureMessage": "Down", "failAttempts": 0}),
        json!({"failAttempts": 2}),
    ] {
        let config = config(changes);
        assert!(problems(&config).is_empty(), "{config}");
    }
}

#[test]
fn validate_locates_a_bad_mode_a_negative_fail_attempts_and_an_out_of_range_delay() {
    assert_eq!(
        problems(&config(
            json!({"mode": "loop", "delayMs": 60_001, "failAttempts": -1})
        )),
        [
            "invalid_config at /delayMs",
            "invalid_config at /failAttempts",
            "invalid_config at /mode",
        ]
    );
}
