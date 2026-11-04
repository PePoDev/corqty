# Development

Use the [documentation index](README.md) to find subsystem contracts. The
[architecture guide](architecture.md) maps crates and services; the
[frontend guide](frontend.md) maps the web application. Commands here run from
the repository root.

## Toolchain and dependencies

| Tool | Repository choice | Why it is needed |
|---|---|---|
| Rust | `1.96.0`, with clippy/rustfmt | Pinned in [rust-toolchain.toml](../rust-toolchain.toml); Cargo builds all Rust crates |
| Node.js | 24 in CI and Docker | Run frontend tooling using the same major as verification |
| pnpm | `10.33.2` | Pinned in [package.json](../package.json); use the committed lockfile |
| PostgreSQL | 18 | Application and test Compose images |
| Docker Compose | 2.24.4 or newer | Production override uses `!reset` |
| PowerShell | Required for smoke/audit scripts | Use `pwsh` outside Windows when running `.ps1` scripts |
| OpenSSL | Credential initializer on POSIX | Generates local database passwords and encryption key |

Install these tools through your platform's supported mechanism. A cold build
compiles Wasmtime and can take several minutes. On constrained machines use
`--jobs 2` for Cargo; the workspace already reduces dependency debug information.
Keep generated artifacts, credentials, and database dumps out of commits.

## Repository layout

```text
corqty/
  apps/web/                 Web application and frontend tests
  crates/                   Rust API, domain, persistence, workers, executors
  migrations/               Ordered forward SQL migrations
  plugins/example-wasm/     Sample source, manifest, and component binary
  docker/                   Dockerfiles, bootstrap, test/production overrides
  docker-compose.yml        Main application Compose entry point
  scripts/                  Initialization, smoke verification, Rust audit
  docs/                     Maintained Markdown documentation and visual assets
  .github/workflows/ci.yml   Linux verification pipeline
```

Do not recreate historical planning folders or use current user documentation
as a scratchpad. Document the behavior that is actually implemented.

## Fastest local setup

Follow the [root quick start](../README.md#start-locally). The initializer creates
`.env`; `docker compose up --build --wait` builds and starts the full application.
Use `docker compose ps` and `docker compose logs --tail=100 api worker web` to
inspect it. Use `docker compose down` to stop it while retaining named volumes.

To run the web app with live editing while using containerized API/workers:

```sh
docker compose up --build --wait postgres migrate api worker
pnpm install --frozen-lockfile
pnpm --filter @corqty/web dev
```

If the containerized web service already occupies port 3000, stop just that
service first with `docker compose stop web`. The development web proxy defaults
to `http://127.0.0.1:8080`. If you changed the API host port, supply the matching
`CORQTY_API_ORIGIN` to the web development process. Rebuild/restart the affected
service after changing build-time settings.

## Run Rust processes on the host

Start only the application database with `docker compose up -d --wait postgres`.
Use the generated credentials from your protected configuration. Supply the
following environment to the corresponding terminal, replacing Compose's
`postgres` hostname with the reachable host/port in database URLs:

| Process | Required environment | Additional local settings |
|---|---|---|
| Migrator | `DATABASE_URL_OWNER` | Owner URL for the application database |
| API | `DATABASE_URL_API`, `CORQTY_MASTER_KEY` | `CORQTY_SECURE_COOKIES=false` for local HTTP; public web URL matching the browser |
| Worker | `DATABASE_URL_WORKER`, same `CORQTY_MASTER_KEY` | Capacity and trusted-shell setting as needed |
| Web | None for default local proxy | `CORQTY_API_ORIGIN` if API is elsewhere |

Then run the migrator once, followed by each long-running service in its own
terminal:

```sh
cargo run -p corqty-migrator --jobs 2
cargo run -p corqty-api --jobs 2
cargo run -p corqty-worker --jobs 2
pnpm --filter @corqty/web dev
```

These are separate terminal commands, not a script that starts all services.
Rust does not load `.env` automatically. Do not mix application database URLs
with the disposable integration database on port 55432. The trusted shell
executor expects Linux `/bin/sh`; use the supplied Linux container for shell
behavior even when editing code on Windows.

## Making changes

1. Read the relevant guide, source, and tests. Identify public behavior,
   authorization, persistence, and deployment effects before editing.
2. Keep the existing boundaries: domain rules without HTTP/SQL, persistence for
   scoped transactions, API for transport, and SDK/host for executor contracts.
3. Add meaningful regression coverage for behavioral changes, particularly
   tenant access, leases, secret handling, immutable versions, and retries.
4. Update the affected guides, examples, source links, diagrams, and visual
   briefs in the same task. Follow the [agent requirements](../README.md#agent-documentation-requirements).
5. Run the appropriate [checks](testing.md), review the diff, and commit the
   completed task. Follow the repository owner's branch/push instructions.

Changes to public JSON require reviewing both Rust models and
[`apps/web/src/api/types.ts`](../apps/web/src/api/types.ts). Changes to graph
validation should preserve parity with the frontend graph model and shared
fixtures. New runtime capabilities require explicit denial tests and approval
semantics; adding a host function is a security boundary change.

## Database changes

Create a new ordered SQL migration in [migrations](../migrations). Never edit a
migration already applied to a deployed database: stored checksums detect
changes. Review roles, grants, RLS, foreign keys, indexes, audit behavior, and
security-definer function ownership together. The API and worker must not
receive the owner's credentials.

Migrations are embedded into Rust binaries. Rebuild the migrator after adding a
migration, run it before dependent services, and test against a restored copy
before upgrading a real environment. Integration tests reset all application
tables and reseed the system catalog; use only their disposable database.
See [data model](data-model.md) and [operations](operations.md).

## Dependencies and generated assets

- Preserve `Cargo.lock` and `pnpm-lock.yaml`; review and audit intentional updates.
- Run the guarded Rust audit rather than broadening its documented exception.
  See [dependency audit scope](security.md#dependency-audit-scope).
- Rebuild the committed WASM example with
  `cargo run -p corqty-plugin-host --example build-sample`; its tests compare
  binary and source. The files live in [plugins/example-wasm](../plugins/example-wasm).
- Screenshots must use synthetic tenant data and hide secret/token values.
  Follow the [visual asset briefs](assets/README.md); label unfinished images as
  placeholders instead of inventing UI.

The project is [Apache-2.0 licensed](../LICENSE). Retain third-party notices,
including the notice accompanying the vendored UI CSS, and record relevant
licenses when introducing dependencies. Security issues use the private
[reporting process](security.md#reporting-a-vulnerability).
