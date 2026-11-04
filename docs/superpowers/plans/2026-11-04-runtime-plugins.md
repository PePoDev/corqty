# Milestone 4: runtime component execution

Implemented on `main` with secrets and automation integration. The user requested
direct commits and pushes; no additional milestone branches or pull requests are used.

## Contract

- `crates/plugin-sdk/wit/executor.wit` is `corqty:executor@1.0.0`, host contract `1`.
  Components export metadata, config validation and execution. JSON inputs carry
  opaque secret references; only the explicit `secret-read` import resolves them.
- Uploads contain a JSON manifest and base64 **WebAssembly Component bytecode**.
  Native serialized Wasmtime code, core-only modules, source code and native
  libraries are rejected. Wasmtime compiles validated bytecode internally.
- Immutable `(organization, key, version)` records retain component bytes and
  SHA-256 checksum. Capabilities require approval of that exact checksum and
  manifest. Log-only components need no elevated approval.
- Each operation starts a fresh Store. No WASI ambient capabilities are linked.
  The host provides bounded logs, bound-secret reads, flat scratch-file reads and
  writes, exact approved host/port TCP exchanges and named executable invocation.
- Component maximum: 25 MiB; one linear memory: 256 MiB; fuel: 100 million per
  guest call; epoch checks every 10 ms; node deadline and cancellation also bound
  async host operations. Metadata/validation get a five-second deadline.
- Compilations are limited to two in flight and a 16-entry checksum cache. The
  compilation permit remains held even if its awaiting caller is canceled.
- Scratch storage is unique per attempt and removed by the worker. Host file
  names cannot traverse directories or follow existing symlinks. Scratch writes
  have a 10 MiB cumulative budget; each host payload is at most 1 MiB.
- **Command execution is trusted code.** Both approval and invocation require
  operator `CORQTY_SHELL_EXECUTOR=enabled`. Production Compose forces it off.
  Unix command process groups are killed on exit/cancellation/deadline. As with
  built-in shell, a deliberately daemonized process can escape its group.

## Integration

`202611040002_plugins.sql` adds forced-RLS tables, immutable version permissions,
fenced worker payload lookup and mixed-worker compatibility. A worker advertises
host contract `1`; older workers cannot claim runtime jobs. API queue reasons use
the same matching rule. Disabling a plugin prevents new publishes and runs while
existing queued descendants remain executable.

Publish and run checks share an organization advisory lock with plugin disabling.
Structural validation receives known secret and executor identities before its
diagnostic cap, so valid 500-node graphs do not exhaust the diagnostic budget.
Guest diagnostics are bounded individually and across the graph.

The admin Plugins screen installs, approves and enables/disables exact versions.
The catalog creates typed node definitions bound to their exact executor version.
The sample in `examples/plugins` installs and executes through normal workers in
the Compose smoke test; no separate cross-compilation toolchain is needed.

## Verification

- Component execution and byte-for-byte sample reproducibility.
- Core modules/native uploads rejected; mismatched exported manifests rejected.
- Fuel exhaustion, memory growth, deadline and cancellation coverage.
- Denied network, filesystem, command and unbound-secret access.
- API installation, duplicate version conflict, exact-checksum approval,
  tenant isolation, mixed-worker filtering and lease-fenced retrieval.
- Disabled plugin blocks new runs but does not invalidate existing work.
- Foundation Compose smoke uploads, approves, defines, publishes and executes
  the sample component using the three-worker production image.

Current Wasmtime documentation was checked using Context7. The locked 49.x line
supports the repository's Rust 1.96 toolchain; Wasmtime 50 requires Rust 1.97.
