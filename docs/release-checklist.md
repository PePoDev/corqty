# First-release acceptance record

Recorded 2026-11-04 against the integrated Milestones 1–5 implementation.
This records twelve release acceptance criteria and their concrete checks.
A local result is evidence for that checked environment, not a claim
that a public SaaS deployment has been provisioned or independently audited.

## Acceptance criteria

| # | Criterion | Evidence and current status |
|---|---|---|
| 1 | Fresh clone starts through documented Compose setup | Credential initializers and one Compose startup command documented; fresh assembled Compose stack and both live smoke scripts passed locally. |
| 2 | Accounts, organization invitations, and fixed roles | Foundation smoke and the full Rust authorization, secrets and automation role/tenant suites passed. |
| 3 | Both blueprints, graph editing, and immediate cycle/type feedback | Studio/catalog/frontend tests passed: 136 tests on Node 24 and Node 26. Foundation smoke covers blueprint and draft API behavior. |
| 4 | Atomic immutable publish with checksums and invalid-graph rejection | Foundation smoke and full publish/domain/database suites passed, including large resource-aware graphs. |
| 5 | Manual, REST, cron, signed webhook, pinned versions, deduplication | Both live smoke suites passed, including hooks through the Next.js proxy; six automation HTTP/SQL tests passed. Four cron unit tests passed, including timezone, DST restart, and wildcard-step regressions. |
| 6 | Three workers claim independent ready nodes without duplicate release | Three-worker smoke and the full concurrent queue/worker suites passed. |
| 7 | Retries, lease expiry, crashes, cancellation, fan-in, propagation | Execution smoke, full worker/scheduler tests and Linux shell/plugin contracts passed. |
| 8 | Replayable live outputs/logs in the UI | Foundation smoke passed SSE through the web proxy; frontend run/SSE tests passed on Node 24 and 26. |
| 9 | Compatible precompiled component upload, approval, bounded execution | `hello.wasm` installed/approved/executed in live Compose; focused plugin HTTP tests and 19 Linux plugin-host tests passed, including capability/limit and process-group checks. |
| 10 | Explicit opt-in trusted shell consumes/produces JSON | Live foundation smoke passed shell execution in Docker. Production override forces trusted execution off. |
| 11 | Encrypted, redacted, tenant-isolated secrets | Full secrets API, persistence fencing, and worker redaction/output-rejection suites passed; live smoke confirmed metadata reads omit values. |
| 12 | Complete automated and Compose suites pass | Both live Compose suites, 213 Rust workspace tests, 136 frontend tests, lint/build and Linux contracts passed locally. CI also passed every check on code revision `2dd5b3d`; see the run below. |

## Verification ledger

- [x] Fresh local Compose stack starts, with three workers for smoke verification.
- [x] `scripts/smoke-foundation.ps1` passes, including shell, SSE, and runtime component.
- [x] `scripts/smoke-automation.ps1` passes, including REST/webhook replay and real cron.
- [x] Frontend: 136 tests pass on Node 24 and Node 26; production build and lint pass.
- [x] Frontend dependency audit is clean at the checked lockfile.
- [x] Guarded Rust audit passed after TLS/Tokio dependency updates; its sole
  `RUSTSEC-2023-0071` exception is an uncompiled optional dependency, enforced by
  a workspace/all-targets dependency-tree check. See [audit scope](security.md#dependency-audit-scope).
- [x] Focused automation, plugin, and secrets integration tests pass.
- [x] Targeted Rust clippy checks pass with warnings denied.
- [x] Production Compose override parses with `config --quiet`.
- [x] Final `cargo fmt --check`.
- [x] Final `cargo clippy --workspace --all-targets -- -D warnings`.
- [x] Final `cargo test --workspace`: 213 passing tests, including PostgreSQL integration tests.
- [x] Linux contract tests: 15 shell and 19 plugin-host tests passed, including
  process-group cleanup (34 total).
- [x] Pushed-revision CI passed on `2dd5b3db43b4f774a263d7ed30a7e4e2dc007b9a`:
  [GitHub Actions run 41837404675](https://github.com/PePoDev/corqty/actions/runs/41837404675).
  This includes workspace formatting/clippy/tests, frontend lint/tests/build,
  both dependency audits, a fresh three-worker Compose build, and both smoke
  suites. This records the original release implementation; later changes must
  be verified on their own revision.

The first full Windows compilation exceeded available paging resources. Bounded
build concurrency and reduced dependency debug information resolved it; the
complete workspace suite subsequently passed.

## Documentation and repository-layout verification

The 2026-11-04 layout update at `b5d47b146e1842e37d546d02cc58f660f599ca77`
moves Docker support files under `docker/` and the sample under
`plugins/example-wasm/`. It passed:

- Rebuilding the sample, 18 plugin-host tests, and two plugin API tests.
- Main, standalone test, and production Compose configuration checks, including
  the resolved PostgreSQL initialization mount and web Dockerfile path.
- [CI run 41853170198](https://github.com/PePoDev/corqty/actions/runs/41853170198):
  full Rust/frontend checks, both dependency audits, fresh three-worker Compose
  startup, and both live smoke suites.

The accompanying documentation was reviewed against source. Checks covered 24
Markdown files, 481 local links/anchors, 13 JSON examples, 28 parsed Mermaid
diagrams, and eight valid SVG placeholders. Two existing product screenshots
were visually reviewed and copied unchanged; future capture requests are
explicitly labeled in the [visual tour](visual-tour.md). Commits following the
layout revision only update documentation and visual assets.

## Deployment-owner launch checks

These depend on the destination environment and are distinct from repository
implementation and local smoke verification:

- [ ] Public domain, HTTPS certificate issuance, and secure-cookie login verified.
- [ ] Database/API private networking and upstream client-aware rate limits checked.
- [ ] Protected database and master-key backups created; isolated restore drill passed.
- [ ] Resource capacity, queue/worker monitoring, and incident ownership established.
- [ ] Public signup/access policy and account-support process chosen.
- [ ] Maintainer private vulnerability reporting/contact configured as desired.

Use [operations](operations.md) for deployment/recovery procedures and
[security](security.md) for trust boundaries. Billing, OIDC, SMTP, a public
plugin marketplace, and real infrastructure executors are outside this release.
