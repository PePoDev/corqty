# Contributing

Start with the [README](README.md), the [approved product specification](docs/superpowers/specs/2026-07-28-corqty-workflow-orchestrator-design.md), and the relevant crate or web tests.
For a substantial change, open an issue describing the user problem and proposed
scope before investing in a large implementation. Small fixes can go directly
to a pull request. Maintainer availability varies; no response time is promised.

## Development

Use the Rust toolchain and pnpm version pinned in the repository; CI uses Node
24 and PostgreSQL 18. Follow the README's database, lint, test, and build commands.
Use a dedicated disposable database. Integration tests reset its data and cannot
run concurrently across separate Cargo processes on the same port.

Keep responsibilities in their existing crates: domain rules without HTTP/SQL,
persistence for tenant transactions, API handlers for transport and authorization,
workers for execution, and plugin SDK/host for executor contracts. Web routes
use the Next.js App Router; do not add `src/pages`.

## Changes and review

- Keep changes focused and use conventional commit subjects such as
  `fix(worker): fence stale completions`.
- Add regression tests for behavioral fixes and meaningful tests for new
  execution, permission, or persistence behavior. Include the commands and
  results in the pull request.
- Treat every organization-scoped identifier as untrusted. Preserve RLS,
  capability checks, fencing, and safe audit metadata. Never add secret values
  to logs, fixtures, screenshots, or issue reports.
- Add a new migration instead of modifying an applied migration. Explain
  rollout, compatibility, and recovery implications for schema/API changes.
- Update operating and user documentation when behavior changes. New runtime
  host capabilities require explicit approval rules and denial tests.
- Run formatting, lint, relevant tests, and the production web build. Run the
  full integration and smoke suites for changes crossing execution boundaries.
- Run `pnpm audit --audit-level=low` and the guarded
  `scripts/audit-rust.ps1` check before changing dependency locks. The sole
  unused-dependency exception is explained in [SECURITY.md](SECURITY.md);
  do not broaden it to suppress a compiled dependency's advisory.

Describe the problem, resulting behavior, verification, and any remaining
limitations. Maintainers review correctness, security, scope, and maintainability
before merging. Do not include generated build directories, local `.env` files,
database dumps, or credentials.

Contributions are submitted under the repository's [Apache-2.0 license](LICENSE).
Retain required third-party notices and identify new dependencies and their
licenses. Follow the [code of conduct](CODE_OF_CONDUCT.md).
