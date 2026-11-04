//! Capability-scoped Component Model host. No WASI, inherited environment, filesystem,
//! sockets or process imports are linked. Components get only the explicit WIT host.
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use chrono::Utc;
use corqty_domain::diagnostic::Diagnostic;
use corqty_plugin_sdk::{ExecuteRequest, ExecutionContext, ExecutorError, LogChunk, LogStream};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc::Sender,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use wasmtime::{
    Config, Engine, Store, StoreLimits, StoreLimitsBuilder,
    component::{Component, HasSelf, Linker},
};
use zeroize::Zeroize;

pub const MAX_COMPONENT_BYTES: usize = 25 * 1024 * 1024;
pub const MAX_MEMORY_BYTES: usize = 256 * 1024 * 1024;
pub const CALL_FUEL: u64 = 100_000_000;
const MAX_HOST_BYTES: usize = 1024 * 1024;

wasmtime::component::bindgen!({
    path: "../plugin-sdk/wit",
    world: "executor",
    imports: { default: async },
    exports: { default: async },
});

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PluginCapability {
    Log,
    SecretRead,
    ScratchFs,
    Network { host: String, port: u16 },
    CommandExec { executables: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginManifest {
    pub key: String,
    pub version: String,
    pub host_contract: String,
    pub capabilities: Vec<PluginCapability>,
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), ExecutorError> {
        if !super::is_valid_key(&self.key)
            || matches!(self.key.as_str(), "mock" | "shell")
            || !super::is_valid_version(&self.version)
            || self.version.len() > 40
            || self.host_contract != corqty_plugin_sdk::HOST_CONTRACT
            || self.capabilities.len() > 32
        {
            return Err(failure(
                "invalid_manifest",
                "Invalid plugin identity, host contract or capabilities.",
            ));
        }
        for (i, capability) in self.capabilities.iter().enumerate() {
            let valid = match capability {
                PluginCapability::Network { host, port } => {
                    !host.is_empty()
                        && host.len() <= 253
                        && host
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b".-:".contains(&b))
                        && *port != 0
                }
                PluginCapability::CommandExec { executables } => {
                    !executables.is_empty()
                        && executables.len() <= 16
                        && executables.iter().all(|name| {
                            !name.is_empty()
                                && name.len() <= 100
                                && name
                                    .bytes()
                                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                        })
                }
                _ => true,
            };
            if !valid || self.capabilities[..i].contains(capability) {
                return Err(failure(
                    "invalid_manifest",
                    "Invalid or duplicate capability rule.",
                ));
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn requires_approval(&self) -> bool {
        self.capabilities
            .iter()
            .any(|c| *c != PluginCapability::Log)
    }
}

#[must_use]
pub fn checksum(component: &[u8]) -> String {
    format!("{:x}", Sha256::digest(component))
}

fn engine() -> Result<Engine, ExecutorError> {
    static ENGINE: OnceLock<Result<Engine, String>> = OnceLock::new();
    ENGINE
        .get_or_init(|| {
            let mut config = Config::new();
            config
                .wasm_component_model(true)
                .consume_fuel(true)
                .epoch_interruption(true);
            let engine = Engine::new(&config).map_err(|error| error.to_string())?;
            let ticker = engine.clone();
            // One process-wide epoch clock, rather than a thread per invocation.
            std::thread::Builder::new()
                .name("corqty-wasm-epoch".into())
                .spawn(move || {
                    loop {
                        std::thread::sleep(Duration::from_millis(10));
                        ticker.increment_epoch();
                    }
                })
                .map_err(|error| error.to_string())?;
            Ok(engine)
        })
        .clone()
        .map_err(|_| {
            failure(
                "plugin_host_failed",
                "Could not initialize the component engine.",
            )
        })
}

#[derive(Clone)]
pub struct RuntimePlugin {
    manifest: PluginManifest,
    component: Component,
    engine: Engine,
    approved: Vec<PluginCapability>,
}

impl RuntimePlugin {
    /// Compile bytecode only; never deserialize native machine-code artifacts supplied by users.
    pub async fn new(
        manifest: PluginManifest,
        bytes: Vec<u8>,
        approved: Vec<PluginCapability>,
    ) -> Result<Self, ExecutorError> {
        manifest.validate()?;
        if bytes.len() > corqty_domain::limits::Limits::deployment().component_bytes
            || !bytes.starts_with(b"\0asm\x0d\0\x01\0")
        {
            return Err(failure(
                "invalid_component",
                "Upload a WebAssembly Component within the configured size limit (at most 25 MiB).",
            ));
        }
        if approved.iter().any(|c| !manifest.capabilities.contains(c)) {
            return Err(failure(
                "invalid_approval",
                "Approval exceeds the declared manifest.",
            ));
        }
        let engine = engine()?;
        let component = compile_cached(&engine, bytes).await?;
        Ok(Self {
            manifest,
            component,
            engine,
            approved,
        })
    }

    fn store(&self, state: HostState) -> Result<Store<HostState>, ExecutorError> {
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| &mut state.limits);
        store.set_fuel(CALL_FUEL).map_err(trap)?;
        store
            .fuel_async_yield_interval(Some(10_000))
            .map_err(trap)?;
        store.set_epoch_deadline(1);
        store.epoch_deadline_callback(|ctx| {
            if ctx.data().cancel.is_cancelled()
                || tokio::time::Instant::now() >= ctx.data().deadline
            {
                return Err(wasmtime::Error::msg("component deadline or cancellation"));
            }
            Ok(wasmtime::UpdateDeadline::Yield(1))
        });
        Ok(store)
    }

    async fn instantiate(&self, store: &mut Store<HostState>) -> Result<Executor, ExecutorError> {
        let mut linker = Linker::new(&self.engine);
        Executor::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state).map_err(trap)?;
        Executor::instantiate_async(store, &self.component, &linker)
            .await
            .map_err(trap)
    }

    /// Installation checks the full interface and exact exported metadata with no privileges.
    pub async fn verify(&self) -> Result<(), ExecutorError> {
        let mut store = self.store(HostState::restricted())?;
        let executor = self.instantiate(&mut store).await?;
        store.set_fuel(CALL_FUEL).map_err(trap)?;
        let metadata = executor.call_metadata(&mut store).await.map_err(trap)?;
        let declared: PluginManifest = serde_json::from_str(&metadata)
            .map_err(|_| failure("invalid_manifest", "Component metadata must be a manifest."))?;
        if declared != self.manifest {
            return Err(failure(
                "manifest_mismatch",
                "Component metadata differs from the upload manifest.",
            ));
        }
        Ok(())
    }

    pub async fn validate(&self, config: &Value) -> Result<Vec<Diagnostic>, ExecutorError> {
        let mut store = self.store(HostState::restricted())?;
        let executor = self.instantiate(&mut store).await?;
        store.set_fuel(CALL_FUEL).map_err(trap)?;
        let result = executor
            .call_validate(&mut store, &config.to_string())
            .await
            .map_err(trap)?;
        if result.len() > MAX_HOST_BYTES {
            return Err(failure(
                "invalid_diagnostics",
                "Component diagnostics exceed the limit.",
            ));
        }
        let diagnostics: Vec<Diagnostic> = serde_json::from_str(&result).map_err(|_| {
            failure(
                "invalid_diagnostics",
                "Component validate must return a diagnostic array.",
            )
        })?;
        if diagnostics.len() > 200
            || diagnostics
                .iter()
                .any(|d| d.code.len() > 100 || d.pointer.len() > 1024 || d.message.len() > 2000)
        {
            return Err(failure(
                "invalid_diagnostics",
                "Too many component diagnostics.",
            ));
        }
        Ok(diagnostics)
    }

    pub async fn execute_runtime(
        &self,
        request: ExecuteRequest,
        context: &ExecutionContext,
    ) -> Result<Map<String, Value>, ExecutorError> {
        if context.cancel.is_cancelled() {
            return Err(failure("canceled", "Execution was canceled."));
        }
        if request.deadline <= Utc::now() {
            return Err(failure("timeout", "Component deadline exceeded."));
        }
        if self
            .manifest
            .capabilities
            .iter()
            .any(|c| *c != PluginCapability::Log && !self.approved.contains(c))
        {
            return Err(failure(
                "capability_denied",
                "The plugin needs approval for its declared capabilities.",
            ));
        }
        let remaining = (request.deadline - Utc::now()).to_std().unwrap_or_default();
        let deadline = tokio::time::Instant::now() + remaining;
        let state = HostState {
            limits: limits(),
            capabilities: self.approved.clone(),
            cancel: context.cancel.clone(),
            deadline,
            scratch: Some(context.scratch_dir.clone()),
            secrets: context.bound_secrets.clone(),
            logs: Some(context.logs.clone()),
            scratch_bytes: 0,
            command_exec_allowed: std::env::var("CORQTY_SHELL_EXECUTOR")
                .is_ok_and(|value| value == "enabled"),
        };
        let mut store = self.store(state)?;
        let payload = json!({"executionId":request.execution_id,"attempt":request.attempt,
            "workflowId":request.workflow_id,"runId":request.run_id,"nodeId":request.node_id,
            "inputs":request.inputs,"config":request.config,"deadline":request.deadline})
        .to_string();
        let call = async {
            let executor = self.instantiate(&mut store).await?;
            store.set_fuel(CALL_FUEL).map_err(trap)?;
            let result = executor
                .call_execute(&mut store, &payload)
                .await
                .map_err(trap)?
                .map_err(guest_error)?;
            if result.len() > MAX_HOST_BYTES {
                return Err(failure(
                    "output_too_large",
                    "Component output exceeds 1 MiB.",
                ));
            }
            serde_json::from_str::<Map<String, Value>>(&result)
                .map_err(|_| failure("invalid_output", "Component output must be a JSON object."))
        };
        tokio::select! {
            () = context.cancel.cancelled() => Err(failure("canceled", "Execution was canceled.")),
            result = tokio::time::timeout_at(deadline, call) => result.unwrap_or_else(|_| Err(failure("timeout", "Component deadline exceeded."))),
        }
    }
}

async fn compile_cached(engine: &Engine, bytes: Vec<u8>) -> Result<Component, ExecutorError> {
    static CACHE: OnceLock<Mutex<VecDeque<(String, Component)>>> = OnceLock::new();
    static COMPILING: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
    let cache = CACHE.get_or_init(Mutex::default);
    let digest = checksum(&bytes);
    if let Some((_, component)) = cache
        .lock()
        .expect("component cache")
        .iter()
        .find(|(key, _)| *key == digest)
    {
        return Ok(component.clone());
    }
    let _permit = COMPILING
        .acquire()
        .await
        .map_err(|_| failure("plugin_host_failed", "Component compiler unavailable."))?;
    if let Some((_, component)) = cache
        .lock()
        .expect("component cache")
        .iter()
        .find(|(key, _)| *key == digest)
    {
        return Ok(component.clone());
    }
    let engine = engine.clone();
    let component = tokio::task::spawn_blocking(move || {
        let _permit = _permit;
        Component::from_binary(&engine, &bytes)
    })
    .await
    .map_err(|_| failure("invalid_component", "Component compilation failed."))?
    .map_err(|_| {
        failure(
            "invalid_component",
            "The upload is not a valid WebAssembly Component.",
        )
    })?;
    let mut entries = cache.lock().expect("component cache");
    if entries.len() >= 16 {
        entries.pop_front();
    }
    entries.push_back((digest, component.clone()));
    Ok(component)
}

fn limits() -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(corqty_domain::limits::Limits::deployment().wasm_memory_bytes)
        .memories(1)
        .tables(8)
        .table_elements(100_000)
        .instances(32)
        .trap_on_grow_failure(true)
        .build()
}
fn failure(code: &str, message: &str) -> ExecutorError {
    ExecutorError {
        code: code.into(),
        message: message.into(),
        retryable: false,
    }
}

/// WIT transports errors as JSON so guests retain the compiled executor's typed contract.
/// A plain text error remains supported and is always non-retryable.
fn guest_error(mut message: String) -> ExecutorError {
    const MAX_ERROR_BYTES: usize = 65_536;
    if message.len() > MAX_ERROR_BYTES {
        let mut boundary = MAX_ERROR_BYTES;
        while !message.is_char_boundary(boundary) {
            boundary -= 1;
        }
        message.truncate(boundary);
        return failure("plugin_error", &message);
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TypedError {
        code: String,
        message: String,
        retryable: bool,
    }
    if let Ok(error) = serde_json::from_str::<TypedError>(&message)
        && (1..=100).contains(&error.code.len())
        && error
            .code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        && error.message.len() <= MAX_ERROR_BYTES
    {
        return ExecutorError {
            code: error.code,
            message: error.message,
            retryable: error.retryable,
        };
    }
    failure("plugin_error", &message)
}
fn trap(_: wasmtime::Error) -> ExecutorError {
    failure(
        "plugin_trap",
        "Component trapped or exceeded its resource limits.",
    )
}

struct HostState {
    limits: StoreLimits,
    capabilities: Vec<PluginCapability>,
    cancel: CancellationToken,
    deadline: tokio::time::Instant,
    scratch: Option<PathBuf>,
    secrets: HashMap<Uuid, String>,
    logs: Option<Sender<LogChunk>>,
    scratch_bytes: usize,
    command_exec_allowed: bool,
}
impl Drop for HostState {
    fn drop(&mut self) {
        for value in self.secrets.values_mut() {
            value.zeroize();
        }
    }
}
impl HostState {
    fn restricted() -> Self {
        Self {
            limits: limits(),
            capabilities: vec![],
            cancel: CancellationToken::new(),
            deadline: tokio::time::Instant::now() + Duration::from_secs(5),
            scratch: None,
            secrets: HashMap::new(),
            logs: None,
            scratch_bytes: 0,
            command_exec_allowed: false,
        }
    }
    fn has(&self, capability: &PluginCapability) -> Result<(), String> {
        if self.capabilities.contains(capability) {
            Ok(())
        } else {
            Err("capability_denied".into())
        }
    }
    fn scratch_path(&self, name: &str) -> Result<PathBuf, String> {
        self.has(&PluginCapability::ScratchFs)?;
        if name.is_empty()
            || name.len() > 100
            || name == "."
            || name == ".."
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err("invalid_scratch_name".into());
        }
        let path = self.scratch.as_ref().ok_or("capability_denied")?.join(name);
        if std::fs::symlink_metadata(&path)
            .is_ok_and(|m| !m.is_file() || m.file_type().is_symlink())
        {
            return Err("invalid_scratch_file".into());
        }
        Ok(path)
    }
}

impl corqty::executor::host::Host for HostState {
    async fn log(&mut self, message: String) {
        if message.len() <= MAX_HOST_BYTES
            && let Some(logs) = &self.logs
        {
            let _ = tokio::time::timeout_at(
                self.deadline,
                logs.send(LogChunk {
                    stream: LogStream::System,
                    text: message,
                }),
            )
            .await;
        }
    }
    async fn secret_read(&mut self, id: String) -> Result<String, String> {
        self.has(&PluginCapability::SecretRead)?;
        Uuid::parse_str(&id)
            .ok()
            .and_then(|id| self.secrets.get(&id).cloned())
            .ok_or_else(|| "secret_not_bound".into())
    }
    async fn scratch_read(&mut self, name: String) -> Result<Vec<u8>, String> {
        let path = self.scratch_path(&name)?;
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|_| "scratch_read_failed")?;
        let mut bytes = Vec::new();
        file.take((MAX_HOST_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| "scratch_read_failed")?;
        if bytes.len() > MAX_HOST_BYTES {
            return Err("host_limit_exceeded".into());
        }
        Ok(bytes)
    }
    async fn scratch_write(&mut self, name: String, bytes: Vec<u8>) -> Result<(), String> {
        let path = self.scratch_path(&name)?;
        self.scratch_bytes = self.scratch_bytes.saturating_add(bytes.len());
        if self.scratch_bytes > 10 * MAX_HOST_BYTES {
            return Err("host_limit_exceeded".into());
        }
        tokio::fs::write(path, bytes)
            .await
            .map_err(|_| "scratch_write_failed".into())
    }
    async fn network(
        &mut self,
        host: String,
        port: u16,
        request: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        self.has(&PluginCapability::Network {
            host: host.clone(),
            port,
        })?;
        if request.len() > MAX_HOST_BYTES {
            return Err("host_limit_exceeded".into());
        }
        let exchange = async {
            let mut socket = tokio::net::TcpStream::connect((host.as_str(), port))
                .await
                .map_err(|_| "network_failed")?;
            socket
                .write_all(&request)
                .await
                .map_err(|_| "network_failed")?;
            socket.shutdown().await.map_err(|_| "network_failed")?;
            let mut response = Vec::new();
            socket
                .take((MAX_HOST_BYTES + 1) as u64)
                .read_to_end(&mut response)
                .await
                .map_err(|_| "network_failed")?;
            if response.len() > MAX_HOST_BYTES {
                return Err("host_limit_exceeded");
            }
            Ok(response)
        };
        tokio::time::timeout_at(self.deadline, exchange)
            .await
            .map_err(|_| "timeout")?
            .map_err(str::to_owned)
    }
    async fn command_exec(
        &mut self,
        executable: String,
        arguments: Vec<String>,
    ) -> Result<String, String> {
        if !self.capabilities.iter().any(|c|matches!(c,PluginCapability::CommandExec {executables} if executables.contains(&executable))) { return Err("capability_denied".into()); }
        if !self.command_exec_allowed {
            return Err("command_exec_unavailable".into());
        }
        if arguments.len() > 100 || arguments.iter().map(String::len).sum::<usize>() > 65536 {
            return Err("host_limit_exceeded".into());
        }
        let mut command = tokio::process::Command::new(executable);
        command
            .args(arguments)
            .env_clear()
            .env("PATH", "/usr/local/bin:/usr/bin:/bin")
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        if let Some(scratch) = &self.scratch {
            command.current_dir(scratch);
        }
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().map_err(|_| "command_failed")?;
        #[cfg(unix)]
        let _group = CommandGroup(
            child
                .id()
                .and_then(|id| i32::try_from(id).ok())
                .map(nix::unistd::Pid::from_raw),
        );
        let stdout = child.stdout.take().ok_or("command_failed")?;
        let run = async {
            let mut output = Vec::new();
            stdout
                .take((MAX_HOST_BYTES + 1) as u64)
                .read_to_end(&mut output)
                .await
                .map_err(|_| "command_failed")?;
            if output.len() > MAX_HOST_BYTES {
                return Err("host_limit_exceeded");
            }
            let status = child.wait().await.map_err(|_| "command_failed")?;
            if !status.success() {
                return Err("command_failed");
            }
            String::from_utf8(output).map_err(|_| "invalid_utf8")
        };
        tokio::time::timeout_at(self.deadline, run)
            .await
            .map_err(|_| "timeout")?
            .map_err(str::to_owned)
    }
    async fn cancelled(&mut self) -> bool {
        self.cancel.is_cancelled() || tokio::time::Instant::now() >= self.deadline
    }
}

#[cfg(unix)]
struct CommandGroup(Option<nix::unistd::Pid>);
#[cfg(unix)]
impl Drop for CommandGroup {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            let _ = nix::sys::signal::killpg(pid, nix::sys::signal::Signal::SIGKILL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::corqty::executor::host::Host;
    use super::*;

    #[test]
    fn guest_errors_preserve_valid_codes_and_retryability_and_bound_plain_text() {
        let error = guest_error(
            r#"{"code":"service_unavailable","message":"Try again","retryable":true}"#.into(),
        );
        assert_eq!(error.code, "service_unavailable");
        assert_eq!(error.message, "Try again");
        assert!(error.retryable);
        for text in [
            "A plain failure",
            r#"{"code":"Bad Code","message":"invalid","retryable":true}"#,
            r#"{"code":"ok","message":"invalid","retryable":"yes"}"#,
        ] {
            let error = guest_error(text.into());
            assert_eq!(error.code, "plugin_error");
            assert!(!error.retryable);
        }
        let error = guest_error("🔐".repeat(20_000));
        assert!(error.message.len() <= 65_536);
        assert!(error.message.chars().all(|character| character == '🔐'));
    }

    #[tokio::test]
    async fn approved_commands_still_require_the_operator_trust_gate() {
        let mut host = HostState::restricted();
        host.capabilities.push(PluginCapability::CommandExec {
            executables: vec!["sh".into()],
        });
        assert_eq!(
            host.command_exec("sh".into(), vec!["-c".into(), "exit 0".into()])
                .await,
            Err("command_exec_unavailable".into())
        );
    }

    #[tokio::test]
    async fn epoch_interrupts_an_active_guest_with_unlimited_fuel() {
        let manifest: PluginManifest =
            serde_json::from_str(include_str!("../../../examples/plugins/manifest.json")).unwrap();
        let text = include_str!("../../../examples/plugins/hello.wat").replace(
            "(result i32) i32.const 16)",
            "(result i32) (loop br 0) i32.const 16)",
        );
        let plugin = RuntimePlugin::new(manifest, wat::parse_str(text).unwrap(), vec![])
            .await
            .unwrap();
        for cancel in [false, true] {
            let mut state = HostState::restricted();
            if cancel {
                let token = state.cancel.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    token.cancel();
                });
            } else {
                state.deadline = tokio::time::Instant::now() + Duration::from_millis(20);
            }
            let mut store = plugin.store(state).unwrap();
            let executor = plugin.instantiate(&mut store).await.unwrap();
            // This cannot exhaust fuel within the test: only the epoch callback can stop it.
            store.set_fuel(u64::MAX).unwrap();
            let result = tokio::time::timeout(
                Duration::from_secs(1),
                executor.call_execute(&mut store, "{}"),
            )
            .await
            .expect("an active guest is interrupted by an epoch within one second");
            assert!(result.is_err());
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn approved_command_deadline_terminates_its_child_process_group() {
        let scratch = tempfile::tempdir().unwrap();
        let mut host = HostState::restricted();
        host.command_exec_allowed = true;
        host.capabilities.push(PluginCapability::CommandExec {
            executables: vec!["sh".into()],
        });
        host.scratch = Some(scratch.path().to_owned());
        host.deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            host.command_exec(
                "sh".into(),
                vec!["-c".into(), "sleep 60 & echo $! > child.pid; wait".into()],
            ),
        )
        .await
        .unwrap();
        assert_eq!(result, Err("timeout".into()));
        let pid = std::fs::read_to_string(scratch.path().join("child.pid")).unwrap();
        let process = format!("/proc/{}/stat", pid.trim());
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                // Docker PID 1 may defer reaping, so a zombie is already terminated.
                match std::fs::read_to_string(&process) {
                    Err(_) => break,
                    Ok(stat)
                        if stat.rsplit_once(')').is_some_and(|(_, fields)| {
                            fields.split_whitespace().next() == Some("Z")
                        }) =>
                    {
                        break;
                    }
                    _ => tokio::time::sleep(Duration::from_millis(10)).await,
                }
            }
        })
        .await
        .expect("the subprocess cannot survive the host command deadline");
    }

    #[tokio::test]
    async fn capabilities_are_denied_before_any_host_io() {
        let mut host = HostState::restricted();
        assert_eq!(
            host.network("127.0.0.1".into(), 1, vec![]).await,
            Err("capability_denied".into())
        );
        assert_eq!(
            host.scratch_read("secret".into()).await,
            Err("capability_denied".into())
        );
        assert_eq!(
            host.scratch_write("secret".into(), vec![]).await,
            Err("capability_denied".into())
        );
        assert_eq!(
            host.command_exec("sh".into(), vec![]).await,
            Err("capability_denied".into())
        );
        assert_eq!(
            host.secret_read(Uuid::new_v4().to_string()).await,
            Err("capability_denied".into())
        );
        host.capabilities.push(PluginCapability::SecretRead);
        let id = Uuid::new_v4();
        host.secrets.insert(id, "bound-value".into());
        assert_eq!(
            host.secret_read(id.to_string()).await,
            Ok("bound-value".into())
        );
        assert_eq!(
            host.secret_read(Uuid::new_v4().to_string()).await,
            Err("secret_not_bound".into())
        );
    }

    #[test]
    fn approved_scratch_rejects_paths_and_parent_traversal() {
        let mut host = HostState::restricted();
        host.capabilities.push(PluginCapability::ScratchFs);
        host.scratch = Some(std::env::temp_dir());
        for path in [
            "..",
            "../outside",
            "/etc/passwd",
            "C:\\secret",
            "a/b",
            "a\\b",
        ] {
            assert!(host.scratch_path(path).is_err(), "{path}");
        }
    }
}
