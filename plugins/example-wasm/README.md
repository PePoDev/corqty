# Hello component

`hello.wasm` is a precompiled WebAssembly Component implementing
[`executor.wit`](../../crates/plugin-sdk/wit/executor.wit). Its source is the small,
auditable `hello.wat`; `manifest.json` is its exact exported metadata. It returns
`{"result":"hello from wasm"}` and requests only logging.

1. In an organization, open **Plugins** as an admin. Upload `hello.wasm` with
   `manifest.json` as the manifest. Its SHA-256 checksum is shown after installation.
2. Create a node definition in **Catalog**, with executor key `hello` and version
   `1.0.0`, no input ports, and a required output `result` with `core.string` and
   schema `{"type":"string"}`. Configuration schema is `{"type":"object"}`.
3. Add the definition to a workflow, publish and run. No worker restart is needed.

Rebuild the committed component with:

```sh
cargo run -p corqty-plugin-host --example build-sample
cargo test -p corqty-plugin-host
```

The runtime tests verify the binary matches the checked-in WAT. The full Compose
smoke test performs installation and execution using the HTTP API.

For real plugins, use a Component Model toolchain to implement the WIT exports.
`metadata` returns the manifest JSON, `validate` returns a diagnostic array, and
`execute` returns a JSON output object or an error string. The execute request
contains `executionId`, `attempt`, `workflowId`, `runId`, `nodeId`, `inputs`,
`config`, and an RFC 3339 `deadline`. The execution ID stays stable across retries.
Config secret references and secret input bindings remain opaque `{secretId}`
objects; call `secret-read` only for explicitly bound IDs after approval.

No ambient WASI imports are supplied. Hosts/ports and executable names require
exact administrator approval. Host commands also require the operator's trusted
shell switch and carry the same trust assumptions as the built-in shell executor.
