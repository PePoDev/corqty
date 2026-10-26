#![cfg(unix)]

use std::time::Duration;

use chrono::Utc;
use corqty_executor_shell::ShellExecutor;
use corqty_plugin_sdk::{
    Capability, ExecuteRequest, ExecutionContext, Executor, ExecutorError, ExecutorMetadata,
    HOST_CONTRACT, LogChunk, LogStream,
};
use serde_json::{Map, Value, json};
use tempfile::TempDir;
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep, timeout};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

type Outcome = Result<Map<String, Value>, ExecutorError>;

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("expected an object, got {other}"),
    }
}

fn command(command: &str) -> Value {
    json!({"command": command})
}

fn request(config: Value) -> ExecuteRequest {
    ExecuteRequest {
        execution_id: Uuid::now_v7(),
        attempt: 1,
        workflow_id: Uuid::now_v7(),
        run_id: Uuid::now_v7(),
        node_id: "shell".to_owned(),
        inputs: object(json!({"input": {"name": "corqty"}})),
        config,
        deadline: Utc::now(),
    }
}

/// Runs one attempt with `scratch` as its scratch directory and returns its outcome and
/// every log chunk it sent. The chunks are received while the attempt runs, as the worker
/// does.
async fn run_in(
    scratch: &TempDir,
    cancel: CancellationToken,
    config: Value,
) -> (Outcome, Vec<LogChunk>) {
    let (logs, mut received) = mpsc::channel(64);
    let collecting = tokio::spawn(async move {
        let mut chunks = Vec::new();
        while let Some(chunk) = received.recv().await {
            chunks.push(chunk);
        }
        chunks
    });
    let context = ExecutionContext {
        cancel,
        scratch_dir: scratch.path().to_owned(),
        logs,
    };
    let outcome = ShellExecutor.execute(request(config), &context).await;
    drop(context);
    (outcome, collecting.await.expect("the collector finishes"))
}

async fn run(config: Value) -> (Outcome, Vec<LogChunk>) {
    let scratch = TempDir::new().expect("a scratch directory");
    run_in(&scratch, CancellationToken::new(), config).await
}

/// The text of every chunk of `stream`, joined.
fn text(chunks: &[LogChunk], stream: LogStream) -> String {
    chunks
        .iter()
        .filter(|chunk| chunk.stream == stream)
        .map(|chunk| chunk.text.as_str())
        .collect()
}

/// Waits until the command has written its process group id (`$$`) to the file `group`.
async fn group_of(scratch: &TempDir) -> u32 {
    loop {
        let written = std::fs::read_to_string(scratch.path().join("group")).unwrap_or_default();
        if let Some(group) = written.strip_suffix('\n') {
            return group.parse().expect("a process group id");
        }
        sleep(Duration::from_millis(10)).await;
    }
}

/// The `/proc/<pid>/stat` lines of the processes in `group` that are still alive. A zombie
/// is dead: it only waits for whoever adopted it to reap it.
fn survivors(group: u32) -> Vec<String> {
    std::fs::read_dir("/proc")
        .expect("/proc is readable")
        .filter_map(|entry| std::fs::read_to_string(entry.ok()?.path().join("stat")).ok())
        .filter(|stat| {
            // After the command name come the state, the parent pid and the process group.
            let fields: Vec<&str> = stat
                .rsplit_once(')')
                .map_or(Vec::new(), |(_, rest)| rest.split_whitespace().collect());
            fields.len() > 2 && fields[0] != "Z" && fields[2] == group.to_string()
        })
        .collect()
}

/// Cancels the attempt once the command has written its group id, and returns the outcome,
/// the group id and how long the attempt took to return after the cancel.
async fn cancel_once_started(script: &str) -> (Outcome, u32, Duration) {
    let scratch = TempDir::new().expect("a scratch directory");
    let cancel = CancellationToken::new();
    let attempt = run_in(&scratch, cancel.clone(), command(script));
    let canceling = async {
        let group = group_of(&scratch).await;
        cancel.cancel();
        (group, Instant::now())
    };
    let ((outcome, _), (group, canceled_at)) = tokio::join!(attempt, canceling);
    (outcome, group, canceled_at.elapsed())
}

/// What `validate` reports for `config`, as sorted "code at pointer" lines.
fn problems(config: &Value) -> Vec<String> {
    let mut problems: Vec<String> = ShellExecutor
        .validate(config)
        .into_iter()
        .map(|diagnostic| format!("{} at {}", diagnostic.code, diagnostic.pointer))
        .collect();
    problems.sort();
    problems
}

#[test]
fn the_metadata_names_shell_1_0_0_with_scratch_and_bin_sh_capabilities() {
    assert_eq!(
        ShellExecutor.metadata(),
        ExecutorMetadata {
            key: "shell",
            version: "1.0.0",
            host_contract: HOST_CONTRACT,
            capabilities: vec![
                Capability::Log,
                Capability::ScratchFs,
                Capability::CommandExec {
                    executables: vec!["/bin/sh".to_owned()],
                },
            ],
            timeout_retryable: true,
        }
    );
}

#[tokio::test]
async fn the_command_reads_the_input_file_and_its_output_file_becomes_the_result() {
    let (outcome, _) = run(command(
        r#"printf '{"copy": %s}' "$(cat "$CORQTY_INPUT")" > "$CORQTY_OUTPUT""#,
    ))
    .await;

    assert_eq!(
        outcome.expect("the attempt succeeds"),
        object(json!({"copy": {"input": {"name": "corqty"}}}))
    );
}

#[tokio::test]
async fn the_command_sees_only_the_fixed_environment_and_allowed_config_variables() {
    let scratch = TempDir::new().expect("a scratch directory");
    let config = json!({
        "command": r#"env; echo '{}' > "$CORQTY_OUTPUT""#,
        "env": {"GREETING": "hello", "lower": "x", "1ST": "x", "CORQTY_INPUT": "/etc/passwd"},
    });

    let (outcome, chunks) = run_in(&scratch, CancellationToken::new(), config).await;
    let mut variables: Vec<String> = text(&chunks, LogStream::Stdout)
        .lines()
        .map(str::to_owned)
        .collect();
    variables.sort();
    let skipped: Vec<String> = chunks
        .into_iter()
        .filter(|chunk| chunk.stream == LogStream::System)
        .map(|chunk| chunk.text)
        .collect();

    outcome.expect("the attempt succeeds");
    let scratch = scratch.path().display();
    assert_eq!(
        variables,
        [
            format!("CORQTY_INPUT={scratch}/input.json"),
            format!("CORQTY_OUTPUT={scratch}/output.json"),
            "GREETING=hello".to_owned(),
            format!("HOME={scratch}"),
            "LANG=C.UTF-8".to_owned(),
            "PATH=/usr/local/bin:/usr/bin:/bin".to_owned(),
            format!("PWD={scratch}"),
            format!("TMPDIR={scratch}"),
        ]
    );
    let rule = "names must match ^[A-Z_][A-Z0-9_]*$ and must not start with CORQTY_.";
    assert_eq!(
        skipped,
        [
            format!("Skipped the environment variable 1ST: {rule}"),
            format!("Skipped the environment variable CORQTY_INPUT: {rule}"),
            format!("Skipped the environment variable lower: {rule}"),
        ]
    );
}

#[tokio::test]
async fn stdout_and_stderr_arrive_as_log_chunks_of_their_own_stream() {
    let (outcome, chunks) = run(command(
        r#"echo out; echo err >&2; echo '{}' > "$CORQTY_OUTPUT""#,
    ))
    .await;

    outcome.expect("the attempt succeeds");
    assert_eq!(text(&chunks, LogStream::Stdout), "out\n");
    assert_eq!(text(&chunks, LogStream::Stderr), "err\n");
    assert_eq!(text(&chunks, LogStream::System), "");
}

#[tokio::test]
async fn characters_split_between_reads_arrive_whole() {
    // `cat` writes in large blocks, so reads end at even offsets: inside a two-byte `é`.
    let (outcome, chunks) = run(command(
        r#"{ printf a; printf 'é%.0s' $(seq 100000); } > log; cat log; echo '{}' > "$CORQTY_OUTPUT""#,
    ))
    .await;

    outcome.expect("the attempt succeeds");
    assert_eq!(
        text(&chunks, LogStream::Stdout),
        format!("a{}", "é".repeat(100_000))
    );
}

#[tokio::test]
async fn twenty_mebibytes_of_output_stop_at_the_log_cap_without_blocking_the_command() {
    let (outcome, chunks) = timeout(
        Duration::from_secs(10),
        run(command(
            r#"head -c 20971520 /dev/zero | tr '\000' a; echo '{}' > "$CORQTY_OUTPUT""#,
        )),
    )
    .await
    .expect("the attempt finishes within 10 s");

    assert_eq!(outcome.expect("the attempt succeeds"), Map::new());
    assert!(chunks.iter().all(|chunk| chunk.text.len() <= 8192));
    assert_eq!(
        chunks.iter().map(|chunk| chunk.text.len()).sum::<usize>(),
        10_485_760
    );
}

#[tokio::test]
async fn a_non_zero_exit_fails_without_retry_and_names_the_exit_status() {
    let (outcome, _) = run(command(r#"echo '{}' > "$CORQTY_OUTPUT"; exit 3"#)).await;

    assert_eq!(
        outcome.expect_err("the attempt fails"),
        ExecutorError {
            code: "shell_exit".to_owned(),
            message: "The command failed (exit status: 3).".to_owned(),
            retryable: false,
        }
    );
}

#[tokio::test]
async fn a_missing_output_file_fails_without_retry() {
    let (outcome, _) = run(command("true")).await;

    assert_eq!(
        outcome.expect_err("the attempt fails"),
        ExecutorError {
            code: "shell_output_invalid".to_owned(),
            message: "The command did not write the output file.".to_owned(),
            retryable: false,
        }
    );
}

#[tokio::test]
async fn an_output_file_without_a_json_object_fails_without_retry() {
    for script in [
        r#"echo '[1]' > "$CORQTY_OUTPUT""#,
        r#"echo 'not json' > "$CORQTY_OUTPUT""#,
    ] {
        let (outcome, _) = run(command(script)).await;
        let error = outcome.expect_err("the attempt fails");

        assert_eq!(
            (error.code.as_str(), error.retryable),
            ("shell_output_invalid", false)
        );
        assert!(
            error
                .message
                .starts_with("The output file does not hold a JSON object: "),
            "{}",
            error.message
        );
    }
}

#[tokio::test]
async fn the_output_file_may_hold_at_most_one_mebibyte() {
    // `{"pad":""}` is 10 bytes.
    let script = |pad: usize| {
        format!(
            r#"printf '{{"pad":"%s"}}' "$(head -c {pad} /dev/zero | tr '\000' a)" > "$CORQTY_OUTPUT""#
        )
    };

    let (largest, _) = run(command(&script(1_048_566))).await;
    let (too_large, _) = run(command(&script(1_048_567))).await;

    assert_eq!(
        largest.expect("1 MiB is accepted")["pad"]
            .as_str()
            .map(str::len),
        Some(1_048_566)
    );
    assert_eq!(
        too_large.expect_err("1 MiB + 1 is rejected"),
        ExecutorError {
            code: "shell_output_invalid".to_owned(),
            message: "The output file is larger than 1 MiB.".to_owned(),
            retryable: false,
        }
    );
}

#[tokio::test]
async fn a_cancel_kills_the_whole_process_tree_within_five_seconds() {
    let (outcome, group, took) =
        cancel_once_started("sleep 30 & sleep 30 & echo $$ > group; wait").await;

    assert_eq!(
        outcome.expect_err("a canceled attempt fails"),
        ExecutorError {
            code: "canceled".to_owned(),
            message: "The attempt was canceled.".to_owned(),
            retryable: false,
        }
    );
    assert!(took < Duration::from_secs(5), "{took:?}");
    assert_eq!(survivors(group), Vec::<String>::new());
}

#[tokio::test]
async fn a_command_that_ignores_sigterm_is_killed_after_the_three_second_grace() {
    let (outcome, group, took) =
        cancel_once_started("trap '' TERM; sleep 30 & echo $$ > group; wait").await;

    assert_eq!(
        outcome.expect_err("a canceled attempt fails").code,
        "canceled"
    );
    assert!(took >= Duration::from_secs(3), "{took:?}");
    assert!(took < Duration::from_secs(5), "{took:?}");
    assert_eq!(survivors(group), Vec::<String>::new());
}

#[tokio::test]
async fn a_config_without_a_command_fails_without_retry() {
    let (outcome, _) = run(json!({"env": {}})).await;
    let error = outcome.expect_err("the attempt fails");

    assert_eq!(
        (error.code.as_str(), error.retryable),
        ("shell_config_invalid", false)
    );
}

#[test]
fn validate_accepts_the_seeded_default_and_allowed_variable_names() {
    for config in [
        json!({"command": r#"jq '{result: .}' "$CORQTY_INPUT" > "$CORQTY_OUTPUT""#, "env": {}}),
        json!({"command": "true", "env": {"API_URL": "https://example.test", "_TOKEN_2": ""}}),
        json!({"command": "true"}),
    ] {
        assert!(problems(&config).is_empty(), "{config}");
    }
}

#[test]
fn validate_locates_a_missing_or_empty_command_bad_variables_and_extra_keys() {
    assert_eq!(problems(&json!({})), ["invalid_config at "]);
    assert_eq!(
        problems(&json!({
            "command": "",
            "env": {"lower": "x", "CORQTY_INPUT": "x", "COUNT": 5},
            "shell": "bash",
        })),
        [
            "invalid_config at ",
            "invalid_config at /command",
            "invalid_config at /env",
            "invalid_config at /env",
            "invalid_config at /env/COUNT",
        ]
    );
}
