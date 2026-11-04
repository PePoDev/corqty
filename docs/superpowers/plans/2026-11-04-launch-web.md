# Web launch completion — 2026-11-04

This follows the approved runtime/secrets and automation milestones. API
contracts are shared with the backend implementation plans. Existing
monochrome tokens, native accessible form controls, App Router pages, and
organization-scoped query keys remain the UI conventions.

## Implemented behavior

- Secrets: admin create/replace/delete; editor metadata-only list and named
  picker in both input bindings and configuration widgets. Plaintext fields
  clear after submission, including failures. Deletion requires a concrete
  confirmation and displays reference-protection errors. Exact-value
  redaction and transformed-value limitations are visible.
- Runtime components: bounded component upload with binary-header checks,
  editable manifest, declared capability list, SHA-256 identity, exact
  checksum/version capability approval, and enable/disable. Approval needs
  explicit acknowledgement; host-command trust requirements are visible.
- Catalog: editors can create immutable node-definition versions with an
  arbitrary exact executor key/version, typed ports, schemas, widgets, and
  defaults. Backend pointer diagnostics retain the submitted form. Approved
  runtime plugins link to this form with their executor identity populated.
- Service tokens: admin create/revoke, scope display, copyable one-time
  credential, and an idempotent REST request example. Credentials never enter
  list query data, browser storage, or URLs. Credential mutations have zero
  garbage-collection delay and reset after handling.
- Triggers: admin/editor create/edit/enable/disable/delete, exact published
  version selection, five-field cron/IANA timezone, overlap policy, JSON
  input, once-only webhook credentials, and signing/deduplication guidance.
  All members may inspect trigger metadata.
- New routes and navigation respect capabilities. Direct visits to admin
  routes also withhold their fetches and controls. Organization-keyed page
  instances prevent short-lived form/credential state crossing organizations.
- The overview now links to workflow creation, the catalog, and triggers.
  Palette items have keyboard-accessible Add actions alongside dragging;
  additions use the existing graph insertion functions and announce their
  results. Enter/Space insertion is covered for both nodes and blueprints.

## Production serving and dependency maintenance

`apps/web/Dockerfile` installs locked dependencies, compiles a standalone
Next server, and copies only runtime output/static assets to an unprivileged
Node image. `CORQTY_API_ORIGIN` is a build argument: Next rewrites are baked
into the build, so changing the internal API address requires rebuilding.
Both `/api/v1` and `/hooks/v1` proxy to that origin. The web server adds
clickjacking, MIME-sniffing, referrer, base-URL, and feature-policy headers.
Authenticated API fetches use `cache: no-store`.

Patched compatible Next, Vite, PostCSS, Undici and Vitest releases replace
vulnerable dependencies. The component-generation CLI is no longer a build
dependency; the seven CSS variants used by checked-in shadcn components are
local with the original MIT notice. Future component generation can use
`pnpm dlx shadcn@4.23.0` and review its output. `pnpm audit --audit-level low`
reports no known vulnerabilities at verification time.

## Verification

- The original SSE CI failure was reproduced on Node 24.22.2 and fixed by
  isolating native-fetch socket pools per test; see the execution-core plan
  follow-up. Assertion timeouts were unchanged.
- The suite has 18 files and 136 tests, including 12 new management
  interaction/RBAC tests, 3 catalog-creation tests, and keyboard palette
  insertion. The suite caps jsdom
  workers at 2 to avoid oversubscribing CI or a host also compiling Rust.
  All 136 passed on Node 24.22.2 (31.44 seconds) and the local Node 26 host
  (29.30 seconds).
- `pnpm web:lint`: passed, no diagnostics.
- `pnpm web:build`: passed, standalone output created with all new routes.
- `pnpm audit --audit-level low`: no known vulnerabilities.

## Live production acceptance

The standalone Compose build was exercised through the browser on localhost,
using a separate synthetic account and organization. Registration, secret
creation/replacement, sample WebAssembly Component upload, plugin disable and
re-enable, custom typed definition creation, one-time service-token dismissal,
and token revocation succeeded. The revoked record persisted after revisiting.
The browser console had no warnings or errors. Management layout was also
checked at a 390-pixel viewport, with scrollable navigation and readable forms.

After rebuilding the keyboard palette improvement, Enter added the custom
component node, autosave completed, publishing produced version 1, and a
manual run reached `succeeded` through the live event stream. Its node output
was `{"result":"hello from wasm"}`. An enabled daily cron pinned to version 1
with `Asia/Bangkok` displayed the correct next occurrence; disabling it
persisted. A disabled signed-webhook trigger returned its path and one-time
secret, and dismissing the secret removed it from the screen. Both synthetic
triggers were left disabled and the synthetic service token revoked.

The overview copy was updated after this browser build and verified by
TypeScript/lint; the final production rebuild includes it.
