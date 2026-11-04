# Corqty release completion

Updated 2026-11-04. Work continues directly on `main`; no feature branches or new
pull requests are used, per the user's instruction.

## Implementation

- [x] Foundation: accounts, organizations, invitations, RBAC, audit, tenant RLS.
- [x] Catalog/studio: typed graphs, immutable definitions/versions, blueprints,
  schema widgets, revision-aware saving, keyboard-accessible palette.
- [x] Execution: durable queue, three-worker scaling, retries, leases,
  cancellation, trusted shell, replayable SSE and live monitoring.
- [x] Runtime plugins/secrets: encrypted organization secrets, fenced retrieval,
  streaming redaction, output leak rejection, capability-approved components,
  resource/deadline limits, sample installation and execution.
- [x] Automation/hardening: scoped hashed tokens, REST idempotency, pinned cron
  and signed webhooks, DST/misfire/overlap rules, retention, bounded deployment
  limits, rate limits, production images, HTTPS configuration, operating docs.
- [x] Open-source essentials: Apache-2.0 license, contribution/security/conduct
  policies, dependency audits, complete acceptance checklist.

## Verified locally

- 213 Rust workspace tests, including PostgreSQL integration tests.
- 136 frontend tests on Node 24 and Node 26; lint and production build.
- 34 Linux shell/plugin contract tests, including process-group cleanup,
  capability denial, active epoch interruption, memory and fuel limits.
- Production Compose with three workers; foundation/component and automation
  smoke suites, including signed hooks through Next.js and real cron dispatch.
- Browser walkthrough: secrets, plugin installation, typed custom definition,
  keyboard insertion, publish, successful WASM run/SSE, tokens and triggers.
- Frontend dependency audit clean. Guarded Rust audit passes; its only exception
  is an unused optional SQLx MySQL/RSA package, with a CI dependency-tree guard.
- Production Compose and Caddy configuration validation.

## Final release bookkeeping

- [ ] Complete the pushed revision's CI and record its run in docs/release-checklist.md.
- [x] Remove unused Docker containers/images after delivery verification:
  one stopped container and 20 unused images removed, reclaiming 6.42 GB.
  Seven healthy running containers and all data volumes were preserved.

Deployment-specific domain/TLS, backups/restore drills, capacity/monitoring, and
customer access/support decisions belong to the deployment owner. They are
listed explicitly in docs/release-checklist.md. No public hosting was provisioned.

See docs/operations.md, docs/automation.md, examples/plugins/README.md and the
2026-11-04 milestone plans for implementation decisions and operating details.
