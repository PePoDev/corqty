#![forbid(unsafe_code)]

//! The trusted shell executor `shell@1.0.0` (spec §10.4). It runs `/bin/sh -c <command>` as
//! the worker's own user, in the attempt's scratch directory, with a small fixed environment.
//! It is not a security boundary: a command can do anything the worker container allows, so
//! only an organization admin can turn it on, for users they trust.

use corqty_domain::diagnostic::Diagnostic;
use corqty_domain::schema::{compile, instance_errors};
use corqty_plugin_sdk::{
    Capability, ExecuteRequest, ExecutionContext, Executor, ExecutorError, ExecutorMetadata,
    HOST_CONTRACT,
};
use futures_util::future::BoxFuture;
use serde_json::{Map, Value, json};

pub struct ShellExecutor;

impl Executor for ShellExecutor {
    fn metadata(&self) -> ExecutorMetadata {
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
    }

    /// The seeded `corqty.shell.command` config schema plus the rule `execute` applies to
    /// environment names.
    fn validate(&self, config: &Value) -> Vec<Diagnostic> {
        let schema = json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "minLength": 1, "maxLength": 16_384},
                "env": {
                    "type": "object",
                    "additionalProperties": {"type": "string"},
                    "propertyNames": {
                        "pattern": "^[A-Z_][A-Z0-9_]*$",
                        "not": {"pattern": "^CORQTY_"},
                    },
                },
            },
            "required": ["command"],
            "additionalProperties": false,
        });
        let validator = compile(&schema).expect("the shell config schema compiles");
        instance_errors(&validator, config)
            .into_iter()
            .map(|(pointer, message)| Diagnostic::new("invalid_config", pointer, message))
            .collect()
    }

    /// Writes the inputs to `CORQTY_INPUT`, runs the command and returns the JSON object it
    /// wrote to `CORQTY_OUTPUT`. Stdout and stderr become log chunks.
    fn execute<'a>(
        &'a self,
        request: ExecuteRequest,
        context: &'a ExecutionContext,
    ) -> BoxFuture<'a, Result<Map<String, Value>, ExecutorError>> {
        Box::pin(imp::execute(request, context))
    }
}

fn failure(code: &str, message: String, retryable: bool) -> ExecutorError {
    ExecutorError {
        code: code.to_owned(),
        message,
        retryable,
    }
}

#[cfg(unix)]
mod imp {
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::pin::pin;
    use std::process::Stdio;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use corqty_domain::runtime::{MAX_LOG_BYTES_PER_NODE_RUN, MAX_NODE_OUTPUT_BYTES};
    use corqty_plugin_sdk::{ExecuteRequest, ExecutionContext, ExecutorError, LogChunk, LogStream};
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;
    use serde::Deserialize;
    use serde_json::{Map, Value};
    use tokio::io::{AsyncRead, AsyncReadExt};
    use tokio::process::Command;
    use tokio::sync::mpsc::Sender;
    use tokio::time::timeout;

    use crate::failure;

    /// How long the shell may take to exit after SIGTERM before the group gets SIGKILL.
    const TERM_GRACE: Duration = Duration::from_secs(3);
    /// How long logs are still read once the group is dead. A process that left the group
    /// (`setsid`) could otherwise hold a pipe open forever.
    const DRAIN_GRACE: Duration = Duration::from_secs(2);
    const CHUNK_BYTES: usize = 8 * 1024;

    #[derive(Deserialize)]
    struct Config {
        command: String,
        #[serde(default)]
        env: BTreeMap<String, String>,
    }

    /// The command's process group. Dropping it SIGKILLs every process left in the group, so
    /// an attempt whose future the worker drops leaves nothing running.
    struct Group(Option<Pid>);

    impl Group {
        fn signal(&self, signal: Signal) {
            if let Some(group) = self.0 {
                // ESRCH means the group is already gone.
                let _ = killpg(group, signal);
            }
        }

        /// SIGKILLs the group and forgets it: the id of an empty group can be reused.
        fn kill(&mut self) {
            self.signal(Signal::SIGKILL);
            self.0 = None;
        }
    }

    impl Drop for Group {
        fn drop(&mut self) {
            self.kill();
        }
    }

    pub async fn execute(
        request: ExecuteRequest,
        context: &ExecutionContext,
    ) -> Result<Map<String, Value>, ExecutorError> {
        let config: Config = serde_json::from_value(request.config).map_err(|error| {
            failure(
                "shell_config_invalid",
                format!("The shell cannot read its config: {error}."),
                false,
            )
        })?;
        let scratch = &context.scratch_dir;
        let input = scratch.join("input.json");
        let output = scratch.join("output.json");
        // ponytail: blocking file I/O of at most 1 MiB in the scratch directory.
        std::fs::write(&input, Value::Object(request.inputs).to_string()).map_err(io_failure)?;

        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(&config.command)
            .current_dir(scratch)
            .env_clear()
            .env("PATH", "/usr/local/bin:/usr/bin:/bin")
            .env("LANG", "C.UTF-8")
            .env("HOME", scratch)
            .env("TMPDIR", scratch);
        for (name, value) in &config.env {
            if allowed(name) {
                command.env(name, value);
            } else {
                let text = format!(
                    "Skipped the environment variable {name}: names must match \
                     ^[A-Z_][A-Z0-9_]*$ and must not start with CORQTY_."
                );
                let chunk = LogChunk {
                    stream: LogStream::System,
                    text,
                };
                let _ = context.logs.send(chunk).await;
            }
        }
        command
            .env("CORQTY_INPUT", &input)
            .env("CORQTY_OUTPUT", &output)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // The shell leads a new process group, so one signal reaches every process the
            // command starts.
            .process_group(0);
        let mut child = command.spawn().map_err(io_failure)?;
        let mut group = Group(
            child
                .id()
                .and_then(|id| i32::try_from(id).ok())
                .map(Pid::from_raw),
        );

        let sent = AtomicUsize::new(0);
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let mut forwarding = pin!(async {
            tokio::join!(
                forward(stdout, LogStream::Stdout, &sent, &context.logs),
                forward(stderr, LogStream::Stderr, &sent, &context.logs),
            );
        });
        let mut supervising = pin!(async {
            let status = tokio::select! {
                biased;
                () = context.cancel.cancelled() => None,
                status = child.wait() => Some(status),
            };
            if status.is_none() {
                group.signal(Signal::SIGTERM);
                if timeout(TERM_GRACE, child.wait()).await.is_err() {
                    group.signal(Signal::SIGKILL);
                    let _ = child.wait().await;
                }
            }
            // Background jobs must not outlive the command or hold the pipes open.
            group.kill();
            status
        });
        let status = tokio::select! {
            status = &mut supervising => {
                let _ = timeout(DRAIN_GRACE, forwarding).await;
                status
            }
            () = &mut forwarding => supervising.await,
        };

        let Some(status) = status else {
            return Err(failure(
                "canceled",
                "The attempt was canceled.".to_owned(),
                false,
            ));
        };
        let status = status.map_err(io_failure)?;
        if !status.success() {
            return Err(failure(
                "shell_exit",
                format!("The command failed ({status})."),
                false,
            ));
        }
        read_output(&output)
    }

    /// Whether a config variable may reach the command: an upper-case name that cannot
    /// replace `CORQTY_INPUT` or `CORQTY_OUTPUT`.
    fn allowed(name: &str) -> bool {
        let mut bytes = name.bytes();
        bytes
            .next()
            .is_some_and(|first| first == b'_' || first.is_ascii_uppercase())
            && bytes.all(|byte| byte == b'_' || byte.is_ascii_uppercase() || byte.is_ascii_digit())
            && !name.starts_with("CORQTY_")
    }

    /// Sends a pipe to the log channel in chunks of at most 8 KiB until the attempt has sent
    /// more than the log cap. Then it keeps reading and drops the rest, so the command never
    /// blocks on a full pipe.
    async fn forward(
        mut pipe: impl AsyncRead + Unpin,
        stream: LogStream,
        sent: &AtomicUsize,
        logs: &Sender<LogChunk>,
    ) {
        let mut buffer = vec![0; CHUNK_BYTES];
        let mut pending = Vec::new();
        while let Ok(read @ 1..) = pipe.read(&mut buffer).await {
            pending.extend_from_slice(&buffer[..read]);
            // A character split between two reads waits for its last bytes.
            let complete = match std::str::from_utf8(&pending) {
                Err(error) if error.error_len().is_none() => error.valid_up_to(),
                _ => pending.len(),
            };
            let rest = pending.split_off(complete);
            let text = String::from_utf8_lossy(&pending).into_owned();
            pending = rest;
            // The chunk that takes the log past the cap still goes out whole, so the database
            // sees the log pass the cap and records the truncation.
            let before = sent.fetch_add(text.len(), Ordering::Relaxed);
            if before <= MAX_LOG_BYTES_PER_NODE_RUN && !text.is_empty() {
                let _ = logs.send(LogChunk { stream, text }).await;
            }
        }
    }

    fn read_output(path: &Path) -> Result<Map<String, Value>, ExecutorError> {
        let invalid = |message: String| failure("shell_output_invalid", message, false);
        let size = std::fs::metadata(path)
            .map_err(|_| invalid("The command did not write the output file.".to_owned()))?
            .len();
        if size > MAX_NODE_OUTPUT_BYTES as u64 {
            return Err(invalid("The output file is larger than 1 MiB.".to_owned()));
        }
        let bytes = std::fs::read(path).map_err(io_failure)?;
        serde_json::from_slice(&bytes).map_err(|error| {
            invalid(format!(
                "The output file does not hold a JSON object: {error}."
            ))
        })
    }

    fn io_failure(error: std::io::Error) -> ExecutorError {
        failure(
            "shell_io_error",
            format!("The shell executor hit an I/O error: {error}."),
            true,
        )
    }
}

#[cfg(not(unix))]
mod imp {
    use corqty_plugin_sdk::{ExecuteRequest, ExecutionContext, ExecutorError};
    use serde_json::{Map, Value};

    use crate::failure;

    pub async fn execute(
        _request: ExecuteRequest,
        _context: &ExecutionContext,
    ) -> Result<Map<String, Value>, ExecutorError> {
        Err(failure(
            "shell_unsupported",
            "The shell executor runs only on Linux.".to_owned(),
            false,
        ))
    }
}
