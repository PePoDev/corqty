use std::sync::Arc;

use chrono::Utc;
use corqty_domain::diagnostic::Diagnostic;
use corqty_plugin_host::{Registry, RegistryError};
use corqty_plugin_sdk::{
    Capability, ExecuteRequest, ExecutionContext, Executor, ExecutorError, ExecutorMetadata,
    HOST_CONTRACT, LogChunk, LogStream,
};
use futures_util::future::BoxFuture;
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// An executor that logs one chunk to each stream and returns the request it received.
struct Fake(ExecutorMetadata);

impl Executor for Fake {
    fn metadata(&self) -> ExecutorMetadata {
        self.0.clone()
    }

    fn validate(&self, _config: &Value) -> Vec<Diagnostic> {
        Vec::new()
    }

    fn execute<'a>(
        &'a self,
        request: ExecuteRequest,
        context: &'a ExecutionContext,
    ) -> BoxFuture<'a, Result<Map<String, Value>, ExecutorError>> {
        Box::pin(async move {
            for (stream, text) in [
                (LogStream::Stdout, "starting"),
                (LogStream::Stderr, "warning"),
                (LogStream::System, "done"),
            ] {
                let chunk = LogChunk {
                    stream,
                    text: text.to_owned(),
                };
                let _ = context.logs.send(chunk).await;
            }
            Ok(object(json!({
                "executionId": request.execution_id,
                "attempt": request.attempt,
                "nodeId": request.node_id,
                "inputs": request.inputs,
                "config": request.config,
            })))
        })
    }
}

fn fake(
    key: &'static str,
    version: &'static str,
    host_contract: &'static str,
    capabilities: Vec<Capability>,
) -> Arc<dyn Executor> {
    Arc::new(Fake(ExecutorMetadata {
        key,
        version,
        host_contract,
        capabilities,
        timeout_retryable: false,
    }))
}

fn executor(key: &'static str, version: &'static str) -> Arc<dyn Executor> {
    fake(key, version, HOST_CONTRACT, vec![Capability::Log])
}

fn rejection(executors: Vec<Arc<dyn Executor>>) -> RegistryError {
    Registry::new(executors)
        .err()
        .expect("the registry rejects these executors")
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("expected an object, got {other}"),
    }
}

#[test]
fn the_milestone_three_executors_are_advertised_in_sorted_order() {
    let registry = Registry::new(vec![
        fake(
            "shell",
            "1.0.0",
            HOST_CONTRACT,
            vec![
                Capability::Log,
                Capability::ScratchFs,
                Capability::CommandExec {
                    executables: vec!["/bin/sh".to_owned()],
                },
            ],
        ),
        executor("mock", "1.0.0"),
        executor("mock", "0.9.0"),
    ])
    .expect("the registry accepts these executors");

    assert_eq!(
        registry.advertised(),
        ["mock@0.9.0", "mock@1.0.0", "shell@1.0.0"]
    );
}

#[test]
fn find_matches_the_exact_key_and_version() {
    let registry = Registry::new(vec![executor("mock", "1.0.0"), executor("shell", "1.0.0")])
        .expect("the registry accepts these executors");

    let shell = registry
        .find("shell", "1.0.0")
        .expect("shell@1.0.0 is registered")
        .metadata();
    assert_eq!((shell.key, shell.version), ("shell", "1.0.0"));
    assert!(registry.find("mock", "1.0.1").is_none());
    assert!(registry.find("mock", "1.0").is_none());
    assert!(registry.find("other", "1.0.0").is_none());
}

#[test]
fn keys_must_be_lowercase_slugs_of_at_most_63_characters() {
    let longest: &'static str = "k".repeat(63).leak();
    let too_long: &'static str = "k".repeat(64).leak();
    for key in [
        "",
        "Mock",
        "1mock",
        "-mock",
        "mock_task",
        "mock.task",
        "mock task",
        "mock@1",
        too_long,
    ] {
        assert_eq!(
            rejection(vec![executor(key, "1.0.0")]),
            RegistryError::InvalidKey(key.to_owned()),
            "{key:?}"
        );
    }
    for key in ["m", "mock-2", longest] {
        assert!(
            Registry::new(vec![executor(key, "1.0.0")]).is_ok(),
            "{key:?}"
        );
    }
}

#[test]
fn versions_must_be_three_dot_separated_numbers() {
    for version in [
        "",
        "1",
        "1.0",
        "1.0.0.0",
        "v1.0.0",
        "1..0",
        "1.0.0-beta",
        "1.0.x",
        "1.0.+1",
        " 1.0.0",
    ] {
        assert_eq!(
            rejection(vec![executor("mock", version)]),
            RegistryError::InvalidVersion(format!("mock@{version}")),
            "{version:?}"
        );
    }
    for version in ["0.0.0", "1.0.0", "10.20.30"] {
        assert!(
            Registry::new(vec![executor("mock", version)]).is_ok(),
            "{version:?}"
        );
    }
}

#[test]
fn executors_built_for_another_host_contract_are_rejected() {
    for host_contract in ["", "0", "2", "1.0"] {
        assert_eq!(
            rejection(vec![fake(
                "mock",
                "1.0.0",
                host_contract,
                vec![Capability::Log]
            )]),
            RegistryError::UnsupportedHostContract {
                executor: "mock@1.0.0".to_owned(),
                host_contract: host_contract.to_owned(),
            },
        );
    }
}

#[test]
fn secret_and_network_capabilities_wait_for_milestone_four() {
    for capability in [
        Capability::SecretRead,
        Capability::Network {
            host: "api.example.com".to_owned(),
            port: 443,
        },
    ] {
        assert_eq!(
            rejection(vec![fake(
                "mock",
                "1.0.0",
                HOST_CONTRACT,
                vec![Capability::Log, capability.clone()]
            )]),
            RegistryError::UnsupportedCapability {
                executor: "mock@1.0.0".to_owned(),
                capability,
            },
        );
    }
}

#[test]
fn an_identity_registered_twice_is_rejected() {
    assert_eq!(
        rejection(vec![
            executor("mock", "1.0.0"),
            executor("shell", "1.0.0"),
            executor("mock", "1.0.0"),
        ]),
        RegistryError::DuplicateExecutor("mock@1.0.0".to_owned()),
    );
}

#[tokio::test]
async fn a_found_executor_receives_the_request_and_streams_its_logs() {
    let registry = Registry::new(vec![executor("mock", "1.0.0")])
        .expect("the registry accepts these executors");
    let mock = registry
        .find("mock", "1.0.0")
        .expect("mock@1.0.0 is registered");
    let (logs, mut received) = mpsc::channel(64);
    let context = ExecutionContext {
        cancel: CancellationToken::new(),
        scratch_dir: std::env::temp_dir(),
        logs,
    };
    let execution_id = Uuid::now_v7();
    let request = ExecuteRequest {
        execution_id,
        attempt: 2,
        workflow_id: Uuid::now_v7(),
        run_id: Uuid::now_v7(),
        node_id: "vpc".to_owned(),
        inputs: object(json!({"region": "eu-west-1"})),
        config: json!({"mode": "fixed"}),
        deadline: Utc::now(),
    };

    let output = mock
        .execute(request, &context)
        .await
        .expect("the fake executor succeeds");
    drop(context);
    let mut chunks = Vec::new();
    while let Some(chunk) = received.recv().await {
        chunks.push((chunk.stream.as_str(), chunk.text));
    }

    assert_eq!(
        output,
        object(json!({
            "executionId": execution_id,
            "attempt": 2,
            "nodeId": "vpc",
            "inputs": {"region": "eu-west-1"},
            "config": {"mode": "fixed"},
        }))
    );
    assert_eq!(
        chunks,
        [
            ("stdout", "starting".to_owned()),
            ("stderr", "warning".to_owned()),
            ("system", "done".to_owned()),
        ]
    );
}
