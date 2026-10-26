# Corqty

Generic visual workflow orchestration. Milestone 1 provides the multi-tenant
account, RBAC, audit, API, React shell, PostgreSQL, and Docker foundations.
Milestone 2 adds the workflow catalog and studio: versioned node definitions
and blueprints, a Next.js and XYFlow editor with typed connections and
automatic draft saves, and validated, immutable published versions.
Milestone 3 adds execution: manual runs scheduled in PostgreSQL, scalable
workers with the built-in mock and trusted shell executors, retries, leases
and cancellation, and live run views over server-sent events.

## Local start

```powershell
Copy-Item .env.example .env
docker compose up --build --wait
./scripts/smoke-foundation.ps1
```

Open http://localhost:3000 and create the first account and organization.
The host ports can be changed with `CORQTY_POSTGRES_PORT`,
`CORQTY_API_PORT`, and `CORQTY_WEB_PORT`.

## Workers

Each `worker` container claims node runs from PostgreSQL as the
`corqty_worker` role (`DATABASE_URL_WORKER`) and runs up to
`CORQTY_WORKER_CAPACITY` of them at once (4 by default). Workers publish no
port, so they scale freely:

```powershell
docker compose up --build --wait --scale worker=3
./scripts/smoke-foundation.ps1 -ExpectedWorkers 3
```

`docker compose stop worker` lets running node runs finish for up to 25
seconds. The API retries or fails the node runs of a worker that stops
renewing its leases, for example after a crash.

The shell executor is for trusted users only and is not a security
boundary. An organization admin enables it under Settings. Its commands run
inside the worker container as the worker's unprivileged user, with `bash`
and `jq` available. Commands that need other tools need a custom worker
image built from the `worker` target of `docker/rust/Dockerfile`.

The web service proxies `/api/v1` to the API through a Next.js rewrite. Run
event streams stay live through it because the API sends
`Cache-Control: no-cache, no-transform`: without `no-transform`, Next.js
would compress the stream and hold events back.

## Test

```powershell
docker compose -f docker-compose.test.yml up -d --wait
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm install --frozen-lockfile
pnpm web:lint
pnpm web:test
pnpm web:build
```

The test database is disposable:

```powershell
docker compose -f docker-compose.test.yml down -v
```

After upgrading to Milestone 2, recreate local databases once:
`docker compose down -v` and `docker compose -f docker-compose.test.yml down -v`.
