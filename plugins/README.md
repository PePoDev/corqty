# Plugins

This directory contains plugin implementations and examples maintained with
Corqty. Installation is organization-scoped through the product's Plugins page
or HTTP API; placing a file in this directory does not install it automatically.

| Directory | Purpose |
|---|---|
| [example-wasm](example-wasm/README.md) | Minimal `hello@1.0.0` WebAssembly Component, source, manifest, and committed binary |

Read the [plugin authoring and runtime guide](../docs/plugins.md) for the versioned
WIT interface, manifests, explicit capability approvals, resource limits, and
testing. The [security guide](../docs/security.md) explains host trust boundaries.
