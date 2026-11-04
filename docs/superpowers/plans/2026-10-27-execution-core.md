# Corqty Milestone 3 Execution Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver Milestone 3 from the approved design. That means manual runs pinned to immutable versions, PostgreSQL scheduling with leases, fencing, retries and cancellation, horizontally scalable Rust workers with the built-in mock and trusted shell executors, replayable run events over SSE, and run monitoring in the web app.

**Architecture:** The API creates runs and their node runs in one tenant transaction. Workers reach the database only through a fixed set of `SECURITY DEFINER` functions, fenced by worker id and lease token, and never call the API. One SQL routine applies every node outcome atomically: it handles retries with backoff, releases children through a parent counter, skips descendants, and settles the run. A per-run, gapless event sequence feeds a `LISTEN/NOTIFY` fan-out to SSE streams that resume with `Last-Event-ID`. The API runs a leader-elected loop that recovers expired leases. Executors implement a small logical contract (`plugin-sdk`) and are registered in a checked compiled registry (`plugin-host`).

**Tech Stack:** Rust 1.96 / edition 2024, Axum 0.8 (SSE), Tokio, tokio-util, futures-util, SQLx 0.8 (`PgListener`), PostgreSQL 18, nix (process groups), tempfile, Next.js 16.4, React 19.3, TanStack Query 5, @xyflow/react 12, MSW 3, Vitest, Docker Compose (multi-stage, non-root images).

---

## Scope boundary

This plan implements Milestone 3 ("Execution core") of the approved design on top of Milestone 2 (`docs/superpowers/plans/2026-10-26-catalog-and-studio.md`). It excludes the following, which belong to Milestones 4 and 5:

- secrets storage and secret reads
- the WebAssembly host and runtime plugins
- service tokens and REST, cron and webhook triggers
- idempotency keys
- retention and rate limiting on authentication routes
- deployment-configurable limits

Secret bindings still cannot be published, and the shapes those milestones need are left room for: host-contract versions, trigger source, and the job payload.

Tasks 19 and 20 fix defects that an adversarial review found in Tasks 1-18. The most important is that the trusted shell executor now needs an operator switch (`CORQTY_SHELL_EXECUTOR=enabled`) before any organization admin can turn it on. Do not deploy a build that stops before Task 20.

Every section was produced by implementing the task test-first in a scratch clone and then replayed from its parent commit; the replayed tree matched the verified commit byte for byte. The `Expected:` lines quote real output from those runs. Tasks 15-17 were built in parallel with Tasks 1-14 and touch only `apps/web`, so the plan order 1..20 applies cleanly.

## Decisions settled for this milestone

- Executors are identified as `key@version` (`mock@1.0.0`, `shell@1.0.0`) everywhere.
  - The registry rejects duplicate executors, malformed keys or versions, a foreign host contract, and the `SecretRead` and `Network` capabilities (Milestone 4).
- The mock executor supports a fixed output, echo (`{"result": <inputs>}`), a configured permanent failure and `failAttempts` (retryable failures for the first N attempts).
- The trusted shell executor:
  - runs `/bin/sh -c <command>` in a per-attempt scratch directory with an environment allowlist;
  - reads its inputs from `CORQTY_INPUT` and writes its output to `CORQTY_OUTPUT`;
  - is stopped on cancel or timeout by signalling its process group. Processes that call `setsid` or daemonise are not stopped.

  It is not a security boundary, and it needs both the operator switch and an organization admin's opt-in.
- Node states:
  - Every state change follows spec §11.1, plus `retry_wait → canceled` and `retry_wait → running` (a due retry is claimed directly).
  - Attempts are consumed at claim. An expired lease records `lease_expired` and follows the same retry rule.
  - Backoff is computed in SQL: exponential from 2 s, capped at 5 minutes, with up to 20% jitter.
  - After a cancel request, nothing is retried or claimed, and failures end `canceled`.
  - A worker shutting down never counts as a cancel: its leases expire and are recovered.
- Lock order: a transaction that changes a run first locks the run row, then its node runs ordered by id. Only claims (`SKIP LOCKED`) and heartbeats are exempt.
- Run events:
  - They carry `seq` and `type` (`run.state`, `node.state`, `node.log`, `node.log_truncated`).
  - The SSE stream replays from `Last-Event-ID` or `?after=` and returns 204 when a finished run is already caught up.
  - It ends after the terminal event, on shutdown, or after 10 minutes.
  - At most 256 streams are open per API process, and at most 8 per user.
- Routes: `POST /workflows/{id}/runs`, `GET /runs/{id}`, `POST /runs/{id}/cancel`, `POST /runs/{id}/rerun`, `GET /runs/{id}/events`, `GET /settings` and `PUT /settings/shell`.
- Audit actions: `run.started`, `run.rerun`, `run.canceled`, `organization.shell_enabled` and `organization.shell_disabled`.
- Run monitoring (graph overlays, node details and live logs) happens on `/orgs/{org}/runs/{run}`, which also works below 1024 px. The studio's bottom drawer lists recent runs and streams the newest run's logs.
- Run input, node input and node output are each limited to 1 MiB. Logs are limited to 10 MiB per node run. These are fixed constants until Milestone 5.

## File map

### Database, infrastructure, and CI

- `migrations/202610260001_runs.sql`: runs, node_runs, run_events, worker_instances, RLS, `can_run`, `append_run_event`, `settle_run`, NOTIFY trigger.
- `migrations/202610260002_worker_functions.sql`: worker-only functions (`register_worker`, `claim_node_runs`, `start_node_run`, `heartbeat`, `append_node_log`, `complete_node_run`) and `finish_node_run`.
- `migrations/202610260003_shell_executor.sql`: `organizations.shell_enabled_at` and the shell preset seed.
- `migrations/202610260004_lease_recovery.sql`: `recover_expired_leases`.
- `migrations/202610270001_live_event_details.sql`: node input in terminal node events.
- `docker/rust/Dockerfile`: multi-stage release image with `migrate`, `api` and `worker` targets, running as uid 10001.
- `docker-compose.yml`, `.env.example`, `.github/workflows/ci.yml`, `scripts/smoke-foundation.ps1`: the worker service, scaling to 3 workers, the shell operator switch, and end-to-end run checks.
- `README.md` and the spec: running workers, shell trust, and the Milestone 3 decisions.

### Rust crates

- `crates/domain`: `run.rs` (states, transitions, aggregation, retry classification, run plans) and `runtime.rs` (input resolution, output validation, limits).
- `crates/plugin-sdk`: the logical executor contract.
- `crates/plugin-host`: the checked compiled registry.
- `crates/executor-mock`, `crates/executor-shell`: the built-in executors.
- `crates/persistence`: `runs.rs` (API side) and `worker_queue.rs` (worker side and lease recovery).
- `crates/api`:
  - graceful shutdown (`serve`)
  - `runs.rs` (run routes)
  - `events.rs` (SSE and the listener)
  - `loops.rs` (lease recovery)
  - organization settings
- `crates/worker`: claim loop, heartbeats, cancellation sources, log draining, the `healthcheck` subcommand.
- `crates/test-support`: owner, API and worker pools and their URLs.

### Web application (`apps/web`)

- `src/runs`: run dialog and input form, runs drawer, run view, run graph, log view, `useRun`/`useRunEvents`.
- `src/views/settings-view.tsx` and `src/app/(app)/orgs/[organizationId]/settings`: shell enablement with the trusted-user warning.
- `src/components/status`: run and node-run status badges.
- `src/test/event-source.ts`: EventSource polyfill for SSE tests.

## Task 1: Shut the API down gracefully

**Files:**

- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/api/Cargo.toml`
- Modify: `crates/api/src/lib.rs`
- Modify: `crates/api/src/main.rs`
- Modify: `crates/api/src/state.rs`
- Test: `crates/api/tests/shutdown.rs`

`docker compose stop` and every redeploy send SIGTERM, and the API installs no handler for it. Without one, SIGTERM
either kills the process in the middle of a request or, when the binary is PID 1 in a container without an init, is
ignored until Compose sends SIGKILL 10 seconds later. Milestone 3 also adds work that has to stop cleanly with the
process: the lease-recovery loop (Task 12) and the run-event listener and SSE streams (Task 13).

`AppState` gains `shutdown: CancellationToken` (tokio-util). `AppState::new` creates it, and clones of the state
share it. The new `corqty_api::serve(listener, state)` builds the router and runs `axum::serve` with
`with_graceful_shutdown(state.shutdown.clone().cancelled_owned())`. Once the token is cancelled, the server stops
accepting connections, lets in-flight requests finish, closes idle keep-alive connections, and returns. axum waits
with no time limit for open response bodies, so every long-lived response must also end on this token (Task 13's
SSE streams do). `main.rs` cancels the token on ctrl-c or, on unix, SIGTERM, and calls `serve`.

The test needs no database. The health route never touches the pool, so the test builds `AppState` on a lazy pool
that never connects, and it does not need `#[serial]`. It serves on `127.0.0.1:0` and sends `GET /api/v1/health`
over a raw TCP connection to prove the router is mounted and the server is still running. Then it cancels the token
and requires `serve` to return `Ok(())` within 2 seconds.

The workspace `tokio` dependency gains `io-util` (for the test's `AsyncReadExt` and `AsyncWriteExt`) and `time` (for
`tokio::time::timeout`). sqlx already compiles both features in, which is why the RED run fails only on the missing
`serve` and `shutdown`. Declaring them keeps the API crate independent of sqlx's feature set.

- [ ] **Step 1: Write the failing test**

Create `crates/api/tests/shutdown.rs`:

```rust
use std::time::Duration;

use corqty_api::{config::ApiConfig, serve, state::AppState};
use corqty_persistence::store::PgStore;
use sqlx::PgPool;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

#[tokio::test]
async fn serve_returns_once_the_shutdown_token_is_cancelled() {
    // The health route never touches the database, so the lazy pool never connects.
    let pool = PgPool::connect_lazy("postgres://corqty_api@127.0.0.1:55432/corqty_test").unwrap();
    let state = AppState::new(PgStore::new(pool), ApiConfig::for_tests());
    let shutdown = state.shutdown.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(listener, state));

    let mut stream = TcpStream::connect(address).await.unwrap();
    stream
        .write_all(b"GET /api/v1/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 204"), "{response}");
    assert!(!server.is_finished());

    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .expect("serve returns within 2 s")
        .unwrap()
        .unwrap();
}
```

- [ ] **Step 2: Run the test and verify RED**

```text
cargo test -p corqty-api --test shutdown
```

Expected: the test does not compile because neither `serve` nor the token exists yet:

```text
error[E0432]: unresolved import `corqty_api::serve`
 --> crates\api\tests\shutdown.rs:3:37
  |
3 | use corqty_api::{config::ApiConfig, serve, state::AppState};
  |                                     ^^^^^ no `serve` in the root

error[E0609]: no field `shutdown` on type `AppState`
  --> crates\api\tests\shutdown.rs:16:26
   |
16 |     let shutdown = state.shutdown.clone();
   |                          ^^^^^^^^ unknown field
   |
   = note: available fields are: `store`, `config`, `dummy_password_hash`
```

The last line is ``error: could not compile `corqty-api` (test "shutdown") due to 2 previous errors``.

- [ ] **Step 3: Add tokio-util and declare the tokio features the API uses**

In `Cargo.toml`:

Replace:

```toml
tokio = { version = "1", features = ["macros", "net", "rt-multi-thread", "signal"] }
```

With:

```toml
tokio = { version = "1", features = ["io-util", "macros", "net", "rt-multi-thread", "signal", "time"] }
tokio-util = "0.7"
```

Replace the whole of `crates/api/Cargo.toml`:

```toml
[package]
name = "corqty-api"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
argon2.workspace = true
axum.workspace = true
base64.workspace = true
chrono.workspace = true
corqty-domain = { path = "../domain" }
corqty-persistence = { path = "../persistence" }
getrandom.workspace = true
serde.workspace = true
serde_json.workspace = true
sha2.workspace = true
sqlx.workspace = true
thiserror.workspace = true
tokio.workspace = true
tokio-util.workspace = true
tower-http.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
uuid.workspace = true

[dev-dependencies]
corqty-test-support = { path = "../test-support" }
serial_test = "3"
tower.workspace = true

[lints]
workspace = true
```

The next cargo command (Step 7) updates `Cargo.lock`. It adds the package `tokio-util` 0.7.20, whose dependencies
(`bytes`, `futures-core`, `futures-sink`, `pin-project-lite`, `tokio`) are already locked, and adds `"tokio-util"` to
the `corqty-api` dependency list. No other version changes. If crates.io has a newer 0.7 release by then, pin the
verified one with `cargo update -p tokio-util --precise 0.7.20`.

- [ ] **Step 4: Give the application state a shutdown token**

In `crates/api/src/state.rs`:

Replace:

```rust
use corqty_persistence::store::PgStore;
```

With:

```rust
use corqty_persistence::store::PgStore;
use tokio_util::sync::CancellationToken;
```

Replace:

```rust
    pub dummy_password_hash: Arc<str>,
}
```

With:

```rust
    pub dummy_password_hash: Arc<str>,
    /// Cancelled on ctrl-c or SIGTERM; the server and long-lived tasks stop on it.
    pub shutdown: CancellationToken,
}
```

Replace:

```rust
                .into(),
        }
```

With:

```rust
                .into(),
            shutdown: CancellationToken::new(),
        }
```

- [ ] **Step 5: Serve until the token is cancelled**

In `crates/api/src/lib.rs`:

Replace:

```rust
pub mod workflows;
```

With:

```rust
pub mod workflows;

/// Serves the API on `listener` until `state.shutdown` is cancelled, then lets open
/// requests finish.
pub async fn serve(
    listener: tokio::net::TcpListener,
    state: state::AppState,
) -> std::io::Result<()> {
    let shutdown = state.shutdown.clone().cancelled_owned();
    axum::serve(listener, router::build_router(state))
        .with_graceful_shutdown(shutdown)
        .await
}
```

- [ ] **Step 6: Cancel the token on ctrl-c or SIGTERM**

The signal task logs once and cancels the token. `serve` then returns after the open requests finish, and `main`
exits with status 0. Windows has no SIGTERM, so there the second branch never resolves.

In `crates/api/src/main.rs`:

Replace:

```rust
use corqty_api::{config::ApiConfig, router::build_router, state::AppState};
```

With:

```rust
use corqty_api::{config::ApiConfig, serve, state::AppState};
```

Replace:

```rust
    axum::serve(
        listener,
        build_router(AppState::new(PgStore::new(pool), config)),
    )
    .await?;
    Ok(())
}
```

With:

```rust
    let state = AppState::new(PgStore::new(pool), config);
    let shutdown = state.shutdown.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        tracing::info!("shutting down: finishing open requests");
        shutdown.cancel();
    });
    serve(listener, state).await?;
    Ok(())
}

/// Resolves on ctrl-c, or on SIGTERM (what `docker compose stop` sends) on unix.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install the ctrl-c handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install the SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}
```

- [ ] **Step 7: Verify GREEN**

```text
cargo test -p corqty-api --test shutdown
```

Expected: 1 test passes (`serve_returns_once_the_shutdown_token_is_cancelled`) in about 0.2 s.

- [ ] **Step 8: Run the full backend verification**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo fmt --check
```

Expected: no output, exit code 0.

```text
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: `Finished` with no warnings.

```text
cargo test --workspace
```

Expected: every suite passes. corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 4,
`catalog_http` 4, `members_http` 3, `organizations_http` 1, `problems_http` 2, `publish_http` 2, `shutdown` 1,
`workflows_http` 4. corqty-domain: `account_and_rbac` 6, `catalog_rules` 7, `graph_rules` 8, `publish_rules` 10.
corqty-persistence: `auth_store` 2, `catalog_guards` 3, `final_admin` 2, `migrations` 1, `tenant_isolation` 1,
`test_reset` 1, `workflow_catalog` 6. 76 tests in total.

- [ ] **Step 9: Check the SIGTERM path on Linux**

The `#[cfg(unix)]` branch does not compile on Windows. Run clippy and the test in a Linux container. Then build the
binary, start it against the test database, wait for the health route, send SIGTERM and print the exit status. The
mount is read-only and `--locked` keeps cargo from rewriting the lockfile.

```text
docker run --rm -v "${PWD}:/work:ro" -w /work -e CARGO_TARGET_DIR=/tmp/target --add-host=host.docker.internal:host-gateway rust:1.96-bookworm bash -c 'cargo clippy --locked -p corqty-api --all-targets -- -D warnings && cargo test --locked -p corqty-api --test shutdown && cargo build --locked -p corqty-api && { DATABASE_URL_API=postgres://corqty_api:corqty_api_dev@host.docker.internal:55432/corqty_test CORQTY_BIND_ADDRESS=127.0.0.1:18080 /tmp/target/debug/corqty-api & for attempt in $(seq 50); do curl -sf -o /dev/null http://127.0.0.1:18080/api/v1/health && break; sleep 0.2; done; kill -TERM $!; wait $!; echo exit=$?; }'
```

Expected: clippy prints `Finished` with no warnings, `test serve_returns_once_the_shutdown_token_is_cancelled ... ok`,
a `finished processing request` line for the health check with `status=204`, then
`corqty_api: shutting down: finishing open requests` and `exit=0`.

- [ ] **Step 10: Commit**

```text
git add Cargo.toml Cargo.lock crates/api
git commit -m "feat: shut the API down gracefully"
```

Expected: `git show --stat HEAD` lists exactly the 7 files above (101 insertions, 7 deletions).

## Task 2: Build release images that run as a non-root user

**Files:**

- Modify: `docker/rust/Dockerfile`
- Modify: `docker-compose.yml`
- Test: `scripts/smoke-foundation.ps1`

Today `migrate` and `api` share one image, `FROM rust:1.96-bookworm`. It holds the toolchain and a debug `target/`
(about 5 GB) and runs as root. The `api` image starts through `cargo run`, and Compose overrides the migrate command
with `cargo run -p corqty-migrator`. Milestone 3 adds shell command execution (Task 7's executor, shipped in Task
18's worker image), and the spec requires an unprivileged container user (M3-55). Task 18 adds the worker target
to this file.

The new `docker/rust/Dockerfile` has several stages:

- `builder`: compiles release binaries of `corqty-api` and `corqty-migrator`. BuildKit cache mounts keep the cargo
  registry, the git checkouts and `target/` between builds. `target/` lives only in its cache mount, so the same
  `RUN` copies the binaries out to `/out`. The `# syntax=docker/dockerfile:1` line enables `RUN --mount`. Compose
  always builds with BuildKit, and so does CI. One builder stage serves every target, so `up --build` compiles once.
  The `build.rs` files in `crates/persistence` and `crates/test-support` still pick up migration-only changes when
  `target/` is cached. `rustfmt.toml` is no longer copied, because the build does not read it.
- `runtime`: `debian:bookworm-slim`, whose glibc matches the bookworm builder, plus the `corqty` system user and group
  with uid and gid 10001. There is no home directory and no `ca-certificates`: sqlx uses rustls with the
  `webpki-roots` certificates compiled in, and Compose connects to Postgres over plain TCP.
- `migrate`: the migrator binary, with `USER 10001:10001` and `ENTRYPOINT ["corqty-migrator"]`.
- `api`: curl for the existing Compose healthcheck, the API binary, `USER 10001:10001` and
  `ENTRYPOINT ["corqty-api"]`.

Both targets use `ENTRYPOINT`, so the migrate service's `command:` line is removed. If it were kept, its cargo
arguments would be passed to the migrator.

In `docker-compose.yml`, each Rust service selects its target with `build.target` and sets `init: true`. Docker's
init (tini) then runs as PID 1. It forwards `docker compose stop`'s SIGTERM to Task 1's handler and reaps orphaned
children, which Task 18's worker needs.

The test is a smoke check that reads each container's configured user with `docker inspect`. That works for the
exited `migrate` container as well as for `api`. On the old image the user is empty, which means root.

- [ ] **Step 1: Write the failing check**

In `scripts/smoke-foundation.ps1`:

Replace:

```powershell
    if ($entry.Health -and $entry.Health -ne "healthy") {
        throw "Unhealthy service: $service"
    }
}
```

With:

```powershell
    if ($entry.Health -and $entry.Health -ne "healthy") {
        throw "Unhealthy service: $service"
    }
}

foreach ($service in @("migrate", "api")) {
    $container = docker compose --project-name $ProjectName ps --all --quiet $service
    $user = docker inspect --format "{{.Config.User}}" $container
    if ($user -ne "10001:10001") {
        throw "The $service container runs as user '$user', not 10001:10001"
    }
}
```

- [ ] **Step 2: Run the check against the current images and verify RED**

Start a fresh stack from the unchanged Dockerfile and Compose file:

```text
docker compose --project-name corqty-m3t2 up --build --wait
```

Expected: exit code 0 after `Container corqty-m3t2-migrate-1 Exited`, `Container corqty-m3t2-api-1 Healthy` and
`Container corqty-m3t2-web-1 Healthy`. The build log shows the old debug build
(``Finished `dev` profile [unoptimized + debuginfo]``).

```text
docker compose --project-name corqty-m3t2 exec api id -u
```

Expected: `0`.

```text
./scripts/smoke-foundation.ps1 -ProjectName corqty-m3t2
```

In Windows PowerShell 5.1, run
`powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-foundation.ps1 -ProjectName corqty-m3t2`.

Expected: every earlier check passes. The script then stops at line 320 with:

```text
The migrate container runs as user '', not 10001:10001
```

```text
docker compose --project-name corqty-m3t2 down --volumes
```

Expected: `Volume corqty-m3t2_postgres-data Removed` and `Network corqty-m3t2_default Removed`.

- [ ] **Step 3: Build release binaries into slim images that run as uid 10001**

In `docker/rust/Dockerfile`:

Replace:

```dockerfile
FROM rust:1.96-bookworm
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /workspace
COPY Cargo.toml Cargo.lock rust-toolchain.toml rustfmt.toml ./
COPY crates ./crates
COPY migrations ./migrations
RUN cargo build --locked --bins
CMD ["cargo", "run", "--locked", "-p", "corqty-api"]
```

With:

```dockerfile
# syntax=docker/dockerfile:1
FROM rust:1.96-bookworm AS builder
WORKDIR /workspace
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY migrations ./migrations
# target/ lives in a cache mount, so the binaries are copied out in the same RUN.
RUN --mount=type=cache,id=corqty-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=corqty-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=corqty-target,target=/workspace/target,sharing=locked \
    cargo build --release --locked -p corqty-api -p corqty-migrator \
    && install -D -m 0755 -t /out target/release/corqty-api target/release/corqty-migrator

# The runtime glibc must match the builder's (bookworm).
FROM debian:bookworm-slim AS runtime
RUN groupadd --system --gid 10001 corqty \
    && useradd --system --uid 10001 --gid corqty --no-create-home --shell /usr/sbin/nologin corqty

FROM runtime AS migrate
COPY --from=builder /out/corqty-migrator /usr/local/bin/
USER 10001:10001
ENTRYPOINT ["corqty-migrator"]

FROM runtime AS api
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /out/corqty-api /usr/local/bin/
USER 10001:10001
ENTRYPOINT ["corqty-api"]
```

- [ ] **Step 4: Select the targets and run an init in Compose**

In `docker-compose.yml`:

Replace:

```yaml
      dockerfile: docker/rust/Dockerfile
    command: ["cargo", "run", "--locked", "-p", "corqty-migrator"]
```

With:

```yaml
      dockerfile: docker/rust/Dockerfile
      target: migrate
    init: true
```

Replace:

```yaml
      dockerfile: docker/rust/Dockerfile
    environment:
      DATABASE_URL_API: ${DATABASE_URL_API:-postgres://corqty_api:corqty_api_dev@postgres:5432/corqty}
```

With:

```yaml
      dockerfile: docker/rust/Dockerfile
      target: api
    init: true
    environment:
      DATABASE_URL_API: ${DATABASE_URL_API:-postgres://corqty_api:corqty_api_dev@postgres:5432/corqty}
```

- [ ] **Step 5: Verify GREEN on a fresh stack**

```text
docker compose config --quiet
docker compose --project-name corqty-m3t2 up --build --wait
```

Expected: `config` prints nothing. The build log shows one shared `[api builder 6/6]` step. Its first run prints
`info: syncing channel updates for 1.96.0-x86_64-unknown-linux-gnu`, because rustup installs the toolchain from
`rust-toolchain.toml`. It ends with ``Finished `release` profile [optimized] target(s)`` (about 35 s with a warm
registry cache). The images are built as `corqty-m3t2-migrate` (about 120 MB) and `corqty-m3t2-api` (about 155 MB).
The command exits 0 after `Container corqty-m3t2-migrate-1 Exited`, `Container corqty-m3t2-api-1 Healthy` and
`Container corqty-m3t2-web-1 Healthy`.

```text
docker compose --project-name corqty-m3t2 exec api id
docker compose --project-name corqty-m3t2 exec api cat /proc/1/comm
```

Expected: `uid=10001(corqty) gid=10001(corqty) groups=10001(corqty)` and `docker-init`.

```text
./scripts/smoke-foundation.ps1 -ProjectName corqty-m3t2
```

Expected: the script prints nothing and exits 0.

Check that a migrator rerun works through the entrypoint and that SIGTERM stops the API cleanly through the init:

```text
docker compose --project-name corqty-m3t2 run --rm migrate
docker compose --project-name corqty-m3t2 stop api
docker inspect --format "{{.State.ExitCode}}" corqty-m3t2-api-1
docker compose --project-name corqty-m3t2 logs api
```

Expected: the migrator prints only Compose's container lines and exits 0, because every migration is already
applied. `stop` returns in under a second. The exit code is `0`, and the last log line is
`corqty_api: shutting down: finishing open requests`.

```text
docker compose --project-name corqty-m3t2 down --volumes
```

Expected: `Volume corqty-m3t2_postgres-data Removed` and `Network corqty-m3t2_default Removed`.

- [ ] **Step 6: Run the full backend verification**

No Rust source changed, so these commands confirm the tree is still green.

```text
docker compose -f docker-compose.test.yml up -d --wait
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Expected: `postgres-test` reports `Healthy`. Formatting, Clippy and `git diff --check` exit 0 with no output beyond
`Finished`. Every suite passes, with the same 76 tests as after Task 1. corqty-api: unit 4, `audit_http` 1,
`auth_http` 3, `authorization_matrix` 4, `catalog_http` 4, `members_http` 3, `organizations_http` 1,
`problems_http` 2, `publish_http` 2, `shutdown` 1, `workflows_http` 4. corqty-domain: `account_and_rbac` 6,
`catalog_rules` 7, `graph_rules` 8, `publish_rules` 10. corqty-persistence: `auth_store` 2, `catalog_guards` 3,
`final_admin` 2, `migrations` 1, `tenant_isolation` 1, `test_reset` 1, `workflow_catalog` 6.

- [ ] **Step 7: Commit**

```text
git add docker/rust/Dockerfile docker-compose.yml scripts/smoke-foundation.ps1
git commit -m "feat: build release images that run as a non-root user"
```

Expected: `git show --stat HEAD` lists exactly the 3 files above (39 insertions, 8 deletions).

## Task 3: Model run states, retry classification, and run plans in the domain

**Files:**

- Create: `crates/domain/src/run.rs`
- Modify: `crates/domain/src/lib.rs`
- Test: `crates/domain/tests/run_rules.rs`

Milestone 3 runs workflows, and every later backend task needs the same vocabulary for it. This task adds it as one
pure module, `corqty_domain::run`, with no database and no async code. The SQL functions of Tasks 8, 9 and 12 decide
at run time; this module is the executable reference they are tested against.

- `NodeState` has the eight node states (`blocked`, `queued`, `running`, `retry_wait`, `succeeded`, `failed`,
  `canceled`, `skipped`), and `RunStatus` the five run states (`queued`, `running`, `succeeded`, `failed`,
  `canceled`). Both serialize as snake_case and have `as_str`, the text stored in `node_runs.state` and
  `runs.status`.
- `NodeState::can_transition(self, to)` allows the ten transitions of spec §11.1 plus two more: `retry_wait` to
  `canceled`, because a cancel also stops a retry that has not started yet, and `retry_wait` to `running`, because
  a worker claims a due retry directly instead of passing through `queued`. Every other pair of the 8 × 8 is
  rejected. `is_terminal` is true for `succeeded`, `failed`, `canceled` and `skipped`, and no transition leaves
  them.
- `aggregate(states, cancel_requested)` is the run's final status, or `None` while any node can still change. A
  failure wins, even over a cancel request. Otherwise a cancel request or any canceled node makes the run
  `canceled`, and otherwise it `succeeded`. Skipped nodes decide nothing: they only exist below a failed or
  canceled node. A graph with no nodes aggregates to `succeeded` at once.
- `is_retryable(code, executor_says)` follows the executor's own classification, except that the ten codes in
  `NON_RETRYABLE` (bad inputs and outputs, missing secrets or executors, denied permissions or capabilities) are
  never retried. A retry would fail the same way.
- `plan_run(graph, definitions)` returns the `node_runs` rows a new run starts with, in `topological_order`. Each
  `PlannedNode` copies `retry.maxAttempts` and `timeoutSeconds` from the node and takes its executor from the
  definition as `"<key>@<version>"` (for example `mock@1.0.0`), the form workers advertise. `children` holds the
  distinct target nodes of the node's edges, sorted. `pending_parents` counts the distinct nodes that feed this one,
  not the edges: a parent decrements each child once when it succeeds, so a parent wired into two ports of the
  same child must count once, or the child would stay `blocked` forever. Roots have `pending_parents` 0 and start
  `queued`.

`plan_run` returns `Option<Vec<PlannedNode>>`. It is `None` when the graph has a cycle, an edge to a node it does not
have, or a node whose definition is not in the map. Publish validation rules all three out, so a published version
always has a plan, but the caller gets a value to turn into an error instead of a panic. Retry backoff is not part
of this module: the database computes it when it schedules a retry (Task 9).

The tests rebuild the two seeded blueprints from migration `202610250001` with only the fields `plan_run` reads,
and give each definition only an executor.

- [ ] **Step 1: Write the failing tests**

Create `crates/domain/tests/run_rules.rs`:

```rust
use corqty_domain::{
    graph::GraphSnapshot,
    publish::Definitions,
    run::{NON_RETRYABLE, NodeState, PlannedNode, RunStatus, aggregate, is_retryable, plan_run},
};
use serde_json::{Value, json};

const STATES: [NodeState; 8] = [
    NodeState::Blocked,
    NodeState::Queued,
    NodeState::Running,
    NodeState::RetryWait,
    NodeState::Succeeded,
    NodeState::Failed,
    NodeState::Canceled,
    NodeState::Skipped,
];

/// Definitions that only differ in their executor, the one field `plan_run` reads.
fn definitions(keys: &[(&str, &str)]) -> Definitions {
    keys.iter()
        .map(|&(key, executor)| {
            let (executor_key, executor_version) = executor.split_once('@').unwrap();
            let spec = json!({
                "inputs": [],
                "outputs": [],
                "configSchema": {"type": "object"},
                "uiSchema": {},
                "defaults": {},
                "executor": {"key": executor_key, "version": executor_version}
            });
            ((key.to_owned(), 1), serde_json::from_value(spec).unwrap())
        })
        .collect()
}

/// A node using version 1 of the definition `key`.
fn node(id: &str, key: &str, max_attempts: u32, timeout_seconds: u32) -> Value {
    json!({
        "id": id,
        "definition": {"key": key, "version": 1},
        "displayName": id,
        "position": {"x": 0, "y": 0},
        "config": {},
        "inputs": {},
        "retry": {"maxAttempts": max_attempts},
        "timeoutSeconds": timeout_seconds
    })
}

/// An edge between two `node.port` ends.
fn edge(id: &str, source: &str, target: &str) -> Value {
    let end = |end: &str| {
        let (node, port) = end.split_once('.').unwrap();
        json!({"nodeId": node, "port": port})
    };
    json!({"id": id, "source": end(source), "target": end(target)})
}

fn graph(nodes: &[Value], edges: &[Value]) -> GraphSnapshot {
    serde_json::from_value(json!({
        "schemaVersion": 1,
        "workflowInputSchema": {"type": "object"},
        "nodes": nodes,
        "edges": edges,
        "viewport": {"x": 0, "y": 0, "zoom": 1}
    }))
    .unwrap()
}

fn planned(
    node_id: &str,
    executor: &str,
    max_attempts: i32,
    timeout_seconds: i32,
    children: &[&str],
    pending_parents: i32,
) -> PlannedNode {
    PlannedNode {
        node_id: node_id.to_owned(),
        executor: executor.to_owned(),
        max_attempts,
        timeout_seconds,
        children: children.iter().map(|&child| child.to_owned()).collect(),
        pending_parents,
    }
}

#[test]
fn node_transitions_follow_the_run_lifecycle() {
    use NodeState::{Blocked, Canceled, Failed, Queued, RetryWait, Running, Skipped, Succeeded};
    // Spec §11.1, plus retry_wait -> canceled (cancel stops a pending retry) and
    // retry_wait -> running (a due retry is claimed directly).
    let allowed = [
        (Blocked, Queued),
        (Blocked, Canceled),
        (Blocked, Skipped),
        (Queued, Running),
        (Queued, Canceled),
        (Running, Succeeded),
        (Running, Failed),
        (Running, RetryWait),
        (Running, Canceled),
        (RetryWait, Queued),
        (RetryWait, Running),
        (RetryWait, Canceled),
    ];
    for from in STATES {
        for to in STATES {
            assert_eq!(
                from.can_transition(to),
                allowed.contains(&(from, to)),
                "{from:?} -> {to:?}"
            );
        }
    }
}

#[test]
fn succeeded_failed_canceled_and_skipped_are_terminal() {
    for state in STATES {
        let terminal = matches!(
            state,
            NodeState::Succeeded | NodeState::Failed | NodeState::Canceled | NodeState::Skipped
        );
        assert_eq!(state.is_terminal(), terminal, "{state:?}");
        // Nothing leaves a terminal state.
        if terminal {
            assert!(
                STATES.iter().all(|&to| !state.can_transition(to)),
                "{state:?}"
            );
        }
    }
}

#[test]
fn states_and_statuses_are_snake_case() {
    for (state, text) in STATES.into_iter().zip([
        "blocked",
        "queued",
        "running",
        "retry_wait",
        "succeeded",
        "failed",
        "canceled",
        "skipped",
    ]) {
        assert_eq!(state.as_str(), text);
        assert_eq!(serde_json::to_value(state).unwrap(), json!(text));
        assert_eq!(
            serde_json::from_value::<NodeState>(json!(text)).unwrap(),
            state
        );
    }
    for (status, text) in [
        (RunStatus::Queued, "queued"),
        (RunStatus::Running, "running"),
        (RunStatus::Succeeded, "succeeded"),
        (RunStatus::Failed, "failed"),
        (RunStatus::Canceled, "canceled"),
    ] {
        assert_eq!(status.as_str(), text);
        assert_eq!(serde_json::to_value(status).unwrap(), json!(text));
        assert_eq!(
            serde_json::from_value::<RunStatus>(json!(text)).unwrap(),
            status
        );
    }
}

#[test]
fn runs_settle_only_once_every_node_is_terminal() {
    use NodeState::{Canceled, Failed, Skipped, Succeeded};
    for state in STATES.into_iter().filter(|state| !state.is_terminal()) {
        assert_eq!(aggregate(&[Failed, state], true), None, "{state:?}");
    }
    for (states, cancel_requested, status) in [
        // A failure wins over a cancel request and over canceled nodes.
        (&[Failed, Canceled, Succeeded][..], true, RunStatus::Failed),
        // Nodes skipped below a failure leave the run failed.
        (&[Succeeded, Failed, Skipped], false, RunStatus::Failed),
        (&[Succeeded, Succeeded], true, RunStatus::Canceled),
        (&[Succeeded, Canceled, Skipped], false, RunStatus::Canceled),
        (&[Succeeded, Succeeded], false, RunStatus::Succeeded),
        // A graph without nodes has nothing left to run.
        (&[], false, RunStatus::Succeeded),
    ] {
        assert_eq!(
            aggregate(states, cancel_requested),
            Some(status),
            "{states:?}, cancel requested: {cancel_requested}"
        );
    }
}

#[test]
fn listed_codes_are_never_retried_and_others_follow_the_executor() {
    assert_eq!(
        NON_RETRYABLE,
        [
            "input_missing",
            "input_invalid",
            "input_too_large",
            "output_invalid",
            "output_too_large",
            "secret_unavailable",
            "secret_output_detected",
            "executor_missing",
            "permission_denied",
            "capability_denied",
        ]
    );
    for code in NON_RETRYABLE {
        assert!(!is_retryable(code, true), "{code}");
        assert!(!is_retryable(code, false), "{code}");
    }
    for code in [
        "timeout",
        "lease_expired",
        "mock_retryable_failure",
        "shell_exit",
    ] {
        assert!(is_retryable(code, true), "{code}");
        assert!(!is_retryable(code, false), "{code}");
    }
}

#[test]
fn seeded_blueprints_plan_their_roots_queued_and_children_by_name() {
    let definitions = definitions(&[
        ("corqty.simulated.create-vpc", "mock@1.0.0"),
        ("corqty.simulated.create-rds", "mock@1.0.0"),
        ("corqty.simulated.deploy-service", "mock@1.0.0"),
        ("corqty.mock.normalize-payload", "mock@1.0.0"),
        ("corqty.mock.produce-summary", "mock@1.0.0"),
    ]);
    // The two blueprints of migration 202610250001, reduced to what `plan_run` reads.
    let infrastructure = graph(
        &[
            node("vpc", "corqty.simulated.create-vpc", 1, 900),
            node("rds", "corqty.simulated.create-rds", 1, 900),
            node("deploy", "corqty.simulated.deploy-service", 1, 900),
        ],
        &[
            edge("vpc-rds", "vpc.vpcId", "rds.vpcId"),
            edge("vpc-deploy", "vpc.vpcId", "deploy.vpcId"),
        ],
    );
    assert_eq!(
        plan_run(&infrastructure, &definitions),
        Some(vec![
            planned("vpc", "mock@1.0.0", 1, 900, &["deploy", "rds"], 0),
            planned("rds", "mock@1.0.0", 1, 900, &[], 1),
            planned("deploy", "mock@1.0.0", 1, 900, &[], 1),
        ])
    );

    let data_processing = graph(
        &[
            node("normalize", "corqty.mock.normalize-payload", 1, 900),
            node("summary", "corqty.mock.produce-summary", 1, 900),
        ],
        &[edge(
            "normalize-summary",
            "normalize.normalized",
            "summary.normalized",
        )],
    );
    assert_eq!(
        plan_run(&data_processing, &definitions),
        Some(vec![
            planned("normalize", "mock@1.0.0", 1, 900, &["summary"], 0),
            planned("summary", "mock@1.0.0", 1, 900, &[], 1),
        ])
    );
}

#[test]
fn a_parent_counts_once_however_many_of_its_edges_reach_a_child() {
    let definitions = definitions(&[("acme.split", "mock@1.0.0"), ("acme.merge", "shell@1.0.0")]);
    // The child is listed first; the plan still starts with its parents.
    let graph = graph(
        &[
            node("merge", "acme.merge", 3, 60),
            node("split", "acme.split", 2, 30),
            node("extra", "acme.split", 1, 900),
        ],
        &[
            edge("left", "split.left", "merge.left"),
            edge("right", "split.right", "merge.right"),
            edge("extra", "extra.left", "merge.extra"),
        ],
    );
    assert_eq!(
        plan_run(&graph, &definitions),
        Some(vec![
            planned("split", "mock@1.0.0", 2, 30, &["merge"], 0),
            planned("extra", "mock@1.0.0", 1, 900, &["merge"], 0),
            planned("merge", "shell@1.0.0", 3, 60, &[], 2),
        ])
    );
}

#[test]
fn graphs_that_cannot_publish_have_no_plan() {
    let definitions = definitions(&[("acme.split", "mock@1.0.0")]);
    let cycle = graph(
        &[
            node("a", "acme.split", 1, 900),
            node("b", "acme.split", 1, 900),
        ],
        &[edge("ab", "a.out", "b.in"), edge("ba", "b.out", "a.in")],
    );
    assert_eq!(plan_run(&cycle, &definitions), None);

    let unknown_definition = graph(&[node("a", "acme.unknown", 1, 900)], &[]);
    assert_eq!(plan_run(&unknown_definition, &definitions), None);
}
```

- [ ] **Step 2: Run the tests and verify RED**

```text
cargo test -p corqty-domain --test run_rules
```

Expected: the test does not compile because the module does not exist yet:

```text
error[E0432]: unresolved import `corqty_domain::run`
 --> crates\domain\tests\run_rules.rs:4:5
  |
4 |     run::{NON_RETRYABLE, NodeState, PlannedNode, RunStatus, aggregate, is_retryable, plan_run},
  |     ^^^ could not find `run` in `corqty_domain`
```

The last line is ``error: could not compile `corqty-domain` (test "run_rules") due to 1 previous error``.

- [ ] **Step 3: Add the run module**

Create `crates/domain/src/run.rs`:

```rust
use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::{
    graph::{GraphNode, GraphSnapshot, topological_order},
    publish::Definitions,
};

/// Error codes that are never retried, whatever the executor says: a bad input or output, a
/// missing secret or executor, or a denied permission fails the same way on every attempt.
pub const NON_RETRYABLE: &[&str] = &[
    "input_missing",
    "input_invalid",
    "input_too_large",
    "output_invalid",
    "output_too_large",
    "secret_unavailable",
    "secret_output_detected",
    "executor_missing",
    "permission_denied",
    "capability_denied",
];

/// Where one node of one run stands. Stored as the `node_runs.state` text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    Blocked,
    Queued,
    Running,
    RetryWait,
    Succeeded,
    Failed,
    Canceled,
    Skipped,
}

/// Where a whole run stands. Stored as the `runs.status` text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
}

impl NodeState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::RetryWait => "retry_wait",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
            Self::Skipped => "skipped",
        }
    }

    /// Whether a node may move from `self` to `to`: spec §11.1, plus `retry_wait` to
    /// `canceled` (a cancel stops a pending retry) and to `running` (a due retry is claimed
    /// directly). The SQL functions perform only these transitions.
    #[must_use]
    pub const fn can_transition(self, to: Self) -> bool {
        matches!(
            (self, to),
            (Self::Blocked, Self::Queued | Self::Canceled | Self::Skipped)
                | (Self::Queued, Self::Running | Self::Canceled)
                | (
                    Self::Running,
                    Self::Succeeded | Self::Failed | Self::RetryWait | Self::Canceled
                )
                | (
                    Self::RetryWait,
                    Self::Queued | Self::Running | Self::Canceled
                )
        )
    }

    /// Whether the node is done: no transition leaves these states.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Canceled | Self::Skipped
        )
    }
}

impl RunStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }
}

/// The status a run ends with, or `None` while any node can still change. A failure wins over
/// a cancel request; skipped nodes only sit below a failed or canceled node, so they decide
/// nothing. `corqty.settle_run` applies the same rule in the database.
#[must_use]
pub fn aggregate(states: &[NodeState], cancel_requested: bool) -> Option<RunStatus> {
    if !states.iter().all(|state| state.is_terminal()) {
        return None;
    }
    Some(if states.contains(&NodeState::Failed) {
        RunStatus::Failed
    } else if cancel_requested || states.contains(&NodeState::Canceled) {
        RunStatus::Canceled
    } else {
        RunStatus::Succeeded
    })
}

/// Whether a failed attempt may run again: the executor decides, except for the codes in
/// `NON_RETRYABLE`.
#[must_use]
pub fn is_retryable(code: &str, executor_says: bool) -> bool {
    executor_says && !NON_RETRYABLE.contains(&code)
}

/// The `node_runs` row a node starts a run with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedNode {
    pub node_id: String,
    /// The definition's executor as `"<key>@<version>"`, the form workers advertise.
    pub executor: String,
    pub max_attempts: i32,
    pub timeout_seconds: i32,
    /// The distinct nodes this node's edges feed, sorted.
    pub children: Vec<String>,
    /// The number of distinct nodes that feed this one. Each decrements it once when it
    /// succeeds, so two edges from one parent must count once. Roots start at 0, queued.
    pub pending_parents: i32,
}

/// The nodes a run of `graph` starts with, in `topological_order`. `None` when the graph has a
/// cycle, an edge to a node it lacks or a node whose definition is unknown, which publish
/// validation rules out.
#[must_use]
pub fn plan_run(graph: &GraphSnapshot, definitions: &Definitions) -> Option<Vec<PlannedNode>> {
    let links: Vec<(&str, &str)> = graph
        .edges
        .iter()
        .map(|edge| (edge.source.node_id.as_str(), edge.target.node_id.as_str()))
        .collect();
    let node_ids: Vec<&str> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
    let order = topological_order(&node_ids, &links)?;

    let nodes: HashMap<&str, &GraphNode> = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut children: HashMap<&str, Vec<String>> = HashMap::new();
    let mut pending_parents: HashMap<&str, i32> = HashMap::new();
    // Sorted and distinct, so each child is listed once, in order, and counts each parent once.
    for (source, target) in links.into_iter().collect::<BTreeSet<_>>() {
        children.entry(source).or_default().push(target.to_owned());
        *pending_parents.entry(target).or_default() += 1;
    }
    order
        .into_iter()
        .map(|id| {
            let node = nodes.get(id.as_str())?;
            let reference = (node.definition.key.clone(), node.definition.version);
            let executor = &definitions.get(&reference)?.executor;
            Some(PlannedNode {
                executor: format!("{}@{}", executor.key, executor.version),
                max_attempts: i32::try_from(node.retry.max_attempts).ok()?,
                timeout_seconds: i32::try_from(node.timeout_seconds).ok()?,
                children: children.remove(id.as_str()).unwrap_or_default(),
                pending_parents: pending_parents.get(id.as_str()).copied().unwrap_or(0),
                node_id: id,
            })
        })
        .collect()
}
```

Replace the whole of `crates/domain/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod account;
pub mod catalog;
pub mod diagnostic;
pub mod error;
pub mod graph;
pub mod publish;
pub mod rbac;
pub mod run;
pub mod schema;
pub mod slug;
```

- [ ] **Step 4: Verify GREEN**

```text
cargo test -p corqty-domain --test run_rules
```

Expected: 8 tests pass: `node_transitions_follow_the_run_lifecycle`,
`succeeded_failed_canceled_and_skipped_are_terminal`, `states_and_statuses_are_snake_case`,
`runs_settle_only_once_every_node_is_terminal`, `listed_codes_are_never_retried_and_others_follow_the_executor`,
`seeded_blueprints_plan_their_roots_queued_and_children_by_name`,
`a_parent_counts_once_however_many_of_its_edges_reach_a_child` and `graphs_that_cannot_publish_have_no_plan`.

- [ ] **Step 5: Run the full backend verification**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo fmt --check
```

Expected: no output, exit code 0.

```text
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: `Finished` with no warnings.

```text
cargo test --workspace
```

Expected: every suite passes. corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 4,
`catalog_http` 4, `members_http` 3, `organizations_http` 1, `problems_http` 2, `publish_http` 2, `shutdown` 1,
`workflows_http` 4. corqty-domain: `account_and_rbac` 6, `catalog_rules` 7, `graph_rules` 8, `publish_rules` 10,
`run_rules` 8. corqty-persistence: `auth_store` 2, `catalog_guards` 3, `final_admin` 2, `migrations` 1,
`tenant_isolation` 1, `test_reset` 1, `workflow_catalog` 6. 84 tests in total.

- [ ] **Step 6: Commit**

```text
git add crates/domain
git commit -m "feat: model run states, retry classification, and run plans in the domain"
```

Expected: `git show --stat HEAD` lists exactly the 3 files above (508 insertions).

## Task 4: Resolve node inputs and validate outputs at run time

**Files:**

- Create: `crates/domain/src/runtime.rs`
- Modify: `crates/domain/src/lib.rs`
- Test: `crates/domain/tests/runtime_rules.rs`

Publish validation checks a graph once, against its schemas. At run time a worker still has to build each node's
actual input object and check what the executor returns. This task adds both steps to one pure module,
`corqty_domain::runtime`, so the worker (Task 14) only calls two functions. The module also holds the Milestone 3
size limits that later tasks share.

`resolve_inputs(node, spec, graph, run_input, upstream_outputs)` builds the inputs object, one entry per input port
of the node's definition, in port order:

- If an edge feeds the port, the value is the edge's source port in `upstream_outputs` (parent node id → that
  parent's output object).
- Otherwise the node's binding decides. A `literal` gives its value. A `workflowInput` pointer is evaluated against
  the run input with `Value::pointer`, which reads RFC 6901 escapes (`~1`, `~0`) the same way publish's pointer check
  does. A `secret` binding fails with `secret_unavailable`: workers cannot read secrets until Milestone 4, and
  publish already rejects secret bindings.
- When a port has no value (no binding, a pointer that finds nothing, a parent output without that port, or no
  parent output at all), an optional port is left out of the object and is never set to `null`. A required port
  fails the node with `input_missing`.
- `null` counts as a value. The port's schema decides whether it passes, as spec §8.2 asks.

The whole object is then measured as compact JSON. More than `MAX_NODE_INPUT_BYTES` (1 MiB) fails with
`input_too_large`. This check runs before schema validation, so validation never works through more than one
limit's worth of JSON. Last, each value is checked against its port's full schema with `schema::compile` and
`instance_errors`. There is no coercion, so the string `"20"` is not a number. A violation fails with
`input_invalid`, and the message gives the first problem at `/<port><path>` in the masked form `instance_errors`
produces, so it never repeats the value.

`validate_output(spec, output)` checks the executor's result in this order:

1. More than `MAX_NODE_OUTPUT_BYTES` of JSON fails with `output_too_large`.
2. A key that is not a declared output port fails with `output_invalid`.
3. Any U+0000, in a nested key or in a string at any depth, fails with `output_invalid`. PostgreSQL's jsonb cannot
   store that character, so a completion carrying it would fail on every attempt.
4. Each value must match its port's schema (`output_invalid`).

Declared outputs may be missing. A missing output fails only the required input it feeds, through `input_missing`
on the downstream node.

Both functions return `NodeFailure { code, message }`. Every code they produce is in `run::NON_RETRYABLE`, and the
tests' `failure` helper asserts it, so the worker never retries a failure that would repeat. Port names need no
pointer escaping because `validate_spec` restricts them to letters, digits and underscores.

The limits follow spec §15 and stay fixed until Milestone 5 (`// ponytail:`). Besides the two above,
`MAX_RUN_INPUT_BYTES` is for run creation (Task 10), and `MAX_LOG_BYTES_PER_NODE_RUN` (10 MiB) is for the shell
executor, the log functions and the worker (Tasks 7, 9 and 14).

- [ ] **Step 1: Write the failing tests**

Create `crates/domain/tests/runtime_rules.rs`:

```rust
use std::{collections::HashMap, fmt::Debug};

use corqty_domain::{
    catalog::NodeDefinitionSpec,
    graph::GraphSnapshot,
    run::NON_RETRYABLE,
    runtime::{
        MAX_LOG_BYTES_PER_NODE_RUN, MAX_NODE_INPUT_BYTES, MAX_NODE_OUTPUT_BYTES,
        MAX_RUN_INPUT_BYTES, NodeFailure, resolve_inputs, validate_output,
    },
};
use serde_json::{Map, Value, json};

/// A database that needs a VPC id (from an edge), a region, a size and a nullable label, and
/// takes optional notes (from an edge) and an optional zone. It reports an endpoint, a port
/// and free-form details.
fn spec() -> NodeDefinitionSpec {
    serde_json::from_value(json!({
        "inputs": [
            {
                "name": "vpcId",
                "dataType": "aws.vpc-id",
                "schema": {"type": "string", "pattern": "^vpc-"},
                "required": true
            },
            {
                "name": "region",
                "dataType": "core.string",
                "schema": {"type": "string", "minLength": 1},
                "required": true
            },
            {"name": "size", "dataType": "core.number", "schema": {"type": "number"}, "required": true},
            {
                "name": "label",
                "dataType": "core.string",
                "schema": {"type": ["string", "null"]},
                "required": true
            },
            {"name": "notes", "dataType": "core.any", "schema": {}},
            {"name": "zone", "dataType": "core.string", "schema": {"type": "string"}}
        ],
        "outputs": [
            {"name": "endpoint", "dataType": "core.string", "schema": {"type": "string"}},
            {"name": "port", "dataType": "core.integer", "schema": {"type": "integer", "minimum": 1}},
            {"name": "details", "dataType": "core.object", "schema": {"type": "object"}}
        ],
        "configSchema": {"type": "object"},
        "uiSchema": {},
        "defaults": {},
        "executor": {"key": "mock", "version": "1.0.0"}
    }))
    .unwrap()
}

/// The bindings of a node that resolves: a nested workflow input, a number literal and a
/// `null` literal for the nullable label.
fn bindings() -> Value {
    json!({
        "region": {"kind": "workflowInput", "pointer": "/network/region"},
        "size": {"kind": "literal", "value": 20},
        "label": {"kind": "literal", "value": null}
    })
}

/// Resolves the inputs of node `db`, whose `vpcId` and `notes` are fed by edges from the
/// `vpcId` and `tags` outputs of node `vpc`. `vpc_output` is `None` when `vpc` has no output.
fn resolve(
    bindings: Value,
    run_input: Value,
    vpc_output: Option<Value>,
) -> Result<Map<String, Value>, NodeFailure> {
    let node = |id: &str, key: &str, inputs: Value| {
        json!({
            "id": id,
            "definition": {"key": key, "version": 1},
            "displayName": id,
            "position": {"x": 0, "y": 0},
            "config": {},
            "inputs": inputs,
            "retry": {"maxAttempts": 1},
            "timeoutSeconds": 900
        })
    };
    let edge = |id: &str, output: &str, input: &str| {
        json!({
            "id": id,
            "source": {"nodeId": "vpc", "port": output},
            "target": {"nodeId": "db", "port": input}
        })
    };
    let graph: GraphSnapshot = serde_json::from_value(json!({
        "schemaVersion": 1,
        "workflowInputSchema": {"type": "object"},
        "nodes": [node("vpc", "acme.vpc", json!({})), node("db", "acme.db", bindings)],
        "edges": [edge("vpc-id", "vpcId", "vpcId"), edge("vpc-tags", "tags", "notes")],
        "viewport": {"x": 0, "y": 0, "zoom": 1}
    }))
    .unwrap();
    let upstream_outputs: HashMap<String, Map<String, Value>> = vpc_output
        .map(|output| ("vpc".to_owned(), serde_json::from_value(output).unwrap()))
        .into_iter()
        .collect();
    resolve_inputs(
        &graph.nodes[1],
        &spec(),
        &graph,
        &run_input,
        &upstream_outputs,
    )
}

fn object(value: Value) -> Map<String, Value> {
    serde_json::from_value(value).unwrap()
}

/// The failure in `result`, whose code is never retried.
fn failure<T: Debug>(result: Result<T, NodeFailure>) -> NodeFailure {
    let failure = result.unwrap_err();
    assert!(NON_RETRYABLE.contains(&failure.code), "{failure:?}");
    failure
}

#[test]
fn edges_literals_and_workflow_input_pointers_resolve_to_their_values() {
    let mut bindings = bindings();
    // An RFC 6901 escape: `~1` stands for `/`.
    bindings["zone"] = json!({"kind": "workflowInput", "pointer": "/placement~1zone"});
    let run_input = json!({"network": {"region": "eu-west-1"}, "placement/zone": "eu-west-1a"});
    let vpc_output = json!({"vpcId": "vpc-0a1", "tags": {"team": "data"}, "unused": 1});
    assert_eq!(
        resolve(bindings, run_input, Some(vpc_output)).unwrap(),
        object(json!({
            "vpcId": "vpc-0a1",
            "region": "eu-west-1",
            "size": 20,
            "label": null,
            "notes": {"team": "data"},
            "zone": "eu-west-1a"
        }))
    );
}

#[test]
fn optional_inputs_without_a_value_are_omitted_not_null() {
    let run_input = json!({"network": {"region": "eu-west-1"}});
    // `zone` is unbound and the edge into `notes` finds no `tags` output.
    let inputs = resolve(
        bindings(),
        run_input.clone(),
        Some(json!({"vpcId": "vpc-0a1"})),
    )
    .unwrap();
    assert_eq!(inputs.get("notes"), None);
    assert_eq!(inputs.get("zone"), None);

    // A pointer to a property the run input lacks.
    let mut bindings = bindings();
    bindings["zone"] = json!({"kind": "workflowInput", "pointer": "/zone"});
    let inputs = resolve(bindings, run_input, Some(json!({"vpcId": "vpc-0a1"}))).unwrap();
    assert_eq!(inputs.get("zone"), None);
    assert_eq!(inputs.len(), 4);
}

#[test]
fn null_passes_only_where_the_port_schema_allows_it() {
    let vpc_output = || Some(json!({"vpcId": "vpc-0a1"}));
    let run_input = json!({"network": {"region": "eu-west-1"}});
    let inputs = resolve(bindings(), run_input.clone(), vpc_output()).unwrap();
    assert_eq!(inputs.get("label"), Some(&Value::Null));

    let mut bindings_with_null_size = bindings();
    bindings_with_null_size["size"]["value"] = Value::Null;
    let failed = failure(resolve(bindings_with_null_size, run_input, vpc_output()));
    assert_eq!(failed.code, "input_invalid");

    // A null in the run input is a value, not a missing one.
    let null_region = json!({"network": {"region": null}});
    let failed = failure(resolve(bindings(), null_region, vpc_output()));
    assert_eq!(failed.code, "input_invalid");
}

#[test]
fn required_inputs_without_a_value_fail_as_input_missing() {
    let run_input = json!({"network": {"region": "eu-west-1"}});
    let vpc_output = || Some(json!({"vpcId": "vpc-0a1"}));
    let mut unbound_size = bindings();
    unbound_size.as_object_mut().unwrap().remove("size");
    for (bindings, run_input, vpc_output, port) in [
        // The pointer finds nothing in the run input.
        (bindings(), json!({"network": {}}), vpc_output(), "region"),
        // The upstream output lacks the port the edge reads.
        (bindings(), run_input.clone(), Some(json!({})), "vpcId"),
        // The upstream node has no output at all.
        (bindings(), run_input.clone(), None, "vpcId"),
        // Nothing binds the port.
        (unbound_size, run_input, vpc_output(), "size"),
    ] {
        let failed = failure(resolve(bindings, run_input, vpc_output));
        assert_eq!(failed.code, "input_missing", "{port}");
        assert!(failed.message.contains(port), "{failed:?}");
    }
}

#[test]
fn values_that_break_the_port_schema_fail_as_input_invalid() {
    let run_input = json!({"network": {"region": "eu-west-1"}});
    // An upstream value is checked against the target port's full schema.
    let failed = failure(resolve(
        bindings(),
        run_input.clone(),
        Some(json!({"vpcId": "subnet-0a1"})),
    ));
    assert_eq!(failed.code, "input_invalid");
    assert!(failed.message.contains("/vpcId"), "{failed:?}");
    // The message never repeats the value.
    assert!(!failed.message.contains("subnet-0a1"), "{failed:?}");

    // No coercion: the string "20" is not a number.
    let mut string_size = bindings();
    string_size["size"]["value"] = json!("20");
    let failed = failure(resolve(
        string_size,
        run_input,
        Some(json!({"vpcId": "vpc-0a1"})),
    ));
    assert_eq!(failed.code, "input_invalid");
    assert!(failed.message.contains("/size"), "{failed:?}");
}

#[test]
fn resolved_inputs_over_one_mebibyte_fail_as_input_too_large() {
    let run_input = json!({"network": {"region": "eu-west-1"}});
    let with_notes = |notes: String| {
        resolve(
            bindings(),
            run_input.clone(),
            Some(json!({"vpcId": "vpc-0a1", "tags": notes})),
        )
    };
    let empty = serde_json::to_string(&with_notes(String::new()).unwrap()).unwrap();
    let fits = "a".repeat(MAX_NODE_INPUT_BYTES - empty.len());
    let inputs = with_notes(fits).unwrap();
    assert_eq!(
        serde_json::to_string(&inputs).unwrap().len(),
        MAX_NODE_INPUT_BYTES
    );

    let too_long = "a".repeat(MAX_NODE_INPUT_BYTES - empty.len() + 1);
    assert_eq!(failure(with_notes(too_long)).code, "input_too_large");
}

#[test]
fn secret_bindings_fail_as_secret_unavailable() {
    let mut bindings = bindings();
    bindings["zone"] =
        json!({"kind": "secret", "secretId": "0198c2a0-0000-7000-8000-000000000001"});
    let run_input = json!({"network": {"region": "eu-west-1"}});
    let failed = failure(resolve(
        bindings,
        run_input,
        Some(json!({"vpcId": "vpc-0a1"})),
    ));
    assert_eq!(failed.code, "secret_unavailable");
    assert!(failed.message.contains("zone"), "{failed:?}");
}

#[test]
fn declared_outputs_that_match_their_schemas_pass() {
    for output in [
        json!({"endpoint": "db.internal", "port": 5432, "details": {"engine": "postgres"}}),
        // Outputs are not required: a missing one only fails a required input it feeds.
        json!({"endpoint": "db.internal"}),
        json!({}),
    ] {
        assert_eq!(
            validate_output(&spec(), &object(output.clone())),
            Ok(()),
            "{output}"
        );
    }
}

#[test]
fn undeclared_keys_and_schema_violations_fail_as_output_invalid() {
    for (output, mention) in [
        (json!({"endpoint": "db.internal", "extra": 1}), "extra"),
        (json!({"port": 0}), "/port"),
        // No coercion: the string "5432" is not an integer.
        (json!({"port": "5432"}), "/port"),
    ] {
        let failed = failure(validate_output(&spec(), &object(output)));
        assert_eq!(failed.code, "output_invalid", "{mention}");
        assert!(failed.message.contains(mention), "{failed:?}");
    }
}

#[test]
fn strings_with_nul_characters_fail_as_output_invalid() {
    // PostgreSQL cannot store U+0000 in jsonb, in a value or in a key, at any depth.
    for output in [
        json!({"endpoint": "db\u{0}internal"}),
        json!({"details": {"tags": ["ok", "\u{0}"]}}),
        json!({"details": {"key\u{0}": 1}}),
    ] {
        let failed = failure(validate_output(&spec(), &object(output.clone())));
        assert_eq!(failed.code, "output_invalid", "{output}");
        assert!(failed.message.contains("U+0000"), "{failed:?}");
    }
}

#[test]
fn outputs_over_one_mebibyte_fail_as_output_too_large() {
    let output = |endpoint: String| object(json!({"endpoint": endpoint}));
    let empty = serde_json::to_string(&output(String::new())).unwrap();
    let fits = output("a".repeat(MAX_NODE_OUTPUT_BYTES - empty.len()));
    assert_eq!(
        serde_json::to_string(&fits).unwrap().len(),
        MAX_NODE_OUTPUT_BYTES
    );
    assert_eq!(validate_output(&spec(), &fits), Ok(()));

    let too_large = output("a".repeat(MAX_NODE_OUTPUT_BYTES - empty.len() + 1));
    assert_eq!(
        failure(validate_output(&spec(), &too_large)).code,
        "output_too_large"
    );
}

#[test]
fn limits_match_the_spec() {
    // Spec §15: 1 MiB run input, resolved node input and node output; 10 MiB of logs per node run.
    assert_eq!(MAX_RUN_INPUT_BYTES, 1_048_576);
    assert_eq!(MAX_NODE_INPUT_BYTES, 1_048_576);
    assert_eq!(MAX_NODE_OUTPUT_BYTES, 1_048_576);
    assert_eq!(MAX_LOG_BYTES_PER_NODE_RUN, 10_485_760);
}
```

- [ ] **Step 2: Run the tests and verify RED**

```text
cargo test -p corqty-domain --test runtime_rules
```

Expected: the test does not compile because the module does not exist yet:

```text
error[E0432]: unresolved import `corqty_domain::runtime`
 --> crates\domain\tests\runtime_rules.rs:7:5
  |
7 |     runtime::{
  |     ^^^^^^^ could not find `runtime` in `corqty_domain`
```

The last line is ``error: could not compile `corqty-domain` (test "runtime_rules") due to 1 previous error``.

- [ ] **Step 3: Add the runtime module**

Create `crates/domain/src/runtime.rs`:

```rust
use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::{
    catalog::{NodeDefinitionSpec, PortSpec},
    graph::{GraphNode, GraphSnapshot, InputBinding},
    schema::{compile, instance_errors},
};

// ponytail: fixed limits until Milestone 5 makes them deployment configuration.
/// The largest run input, as serialized JSON.
pub const MAX_RUN_INPUT_BYTES: usize = 1_048_576;
/// The largest inputs object one node gets, as serialized JSON.
pub const MAX_NODE_INPUT_BYTES: usize = 1_048_576;
/// The largest output object one node may return, as serialized JSON.
pub const MAX_NODE_OUTPUT_BYTES: usize = 1_048_576;
/// The most log text kept for one node run; one `node.log_truncated` event marks the cut.
pub const MAX_LOG_BYTES_PER_NODE_RUN: usize = 10_485_760;

/// Why a node run fails around its executor. Every code is in `run::NON_RETRYABLE`, and the
/// message never repeats a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeFailure {
    pub code: &'static str,
    pub message: String,
}

impl NodeFailure {
    fn new(code: &'static str, message: String) -> Self {
        Self { code, message }
    }
}

/// The inputs object `node`'s executor gets. Each input port of `spec` takes the output its
/// edge reads from `upstream_outputs`, else its binding: a literal, or a JSON Pointer into
/// `run_input`. A port without a value is left out when optional; null is a value. Every
/// value must match its port's schema as it is, without coercion.
pub fn resolve_inputs(
    node: &GraphNode,
    spec: &NodeDefinitionSpec,
    graph: &GraphSnapshot,
    run_input: &Value,
    upstream_outputs: &HashMap<String, Map<String, Value>>,
) -> Result<Map<String, Value>, NodeFailure> {
    let mut inputs = Map::new();
    for port in &spec.inputs {
        let edge = graph
            .edges
            .iter()
            .find(|edge| edge.target.node_id == node.id && edge.target.port == port.name);
        let value = match (edge, node.inputs.get(&port.name)) {
            (Some(edge), _) => upstream_outputs
                .get(&edge.source.node_id)
                .and_then(|output| output.get(&edge.source.port)),
            (None, Some(InputBinding::Literal { value })) => Some(value),
            (None, Some(InputBinding::WorkflowInput { pointer })) => run_input.pointer(pointer),
            // ponytail: no secrets until M4, and publish rejects secret bindings until then.
            (None, Some(InputBinding::Secret { .. })) => {
                return Err(NodeFailure::new(
                    "secret_unavailable",
                    format!(
                        "The input \"{}\" needs a secret, which workers cannot read yet.",
                        port.name
                    ),
                ));
            }
            (None, None) => None,
        };
        match value {
            Some(value) => {
                inputs.insert(port.name.clone(), value.clone());
            }
            None if port.required => {
                return Err(NodeFailure::new(
                    "input_missing",
                    format!("The required input \"{}\" has no value.", port.name),
                ));
            }
            None => {}
        }
    }
    // Before validation, so validating costs at most one limit's worth of JSON.
    if json_bytes(&inputs) > MAX_NODE_INPUT_BYTES {
        return Err(NodeFailure::new(
            "input_too_large",
            format!("The inputs have more than {MAX_NODE_INPUT_BYTES} bytes of JSON."),
        ));
    }
    let problem = spec
        .inputs
        .iter()
        .find_map(|port| schema_problem("input", port, inputs.get(&port.name)?));
    match problem {
        Some(problem) => Err(NodeFailure::new("input_invalid", problem)),
        None => Ok(inputs),
    }
}

/// Checks what an executor returned: at most `MAX_NODE_OUTPUT_BYTES` of JSON, only declared
/// output ports, no U+0000 anywhere (jsonb cannot store it), and each value matching its
/// port's schema. Declared outputs may be missing.
pub fn validate_output(
    spec: &NodeDefinitionSpec,
    output: &Map<String, Value>,
) -> Result<(), NodeFailure> {
    if json_bytes(output) > MAX_NODE_OUTPUT_BYTES {
        return Err(NodeFailure::new(
            "output_too_large",
            format!("The output has more than {MAX_NODE_OUTPUT_BYTES} bytes of JSON."),
        ));
    }
    if let Some(name) = output
        .keys()
        .find(|name| !spec.outputs.iter().any(|port| &port.name == *name))
    {
        return Err(NodeFailure::new(
            "output_invalid",
            format!(
                "The node definition has no output \"{}\".",
                name.replace('\0', "\u{fffd}")
            ),
        ));
    }
    // Declared names never hold U+0000; nested keys and strings might.
    if output.values().any(has_nul) {
        return Err(NodeFailure::new(
            "output_invalid",
            "The output contains the character U+0000.".to_owned(),
        ));
    }
    let problem = spec
        .outputs
        .iter()
        .find_map(|port| schema_problem("output", port, output.get(&port.name)?));
    match problem {
        Some(problem) => Err(NodeFailure::new("output_invalid", problem)),
        None => Ok(()),
    }
}

/// The first way `value` breaks `port`'s schema, located by a pointer that starts at the
/// port's name (port names need no escaping). The message masks the value.
fn schema_problem(side: &str, port: &PortSpec, value: &Value) -> Option<String> {
    let Ok(validator) = compile(&port.schema) else {
        return Some(format!(
            "The schema of the {side} /{} does not compile.",
            port.name
        ));
    };
    let (path, message) = instance_errors(&validator, value).into_iter().next()?;
    Some(format!(
        "The {side} at /{}{path} does not match its schema: {message}.",
        port.name
    ))
}

fn json_bytes(object: &Map<String, Value>) -> usize {
    serde_json::to_vec(object)
        .expect("a JSON object always serializes")
        .len()
}

fn has_nul(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains('\0'),
        Value::Array(items) => items.iter().any(has_nul),
        Value::Object(object) => object
            .iter()
            .any(|(key, value)| key.contains('\0') || has_nul(value)),
        _ => false,
    }
}
```

Replace the whole of `crates/domain/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod account;
pub mod catalog;
pub mod diagnostic;
pub mod error;
pub mod graph;
pub mod publish;
pub mod rbac;
pub mod run;
pub mod runtime;
pub mod schema;
pub mod slug;
```

- [ ] **Step 4: Verify GREEN**

```text
cargo test -p corqty-domain --test runtime_rules
```

Expected: 12 tests pass: `edges_literals_and_workflow_input_pointers_resolve_to_their_values`,
`optional_inputs_without_a_value_are_omitted_not_null`, `null_passes_only_where_the_port_schema_allows_it`,
`required_inputs_without_a_value_fail_as_input_missing`, `values_that_break_the_port_schema_fail_as_input_invalid`,
`resolved_inputs_over_one_mebibyte_fail_as_input_too_large`, `secret_bindings_fail_as_secret_unavailable`,
`declared_outputs_that_match_their_schemas_pass`, `undeclared_keys_and_schema_violations_fail_as_output_invalid`,
`strings_with_nul_characters_fail_as_output_invalid`, `outputs_over_one_mebibyte_fail_as_output_too_large` and
`limits_match_the_spec`.

- [ ] **Step 5: Run the full backend verification**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo fmt --check
```

Expected: no output, exit code 0.

```text
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: `Finished` with no warnings.

```text
cargo test --workspace
```

Expected: every suite passes. corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 4,
`catalog_http` 4, `members_http` 3, `organizations_http` 1, `problems_http` 2, `publish_http` 2, `shutdown` 1,
`workflows_http` 4. corqty-domain: `account_and_rbac` 6, `catalog_rules` 7, `graph_rules` 8, `publish_rules` 10,
`run_rules` 8, `runtime_rules` 12. corqty-persistence: `auth_store` 2, `catalog_guards` 3, `final_admin` 2,
`migrations` 1, `tenant_isolation` 1, `test_reset` 1, `workflow_catalog` 6. 96 tests in total.

- [ ] **Step 6: Commit**

```text
git add crates/domain
git commit -m "feat: resolve node inputs and validate outputs at run time"
```

Expected: `git show --stat HEAD` lists exactly the 3 files above (510 insertions).

## Task 5: Define the executor plugin contract and compiled registry

**Files:**

- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Create: `crates/plugin-sdk/Cargo.toml`
- Create: `crates/plugin-sdk/src/lib.rs`
- Create: `crates/plugin-host/Cargo.toml`
- Create: `crates/plugin-host/src/lib.rs`
- Test: `crates/plugin-host/tests/registry.rs`

Workers run nodes through executors. This task fixes the contract between the two before the first executor
exists, so the mock (Task 6), the shell executor (Task 7) and the worker (Task 14) all build against the same
types. It adds two crates.

`corqty-plugin-sdk` holds what an executor implements (spec §10.1):

- The `Executor` trait has three operations. `metadata()` names the executor and what it needs. `validate(config)`
  returns diagnostics located by JSON Pointer; the Milestone 3 worker never calls it, because publish already
  checked config against the definition's `configSchema`, so only the executors' contract tests use it.
  `execute(request, context)` returns a boxed future (`futures_util::future::BoxFuture`) of either the output
  object, one entry per output port, or an `ExecutorError { code, message, retryable }`. The boxed future keeps
  the trait usable as `Arc<dyn Executor>` without async-trait.
- `ExecuteRequest` carries the `execution_id` (the node run id, the same on every attempt, so it is the idempotency
  key), the attempt number, the workflow, run and node identity, the resolved inputs, the node's config and the
  deadline. The deadline is informational: executors watch only `context.cancel` and return code `canceled`
  promptly once it fires. The worker owns the deadline, user cancels and shutdown (D12), and it classifies a
  timeout with `ExecutorMetadata::timeout_retryable`, not with the executor's own error.
- `ExecutionContext` lends the attempt a `CancellationToken`, a `scratch_dir` that the worker creates for this
  attempt and deletes afterwards (executors never create their own), and `logs`, a bounded
  `tokio::sync::mpsc::Sender<LogChunk>`. `send(..).await` waits while the worker catches up, so a chatty process
  cannot fill the worker's memory between flushes. Executors ignore a closed channel.
- `LogChunk { stream, text }`. `LogStream::as_str` gives `stdout`, `stderr` or `system`, the text that
  `append_node_log` stores and `node.log` events carry. Nothing serializes the enum, so it has no serde derive.
- `Capability` is the vocabulary of spec §10.3 (`Log`, `SecretRead`, `ScratchFs`, `Network { host, port }`,
  `CommandExec { executables }`), and `HOST_CONTRACT` is `"1"`.

`corqty-plugin-host` holds the `Registry` a worker builds at startup from the executors compiled into it. The
Wasmtime host for runtime plugins joins it in Milestone 4. `Registry::new(executors)` checks each executor's
metadata in order and returns the first problem as a `RegistryError`, which the worker turns into a non-zero exit:

1. `InvalidKey(key)`: the key does not match `^[a-z][a-z0-9-]{0,62}$`.
2. `InvalidVersion("key@version")`: the version is not three dot-separated runs of ASCII digits. The check reads
   bytes, so `1.0.+1`, which `str::parse` would accept, fails.
3. `UnsupportedHostContract { executor, host_contract }`: the executor targets another host contract.
4. `UnsupportedCapability { executor, capability }`: it declares `SecretRead` or `Network`, which the host grants
   from Milestone 4 on (`// ponytail:`). `Log`, `ScratchFs` and `CommandExec` are granted to every compiled
   executor.
5. `DuplicateExecutor("key@version")`: two executors share an identity.

`find(key, version)` returns the executor with exactly that key and version. `advertised()` returns every
`key@version` sorted, which is the identity form `node_runs.executor` stores (D3) and the list a worker registers
and claims with.

The tests use a fake executor that logs one chunk to each stream and returns the request it received. Its round
trip shows that a registry lookup dispatches through the trait object, that the request arrives intact, and that
log chunks arrive in order with their stream names.

The workspace gains `futures-util = "0.3"` (with its default features, as D1 lists it) and the tokio feature
`sync`, which `tokio::sync::mpsc` needs. sqlx already compiles `sync` in; declaring it keeps the SDK independent of
sqlx's feature set. The SDK depends on `corqty-domain` only for `Diagnostic`, so the domain stays free of tokio.

- [ ] **Step 1: Add the two crates to the workspace**

In `Cargo.toml`:

Replace:

```toml
  "crates/persistence",
  "crates/test-support",
```

With:

```toml
  "crates/persistence",
  "crates/plugin-host",
  "crates/plugin-sdk",
  "crates/test-support",
```

Replace:

```toml
chrono = { version = "0.4", features = ["serde"] }
```

With:

```toml
chrono = { version = "0.4", features = ["serde"] }
futures-util = "0.3"
```

Replace:

```toml
tokio = { version = "1", features = ["io-util", "macros", "net", "rt-multi-thread", "signal", "time"] }
```

With:

```toml
tokio = { version = "1", features = ["io-util", "macros", "net", "rt-multi-thread", "signal", "sync", "time"] }
```

Create `crates/plugin-sdk/Cargo.toml`:

```toml
[package]
name = "corqty-plugin-sdk"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
chrono.workspace = true
corqty-domain = { path = "../domain" }
futures-util.workspace = true
serde_json.workspace = true
tokio.workspace = true
tokio-util.workspace = true
uuid.workspace = true

[lints]
workspace = true
```

Create `crates/plugin-host/Cargo.toml`:

```toml
[package]
name = "corqty-plugin-host"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
corqty-plugin-sdk = { path = "../plugin-sdk" }
thiserror.workspace = true

[dev-dependencies]
chrono.workspace = true
corqty-domain = { path = "../domain" }
futures-util.workspace = true
serde_json.workspace = true
tokio.workspace = true
tokio-util.workspace = true
uuid.workspace = true

[lints]
workspace = true
```

The test helpers need `chrono`, `corqty-domain`, `futures-util`, `serde_json`, `tokio`, `tokio-util` and `uuid` to
implement the trait and build a request and a context, so the host lists them as dev-dependencies only.

Create `crates/plugin-sdk/src/lib.rs`:

```rust
#![forbid(unsafe_code)]
```

Create `crates/plugin-host/src/lib.rs`:

```rust
#![forbid(unsafe_code)]
```

The next cargo command (Step 3) updates `Cargo.lock`. It adds the packages `corqty-plugin-host` and
`corqty-plugin-sdk`, adds `futures-macro` 0.3.33 (pulled in by futures-util's default `async-await-macro` feature;
futures-util 0.3.33 pins it exactly), and adds `"futures-macro"` to the `futures-util` dependency list. No other
version changes: futures-util stays at the locked 0.3.33.

- [ ] **Step 2: Write the failing tests**

Create `crates/plugin-host/tests/registry.rs`:

```rust
use std::sync::Arc;

use chrono::Utc;
use corqty_domain::diagnostic::Diagnostic;
use corqty_plugin_host::{Registry, RegistryError};
use corqty_plugin_sdk::{
    Capability, ExecuteRequest, ExecutionContext, Executor, ExecutorError, ExecutorMetadata,
    HOST_CONTRACT, LogChunk, LogStream,
};
use futures_util::future::BoxFuture;
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// An executor that logs one chunk to each stream and returns the request it received.
struct Fake(ExecutorMetadata);

impl Executor for Fake {
    fn metadata(&self) -> ExecutorMetadata {
        self.0.clone()
    }

    fn validate(&self, _config: &Value) -> Vec<Diagnostic> {
        Vec::new()
    }

    fn execute<'a>(
        &'a self,
        request: ExecuteRequest,
        context: &'a ExecutionContext,
    ) -> BoxFuture<'a, Result<Map<String, Value>, ExecutorError>> {
        Box::pin(async move {
            for (stream, text) in [
                (LogStream::Stdout, "starting"),
                (LogStream::Stderr, "warning"),
                (LogStream::System, "done"),
            ] {
                let chunk = LogChunk {
                    stream,
                    text: text.to_owned(),
                };
                let _ = context.logs.send(chunk).await;
            }
            Ok(object(json!({
                "executionId": request.execution_id,
                "attempt": request.attempt,
                "nodeId": request.node_id,
                "inputs": request.inputs,
                "config": request.config,
            })))
        })
    }
}

fn fake(
    key: &'static str,
    version: &'static str,
    host_contract: &'static str,
    capabilities: Vec<Capability>,
) -> Arc<dyn Executor> {
    Arc::new(Fake(ExecutorMetadata {
        key,
        version,
        host_contract,
        capabilities,
        timeout_retryable: false,
    }))
}

fn executor(key: &'static str, version: &'static str) -> Arc<dyn Executor> {
    fake(key, version, HOST_CONTRACT, vec![Capability::Log])
}

fn rejection(executors: Vec<Arc<dyn Executor>>) -> RegistryError {
    Registry::new(executors)
        .err()
        .expect("the registry rejects these executors")
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("expected an object, got {other}"),
    }
}

#[test]
fn the_milestone_three_executors_are_advertised_in_sorted_order() {
    let registry = Registry::new(vec![
        fake(
            "shell",
            "1.0.0",
            HOST_CONTRACT,
            vec![
                Capability::Log,
                Capability::ScratchFs,
                Capability::CommandExec {
                    executables: vec!["/bin/sh".to_owned()],
                },
            ],
        ),
        executor("mock", "1.0.0"),
        executor("mock", "0.9.0"),
    ])
    .expect("the registry accepts these executors");

    assert_eq!(
        registry.advertised(),
        ["mock@0.9.0", "mock@1.0.0", "shell@1.0.0"]
    );
}

#[test]
fn find_matches_the_exact_key_and_version() {
    let registry = Registry::new(vec![executor("mock", "1.0.0"), executor("shell", "1.0.0")])
        .expect("the registry accepts these executors");

    let shell = registry
        .find("shell", "1.0.0")
        .expect("shell@1.0.0 is registered")
        .metadata();
    assert_eq!((shell.key, shell.version), ("shell", "1.0.0"));
    assert!(registry.find("mock", "1.0.1").is_none());
    assert!(registry.find("mock", "1.0").is_none());
    assert!(registry.find("other", "1.0.0").is_none());
}

#[test]
fn keys_must_be_lowercase_slugs_of_at_most_63_characters() {
    let longest: &'static str = "k".repeat(63).leak();
    let too_long: &'static str = "k".repeat(64).leak();
    for key in [
        "",
        "Mock",
        "1mock",
        "-mock",
        "mock_task",
        "mock.task",
        "mock task",
        "mock@1",
        too_long,
    ] {
        assert_eq!(
            rejection(vec![executor(key, "1.0.0")]),
            RegistryError::InvalidKey(key.to_owned()),
            "{key:?}"
        );
    }
    for key in ["m", "mock-2", longest] {
        assert!(
            Registry::new(vec![executor(key, "1.0.0")]).is_ok(),
            "{key:?}"
        );
    }
}

#[test]
fn versions_must_be_three_dot_separated_numbers() {
    for version in [
        "",
        "1",
        "1.0",
        "1.0.0.0",
        "v1.0.0",
        "1..0",
        "1.0.0-beta",
        "1.0.x",
        "1.0.+1",
        " 1.0.0",
    ] {
        assert_eq!(
            rejection(vec![executor("mock", version)]),
            RegistryError::InvalidVersion(format!("mock@{version}")),
            "{version:?}"
        );
    }
    for version in ["0.0.0", "1.0.0", "10.20.30"] {
        assert!(
            Registry::new(vec![executor("mock", version)]).is_ok(),
            "{version:?}"
        );
    }
}

#[test]
fn executors_built_for_another_host_contract_are_rejected() {
    for host_contract in ["", "0", "2", "1.0"] {
        assert_eq!(
            rejection(vec![fake(
                "mock",
                "1.0.0",
                host_contract,
                vec![Capability::Log]
            )]),
            RegistryError::UnsupportedHostContract {
                executor: "mock@1.0.0".to_owned(),
                host_contract: host_contract.to_owned(),
            },
        );
    }
}

#[test]
fn secret_and_network_capabilities_wait_for_milestone_four() {
    for capability in [
        Capability::SecretRead,
        Capability::Network {
            host: "api.example.com".to_owned(),
            port: 443,
        },
    ] {
        assert_eq!(
            rejection(vec![fake(
                "mock",
                "1.0.0",
                HOST_CONTRACT,
                vec![Capability::Log, capability.clone()]
            )]),
            RegistryError::UnsupportedCapability {
                executor: "mock@1.0.0".to_owned(),
                capability,
            },
        );
    }
}

#[test]
fn an_identity_registered_twice_is_rejected() {
    assert_eq!(
        rejection(vec![
            executor("mock", "1.0.0"),
            executor("shell", "1.0.0"),
            executor("mock", "1.0.0"),
        ]),
        RegistryError::DuplicateExecutor("mock@1.0.0".to_owned()),
    );
}

#[tokio::test]
async fn a_found_executor_receives_the_request_and_streams_its_logs() {
    let registry = Registry::new(vec![executor("mock", "1.0.0")])
        .expect("the registry accepts these executors");
    let mock = registry
        .find("mock", "1.0.0")
        .expect("mock@1.0.0 is registered");
    let (logs, mut received) = mpsc::channel(64);
    let context = ExecutionContext {
        cancel: CancellationToken::new(),
        scratch_dir: std::env::temp_dir(),
        logs,
    };
    let execution_id = Uuid::now_v7();
    let request = ExecuteRequest {
        execution_id,
        attempt: 2,
        workflow_id: Uuid::now_v7(),
        run_id: Uuid::now_v7(),
        node_id: "vpc".to_owned(),
        inputs: object(json!({"region": "eu-west-1"})),
        config: json!({"mode": "fixed"}),
        deadline: Utc::now(),
    };

    let output = mock
        .execute(request, &context)
        .await
        .expect("the fake executor succeeds");
    drop(context);
    let mut chunks = Vec::new();
    while let Some(chunk) = received.recv().await {
        chunks.push((chunk.stream.as_str(), chunk.text));
    }

    assert_eq!(
        output,
        object(json!({
            "executionId": execution_id,
            "attempt": 2,
            "nodeId": "vpc",
            "inputs": {"region": "eu-west-1"},
            "config": {"mode": "fixed"},
        }))
    );
    assert_eq!(
        chunks,
        [
            ("stdout", "starting".to_owned()),
            ("stderr", "warning".to_owned()),
            ("system", "done".to_owned()),
        ]
    );
}
```

- [ ] **Step 3: Run the tests and verify RED**

```text
cargo test -p corqty-plugin-host --test registry
```

Expected: cargo prints `Adding futures-macro v0.3.33`, and the test does not compile because neither crate exports
anything yet:

```text
error[E0432]: unresolved imports `corqty_plugin_host::Registry`, `corqty_plugin_host::RegistryError`
 --> crates\plugin-host\tests\registry.rs:5:26
  |
5 | use corqty_plugin_host::{Registry, RegistryError};
  |                          ^^^^^^^^  ^^^^^^^^^^^^^ no `RegistryError` in the root
  |                          |
  |                          no `Registry` in the root

error[E0432]: unresolved imports `corqty_plugin_sdk::Capability`, `corqty_plugin_sdk::ExecuteRequest`, `corqty_plugin_sdk::ExecutionContext`, `corqty_plugin_sdk::Executor`, `corqty_plugin_sdk::ExecutorError`, `corqty_plugin_sdk::ExecutorMetadata`, `corqty_plugin_sdk::HOST_CONTRACT`, `corqty_plugin_sdk::LogChunk`, `corqty_plugin_sdk::LogStream`
 --> crates\plugin-host\tests\registry.rs:7:5
  |
7 |     Capability, ExecuteRequest, ExecutionContext, Executor, ExecutorError, ExecutorMetadata,
  |     ^^^^^^^^^^  ^^^^^^^^^^^^^^  ^^^^^^^^^^^^^^^^  ^^^^^^^^  ^^^^^^^^^^^^^  ^^^^^^^^^^^^^^^^ no `ExecutorMetadata` in the root
  |     |           |               |                 |         |
  |     |           |               |                 |         no `ExecutorError` in the root
  |     |           |               |                 no `Executor` in the root
  |     |           |               no `ExecutionContext` in the root
  |     |           no `ExecuteRequest` in the root
  |     no `Capability` in the root
8 |     HOST_CONTRACT, LogChunk, LogStream,
  |     ^^^^^^^^^^^^^  ^^^^^^^^  ^^^^^^^^^ no `LogStream` in the root
  |     |              |
  |     |              no `LogChunk` in the root
  |     no `HOST_CONTRACT` in the root
```

The last line is ``error: could not compile `corqty-plugin-host` (test "registry") due to 2 previous errors``.

- [ ] **Step 4: Define the executor contract**

Replace the whole of `crates/plugin-sdk/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

//! The logical contract between the worker and an executor (spec §10.1). Milestone 3 compiles
//! executors into the worker; Milestone 4 runtime plugins implement the same operations.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use corqty_domain::diagnostic::Diagnostic;
use futures_util::future::BoxFuture;
use serde_json::{Map, Value};
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// The host contract version this worker implements.
pub const HOST_CONTRACT: &str = "1";

/// A host operation an executor declares it needs (spec §10.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Capability {
    Log,
    SecretRead,
    ScratchFs,
    Network { host: String, port: u16 },
    CommandExec { executables: Vec<String> },
}

/// Who an executor is and what it needs from the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutorMetadata {
    pub key: &'static str,
    pub version: &'static str,
    pub host_contract: &'static str,
    pub capabilities: Vec<Capability>,
    /// Whether an attempt that reaches its deadline may be retried. The worker enforces
    /// deadlines, so this flag classifies a timeout, not the executor's own error.
    pub timeout_retryable: bool,
}

/// One attempt of one node run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecuteRequest {
    /// The node run id. It stays the same on every attempt, so it is the idempotency key.
    pub execution_id: Uuid,
    pub attempt: u32,
    pub workflow_id: Uuid,
    pub run_id: Uuid,
    pub node_id: String,
    /// The resolved inputs, already validated against the input port schemas.
    pub inputs: Map<String, Value>,
    pub config: Value,
    /// When the worker cancels this attempt. Informational: executors watch only `cancel`.
    pub deadline: DateTime<Utc>,
}

/// What the host lends an executor for one attempt.
#[derive(Debug)]
pub struct ExecutionContext {
    /// Fired by the worker on a deadline, a user cancel or shutdown. The executor then returns
    /// an `ExecutorError` with code `canceled` promptly.
    pub cancel: CancellationToken,
    /// A directory the worker creates for this attempt only and deletes afterwards.
    pub scratch_dir: PathBuf,
    /// A bounded channel: `send(..).await` waits while the worker catches up. Executors ignore
    /// a closed channel.
    pub logs: Sender<LogChunk>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
    System,
}

impl LogStream {
    /// The stream's name in `append_node_log` and in `node.log` events.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::System => "system",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogChunk {
    pub stream: LogStream,
    pub text: String,
}

/// Why an attempt failed. `retryable` is the executor's opinion; the codes in
/// `corqty_domain::run::NON_RETRYABLE` are never retried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutorError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

/// An executor compiled into the worker.
pub trait Executor: Send + Sync {
    fn metadata(&self) -> ExecutorMetadata;

    /// Problems in a node's `config`, located by JSON Pointer. The Milestone 3 worker never
    /// calls it, because publish has already checked config against the definition's schema.
    fn validate(&self, config: &Value) -> Vec<Diagnostic>;

    /// Runs one attempt and returns the output object, one entry per output port.
    fn execute<'a>(
        &'a self,
        request: ExecuteRequest,
        context: &'a ExecutionContext,
    ) -> BoxFuture<'a, Result<Map<String, Value>, ExecutorError>>;
}
```

- [ ] **Step 5: Add the registry**

Replace the whole of `crates/plugin-host/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

//! The executors a worker can run. Milestone 4 adds the Wasmtime host for runtime plugins here.

use std::sync::Arc;

use corqty_plugin_sdk::{Capability, Executor, ExecutorMetadata, HOST_CONTRACT};
use thiserror::Error;

/// Why a worker refuses to start with a set of executors.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("executor key {0:?} must match ^[a-z][a-z0-9-]{{0,62}}$")]
    InvalidKey(String),
    #[error("executor {0} must have a MAJOR.MINOR.PATCH version")]
    InvalidVersion(String),
    #[error("executor {executor} targets host contract {host_contract:?}, not {HOST_CONTRACT:?}")]
    UnsupportedHostContract {
        executor: String,
        host_contract: String,
    },
    #[error("executor {executor} declares {capability:?}, which this host grants from Milestone 4")]
    UnsupportedCapability {
        executor: String,
        capability: Capability,
    },
    #[error("executor {0} is registered twice")]
    DuplicateExecutor(String),
}

/// The executors compiled into this worker, checked once at startup.
pub struct Registry(Vec<Arc<dyn Executor>>);

impl Registry {
    /// Accepts executors with a lowercase slug key, a `MAJOR.MINOR.PATCH` version, this host's
    /// contract, only the capabilities Milestone 3 grants, and a unique `key@version`.
    pub fn new(executors: Vec<Arc<dyn Executor>>) -> Result<Self, RegistryError> {
        let mut identities = Vec::new();
        for executor in &executors {
            let metadata = executor.metadata();
            let identity = identity(&metadata);
            if !is_valid_key(metadata.key) {
                return Err(RegistryError::InvalidKey(metadata.key.to_owned()));
            }
            if !is_valid_version(metadata.version) {
                return Err(RegistryError::InvalidVersion(identity));
            }
            if metadata.host_contract != HOST_CONTRACT {
                return Err(RegistryError::UnsupportedHostContract {
                    executor: identity,
                    host_contract: metadata.host_contract.to_owned(),
                });
            }
            // ponytail: Milestone 3 grants log, scratch-fs and command-exec to every compiled
            // executor; secret-read, network and per-plugin approval arrive in Milestone 4.
            if let Some(capability) = metadata.capabilities.into_iter().find(|capability| {
                matches!(
                    capability,
                    Capability::SecretRead | Capability::Network { .. }
                )
            }) {
                return Err(RegistryError::UnsupportedCapability {
                    executor: identity,
                    capability,
                });
            }
            if identities.contains(&identity) {
                return Err(RegistryError::DuplicateExecutor(identity));
            }
            identities.push(identity);
        }
        Ok(Self(executors))
    }

    /// The executor whose key and version are exactly these.
    #[must_use]
    pub fn find(&self, key: &str, version: &str) -> Option<Arc<dyn Executor>> {
        self.0
            .iter()
            .find(|executor| {
                let metadata = executor.metadata();
                metadata.key == key && metadata.version == version
            })
            .cloned()
    }

    /// Every `key@version` this worker runs, sorted: what it advertises when it claims jobs.
    #[must_use]
    pub fn advertised(&self) -> Vec<String> {
        let mut identities: Vec<String> = self
            .0
            .iter()
            .map(|executor| identity(&executor.metadata()))
            .collect();
        identities.sort();
        identities
    }
}

fn identity(metadata: &ExecutorMetadata) -> String {
    format!("{}@{}", metadata.key, metadata.version)
}

fn is_valid_key(key: &str) -> bool {
    key.len() <= 63
        && key.starts_with(|character: char| character.is_ascii_lowercase())
        && key
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn is_valid_version(version: &str) -> bool {
    version.split('.').count() == 3
        && version
            .split('.')
            .all(|number| !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
}
```

- [ ] **Step 6: Verify GREEN**

```text
cargo test -p corqty-plugin-host --test registry
```

Expected: 8 tests pass: `the_milestone_three_executors_are_advertised_in_sorted_order`,
`find_matches_the_exact_key_and_version`, `keys_must_be_lowercase_slugs_of_at_most_63_characters`,
`versions_must_be_three_dot_separated_numbers`, `executors_built_for_another_host_contract_are_rejected`,
`secret_and_network_capabilities_wait_for_milestone_four`, `an_identity_registered_twice_is_rejected` and
`a_found_executor_receives_the_request_and_streams_its_logs`.

- [ ] **Step 7: Run the full backend verification**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo fmt --check
```

Expected: no output, exit code 0.

```text
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: `Finished` with no warnings.

```text
cargo test --workspace
```

Expected: every suite passes. corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 4,
`catalog_http` 4, `members_http` 3, `organizations_http` 1, `problems_http` 2, `publish_http` 2, `shutdown` 1,
`workflows_http` 4. corqty-domain: `account_and_rbac` 6, `catalog_rules` 7, `graph_rules` 8, `publish_rules` 10,
`run_rules` 8, `runtime_rules` 12. corqty-persistence: `auth_store` 2, `catalog_guards` 3, `final_admin` 2,
`migrations` 1, `tenant_isolation` 1, `test_reset` 1, `workflow_catalog` 6. corqty-plugin-host: `registry` 8. 104
tests in total.

- [ ] **Step 8: Commit**

```text
git add Cargo.toml Cargo.lock crates/plugin-sdk crates/plugin-host
git commit -m "feat: define the executor plugin contract and compiled registry"
```

Expected: `git show --stat HEAD` lists exactly the 7 files above (612 insertions, 1 deletion).

## Task 6: Add the deterministic mock executor

**Files:**

- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Create: `crates/executor-mock/Cargo.toml`
- Create: `crates/executor-mock/src/lib.rs`
- Test: `crates/executor-mock/tests/contract.rs`

Every node of both starter blueprints runs on `mock@1.0.0`, and the worker tests (Task 14) need an executor whose
behaviour its config fixes exactly. This task adds the crate `corqty-executor-mock`. Its one public item,
`MockExecutor`, implements the Task 5 `Executor` trait (D4, spec §10.3).

Its config is the seeded mock config (`mode`, `output`, `delayMs`, `failureMessage`) plus an optional
`failAttempts`. One attempt runs in this order:

1. It reads the config. If it cannot, for example because `mode` is neither `fixed` nor `echo` or `output` is
   missing, the attempt fails with `mock_config_invalid`, not retryable. Reading is lenient on purpose
   (`// ponytail:`): publish has already applied the definition's config schema, and `validate` is the strict
   check.
2. It sends one `system` log chunk, `Simulating <delayMs> ms of work.`. The worker's log path therefore carries
   something even where the shell executor cannot run. A closed channel is ignored.
3. It waits `delayMs`. If `context.cancel` fires first, it returns `canceled` at once. The `select!` is `biased`:
   a token that has already fired wins even over a zero delay, so the outcome never depends on polling order.
4. A non-null `failureMessage` fails every attempt with `mock_failure` and that message, not retryable.
5. An attempt number up to `failAttempts` fails with `mock_retryable_failure`, retryable, for example
   `Attempt 1 fails because failAttempts is 1.`. Attempts count from 1, so `failAttempts: 1` fails the first
   attempt and lets the second succeed.
6. Otherwise `fixed` returns `output` unchanged, and `echo` returns `{"result": <resolved inputs>}`. `result` is
   the output port of `corqty.mock.task`; on a definition without a `result` output, the worker's output
   validation fails the attempt with `output_invalid`.

`validate(config)` compiles the seeded mock config schema plus `failAttempts` (an integer >= 0) with
`corqty_domain::schema::compile`. It reports each `instance_errors` entry as `invalid_config` at its pointer, the
code publish uses for config problems. The Milestone 3 worker never calls it (D2). The seeded definitions' schema
has `additionalProperties: false`, so only an organization definition with its own config schema can set
`failAttempts`.

The metadata is key `mock`, version `1.0.0`, `HOST_CONTRACT`, capabilities `[Log]` and `timeout_retryable: false`,
which the Task 5 registry accepts.

The tests call the executor directly with a fresh context per attempt and use real time. The delay test waits
200 ms. The cancel test fires the token 100 ms into a 10 s delay and requires the result within 1 s
(`tokio::time::timeout`).

- [ ] **Step 1: Add the crate to the workspace**

In `Cargo.toml`:

Replace:

```toml
  "crates/domain",
  "crates/migrator",
```

With:

```toml
  "crates/domain",
  "crates/executor-mock",
  "crates/migrator",
```

Create `crates/executor-mock/Cargo.toml`:

```toml
[package]
name = "corqty-executor-mock"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
corqty-domain = { path = "../domain" }
corqty-plugin-sdk = { path = "../plugin-sdk" }
futures-util.workspace = true
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true

[dev-dependencies]
chrono.workspace = true
tokio-util.workspace = true
uuid.workspace = true

[lints]
workspace = true
```

The executor needs `corqty-domain` for `Diagnostic` and the schema helpers, `futures-util` for `BoxFuture`,
`serde` and `serde_json` to read its config, and `tokio` for `select!` and `sleep` (the workspace features `macros`
and `time`). The tests also need `chrono`, `tokio-util` and `uuid` to build a request and a context.

Create `crates/executor-mock/src/lib.rs`:

```rust
#![forbid(unsafe_code)]
```

The next cargo command (Step 3) updates `Cargo.lock`. It adds the package `corqty-executor-mock` with the
dependencies listed above. No other entry changes.

- [ ] **Step 2: Write the failing tests**

Create `crates/executor-mock/tests/contract.rs`:

```rust
use std::time::Duration;

use chrono::Utc;
use corqty_executor_mock::MockExecutor;
use corqty_plugin_sdk::{
    Capability, ExecuteRequest, ExecutionContext, Executor, ExecutorError, ExecutorMetadata,
    HOST_CONTRACT, LogChunk,
};
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;
use tokio::time::{Instant, timeout};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("expected an object, got {other}"),
    }
}

/// The seeded create-vpc config (fixed output, no delay, no failure) with `changes` applied.
fn config(changes: Value) -> Value {
    let mut config = object(json!({
        "mode": "fixed",
        "output": {"vpcId": "vpc-0simulated"},
        "delayMs": 0,
        "failureMessage": null,
    }));
    config.extend(object(changes));
    Value::Object(config)
}

fn request(attempt: u32, config: Value) -> ExecuteRequest {
    ExecuteRequest {
        execution_id: Uuid::now_v7(),
        attempt,
        workflow_id: Uuid::now_v7(),
        run_id: Uuid::now_v7(),
        node_id: "task".to_owned(),
        inputs: object(json!({"input": {"a": 1}})),
        config,
        deadline: Utc::now(),
    }
}

/// A context whose log receiver stays open, as the worker's does.
fn context() -> (ExecutionContext, mpsc::Receiver<LogChunk>) {
    let (logs, received) = mpsc::channel(64);
    let context = ExecutionContext {
        cancel: CancellationToken::new(),
        scratch_dir: std::env::temp_dir(),
        logs,
    };
    (context, received)
}

async fn execute(attempt: u32, config: Value) -> Result<Map<String, Value>, ExecutorError> {
    let (context, _received) = context();
    MockExecutor
        .execute(request(attempt, config), &context)
        .await
}

/// What `validate` reports for `config`, as sorted "code at pointer" lines.
fn problems(config: &Value) -> Vec<String> {
    let mut problems: Vec<String> = MockExecutor
        .validate(config)
        .into_iter()
        .map(|diagnostic| format!("{} at {}", diagnostic.code, diagnostic.pointer))
        .collect();
    problems.sort();
    problems
}

#[test]
fn the_metadata_names_mock_1_0_0_with_only_the_log_capability() {
    assert_eq!(
        MockExecutor.metadata(),
        ExecutorMetadata {
            key: "mock",
            version: "1.0.0",
            host_contract: HOST_CONTRACT,
            capabilities: vec![Capability::Log],
            timeout_retryable: false,
        }
    );
}

#[tokio::test]
async fn fixed_mode_returns_the_configured_output_and_logs_the_delay() {
    let (context, mut received) = context();

    let output = MockExecutor
        .execute(request(1, config(json!({"delayMs": 20}))), &context)
        .await
        .expect("the attempt succeeds");
    drop(context);
    let mut chunks = Vec::new();
    while let Some(chunk) = received.recv().await {
        chunks.push((chunk.stream.as_str(), chunk.text));
    }

    assert_eq!(output, object(json!({"vpcId": "vpc-0simulated"})));
    assert_eq!(chunks, [("system", "Simulating 20 ms of work.".to_owned())]);
}

#[tokio::test]
async fn echo_mode_returns_the_resolved_inputs_under_result() {
    let output = execute(1, config(json!({"mode": "echo"})))
        .await
        .expect("the attempt succeeds");

    assert_eq!(output, object(json!({"result": {"input": {"a": 1}}})));
}

#[tokio::test]
async fn the_attempt_waits_for_the_configured_delay() {
    let started = Instant::now();

    execute(1, config(json!({"delayMs": 200})))
        .await
        .expect("the attempt succeeds");

    assert!(started.elapsed() >= Duration::from_millis(200));
}

#[tokio::test]
async fn a_cancel_during_a_long_delay_returns_canceled_within_a_second() {
    let (context, _received) = context();
    let cancel = context.cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        cancel.cancel();
    });

    let error = timeout(
        Duration::from_secs(1),
        MockExecutor.execute(request(1, config(json!({"delayMs": 10_000}))), &context),
    )
    .await
    .expect("the attempt returns within 1 s")
    .expect_err("a canceled attempt fails");

    assert_eq!(
        error,
        ExecutorError {
            code: "canceled".to_owned(),
            message: "The attempt was canceled.".to_owned(),
            retryable: false,
        }
    );
}

#[tokio::test]
async fn a_token_canceled_before_the_attempt_wins_over_a_zero_delay() {
    let (context, _received) = context();
    context.cancel.cancel();

    let error = MockExecutor
        .execute(request(1, config(json!({}))), &context)
        .await
        .expect_err("a canceled attempt fails");

    assert_eq!(error.code, "canceled");
}

#[tokio::test]
async fn a_failure_message_fails_every_attempt_without_retry() {
    for attempt in [1, 2] {
        let error = execute(
            attempt,
            config(json!({"failureMessage": "Simulated outage", "failAttempts": 1})),
        )
        .await
        .expect_err("the attempt fails");

        assert_eq!(
            error,
            ExecutorError {
                code: "mock_failure".to_owned(),
                message: "Simulated outage".to_owned(),
                retryable: false,
            }
        );
    }
}

#[tokio::test]
async fn fail_attempts_fail_retryably_and_then_succeed() {
    let config = config(json!({"failAttempts": 1}));

    let error = execute(1, config.clone())
        .await
        .expect_err("attempt 1 fails");
    let output = execute(2, config).await.expect("attempt 2 succeeds");

    assert_eq!(
        error,
        ExecutorError {
            code: "mock_retryable_failure".to_owned(),
            message: "Attempt 1 fails because failAttempts is 1.".to_owned(),
            retryable: true,
        }
    );
    assert_eq!(output, object(json!({"vpcId": "vpc-0simulated"})));
}

#[tokio::test]
async fn a_config_the_mock_cannot_read_fails_without_retry() {
    let error = execute(1, config(json!({"mode": "loop"})))
        .await
        .expect_err("the attempt fails");

    assert_eq!(
        (error.code.as_str(), error.retryable),
        ("mock_config_invalid", false)
    );
}

#[test]
fn validate_accepts_the_seeded_configs_and_fail_attempts() {
    for changes in [
        json!({}),
        json!({"mode": "echo", "delayMs": 60_000, "failureMessage": "Down", "failAttempts": 0}),
        json!({"failAttempts": 2}),
    ] {
        let config = config(changes);
        assert!(problems(&config).is_empty(), "{config}");
    }
}

#[test]
fn validate_locates_a_bad_mode_a_negative_fail_attempts_and_an_out_of_range_delay() {
    assert_eq!(
        problems(&config(
            json!({"mode": "loop", "delayMs": 60_001, "failAttempts": -1})
        )),
        [
            "invalid_config at /delayMs",
            "invalid_config at /failAttempts",
            "invalid_config at /mode",
        ]
    );
}
```

- [ ] **Step 3: Run the tests and verify RED**

```text
cargo test -p corqty-executor-mock --test contract
```

Expected: the test does not compile because the crate exports nothing yet:

```text
error[E0432]: unresolved import `corqty_executor_mock::MockExecutor`
 --> crates\executor-mock\tests\contract.rs:4:5
  |
4 | use corqty_executor_mock::MockExecutor;
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ no `MockExecutor` in the root
```

The last line is ``error: could not compile `corqty-executor-mock` (test "contract") due to 1 previous error``.

- [ ] **Step 4: Implement the mock executor**

Replace the whole of `crates/executor-mock/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

//! The deterministic mock executor `mock@1.0.0` (spec §10.3). It powers both starter
//! blueprints and the scheduler tests: it waits, then returns a fixed output or echoes its
//! inputs, or fails the way its config says.

use std::time::Duration;

use corqty_domain::diagnostic::Diagnostic;
use corqty_domain::schema::{compile, instance_errors};
use corqty_plugin_sdk::{
    Capability, ExecuteRequest, ExecutionContext, Executor, ExecutorError, ExecutorMetadata,
    HOST_CONTRACT, LogChunk, LogStream,
};
use futures_util::future::BoxFuture;
use serde::Deserialize;
use serde_json::{Map, Value, json};

pub struct MockExecutor;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    mode: Mode,
    output: Map<String, Value>,
    delay_ms: u64,
    failure_message: Option<String>,
    #[serde(default)]
    fail_attempts: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Fixed,
    Echo,
}

impl Executor for MockExecutor {
    fn metadata(&self) -> ExecutorMetadata {
        ExecutorMetadata {
            key: "mock",
            version: "1.0.0",
            host_contract: HOST_CONTRACT,
            capabilities: vec![Capability::Log],
            timeout_retryable: false,
        }
    }

    /// The seeded mock config schema plus the optional `failAttempts`, which only definitions
    /// with their own config schema can set.
    fn validate(&self, config: &Value) -> Vec<Diagnostic> {
        let schema = json!({
            "type": "object",
            "properties": {
                "mode": {"enum": ["fixed", "echo"]},
                "output": {"type": "object"},
                "delayMs": {"type": "integer", "minimum": 0, "maximum": 60_000},
                "failureMessage": {"type": ["string", "null"], "maxLength": 500},
                "failAttempts": {"type": "integer", "minimum": 0},
            },
            "required": ["mode", "output", "delayMs"],
            "additionalProperties": false,
        });
        let validator = compile(&schema).expect("the mock config schema compiles");
        instance_errors(&validator, config)
            .into_iter()
            .map(|(pointer, message)| Diagnostic::new("invalid_config", pointer, message))
            .collect()
    }

    /// Waits `delayMs`, unless the token fires first. Then a `failureMessage` fails the
    /// attempt for good, attempts up to `failAttempts` fail retryably, and the rest succeed
    /// with the fixed `output` or with `{"result": <inputs>}`.
    fn execute<'a>(
        &'a self,
        request: ExecuteRequest,
        context: &'a ExecutionContext,
    ) -> BoxFuture<'a, Result<Map<String, Value>, ExecutorError>> {
        Box::pin(async move {
            // ponytail: execute reads config leniently; validate is the strict check, and
            // publish has already applied the definition's config schema.
            let config: Config = serde_json::from_value(request.config).map_err(|error| {
                failure(
                    "mock_config_invalid",
                    format!("The mock cannot read its config: {error}."),
                    false,
                )
            })?;
            let text = format!("Simulating {} ms of work.", config.delay_ms);
            let chunk = LogChunk {
                stream: LogStream::System,
                text,
            };
            let _ = context.logs.send(chunk).await;
            tokio::select! {
                // A token that has already fired wins over a zero delay.
                biased;
                () = context.cancel.cancelled() => {
                    return Err(failure("canceled", "The attempt was canceled.".to_owned(), false));
                }
                () = tokio::time::sleep(Duration::from_millis(config.delay_ms)) => {}
            }
            if let Some(message) = config.failure_message {
                return Err(failure("mock_failure", message, false));
            }
            if request.attempt <= config.fail_attempts {
                let message = format!(
                    "Attempt {} fails because failAttempts is {}.",
                    request.attempt, config.fail_attempts
                );
                return Err(failure("mock_retryable_failure", message, true));
            }
            Ok(match config.mode {
                Mode::Fixed => config.output,
                Mode::Echo => Map::from_iter([("result".to_owned(), request.inputs.into())]),
            })
        })
    }
}

fn failure(code: &str, message: String, retryable: bool) -> ExecutorError {
    ExecutorError {
        code: code.to_owned(),
        message,
        retryable,
    }
}
```

- [ ] **Step 5: Verify GREEN**

```text
cargo test -p corqty-executor-mock --test contract
```

Expected: 11 tests pass: `the_metadata_names_mock_1_0_0_with_only_the_log_capability`,
`fixed_mode_returns_the_configured_output_and_logs_the_delay`, `echo_mode_returns_the_resolved_inputs_under_result`,
`the_attempt_waits_for_the_configured_delay`, `a_cancel_during_a_long_delay_returns_canceled_within_a_second`,
`a_token_canceled_before_the_attempt_wins_over_a_zero_delay`, `a_failure_message_fails_every_attempt_without_retry`,
`fail_attempts_fail_retryably_and_then_succeed`, `a_config_the_mock_cannot_read_fails_without_retry`,
`validate_accepts_the_seeded_configs_and_fail_attempts` and
`validate_locates_a_bad_mode_a_negative_fail_attempts_and_an_out_of_range_delay`.

- [ ] **Step 6: Run the full backend verification**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo fmt --check
```

Expected: no output, exit code 0.

```text
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: `Finished` with no warnings.

```text
cargo test --workspace
```

Expected: every suite passes. corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 4,
`catalog_http` 4, `members_http` 3, `organizations_http` 1, `problems_http` 2, `publish_http` 2, `shutdown` 1,
`workflows_http` 4. corqty-domain: `account_and_rbac` 6, `catalog_rules` 7, `graph_rules` 8, `publish_rules` 10,
`run_rules` 8, `runtime_rules` 12. corqty-executor-mock: `contract` 11. corqty-persistence: `auth_store` 2,
`catalog_guards` 3, `final_admin` 2, `migrations` 1, `tenant_isolation` 1, `test_reset` 1, `workflow_catalog` 6.
corqty-plugin-host: `registry` 8. 115 tests in total.

- [ ] **Step 7: Commit**

```text
git add Cargo.toml Cargo.lock crates/executor-mock
git commit -m "feat: add the deterministic mock executor"
```

Expected: `git show --stat HEAD` lists exactly the 5 files above (411 insertions).

## Task 7: Add the trusted shell executor

**Files:**

- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Create: `crates/executor-shell/Cargo.toml`
- Create: `crates/executor-shell/src/lib.rs`
- Test: `crates/executor-shell/tests/contract.rs`

Spec §10.4 adds a second built-in executor: a trusted shell that admins opt into (Task 11). This task adds the crate
`corqty-executor-shell`. Its one public item, `ShellExecutor`, implements the Task 5 `Executor` trait as
`shell@1.0.0` (D5). It is not a security boundary. A command can do anything the worker container allows, and the
crate documentation says so.

Its config is `{command: string, env?: object<string,string>}`. One attempt runs in this order:

1. It reads the config. If `command` is missing or a value has the wrong type, the attempt fails with
   `shell_config_invalid`, not retryable. As with the mock, `validate` is the strict check.
2. It writes the resolved inputs object as JSON to `input.json` in `context.scratch_dir`. The worker creates that
   directory for each attempt and deletes it afterwards (Task 14). The executor creates no directory of its own.
3. It runs `/bin/sh -c <command>` with the scratch directory as the working directory, after `env_clear()`. The
   environment is `PATH=/usr/local/bin:/usr/bin:/bin`, `LANG=C.UTF-8`, `HOME` and `TMPDIR` (both the scratch
   directory), and `CORQTY_INPUT` and `CORQTY_OUTPUT` (`input.json` and `output.json` in it). Config `env` entries
   are added only when the name matches `^[A-Z_][A-Z0-9_]*$` and does not start with `CORQTY_`. An entry may
   therefore override `PATH`, but never the two file paths. Each skipped name gets one `system` log line, for
   example `Skipped the environment variable lower: names must match ^[A-Z_][A-Z0-9_]*$ and must not start with
   CORQTY_.`, so a dropped variable is visible in the run log.
4. `process_group(0)` makes the shell the leader of a new process group. A guard (`Group`) SIGKILLs the whole
   group when it is dropped. So if the worker drops the attempt's future after its cancel grace (D12), nothing
   keeps running.
5. Stdout and stderr are read in 8 KiB reads and sent as `stdout`/`stderr` log chunks. A UTF-8 character split
   between two reads is held back until its last bytes arrive. Both streams share one budget of
   `MAX_LOG_BYTES_PER_NODE_RUN` (10 MiB) per attempt. Once it is spent, the pipes are still read to the end and
   the rest is dropped, so the command never blocks on a full pipe. The log channel is bounded (capacity 64), and
   `send(..).await` gives back-pressure while the worker catches up.
6. If `context.cancel` fires (deadline, user cancel or shutdown, D12), the group gets SIGTERM. If the shell has
   not exited 3 s later, the group gets SIGKILL. The attempt returns `canceled` (`The attempt was canceled.`, not
   retryable), the same error as the mock.
7. After the shell exits, by itself or after a signal, the group always gets SIGKILL. Background jobs (`sleep 30 &`)
   therefore cannot outlive the attempt or hold the pipes open. The guard then forgets the group id, because an
   empty group's id can be reused. Logs are read for at most 2 more seconds, since a process that left the group
   (`setsid`) could hold a pipe open forever.
8. A non-zero exit fails with `shell_exit`, not retryable, for example `The command failed (exit status: 3).`.
   A signal reads `(signal: 9 (SIGKILL))`.
9. On exit status 0, `output.json` becomes the result. A missing file (`The command did not write the output
   file.`), a file over 1 MiB (`The output file is larger than 1 MiB.`), or content that is not one JSON object
   (`The output file does not hold a JSON object: <serde error>.`) fails with `shell_output_invalid`, not
   retryable. The worker's `validate_output` (Task 4) still checks the object against the output ports.

An I/O failure around the command, such as writing `input.json`, spawning `/bin/sh`, waiting for it or reading the
output file, fails with `shell_io_error` and is retryable. Another worker or a later attempt may succeed. The
metadata is key `shell`, version `1.0.0`, `HOST_CONTRACT`, capabilities `[Log, ScratchFs, CommandExec {
executables: ["/bin/sh"] }]` and `timeout_retryable: true`, the exact shape the Task 5 registry test accepts.

`validate(config)` compiles the D15 config schema with `corqty_domain::schema::compile`. It adds
`propertyNames` for the environment rule (`pattern` plus `not` `^CORQTY_`; the linear-time regex engine has no
look-ahead). Each `instance_errors` entry is reported as `invalid_config` at its pointer. The Milestone 3 worker
never calls it (D2).

Everything unix-only lives in `#[cfg(unix)] mod imp`: the config struct, the guard, the readers and the
constants. The `#[cfg(not(unix))] mod imp` stub fails every attempt with `shell_unsupported` (`The shell executor
runs only on Linux.`, not retryable). Its parameters are `_request` and `_context`. Windows therefore builds and
lints the crate without dead code. `nix` (safe `killpg`, no `unsafe`) is a `[target.'cfg(unix)'.dependencies]`
entry. The test file starts with `#![cfg(unix)]`, so on Windows and macOS hosts it compiles to 0 tests. It runs in
CI (`ubuntu-latest`) and, locally, in the `rust:1.96-bookworm` container.

Each test gets its own `TempDir` as the scratch directory. A spawned task receives the log chunks while the
attempt runs, as the worker does. Two tests cancel once the command has written its process group id (`$$`) to a
file named `group` in the scratch directory. They then check that no live process is left in that group, by
reading `/proc/<pid>/stat` (state, parent pid, process group). `pgrep -g` cannot do this check. The orphaned
`sleep`s are re-parented to PID 1, and in `docker run` without `--init`, PID 1 is `cargo`, which never reaps
them. `pgrep` keeps listing those zombies (state `Z`) although they are dead.

- [ ] **Step 1: Add the crate and its dependencies to the workspace**

In `Cargo.toml`:

Replace:

```toml
  "crates/executor-mock",
  "crates/migrator",
```

With:

```toml
  "crates/executor-mock",
  "crates/executor-shell",
  "crates/migrator",
```

Replace:

```toml
jsonschema = { version = "0.60.1", default-features = false }
```

With:

```toml
jsonschema = { version = "0.60.1", default-features = false }
nix = { version = "0.31", default-features = false, features = ["signal"] }
```

Replace:

```toml
thiserror = "2"
tokio = { version = "1", features = ["io-util", "macros", "net", "rt-multi-thread", "signal", "sync", "time"] }
```

With:

```toml
tempfile = "3"
thiserror = "2"
tokio = { version = "1", features = ["io-util", "macros", "net", "process", "rt-multi-thread", "signal", "sync", "time"] }
```

Create `crates/executor-shell/Cargo.toml`:

```toml
[package]
name = "corqty-executor-shell"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
corqty-domain = { path = "../domain" }
corqty-plugin-sdk = { path = "../plugin-sdk" }
futures-util.workspace = true
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true

[target.'cfg(unix)'.dependencies]
nix.workspace = true

[dev-dependencies]
chrono.workspace = true
tempfile.workspace = true
tokio-util.workspace = true
uuid.workspace = true

[lints]
workspace = true
```

The executor needs `corqty-domain` for `Diagnostic`, the schema helpers and the limits, and `futures-util` for
`BoxFuture`. It uses `serde` and `serde_json` to read its config and output, and `tokio` for `process::Command`
(the new workspace feature `process`), `select!`, `join!`, `timeout` and pipe reads (`io-util`). `nix` (only on
unix) sends signals to the process group. The tests also need `chrono`, `tokio-util` and `uuid` to build a
request and a context, and `tempfile` for a scratch directory per test.

Create `crates/executor-shell/src/lib.rs`:

```rust
#![forbid(unsafe_code)]
```

The next cargo command (Step 3) updates `Cargo.lock` and prints `Locking 6 packages to highest Rust 1.96
compatible versions`. It adds the packages `corqty-executor-shell`, `nix` 0.31.3, `tempfile` 3.27.0,
`cfg_aliases` 0.2.2, `fastrand` 2.5.0, `rustix` 1.1.5 and `linux-raw-sys` 0.12.1. No other entry changes.

- [ ] **Step 2: Write the failing tests**

Create `crates/executor-shell/tests/contract.rs`:

```rust
#![cfg(unix)]

use std::time::Duration;

use chrono::Utc;
use corqty_executor_shell::ShellExecutor;
use corqty_plugin_sdk::{
    Capability, ExecuteRequest, ExecutionContext, Executor, ExecutorError, ExecutorMetadata,
    HOST_CONTRACT, LogChunk, LogStream,
};
use serde_json::{Map, Value, json};
use tempfile::TempDir;
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep, timeout};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

type Outcome = Result<Map<String, Value>, ExecutorError>;

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("expected an object, got {other}"),
    }
}

fn command(command: &str) -> Value {
    json!({"command": command})
}

fn request(config: Value) -> ExecuteRequest {
    ExecuteRequest {
        execution_id: Uuid::now_v7(),
        attempt: 1,
        workflow_id: Uuid::now_v7(),
        run_id: Uuid::now_v7(),
        node_id: "shell".to_owned(),
        inputs: object(json!({"input": {"name": "corqty"}})),
        config,
        deadline: Utc::now(),
    }
}

/// Runs one attempt with `scratch` as its scratch directory and returns its outcome and
/// every log chunk it sent. The chunks are received while the attempt runs, as the worker
/// does.
async fn run_in(
    scratch: &TempDir,
    cancel: CancellationToken,
    config: Value,
) -> (Outcome, Vec<LogChunk>) {
    let (logs, mut received) = mpsc::channel(64);
    let collecting = tokio::spawn(async move {
        let mut chunks = Vec::new();
        while let Some(chunk) = received.recv().await {
            chunks.push(chunk);
        }
        chunks
    });
    let context = ExecutionContext {
        cancel,
        scratch_dir: scratch.path().to_owned(),
        logs,
    };
    let outcome = ShellExecutor.execute(request(config), &context).await;
    drop(context);
    (outcome, collecting.await.expect("the collector finishes"))
}

async fn run(config: Value) -> (Outcome, Vec<LogChunk>) {
    let scratch = TempDir::new().expect("a scratch directory");
    run_in(&scratch, CancellationToken::new(), config).await
}

/// The text of every chunk of `stream`, joined.
fn text(chunks: &[LogChunk], stream: LogStream) -> String {
    chunks
        .iter()
        .filter(|chunk| chunk.stream == stream)
        .map(|chunk| chunk.text.as_str())
        .collect()
}

/// Waits until the command has written its process group id (`$$`) to the file `group`.
async fn group_of(scratch: &TempDir) -> u32 {
    loop {
        let written = std::fs::read_to_string(scratch.path().join("group")).unwrap_or_default();
        if let Some(group) = written.strip_suffix('\n') {
            return group.parse().expect("a process group id");
        }
        sleep(Duration::from_millis(10)).await;
    }
}

/// The `/proc/<pid>/stat` lines of the processes in `group` that are still alive. A zombie
/// is dead: it only waits for whoever adopted it to reap it.
fn survivors(group: u32) -> Vec<String> {
    std::fs::read_dir("/proc")
        .expect("/proc is readable")
        .filter_map(|entry| std::fs::read_to_string(entry.ok()?.path().join("stat")).ok())
        .filter(|stat| {
            // After the command name come the state, the parent pid and the process group.
            let fields: Vec<&str> = stat
                .rsplit_once(')')
                .map_or(Vec::new(), |(_, rest)| rest.split_whitespace().collect());
            fields.len() > 2 && fields[0] != "Z" && fields[2] == group.to_string()
        })
        .collect()
}

/// Cancels the attempt once the command has written its group id, and returns the outcome,
/// the group id and how long the attempt took to return after the cancel.
async fn cancel_once_started(script: &str) -> (Outcome, u32, Duration) {
    let scratch = TempDir::new().expect("a scratch directory");
    let cancel = CancellationToken::new();
    let attempt = run_in(&scratch, cancel.clone(), command(script));
    let canceling = async {
        let group = group_of(&scratch).await;
        cancel.cancel();
        (group, Instant::now())
    };
    let ((outcome, _), (group, canceled_at)) = tokio::join!(attempt, canceling);
    (outcome, group, canceled_at.elapsed())
}

/// What `validate` reports for `config`, as sorted "code at pointer" lines.
fn problems(config: &Value) -> Vec<String> {
    let mut problems: Vec<String> = ShellExecutor
        .validate(config)
        .into_iter()
        .map(|diagnostic| format!("{} at {}", diagnostic.code, diagnostic.pointer))
        .collect();
    problems.sort();
    problems
}

#[test]
fn the_metadata_names_shell_1_0_0_with_scratch_and_bin_sh_capabilities() {
    assert_eq!(
        ShellExecutor.metadata(),
        ExecutorMetadata {
            key: "shell",
            version: "1.0.0",
            host_contract: HOST_CONTRACT,
            capabilities: vec![
                Capability::Log,
                Capability::ScratchFs,
                Capability::CommandExec {
                    executables: vec!["/bin/sh".to_owned()],
                },
            ],
            timeout_retryable: true,
        }
    );
}

#[tokio::test]
async fn the_command_reads_the_input_file_and_its_output_file_becomes_the_result() {
    let (outcome, _) = run(command(
        r#"printf '{"copy": %s}' "$(cat "$CORQTY_INPUT")" > "$CORQTY_OUTPUT""#,
    ))
    .await;

    assert_eq!(
        outcome.expect("the attempt succeeds"),
        object(json!({"copy": {"input": {"name": "corqty"}}}))
    );
}

#[tokio::test]
async fn the_command_sees_only_the_fixed_environment_and_allowed_config_variables() {
    let scratch = TempDir::new().expect("a scratch directory");
    let config = json!({
        "command": r#"env; echo '{}' > "$CORQTY_OUTPUT""#,
        "env": {"GREETING": "hello", "lower": "x", "1ST": "x", "CORQTY_INPUT": "/etc/passwd"},
    });

    let (outcome, chunks) = run_in(&scratch, CancellationToken::new(), config).await;
    let mut variables: Vec<String> = text(&chunks, LogStream::Stdout)
        .lines()
        .map(str::to_owned)
        .collect();
    variables.sort();
    let skipped: Vec<String> = chunks
        .into_iter()
        .filter(|chunk| chunk.stream == LogStream::System)
        .map(|chunk| chunk.text)
        .collect();

    outcome.expect("the attempt succeeds");
    let scratch = scratch.path().display();
    assert_eq!(
        variables,
        [
            format!("CORQTY_INPUT={scratch}/input.json"),
            format!("CORQTY_OUTPUT={scratch}/output.json"),
            "GREETING=hello".to_owned(),
            format!("HOME={scratch}"),
            "LANG=C.UTF-8".to_owned(),
            "PATH=/usr/local/bin:/usr/bin:/bin".to_owned(),
            format!("PWD={scratch}"),
            format!("TMPDIR={scratch}"),
        ]
    );
    let rule = "names must match ^[A-Z_][A-Z0-9_]*$ and must not start with CORQTY_.";
    assert_eq!(
        skipped,
        [
            format!("Skipped the environment variable 1ST: {rule}"),
            format!("Skipped the environment variable CORQTY_INPUT: {rule}"),
            format!("Skipped the environment variable lower: {rule}"),
        ]
    );
}

#[tokio::test]
async fn stdout_and_stderr_arrive_as_log_chunks_of_their_own_stream() {
    let (outcome, chunks) = run(command(
        r#"echo out; echo err >&2; echo '{}' > "$CORQTY_OUTPUT""#,
    ))
    .await;

    outcome.expect("the attempt succeeds");
    assert_eq!(text(&chunks, LogStream::Stdout), "out\n");
    assert_eq!(text(&chunks, LogStream::Stderr), "err\n");
    assert_eq!(text(&chunks, LogStream::System), "");
}

#[tokio::test]
async fn characters_split_between_reads_arrive_whole() {
    // `cat` writes in large blocks, so reads end at even offsets: inside a two-byte `é`.
    let (outcome, chunks) = run(command(
        r#"{ printf a; printf 'é%.0s' $(seq 100000); } > log; cat log; echo '{}' > "$CORQTY_OUTPUT""#,
    ))
    .await;

    outcome.expect("the attempt succeeds");
    assert_eq!(
        text(&chunks, LogStream::Stdout),
        format!("a{}", "é".repeat(100_000))
    );
}

#[tokio::test]
async fn twenty_mebibytes_of_output_stop_at_the_log_cap_without_blocking_the_command() {
    let (outcome, chunks) = timeout(
        Duration::from_secs(10),
        run(command(
            r#"head -c 20971520 /dev/zero | tr '\000' a; echo '{}' > "$CORQTY_OUTPUT""#,
        )),
    )
    .await
    .expect("the attempt finishes within 10 s");

    assert_eq!(outcome.expect("the attempt succeeds"), Map::new());
    assert!(chunks.iter().all(|chunk| chunk.text.len() <= 8192));
    assert_eq!(
        chunks.iter().map(|chunk| chunk.text.len()).sum::<usize>(),
        10_485_760
    );
}

#[tokio::test]
async fn a_non_zero_exit_fails_without_retry_and_names_the_exit_status() {
    let (outcome, _) = run(command(r#"echo '{}' > "$CORQTY_OUTPUT"; exit 3"#)).await;

    assert_eq!(
        outcome.expect_err("the attempt fails"),
        ExecutorError {
            code: "shell_exit".to_owned(),
            message: "The command failed (exit status: 3).".to_owned(),
            retryable: false,
        }
    );
}

#[tokio::test]
async fn a_missing_output_file_fails_without_retry() {
    let (outcome, _) = run(command("true")).await;

    assert_eq!(
        outcome.expect_err("the attempt fails"),
        ExecutorError {
            code: "shell_output_invalid".to_owned(),
            message: "The command did not write the output file.".to_owned(),
            retryable: false,
        }
    );
}

#[tokio::test]
async fn an_output_file_without_a_json_object_fails_without_retry() {
    for script in [
        r#"echo '[1]' > "$CORQTY_OUTPUT""#,
        r#"echo 'not json' > "$CORQTY_OUTPUT""#,
    ] {
        let (outcome, _) = run(command(script)).await;
        let error = outcome.expect_err("the attempt fails");

        assert_eq!(
            (error.code.as_str(), error.retryable),
            ("shell_output_invalid", false)
        );
        assert!(
            error
                .message
                .starts_with("The output file does not hold a JSON object: "),
            "{}",
            error.message
        );
    }
}

#[tokio::test]
async fn the_output_file_may_hold_at_most_one_mebibyte() {
    // `{"pad":""}` is 10 bytes.
    let script = |pad: usize| {
        format!(
            r#"printf '{{"pad":"%s"}}' "$(head -c {pad} /dev/zero | tr '\000' a)" > "$CORQTY_OUTPUT""#
        )
    };

    let (largest, _) = run(command(&script(1_048_566))).await;
    let (too_large, _) = run(command(&script(1_048_567))).await;

    assert_eq!(
        largest.expect("1 MiB is accepted")["pad"]
            .as_str()
            .map(str::len),
        Some(1_048_566)
    );
    assert_eq!(
        too_large.expect_err("1 MiB + 1 is rejected"),
        ExecutorError {
            code: "shell_output_invalid".to_owned(),
            message: "The output file is larger than 1 MiB.".to_owned(),
            retryable: false,
        }
    );
}

#[tokio::test]
async fn a_cancel_kills_the_whole_process_tree_within_five_seconds() {
    let (outcome, group, took) =
        cancel_once_started("sleep 30 & sleep 30 & echo $$ > group; wait").await;

    assert_eq!(
        outcome.expect_err("a canceled attempt fails"),
        ExecutorError {
            code: "canceled".to_owned(),
            message: "The attempt was canceled.".to_owned(),
            retryable: false,
        }
    );
    assert!(took < Duration::from_secs(5), "{took:?}");
    assert_eq!(survivors(group), Vec::<String>::new());
}

#[tokio::test]
async fn a_command_that_ignores_sigterm_is_killed_after_the_three_second_grace() {
    let (outcome, group, took) =
        cancel_once_started("trap '' TERM; sleep 30 & echo $$ > group; wait").await;

    assert_eq!(
        outcome.expect_err("a canceled attempt fails").code,
        "canceled"
    );
    assert!(took >= Duration::from_secs(3), "{took:?}");
    assert!(took < Duration::from_secs(5), "{took:?}");
    assert_eq!(survivors(group), Vec::<String>::new());
}

#[tokio::test]
async fn a_config_without_a_command_fails_without_retry() {
    let (outcome, _) = run(json!({"env": {}})).await;
    let error = outcome.expect_err("the attempt fails");

    assert_eq!(
        (error.code.as_str(), error.retryable),
        ("shell_config_invalid", false)
    );
}

#[test]
fn validate_accepts_the_seeded_default_and_allowed_variable_names() {
    for config in [
        json!({"command": r#"jq '{result: .}' "$CORQTY_INPUT" > "$CORQTY_OUTPUT""#, "env": {}}),
        json!({"command": "true", "env": {"API_URL": "https://example.test", "_TOKEN_2": ""}}),
        json!({"command": "true"}),
    ] {
        assert!(problems(&config).is_empty(), "{config}");
    }
}

#[test]
fn validate_locates_a_missing_or_empty_command_bad_variables_and_extra_keys() {
    assert_eq!(problems(&json!({})), ["invalid_config at "]);
    assert_eq!(
        problems(&json!({
            "command": "",
            "env": {"lower": "x", "CORQTY_INPUT": "x", "COUNT": 5},
            "shell": "bash",
        })),
        [
            "invalid_config at ",
            "invalid_config at /command",
            "invalid_config at /env",
            "invalid_config at /env",
            "invalid_config at /env/COUNT",
        ]
    );
}
```

- [ ] **Step 3: Run the tests on Linux and verify RED**

```text
docker run --rm -v "${PWD}:/work" -w /work -e CARGO_TARGET_DIR=/tmp/target rust:1.96-bookworm cargo test -p corqty-executor-shell
```

Expected: the first run in a fresh container prints `info: syncing channel updates for 1.96.0-x86_64-unknown-linux-gnu`
and downloads 5 components, because `rust-toolchain.toml` pins 1.96.0 and the image ships 1.96.1. It also
downloads the crates. The test then does not compile, because the crate exports nothing yet:

```text
error[E0432]: unresolved import `corqty_executor_shell::ShellExecutor`
 --> crates/executor-shell/tests/contract.rs:6:5
  |
6 | use corqty_executor_shell::ShellExecutor;
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ no `ShellExecutor` in the root
```

The last line is ``error: could not compile `corqty-executor-shell` (test "contract") due to 1 previous error``.
On a Windows host, `cargo test -p corqty-executor-shell` compiles and reports `running 0 tests` for
`tests\contract.rs`.

- [ ] **Step 4: Implement the shell executor**

Replace the whole of `crates/executor-shell/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

//! The trusted shell executor `shell@1.0.0` (spec §10.4). It runs `/bin/sh -c <command>` as
//! the worker's own user, in the attempt's scratch directory, with a small fixed environment.
//! It is not a security boundary: a command can do anything the worker container allows, so
//! only an organization admin can turn it on, for users they trust.

use corqty_domain::diagnostic::Diagnostic;
use corqty_domain::schema::{compile, instance_errors};
use corqty_plugin_sdk::{
    Capability, ExecuteRequest, ExecutionContext, Executor, ExecutorError, ExecutorMetadata,
    HOST_CONTRACT,
};
use futures_util::future::BoxFuture;
use serde_json::{Map, Value, json};

pub struct ShellExecutor;

impl Executor for ShellExecutor {
    fn metadata(&self) -> ExecutorMetadata {
        ExecutorMetadata {
            key: "shell",
            version: "1.0.0",
            host_contract: HOST_CONTRACT,
            capabilities: vec![
                Capability::Log,
                Capability::ScratchFs,
                Capability::CommandExec {
                    executables: vec!["/bin/sh".to_owned()],
                },
            ],
            timeout_retryable: true,
        }
    }

    /// The seeded `corqty.shell.command` config schema plus the rule `execute` applies to
    /// environment names.
    fn validate(&self, config: &Value) -> Vec<Diagnostic> {
        let schema = json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "minLength": 1, "maxLength": 16_384},
                "env": {
                    "type": "object",
                    "additionalProperties": {"type": "string"},
                    "propertyNames": {
                        "pattern": "^[A-Z_][A-Z0-9_]*$",
                        "not": {"pattern": "^CORQTY_"},
                    },
                },
            },
            "required": ["command"],
            "additionalProperties": false,
        });
        let validator = compile(&schema).expect("the shell config schema compiles");
        instance_errors(&validator, config)
            .into_iter()
            .map(|(pointer, message)| Diagnostic::new("invalid_config", pointer, message))
            .collect()
    }

    /// Writes the inputs to `CORQTY_INPUT`, runs the command and returns the JSON object it
    /// wrote to `CORQTY_OUTPUT`. Stdout and stderr become log chunks.
    fn execute<'a>(
        &'a self,
        request: ExecuteRequest,
        context: &'a ExecutionContext,
    ) -> BoxFuture<'a, Result<Map<String, Value>, ExecutorError>> {
        Box::pin(imp::execute(request, context))
    }
}

fn failure(code: &str, message: String, retryable: bool) -> ExecutorError {
    ExecutorError {
        code: code.to_owned(),
        message,
        retryable,
    }
}

#[cfg(unix)]
mod imp {
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::pin::pin;
    use std::process::Stdio;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use corqty_domain::runtime::{MAX_LOG_BYTES_PER_NODE_RUN, MAX_NODE_OUTPUT_BYTES};
    use corqty_plugin_sdk::{ExecuteRequest, ExecutionContext, ExecutorError, LogChunk, LogStream};
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;
    use serde::Deserialize;
    use serde_json::{Map, Value};
    use tokio::io::{AsyncRead, AsyncReadExt};
    use tokio::process::Command;
    use tokio::sync::mpsc::Sender;
    use tokio::time::timeout;

    use crate::failure;

    /// How long the shell may take to exit after SIGTERM before the group gets SIGKILL.
    const TERM_GRACE: Duration = Duration::from_secs(3);
    /// How long logs are still read once the group is dead. A process that left the group
    /// (`setsid`) could otherwise hold a pipe open forever.
    const DRAIN_GRACE: Duration = Duration::from_secs(2);
    const CHUNK_BYTES: usize = 8 * 1024;

    #[derive(Deserialize)]
    struct Config {
        command: String,
        #[serde(default)]
        env: BTreeMap<String, String>,
    }

    /// The command's process group. Dropping it SIGKILLs every process left in the group, so
    /// an attempt whose future the worker drops leaves nothing running.
    struct Group(Option<Pid>);

    impl Group {
        fn signal(&self, signal: Signal) {
            if let Some(group) = self.0 {
                // ESRCH means the group is already gone.
                let _ = killpg(group, signal);
            }
        }

        /// SIGKILLs the group and forgets it: the id of an empty group can be reused.
        fn kill(&mut self) {
            self.signal(Signal::SIGKILL);
            self.0 = None;
        }
    }

    impl Drop for Group {
        fn drop(&mut self) {
            self.kill();
        }
    }

    pub async fn execute(
        request: ExecuteRequest,
        context: &ExecutionContext,
    ) -> Result<Map<String, Value>, ExecutorError> {
        let config: Config = serde_json::from_value(request.config).map_err(|error| {
            failure(
                "shell_config_invalid",
                format!("The shell cannot read its config: {error}."),
                false,
            )
        })?;
        let scratch = &context.scratch_dir;
        let input = scratch.join("input.json");
        let output = scratch.join("output.json");
        // ponytail: blocking file I/O of at most 1 MiB in the scratch directory.
        std::fs::write(&input, Value::Object(request.inputs).to_string()).map_err(io_failure)?;

        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(&config.command)
            .current_dir(scratch)
            .env_clear()
            .env("PATH", "/usr/local/bin:/usr/bin:/bin")
            .env("LANG", "C.UTF-8")
            .env("HOME", scratch)
            .env("TMPDIR", scratch);
        for (name, value) in &config.env {
            if allowed(name) {
                command.env(name, value);
            } else {
                let text = format!(
                    "Skipped the environment variable {name}: names must match \
                     ^[A-Z_][A-Z0-9_]*$ and must not start with CORQTY_."
                );
                let chunk = LogChunk {
                    stream: LogStream::System,
                    text,
                };
                let _ = context.logs.send(chunk).await;
            }
        }
        command
            .env("CORQTY_INPUT", &input)
            .env("CORQTY_OUTPUT", &output)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // The shell leads a new process group, so one signal reaches every process the
            // command starts.
            .process_group(0);
        let mut child = command.spawn().map_err(io_failure)?;
        let mut group = Group(
            child
                .id()
                .and_then(|id| i32::try_from(id).ok())
                .map(Pid::from_raw),
        );

        let budget = AtomicUsize::new(MAX_LOG_BYTES_PER_NODE_RUN);
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let mut forwarding = pin!(async {
            tokio::join!(
                forward(stdout, LogStream::Stdout, &budget, &context.logs),
                forward(stderr, LogStream::Stderr, &budget, &context.logs),
            );
        });
        let mut supervising = pin!(async {
            let status = tokio::select! {
                biased;
                () = context.cancel.cancelled() => None,
                status = child.wait() => Some(status),
            };
            if status.is_none() {
                group.signal(Signal::SIGTERM);
                if timeout(TERM_GRACE, child.wait()).await.is_err() {
                    group.signal(Signal::SIGKILL);
                    let _ = child.wait().await;
                }
            }
            // Background jobs must not outlive the command or hold the pipes open.
            group.kill();
            status
        });
        let status = tokio::select! {
            status = &mut supervising => {
                let _ = timeout(DRAIN_GRACE, forwarding).await;
                status
            }
            () = &mut forwarding => supervising.await,
        };

        let Some(status) = status else {
            return Err(failure(
                "canceled",
                "The attempt was canceled.".to_owned(),
                false,
            ));
        };
        let status = status.map_err(io_failure)?;
        if !status.success() {
            return Err(failure(
                "shell_exit",
                format!("The command failed ({status})."),
                false,
            ));
        }
        read_output(&output)
    }

    /// Whether a config variable may reach the command: an upper-case name that cannot
    /// replace `CORQTY_INPUT` or `CORQTY_OUTPUT`.
    fn allowed(name: &str) -> bool {
        let mut bytes = name.bytes();
        bytes
            .next()
            .is_some_and(|first| first == b'_' || first.is_ascii_uppercase())
            && bytes.all(|byte| byte == b'_' || byte.is_ascii_uppercase() || byte.is_ascii_digit())
            && !name.starts_with("CORQTY_")
    }

    /// Sends a pipe to the log channel in chunks of at most 8 KiB while the attempt's log
    /// budget lasts. Then it keeps reading and drops the rest, so the command never blocks
    /// on a full pipe.
    async fn forward(
        mut pipe: impl AsyncRead + Unpin,
        stream: LogStream,
        budget: &AtomicUsize,
        logs: &Sender<LogChunk>,
    ) {
        let mut buffer = vec![0; CHUNK_BYTES];
        let mut pending = Vec::new();
        while let Ok(read @ 1..) = pipe.read(&mut buffer).await {
            pending.extend_from_slice(&buffer[..read]);
            // A character split between two reads waits for its last bytes.
            let complete = match std::str::from_utf8(&pending) {
                Err(error) if error.error_len().is_none() => error.valid_up_to(),
                _ => pending.len(),
            };
            let rest = pending.split_off(complete);
            let mut text = String::from_utf8_lossy(&pending).into_owned();
            pending = rest;
            let (Ok(left) | Err(left)) =
                budget.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                    Some(left.saturating_sub(text.len()))
                });
            text.truncate(text.floor_char_boundary(left));
            if !text.is_empty() {
                let _ = logs.send(LogChunk { stream, text }).await;
            }
        }
    }

    fn read_output(path: &Path) -> Result<Map<String, Value>, ExecutorError> {
        let invalid = |message: String| failure("shell_output_invalid", message, false);
        let size = std::fs::metadata(path)
            .map_err(|_| invalid("The command did not write the output file.".to_owned()))?
            .len();
        if size > MAX_NODE_OUTPUT_BYTES as u64 {
            return Err(invalid("The output file is larger than 1 MiB.".to_owned()));
        }
        let bytes = std::fs::read(path).map_err(io_failure)?;
        serde_json::from_slice(&bytes).map_err(|error| {
            invalid(format!(
                "The output file does not hold a JSON object: {error}."
            ))
        })
    }

    fn io_failure(error: std::io::Error) -> ExecutorError {
        failure(
            "shell_io_error",
            format!("The shell executor hit an I/O error: {error}."),
            true,
        )
    }
}

#[cfg(not(unix))]
mod imp {
    use corqty_plugin_sdk::{ExecuteRequest, ExecutionContext, ExecutorError};
    use serde_json::{Map, Value};

    use crate::failure;

    pub async fn execute(
        _request: ExecuteRequest,
        _context: &ExecutionContext,
    ) -> Result<Map<String, Value>, ExecutorError> {
        Err(failure(
            "shell_unsupported",
            "The shell executor runs only on Linux.".to_owned(),
            false,
        ))
    }
}
```

- [ ] **Step 5: Verify GREEN on Linux**

```text
docker run --rm -v "${PWD}:/work" -w /work -e CARGO_TARGET_DIR=/tmp/target rust:1.96-bookworm cargo test -p corqty-executor-shell
```

Expected: `tests/contract.rs` runs 15 tests, and all pass in about 3 s (the SIGTERM test waits out the 3 s
grace): `the_metadata_names_shell_1_0_0_with_scratch_and_bin_sh_capabilities`,
`the_command_reads_the_input_file_and_its_output_file_becomes_the_result`,
`the_command_sees_only_the_fixed_environment_and_allowed_config_variables`,
`stdout_and_stderr_arrive_as_log_chunks_of_their_own_stream`, `characters_split_between_reads_arrive_whole`,
`twenty_mebibytes_of_output_stop_at_the_log_cap_without_blocking_the_command`,
`a_non_zero_exit_fails_without_retry_and_names_the_exit_status`, `a_missing_output_file_fails_without_retry`,
`an_output_file_without_a_json_object_fails_without_retry`, `the_output_file_may_hold_at_most_one_mebibyte`,
`a_cancel_kills_the_whole_process_tree_within_five_seconds`,
`a_command_that_ignores_sigterm_is_killed_after_the_three_second_grace`,
`a_config_without_a_command_fails_without_retry`, `validate_accepts_the_seeded_default_and_allowed_variable_names`
and `validate_locates_a_missing_or_empty_command_bad_variables_and_extra_keys`.

Two negative controls show that the process tests can fail. Signalling only the shell (`child.start_kill()`)
instead of the group leaves both `sleep`s alive in `survivors`. Sending each read as it arrives, without holding
back a split character, makes `characters_split_between_reads_arrive_whole` fail on U+FFFD.

- [ ] **Step 6: Lint the unix code on Linux**

Windows never compiles `#[cfg(unix)] mod imp`, so the host's clippy run does not cover it.

```text
docker run --rm -v "${PWD}:/work" -w /work -e CARGO_TARGET_DIR=/tmp/target rust:1.96-bookworm cargo clippy --workspace --all-targets -- -D warnings
```

Expected: `Finished` with no warnings.

- [ ] **Step 7: Run the full backend verification**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo fmt --check
```

Expected: no output, exit code 0.

```text
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: `Finished` with no warnings.

```text
cargo test --workspace
```

Expected: every suite passes. corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 4,
`catalog_http` 4, `members_http` 3, `organizations_http` 1, `problems_http` 2, `publish_http` 2, `shutdown` 1,
`workflows_http` 4. corqty-domain: `account_and_rbac` 6, `catalog_rules` 7, `graph_rules` 8, `publish_rules` 10,
`run_rules` 8, `runtime_rules` 12. corqty-executor-mock: `contract` 11. corqty-executor-shell: `contract` 0 on
Windows and macOS, 15 on Linux. corqty-persistence: `auth_store` 2, `catalog_guards` 3, `final_admin` 2,
`migrations` 1, `tenant_isolation` 1, `test_reset` 1, `workflow_catalog` 6. corqty-plugin-host: `registry` 8. That
is 115 tests on Windows and 130 on Linux (CI).

- [ ] **Step 8: Commit**

```text
git add Cargo.toml Cargo.lock crates/executor-shell
git commit -m "feat: add the trusted shell executor"
```

Expected: `git show --stat HEAD` lists exactly the 5 files above (853 insertions, 1 deletion).

## Task 8: Store runs, node runs, run events, and worker instances

**Files:**

- Create: `migrations/202610260001_runs.sql`
- Create: `crates/persistence/src/runs.rs`
- Modify: `crates/persistence/src/lib.rs`
- Modify: `crates/persistence/src/store.rs`
- Modify: `crates/domain/src/run.rs`
- Modify: `crates/test-support/src/lib.rs`
- Test: `crates/persistence/tests/run_store.rs`

This task gives runs a place in the database, the schema of design §3.1-3.3, and the API side of the run repository
(§3.5). The worker functions that claim and complete nodes come in Task 9, and the HTTP routes come in Task 10.

Migration `202610260001_runs.sql` adds four tables:

- `runs` pins one published version. A composite foreign key `(organization_id, workflow_version_id)` points to
  `workflow_versions`, which therefore gains `UNIQUE (organization_id, id)`. The key does not cascade, because a
  version that runs reference is never deleted. Each run has a `status`, an optional `cancel_requested_at`, the
  run it repeats (`rerun_of`), and `last_event_seq`, which numbers its events.
- `node_runs` holds one row per node, inserted from the Task 3 plan. Roots start `queued` and every other node
  `blocked`. `children` and `pending_parents` drive the release of children (Task 9). The lease columns
  (`lease_token`, `leased_by`, `lease_expires_at`), `available_at`, `log_bytes` and `log_truncated` are there for
  the worker functions. Partial indexes cover claimable rows (`queued`, `retry_wait`) and expired leases.
- `run_events` is keyed by `(run_id, seq)`. Each run numbers its own events from 1 without gaps, so an event stream
  can resume after any seq.
- `worker_instances` is system-wide, because workers serve every organization. Host contracts stay an empty array
  until Milestone 4.

All four tables use FORCE row-level security. Members of the organization read runs, node runs and events.
`corqty.can_run` (admin, editor or runner) guards every write: inserting and updating runs and node runs, and
inserting events. `corqty_api` gets no DELETE, and no UPDATE on `run_events`, which are append-only. Workers write
through definer functions (Task 9), so the API itself only creates, reads and cancels runs. `worker_instances` has
a policy only so that the guard in `catalog_guards.rs` passes: `USING (false)` for `corqty_api`, which has no grant
on the table anyway. The API will learn about workers through a definer function as well. `corqty_worker` gets
USAGE on the `corqty` schema and no table privilege, so `catalog_guards.rs` stays green.

Two shared SQL functions keep the event numbering and the run's final status out of Rust. Both are SECURITY
INVOKER and granted to `corqty_api` only. When the API calls them, they run under its row-level security. When the
worker's definer functions call them, they run as the owner.

- `corqty.append_run_event(run, payload)` increments `runs.last_event_seq` and inserts the event with that `seq`,
  overwriting any `seq` in the payload. The UPDATE holds the runs row lock until commit. Every writer that changes
  a run's node runs takes that lock first, so events are numbered without gaps and committed in seq order. The
  function drops only top-level null fields from the payload, so `error: null` disappears. It does not use
  `jsonb_strip_nulls`, which would also strip nulls inside a node's output. It returns NULL when the caller cannot
  update the run.
- `corqty.settle_run(run)` finishes a run that is still `queued` or `running` once none of its nodes can change.
  It uses the rule of `corqty_domain::run::aggregate`: a failure wins, then a cancel request or a canceled node,
  else success. The function appends the terminal `run.state` event, which is the run's last event. A run without
  nodes settles at once, and a settled run is left alone.

A row trigger on `run_events` sends `pg_notify('corqty_run_events', run_id)`. The SSE route (Task 13) uses it to
wake streams. The payload is only the run id, never tenant data.

The repository `corqty_persistence::runs` works on the caller's tenant transaction:

- `create_run(conn, &NewRun)` inserts the run and the whole plan in one statement (`jsonb_to_recordset`, so
  `PlannedNode` now derives `Serialize`). It then appends `run.state queued` as event 1, calls `settle_run` for an
  empty plan, and writes the audit row in the same transaction: `run.started {runId, version}`, or
  `run.rerun {runId, sourceRunId, version}` when `rerun_of` is set. The resource is the run. Node states are not
  evented at creation, because the run snapshot carries them.
- `list_runs(conn, workflow)` returns the 50 newest runs. `get_run(conn, run)` reads the run (with the workflow
  name and the pinned graph) before its nodes, never the other way round. The node states are then at least as new
  as `lastEventSeq`.
- `cancel_run(conn, organization, actor, run)` locks the runs row first (`NotFound` if invisible, `Finished` if
  terminal). It then cancels the `blocked`, `queued` and `retry_wait` nodes in one statement ordered by id, with one
  `node.state canceled` event per node. That is the lock order every writer of a run follows. It sets
  `cancel_requested_at`, settles the run and audits `run.canceled {runId}`. Running nodes are left to their
  workers.
- `events_after(conn, run, after, limit)`, `append_run_event` and `settle_run` are thin wrappers.

`corqty.live_executors()` and the derived `queueReason` need the Task 9 functions, so they are not part of this
task.

`corqty-test-support` now exports the connection URLs `OWNER_URL`, `API_URL` and `WORKER_URL`. `TestDatabase` gains
a `worker` pool of 10 connections, enough for 8 concurrent claimers in Task 9. Concurrency tests open connections
of their own, because the API pool has only one. `PgStore::pool()` exposes the API pool for the background loops
of Tasks 12 and 13.

The tests use organization A, with one member per role, and organization B, with its own admin. As in
`workflow_catalog.rs`, A's editor publishes the simulated-infrastructure blueprint as version 1 of a workflow.

- [ ] **Step 1: Write the failing tests**

Create `crates/persistence/tests/run_store.rs`:

```rust
use std::time::Duration;

use corqty_domain::{
    publish::canonical_checksum,
    run::{NodeState, PlannedNode, RunStatus, aggregate, plan_run},
};
use corqty_persistence::{
    catalog,
    runs::{self, CancelError, NewRun},
    tenant::begin_tenant,
    workflows,
};
use corqty_test_support::{API_URL, OWNER_URL, TestDatabase};
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::{Connection, PgConnection, Postgres, Transaction, postgres::PgListener};
use tokio::task::JoinSet;
use uuid::Uuid;

/// Organization A with one member per role and a workflow whose version 1 is the simulated
/// infrastructure blueprint, and organization B with its own admin.
struct Fixture {
    database: TestDatabase,
    org_a: Uuid,
    org_b: Uuid,
    admin: Uuid,
    editor: Uuid,
    runner: Uuid,
    viewer: Uuid,
    admin_b: Uuid,
    workflow_id: Uuid,
    version_id: Uuid,
    /// Version 1's plan: vpc, then rds and deploy.
    plan: Vec<PlannedNode>,
}

impl Fixture {
    async fn new() -> Self {
        let database = TestDatabase::reset().await;
        let [org_a, org_b, admin, editor, runner, viewer, admin_b] =
            std::array::from_fn(|_| Uuid::new_v4());
        sqlx::query(
            "INSERT INTO users(id, email)
             SELECT id, id || '@example.com' FROM unnest($1::uuid[]) AS id",
        )
        .bind(vec![admin, editor, runner, viewer, admin_b])
        .execute(&database.owner)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO organizations(id, name, slug, created_by)
             VALUES ($1, 'A', 'a', $3), ($2, 'B', 'b', $4)",
        )
        .bind(org_a)
        .bind(org_b)
        .bind(admin)
        .bind(admin_b)
        .execute(&database.owner)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships(organization_id, user_id, role)
             VALUES ($1, $3, 'admin'), ($1, $4, 'editor'), ($1, $5, 'runner'),
                    ($1, $6, 'viewer'), ($2, $7, 'admin')",
        )
        .bind(org_a)
        .bind(org_b)
        .bind(admin)
        .bind(editor)
        .bind(runner)
        .bind(viewer)
        .bind(admin_b)
        .execute(&database.owner)
        .await
        .unwrap();

        let blueprint_id = "00000000-0000-7000-8000-000000000101".parse().unwrap();
        let mut tx = begin_tenant(&database.api, editor, org_a).await.unwrap();
        let blueprint = catalog::get_blueprint(&mut tx, blueprint_id).await.unwrap();
        let graph = blueprint.unwrap().graph;
        let workflow = workflows::insert_workflow(
            &mut tx,
            org_a,
            editor,
            "Provision",
            "provision",
            &graph,
            None,
        )
        .await
        .unwrap();
        let checksum = canonical_checksum(&graph);
        let version =
            workflows::insert_version(&mut tx, org_a, editor, workflow.id, 1, &graph, &checksum)
                .await
                .unwrap();
        let definitions = catalog::definitions_map(&mut tx).await.unwrap();
        tx.commit().await.unwrap();

        Self {
            database,
            org_a,
            org_b,
            admin,
            editor,
            runner,
            viewer,
            admin_b,
            workflow_id: workflow.id,
            version_id: version.id,
            plan: plan_run(&graph, &definitions).unwrap(),
        }
    }

    async fn tx(&self, user: Uuid, organization: Uuid) -> Transaction<'static, Postgres> {
        begin_tenant(&self.database.api, user, organization)
            .await
            .unwrap()
    }

    /// Runs `sql` as `user` in `organization` and rolls it back. Returns the rows it touched,
    /// or None when PostgreSQL refused it with 42501 (a privilege or row-level security check).
    async fn attempt(&self, user: Uuid, organization: Uuid, sql: &str) -> Option<u64> {
        let mut tx = self.tx(user, organization).await;
        let result = sqlx::query(sql).execute(&mut *tx).await;
        tx.rollback().await.unwrap();
        match result {
            Ok(done) => Some(done.rows_affected()),
            Err(error) => {
                assert_eq!(code(&error).as_deref(), Some("42501"), "{sql}: {error}");
                None
            }
        }
    }

    /// A manual run of version 1 in organization A.
    fn run<'a>(&self, actor: Uuid, input: &'a Value, nodes: &'a [PlannedNode]) -> NewRun<'a> {
        NewRun {
            organization_id: self.org_a,
            actor_user_id: actor,
            workflow_id: self.workflow_id,
            workflow_version_id: self.version_id,
            version: 1,
            input,
            nodes,
            rerun_of: None,
        }
    }

    /// Creates a run of `nodes` as the runner, commits it, and returns its id.
    async fn create(&self, nodes: &[PlannedNode]) -> Uuid {
        let input = json!({"region": "eu-west-1"});
        let mut tx = self.tx(self.runner, self.org_a).await;
        let run = runs::create_run(&mut tx, &self.run(self.runner, &input, nodes))
            .await
            .unwrap();
        tx.commit().await.unwrap();
        run.id
    }
}

fn code(error: &sqlx::Error) -> Option<String> {
    let code = error.as_database_error().and_then(|error| error.code());
    code.map(String::from)
}

/// A node without parents, so it starts queued.
fn root(node_id: &str) -> PlannedNode {
    PlannedNode {
        node_id: node_id.to_owned(),
        executor: "mock@1.0.0".to_owned(),
        max_attempts: 1,
        timeout_seconds: 900,
        children: Vec::new(),
        pending_parents: 0,
    }
}

fn run_state(state: &str, seq: i64) -> Value {
    json!({"type": "run.state", "state": state, "seq": seq})
}

fn node_canceled(node_id: &str, seq: i64) -> Value {
    json!({"type": "node.state", "nodeId": node_id, "state": "canceled", "attempt": 0, "seq": seq})
}

/// The run's event payloads in sequence order, read as the owner.
async fn events(database: &TestDatabase, run_id: Uuid) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM run_events WHERE run_id = $1 ORDER BY seq")
        .bind(run_id)
        .fetch_all(&database.owner)
        .await
        .unwrap()
}

/// Each node of the run with its state, in plan order.
async fn node_states(database: &TestDatabase, run_id: Uuid) -> Vec<(String, String)> {
    sqlx::query_as("SELECT node_id, state FROM node_runs WHERE run_id = $1 ORDER BY id")
        .bind(run_id)
        .fetch_all(&database.owner)
        .await
        .unwrap()
}

/// Every run audit row in order: (action, resource type, resource id, metadata).
async fn run_audit(database: &TestDatabase) -> Vec<(String, String, String, Value)> {
    sqlx::query_as(
        "SELECT action, resource_type, resource_id, metadata FROM audit_events
         WHERE action LIKE 'run.%'
         ORDER BY id",
    )
    .fetch_all(&database.owner)
    .await
    .unwrap()
}

/// An expected run audit row. The resource is the run.
fn audit(action: &str, run_id: Uuid, metadata: Value) -> (String, String, String, Value) {
    let action = action.to_owned();
    (action, "run".to_owned(), run_id.to_string(), metadata)
}

#[tokio::test]
#[serial]
async fn runners_create_runs_from_the_plan_and_viewers_cannot() {
    let fixture = Fixture::new().await;
    let (a, input) = (fixture.org_a, json!({"region": "eu-west-1"}));
    for (user, allowed) in [
        (fixture.admin, true),
        (fixture.editor, true),
        (fixture.viewer, false),
    ] {
        let mut tx = fixture.tx(user, a).await;
        let created = runs::create_run(&mut tx, &fixture.run(user, &input, &fixture.plan)).await;
        tx.rollback().await.unwrap();
        match created {
            Ok(_) => assert!(allowed, "{user}"),
            Err(error) => {
                assert!(!allowed, "{user}: {error}");
                assert_eq!(code(&error).as_deref(), Some("42501"));
            }
        }
    }

    let mut tx = fixture.tx(fixture.runner, a).await;
    let new = fixture.run(fixture.runner, &input, &fixture.plan);
    let first = runs::create_run(&mut tx, &new).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        (
            first.workflow_id,
            first.version,
            first.status.as_str(),
            first.trigger_source.as_str()
        ),
        (fixture.workflow_id, 1, "queued", "manual")
    );
    assert_eq!(
        (first.started_at, first.finished_at, first.cancel_requested),
        (None, None, false)
    );
    // Roots start queued; every other node waits for its distinct parents.
    let stored: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object(
             'nodeId', node_id, 'state', state, 'executor', executor, 'attempt', attempt,
             'maxAttempts', max_attempts, 'timeoutSeconds', timeout_seconds,
             'pendingParents', pending_parents, 'children', children
         )
         FROM node_runs WHERE run_id = $1 ORDER BY id",
    )
    .bind(first.id)
    .fetch_all(&fixture.database.owner)
    .await
    .unwrap();
    let row = |node_id: &str, state: &str, pending_parents: i32, children: &[&str]| {
        json!({
            "nodeId": node_id, "state": state, "executor": "mock@1.0.0", "attempt": 0,
            "maxAttempts": 1, "timeoutSeconds": 900, "pendingParents": pending_parents,
            "children": children
        })
    };
    assert_eq!(
        stored,
        [
            row("vpc", "queued", 0, &["deploy", "rds"]),
            row("rds", "blocked", 1, &[]),
            row("deploy", "blocked", 1, &[]),
        ]
    );
    // Node states are in the snapshot, so creation records only the run's own state.
    assert_eq!(
        events(&fixture.database, first.id).await,
        [run_state("queued", 1)]
    );

    let mut tx = fixture.tx(fixture.admin, a).await;
    let rerun = NewRun {
        rerun_of: Some(first.id),
        ..fixture.run(fixture.admin, &input, &fixture.plan)
    };
    let second = runs::create_run(&mut tx, &rerun).await.unwrap();
    tx.commit().await.unwrap();
    let rerun_of: Option<Uuid> = sqlx::query_scalar("SELECT rerun_of FROM runs WHERE id = $1")
        .bind(second.id)
        .fetch_one(&fixture.database.owner)
        .await
        .unwrap();
    assert_eq!(rerun_of, Some(first.id));

    // A plan without nodes has nothing to wait for, so its run succeeds at once.
    let mut tx = fixture.tx(fixture.runner, a).await;
    let new = fixture.run(fixture.runner, &input, &[]);
    let empty = runs::create_run(&mut tx, &new).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        (empty.status.as_str(), empty.finished_at.is_some()),
        ("succeeded", true)
    );
    assert_eq!(
        events(&fixture.database, empty.id).await,
        [run_state("queued", 1), run_state("succeeded", 2)]
    );

    let mut tx = fixture.tx(fixture.viewer, a).await;
    let listed = runs::list_runs(&mut tx, fixture.workflow_id).await.unwrap();
    let listed: Vec<Uuid> = listed.into_iter().map(|run| run.id).collect();
    assert_eq!(listed, [empty.id, second.id, first.id]);
    let stored = runs::get_run(&mut tx, first.id).await.unwrap().unwrap();
    let version = workflows::get_version(&mut tx, fixture.workflow_id, 1);
    let version = version.await.unwrap().unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        (
            stored.summary.id,
            stored.workflow_name.as_str(),
            &stored.input,
            stored.last_event_seq,
            &stored.graph
        ),
        (first.id, "Provision", &input, 1, &version.graph)
    );
    // Every field is present, with null for a missing value.
    let node = |node_id: &str, state: &str| {
        json!({
            "nodeId": node_id, "state": state, "attempt": 0, "maxAttempts": 1,
            "executor": "mock@1.0.0", "input": null, "output": null, "error": null,
            "startedAt": null, "finishedAt": null
        })
    };
    assert_eq!(
        serde_json::to_value(&stored.nodes).unwrap(),
        json!([
            node("vpc", "queued"),
            node("rds", "blocked"),
            node("deploy", "blocked")
        ])
    );

    assert_eq!(
        run_audit(&fixture.database).await,
        [
            audit(
                "run.started",
                first.id,
                json!({"runId": first.id, "version": 1})
            ),
            audit(
                "run.rerun",
                second.id,
                json!({"runId": second.id, "sourceRunId": first.id, "version": 1})
            ),
            audit(
                "run.started",
                empty.id,
                json!({"runId": empty.id, "version": 1})
            ),
        ]
    );
}

#[tokio::test]
#[serial]
async fn runs_node_runs_and_events_stay_inside_their_organization() {
    let fixture = Fixture::new().await;
    let (a, workflow, version) = (fixture.org_a, fixture.workflow_id, fixture.version_id);
    let run = fixture.create(&fixture.plan).await;

    // Organization B finds nothing through the repository and changes nothing.
    let (b, admin_b) = (fixture.org_b, fixture.admin_b);
    let mut tx = fixture.tx(admin_b, b).await;
    assert!(runs::list_runs(&mut tx, workflow).await.unwrap().is_empty());
    assert!(runs::get_run(&mut tx, run).await.unwrap().is_none());
    assert!(
        runs::events_after(&mut tx, run, 0, 500)
            .await
            .unwrap()
            .is_empty()
    );
    let event = json!({"type": "run.state", "state": "running"});
    let appended = runs::append_run_event(&mut tx, run, &event).await.unwrap();
    assert_eq!(appended, None);
    let canceled = runs::cancel_run(&mut tx, b, admin_b, run).await;
    assert!(
        matches!(canceled, Err(CancelError::NotFound)),
        "{canceled:?}"
    );
    tx.commit().await.unwrap();
    assert_eq!(
        events(&fixture.database, run).await,
        [run_state("queued", 1)]
    );

    for (table, rows) in [("runs", 1), ("node_runs", 3), ("run_events", 1)] {
        let sql = format!("SELECT FROM {table} WHERE organization_id = '{a}'");
        let viewer = fixture.attempt(fixture.viewer, a, &sql).await;
        assert_eq!(viewer, Some(rows), "{sql}");
        let other = fixture.attempt(admin_b, b, &sql).await;
        assert_eq!(other, Some(0), "{sql}");
    }

    // (statement, rows for A's admin, editor and runner, rows for A's viewer and B's admin)
    let writes = [
        (
            format!(
                "INSERT INTO runs(organization_id, workflow_id, workflow_version_id, version, input)
                 VALUES ('{a}', '{workflow}', '{version}', 1, '{{}}')"
            ),
            Some(1),
            None,
        ),
        (
            format!(
                "INSERT INTO node_runs(
                     organization_id, run_id, node_id, executor, state,
                     max_attempts, timeout_seconds, pending_parents
                 ) VALUES ('{a}', '{run}', 'extra', 'mock@1.0.0', 'queued', 1, 900, 0)"
            ),
            Some(1),
            None,
        ),
        (
            format!(
                "INSERT INTO run_events(organization_id, run_id, seq, payload)
                 VALUES ('{a}', '{run}', 2, '{{}}')"
            ),
            Some(1),
            None,
        ),
        ("UPDATE runs SET input = input".to_owned(), Some(1), Some(0)),
        (
            "UPDATE node_runs SET attempt = attempt".to_owned(),
            Some(3),
            Some(0),
        ),
    ];
    for (sql, runners, others) in &writes {
        for (user, organization, expected) in [
            (fixture.admin, a, runners),
            (fixture.editor, a, runners),
            (fixture.runner, a, runners),
            (fixture.viewer, a, others),
            (admin_b, b, others),
        ] {
            let rows = fixture.attempt(user, organization, sql).await;
            assert_eq!(rows, *expected, "{sql}");
        }
    }

    // Events are append-only, and nothing about a run is ever deleted.
    for sql in [
        "UPDATE run_events SET payload = payload",
        "DELETE FROM runs",
        "DELETE FROM node_runs",
        "DELETE FROM run_events",
    ] {
        assert_eq!(fixture.attempt(fixture.admin, a, sql).await, None, "{sql}");
    }
}

#[tokio::test]
#[serial]
async fn concurrent_appends_number_the_events_of_a_run_without_gaps() {
    let fixture = Fixture::new().await;
    let run_id = fixture.create(&fixture.plan).await;

    // Eight connections of their own, so that the appends really overlap.
    let mut writers = JoinSet::new();
    for writer in 0..8 {
        writers.spawn(async move {
            let mut conn = PgConnection::connect(OWNER_URL).await.unwrap();
            for line in 0..25 {
                let text = format!("writer {writer} line {line}");
                let event =
                    json!({"type": "node.log", "nodeId": "vpc", "stream": "stdout", "text": text});
                let seq = runs::append_run_event(&mut conn, run_id, &event).await;
                assert!(seq.unwrap().is_some());
            }
        });
    }
    writers.join_all().await;

    let stored: Vec<(i64, Value)> =
        sqlx::query_as("SELECT seq, payload FROM run_events WHERE run_id = $1 ORDER BY seq")
            .bind(run_id)
            .fetch_all(&fixture.database.owner)
            .await
            .unwrap();
    let seqs: Vec<i64> = stored.iter().map(|(seq, _)| *seq).collect();
    assert_eq!(seqs, (1..=201).collect::<Vec<_>>());
    assert!(stored.iter().all(|(seq, payload)| payload["seq"] == *seq));
    let last: i64 = sqlx::query_scalar("SELECT last_event_seq FROM runs WHERE id = $1")
        .bind(run_id)
        .fetch_one(&fixture.database.owner)
        .await
        .unwrap();
    assert_eq!(last, 201);
}

#[tokio::test]
#[serial]
async fn appended_events_drop_only_their_own_null_fields() {
    let fixture = Fixture::new().await;
    let run_id = fixture.create(&fixture.plan).await;

    // A null that belongs to a node's output is data and stays.
    let mut tx = fixture.tx(fixture.runner, fixture.org_a).await;
    let event = json!({
        "type": "node.state", "nodeId": "vpc", "state": "succeeded", "attempt": 1,
        "error": null, "output": {"vpcId": "vpc-1", "tags": null}, "seq": 99
    });
    let seq = runs::append_run_event(&mut tx, run_id, &event)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(seq, Some(2));
    assert_eq!(
        events(&fixture.database, run_id).await[1],
        json!({
            "type": "node.state", "nodeId": "vpc", "state": "succeeded", "attempt": 1,
            "output": {"vpcId": "vpc-1", "tags": null}, "seq": 2
        })
    );
}

#[tokio::test]
#[serial]
async fn each_event_notifies_listeners_with_its_run_id() {
    let fixture = Fixture::new().await;
    // The API listens as itself; LISTEN needs no privilege.
    let mut listener = PgListener::connect(API_URL).await.unwrap();
    listener.listen("corqty_run_events").await.unwrap();

    let run_id = fixture.create(&fixture.plan).await;

    let notification = tokio::time::timeout(Duration::from_secs(5), listener.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (notification.channel(), notification.payload()),
        ("corqty_run_events", run_id.to_string().as_str())
    );
}

#[tokio::test]
#[serial]
async fn settling_gives_a_finished_run_the_status_the_domain_aggregates() {
    use NodeState::{Blocked, Canceled, Failed, Queued, RetryWait, Running, Skipped, Succeeded};
    let fixture = Fixture::new().await;
    let owner = &fixture.database.owner;
    let cases: [(&[NodeState], bool); 9] = [
        (&[Succeeded, Running], false),
        (&[Queued, Blocked], true),
        (&[RetryWait, Canceled], true),
        (&[Succeeded, Succeeded], false),
        (&[Succeeded, Succeeded], true),
        (&[Succeeded, Canceled, Skipped], false),
        (&[Failed, Skipped], false),
        (&[Succeeded, Failed, Canceled, Skipped], true),
        (&[], false),
    ];
    for (states, cancel_requested) in cases {
        let nodes: Vec<_> = (0..states.len())
            .map(|index| root(&format!("n{index}")))
            .collect();
        let run_id = fixture.create(&nodes).await;
        let node_ids: Vec<&str> = nodes.iter().map(|node| node.node_id.as_str()).collect();
        let texts: Vec<&str> = states.iter().map(|state| state.as_str()).collect();
        sqlx::query(
            "UPDATE node_runs n SET state = s.state
             FROM unnest($2::text[], $3::text[]) AS s(node_id, state)
             WHERE n.run_id = $1 AND n.node_id = s.node_id",
        )
        .bind(run_id)
        .bind(&node_ids)
        .bind(&texts)
        .execute(owner)
        .await
        .unwrap();
        sqlx::query("UPDATE runs SET cancel_requested_at = now() WHERE id = $1 AND $2")
            .bind(run_id)
            .bind(cancel_requested)
            .execute(owner)
            .await
            .unwrap();
        // The second call finds the run settled and leaves it alone.
        for _ in 0..2 {
            let mut tx = fixture.tx(fixture.runner, fixture.org_a).await;
            runs::settle_run(&mut tx, run_id).await.unwrap();
            tx.commit().await.unwrap();
        }

        let expected = aggregate(states, cancel_requested);
        let stored: (String, bool) =
            sqlx::query_as("SELECT status, finished_at IS NOT NULL FROM runs WHERE id = $1")
                .bind(run_id)
                .fetch_one(owner)
                .await
                .unwrap();
        let status = expected.map_or("queued", RunStatus::as_str);
        let case = format!("{states:?} with cancel requested {cancel_requested}");
        assert_eq!(stored, (status.to_owned(), expected.is_some()), "{case}");
        let mut log = vec![run_state("queued", 1)];
        log.extend(expected.map(|status| run_state(status.as_str(), 2)));
        assert_eq!(events(&fixture.database, run_id).await, log, "{case}");
    }
}

#[tokio::test]
#[serial]
async fn canceling_cancels_the_waiting_nodes_and_settles_the_run_once_nothing_runs() {
    let fixture = Fixture::new().await;
    let (a, runner, owner) = (fixture.org_a, fixture.runner, &fixture.database.owner);
    let waiting = fixture.create(&fixture.plan).await;
    let started = fixture.create(&fixture.plan).await;
    // As a worker would leave it: the second run's vpc is running.
    sqlx::query(
        "UPDATE node_runs SET state = 'running', attempt = 1, started_at = now()
         WHERE run_id = $1 AND node_id = 'vpc'",
    )
    .bind(started)
    .execute(owner)
    .await
    .unwrap();
    sqlx::query("UPDATE runs SET status = 'running', started_at = now() WHERE id = $1")
        .bind(started)
        .execute(owner)
        .await
        .unwrap();

    // A viewer cannot lock the run, and organization B cannot see it.
    for (user, organization) in [(fixture.viewer, a), (fixture.admin_b, fixture.org_b)] {
        let mut tx = fixture.tx(user, organization).await;
        let canceled = runs::cancel_run(&mut tx, organization, user, waiting).await;
        tx.rollback().await.unwrap();
        assert!(
            matches!(canceled, Err(CancelError::NotFound)),
            "{canceled:?}"
        );
    }

    let mut tx = fixture.tx(runner, a).await;
    let canceled = runs::cancel_run(&mut tx, a, runner, waiting).await.unwrap();
    let requested = runs::cancel_run(&mut tx, a, runner, started).await.unwrap();
    tx.commit().await.unwrap();
    let summary = |run: &runs::RunSummaryRecord| {
        let finished = run.finished_at.is_some();
        (run.status.clone(), run.cancel_requested, finished)
    };
    assert_eq!(summary(&canceled), ("canceled".to_owned(), true, true));
    assert_eq!(
        events(&fixture.database, waiting).await,
        [
            run_state("queued", 1),
            node_canceled("vpc", 2),
            node_canceled("rds", 3),
            node_canceled("deploy", 4),
            run_state("canceled", 5),
        ]
    );
    // The running node is left to its worker, so the run is not settled yet.
    assert_eq!(summary(&requested), ("running".to_owned(), true, false));
    let state = |node_id: &str, state: &str| (node_id.to_owned(), state.to_owned());
    assert_eq!(
        node_states(&fixture.database, started).await,
        [
            state("vpc", "running"),
            state("rds", "canceled"),
            state("deploy", "canceled")
        ]
    );
    assert_eq!(
        events(&fixture.database, started).await,
        [
            run_state("queued", 1),
            node_canceled("rds", 2),
            node_canceled("deploy", 3),
        ]
    );

    // Once the worker has stopped the node, the run settles as canceled, and a finished
    // run cannot be canceled again.
    sqlx::query("UPDATE node_runs SET state = 'canceled' WHERE run_id = $1 AND node_id = 'vpc'")
        .bind(started)
        .execute(owner)
        .await
        .unwrap();
    let mut tx = fixture.tx(runner, a).await;
    runs::settle_run(&mut tx, started).await.unwrap();
    let again = runs::cancel_run(&mut tx, a, runner, waiting).await;
    tx.commit().await.unwrap();
    assert!(matches!(again, Err(CancelError::Finished)), "{again:?}");
    let log = events(&fixture.database, started).await;
    assert_eq!(log.last(), Some(&run_state("canceled", 4)));

    let mut tx = fixture.tx(fixture.viewer, a).await;
    let page = runs::events_after(&mut tx, waiting, 1, 2).await.unwrap();
    tx.commit().await.unwrap();
    let page: Vec<_> = page
        .into_iter()
        .map(|event| (event.seq, event.payload))
        .collect();
    assert_eq!(
        page,
        [(2, node_canceled("vpc", 2)), (3, node_canceled("rds", 3))]
    );

    assert_eq!(
        run_audit(&fixture.database).await[2..],
        [
            audit("run.canceled", waiting, json!({"runId": waiting})),
            audit("run.canceled", started, json!({"runId": started})),
        ]
    );
}

#[tokio::test]
#[serial]
async fn worker_instances_stay_hidden_and_the_worker_role_reaches_no_run_data() {
    let fixture = Fixture::new().await;
    let run_id = fixture.create(&fixture.plan).await;
    sqlx::query(
        "INSERT INTO worker_instances(id, executors, capacity)
         VALUES ($1, ARRAY['mock@1.0.0'], 4)",
    )
    .bind(Uuid::now_v7())
    .execute(&fixture.database.owner)
    .await
    .unwrap();

    // The API learns about workers only through definer functions.
    let sql = "SELECT FROM worker_instances";
    for (user, organization) in [
        (fixture.admin, fixture.org_a),
        (fixture.admin_b, fixture.org_b),
    ] {
        assert_eq!(fixture.attempt(user, organization, sql).await, None);
    }

    // The worker has no table grants, and the shared run functions belong to the API.
    for sql in [
        "SELECT FROM runs".to_owned(),
        "SELECT FROM worker_instances".to_owned(),
        format!("SELECT corqty.append_run_event('{run_id}', '{{}}')"),
        format!("SELECT corqty.settle_run('{run_id}')"),
    ] {
        let error = sqlx::query(&sql)
            .execute(&fixture.database.worker)
            .await
            .unwrap_err();
        assert_eq!(code(&error).as_deref(), Some("42501"), "{sql}: {error}");
    }
}
```

The eight tests:

- `runners_create_runs_from_the_plan_and_viewers_cannot`: admins and editors may create runs, and a viewer gets
  42501. The stored plan matches the expected rows. The run lists newest first and reads back as described above.
  An empty plan succeeds at once. A rerun records `rerun_of`, and the audit rows match exactly.
- `runs_node_runs_and_events_stay_inside_their_organization`: through the repository and through raw SQL,
  organization B sees and changes nothing. Only admins, editors and runners of A write, and nobody updates an
  event or deletes anything.
- `concurrent_appends_number_the_events_of_a_run_without_gaps`: 8 connections append 25 events each, and the seqs
  come out exactly 1 to 201.
- `appended_events_drop_only_their_own_null_fields`: a null inside a node's output survives, and the stored `seq`
  replaces the caller's.
- `each_event_notifies_listeners_with_its_run_id`: a `PgListener` on `API_URL` receives the run id.
- `settling_gives_a_finished_run_the_status_the_domain_aggregates`: nine state mixes, each compared with
  `aggregate`.
- `canceling_cancels_the_waiting_nodes_and_settles_the_run_once_nothing_runs`: the events, a running node that
  holds the run open until it stops, `Finished` on the second cancel, `events_after` paging, and the audit rows.
- `worker_instances_stay_hidden_and_the_worker_role_reaches_no_run_data`: `corqty_api` and `corqty_worker` both
  get 42501.

- [ ] **Step 2: Run the tests and verify RED**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-persistence --test run_store
```

Expected: the test does not compile. The repository module, the URLs and the worker pool do not exist yet:

```text
error[E0432]: unresolved import `corqty_persistence::runs`
 --> crates\persistence\tests\run_store.rs:9:5
  |
9 |     runs::{self, CancelError, NewRun},
  |     ^^^^ could not find `runs` in `corqty_persistence`

error[E0432]: unresolved imports `corqty_test_support::API_URL`, `corqty_test_support::OWNER_URL`
  --> crates\persistence\tests\run_store.rs:13:27
   |
13 | use corqty_test_support::{API_URL, OWNER_URL, TestDatabase};
   |                           ^^^^^^^  ^^^^^^^^^ no `OWNER_URL` in the root
   |                           |
   |                           no `API_URL` in the root

error[E0609]: no field `worker` on type `TestDatabase`
   --> crates\persistence\tests\run_store.rs:769:40
    |
769 |             .execute(&fixture.database.worker)
    |                                        ^^^^^^ unknown field
    |
    = note: available fields are: `owner`, `api`
```

The last line is ``error: could not compile `corqty-persistence` (test "run_store") due to 3 previous errors``.

- [ ] **Step 3: Export the connection URLs and a worker pool from test-support**

In `crates/test-support/src/lib.rs`:

Replace:

```rust
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

pub struct TestDatabase {
    pub owner: PgPool,
    pub api: PgPool,
}
```

With:

```rust
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

/// The test database as each role connects to it. Tests that need connections of their own,
/// for example a listener or concurrent writers, connect with these.
pub const OWNER_URL: &str = "postgres://corqty_owner:corqty_owner_dev@127.0.0.1:55432/corqty_test";
pub const API_URL: &str = "postgres://corqty_api:corqty_api_dev@127.0.0.1:55432/corqty_test";
pub const WORKER_URL: &str =
    "postgres://corqty_worker:corqty_worker_dev@127.0.0.1:55432/corqty_test";

pub struct TestDatabase {
    pub owner: PgPool,
    pub api: PgPool,
    pub worker: PgPool,
}
```

Replace:

```rust
            .connect("postgres://corqty_owner:corqty_owner_dev@127.0.0.1:55432/corqty_test")
```

With:

```rust
            .connect(OWNER_URL)
```

Replace:

```rust
            .connect("postgres://corqty_api:corqty_api_dev@127.0.0.1:55432/corqty_test")
```

With:

```rust
            .connect(API_URL)
```

Replace:

```rust
        Self { owner, api }
```

With:

```rust
        let worker = PgPoolOptions::new()
            // Enough for eight workers that claim at the same time.
            .max_connections(10)
            .connect(WORKER_URL)
            .await
            .expect("connect worker test database");

        Self { owner, api, worker }
```

The roles and their `*_dev` passwords already exist in the test database and in CI, because
`docker/postgres/init/001-roles.sh` creates `corqty_worker` with `CONNECT`.

- [ ] **Step 4: Add the run tables, policies and shared run functions**

Create `migrations/202610260001_runs.sql`:

```sql
CREATE FUNCTION corqty.can_run(target_organization_id uuid)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    SELECT EXISTS (
        SELECT 1
        FROM memberships
        WHERE organization_id = target_organization_id
          AND user_id = corqty.current_user_id()
          AND role IN ('admin', 'editor', 'runner')
    )
$$;

REVOKE ALL ON FUNCTION corqty.can_run(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.can_run(uuid) TO corqty_api;

-- Workers reach run data only through definer functions in this schema, each granted to
-- corqty_worker on its own. The role gets no table privileges.
GRANT USAGE ON SCHEMA corqty TO corqty_worker;

-- Lets runs reference a version of their own organization only.
ALTER TABLE workflow_versions ADD CONSTRAINT workflow_versions_org_id UNIQUE (organization_id, id);

-- A run pins one published version, which is never deleted while runs reference it.
CREATE TABLE runs (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    workflow_id uuid NOT NULL,
    workflow_version_id uuid NOT NULL,
    version integer NOT NULL,
    trigger_source text NOT NULL DEFAULT 'manual' CHECK (trigger_source IN ('manual')),
    input jsonb NOT NULL,
    status text NOT NULL DEFAULT 'queued'
        CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'canceled')),
    cancel_requested_at timestamptz,
    rerun_of uuid REFERENCES runs(id) ON DELETE SET NULL,
    created_by uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    started_at timestamptz,
    finished_at timestamptz,
    last_event_seq bigint NOT NULL DEFAULT 0,
    FOREIGN KEY (organization_id, workflow_id) REFERENCES workflows (organization_id, id) ON DELETE CASCADE,
    FOREIGN KEY (organization_id, workflow_version_id) REFERENCES workflow_versions (organization_id, id),
    CONSTRAINT runs_org_id UNIQUE (organization_id, id)
);
CREATE INDEX runs_workflow_created_idx ON runs (workflow_id, created_at DESC, id DESC);

-- One row per node of a run. `children` and `pending_parents` come from the plan: a node
-- that succeeds decrements each child once, and the child is queued when it reaches 0.
CREATE TABLE node_runs (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    organization_id uuid NOT NULL,
    run_id uuid NOT NULL,
    node_id text NOT NULL,
    executor text NOT NULL,
    state text NOT NULL CHECK (
        state IN (
            'blocked', 'queued', 'running', 'retry_wait',
            'succeeded', 'failed', 'canceled', 'skipped'
        )
    ),
    attempt integer NOT NULL DEFAULT 0,
    max_attempts integer NOT NULL CHECK (max_attempts BETWEEN 1 AND 5),
    timeout_seconds integer NOT NULL CHECK (timeout_seconds BETWEEN 1 AND 14400),
    pending_parents integer NOT NULL CHECK (pending_parents >= 0),
    children text[] NOT NULL DEFAULT '{}',
    available_at timestamptz NOT NULL DEFAULT now(),
    lease_token bigint NOT NULL DEFAULT 0,
    leased_by uuid,
    lease_expires_at timestamptz,
    input jsonb,
    output jsonb,
    error jsonb,
    log_bytes bigint NOT NULL DEFAULT 0,
    log_truncated boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    started_at timestamptz,
    finished_at timestamptz,
    FOREIGN KEY (organization_id, run_id) REFERENCES runs (organization_id, id) ON DELETE CASCADE,
    CONSTRAINT node_runs_run_node UNIQUE (run_id, node_id)
);
CREATE INDEX node_runs_claimable ON node_runs (available_at, created_at)
    WHERE state IN ('queued', 'retry_wait');
CREATE INDEX node_runs_expired ON node_runs (lease_expires_at) WHERE state = 'running';

-- Each run numbers its own events from 1 without gaps, so a stream can resume after any seq.
CREATE TABLE run_events (
    organization_id uuid NOT NULL,
    run_id uuid NOT NULL,
    seq bigint NOT NULL,
    payload jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (run_id, seq),
    FOREIGN KEY (organization_id, run_id) REFERENCES runs (organization_id, id) ON DELETE CASCADE
);

-- System-wide: workers serve every organization. Host contracts stay empty until plugins
-- are loaded at run time.
CREATE TABLE worker_instances (
    id uuid PRIMARY KEY,
    executors text[] NOT NULL,
    host_contracts text[] NOT NULL DEFAULT '{}',
    capacity integer NOT NULL CHECK (capacity >= 1),
    started_at timestamptz NOT NULL DEFAULT now(),
    last_heartbeat_at timestamptz NOT NULL DEFAULT now()
);

-- Wakes the API's event streams. The payload is the run id, never tenant data, and the
-- notifications of one transaction for one run arrive as one at commit.
CREATE FUNCTION corqty.notify_run_event()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM pg_notify('corqty_run_events', NEW.run_id::text);
    RETURN NULL;
END
$$;

REVOKE ALL ON FUNCTION corqty.notify_run_event() FROM PUBLIC;

CREATE TRIGGER run_events_notify AFTER INSERT ON run_events
    FOR EACH ROW EXECUTE FUNCTION corqty.notify_run_event();

-- Appends one event and returns its seq, or NULL when the caller cannot update the run. The
-- UPDATE holds the runs row lock until commit (writers of a run's node_runs take it first),
-- so concurrent appends number events without gaps and commit them in seq order. Only
-- top-level nulls are dropped: a null inside a node's output is data.
CREATE FUNCTION corqty.append_run_event(p_run_id uuid, p_payload jsonb)
RETURNS bigint
LANGUAGE sql
SET search_path = public, pg_temp
AS $$
    WITH run AS (
        UPDATE runs
        SET last_event_seq = last_event_seq + 1
        WHERE id = p_run_id
        RETURNING organization_id, last_event_seq
    )
    INSERT INTO run_events(organization_id, run_id, seq, payload)
    SELECT organization_id, p_run_id, last_event_seq,
           (p_payload - ARRAY(SELECT key FROM jsonb_each(p_payload) WHERE value = 'null'::jsonb))
               || jsonb_build_object('seq', last_event_seq)
    FROM run
    RETURNING seq
$$;

-- Finishes the run once none of its nodes can change: a failure wins, then a cancel request
-- or a canceled node, else success (the rule of corqty_domain::run::aggregate). A run
-- without nodes finishes at once. The terminal run.state event is the run's last event.
CREATE FUNCTION corqty.settle_run(p_run_id uuid)
RETURNS void
LANGUAGE plpgsql
SET search_path = public, pg_temp
AS $$
DECLARE
    v_status text;
BEGIN
    UPDATE runs AS r
    SET status = CASE
            WHEN nodes.failed > 0 THEN 'failed'
            WHEN r.cancel_requested_at IS NOT NULL OR nodes.canceled > 0 THEN 'canceled'
            ELSE 'succeeded'
        END,
        finished_at = now()
    FROM (
        SELECT count(*) FILTER (WHERE state = 'failed') AS failed,
               count(*) FILTER (WHERE state = 'canceled') AS canceled,
               count(*) FILTER (
                   WHERE state NOT IN ('succeeded', 'failed', 'canceled', 'skipped')
               ) AS pending
        FROM node_runs
        WHERE run_id = p_run_id
    ) AS nodes
    WHERE r.id = p_run_id
      AND r.status IN ('queued', 'running')
      AND nodes.pending = 0
    RETURNING r.status INTO v_status;
    IF FOUND THEN
        PERFORM corqty.append_run_event(
            p_run_id,
            jsonb_build_object('type', 'run.state', 'state', v_status)
        );
    END IF;
END
$$;

-- SECURITY INVOKER: called by corqty_api they run under its row-level security, and called
-- from the worker's definer functions they run as the owner.
REVOKE ALL ON FUNCTION corqty.append_run_event(uuid, jsonb) FROM PUBLIC;
REVOKE ALL ON FUNCTION corqty.settle_run(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.append_run_event(uuid, jsonb) TO corqty_api;
GRANT EXECUTE ON FUNCTION corqty.settle_run(uuid) TO corqty_api;

-- Workers write runs through definer functions, so corqty_api only creates, reads and
-- cancels them. Events are append-only, and nothing is deleted.
GRANT SELECT, INSERT, UPDATE ON runs, node_runs TO corqty_api;
GRANT SELECT, INSERT ON run_events TO corqty_api;

ALTER TABLE runs ENABLE ROW LEVEL SECURITY;
ALTER TABLE runs FORCE ROW LEVEL SECURITY;
CREATE POLICY runs_select ON runs
    FOR SELECT TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.is_member(corqty.current_organization_id()))
    );
CREATE POLICY runs_insert ON runs
    FOR INSERT TO corqty_api
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_run(corqty.current_organization_id()))
    );
CREATE POLICY runs_update ON runs
    FOR UPDATE TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_run(corqty.current_organization_id()))
    )
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_run(corqty.current_organization_id()))
    );

ALTER TABLE node_runs ENABLE ROW LEVEL SECURITY;
ALTER TABLE node_runs FORCE ROW LEVEL SECURITY;
CREATE POLICY node_runs_select ON node_runs
    FOR SELECT TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.is_member(corqty.current_organization_id()))
    );
CREATE POLICY node_runs_insert ON node_runs
    FOR INSERT TO corqty_api
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_run(corqty.current_organization_id()))
    );
CREATE POLICY node_runs_update ON node_runs
    FOR UPDATE TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_run(corqty.current_organization_id()))
    )
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_run(corqty.current_organization_id()))
    );

ALTER TABLE run_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE run_events FORCE ROW LEVEL SECURITY;
CREATE POLICY run_events_select ON run_events
    FOR SELECT TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.is_member(corqty.current_organization_id()))
    );
CREATE POLICY run_events_insert ON run_events
    FOR INSERT TO corqty_api
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_run(corqty.current_organization_id()))
    );

-- No role reads worker_instances directly: the policy only satisfies the forced row-level
-- security guard, and corqty_api has no grant on the table.
ALTER TABLE worker_instances ENABLE ROW LEVEL SECURITY;
ALTER TABLE worker_instances FORCE ROW LEVEL SECURITY;
CREATE POLICY worker_instances_none ON worker_instances
    FOR SELECT TO corqty_api
    USING (false);
```

`append_run_event` is a SQL function: the UPDATE in its CTE takes the runs row lock, and the INSERT reads the new
`last_event_seq` from it. `settle_run` is plpgsql because it appends an event after its UPDATE. A second UPDATE of
the same row inside one SQL statement would be unpredictable. Its variable is `v_status` and its parameter
`p_run_id`, so neither clashes with a column name.

- [ ] **Step 5: Let a plan serialize with its column names**

In `crates/domain/src/run.rs`:

Replace:

```rust
/// The `node_runs` row a node starts a run with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedNode {
```

With:

```rust
/// The `node_runs` row a node starts a run with. It serializes with the column names, so a
/// whole plan can be inserted from one JSON array.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlannedNode {
```

`children` is a `text[]` per row, and an array of arrays cannot be bound with `UNNEST`. `jsonb_to_recordset` turns
each JSON array into a `text[]` column instead.

- [ ] **Step 6: Add the runs repository and the pool accessor**

Create `crates/persistence/src/runs.rs`:

```rust
use chrono::{DateTime, Utc};
use corqty_domain::{graph::GraphSnapshot, run::PlannedNode};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, types::Json};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum CancelError {
    #[error("run was not found")]
    NotFound,
    #[error("run has already finished")]
    Finished,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// A run of a published version, with the plan its nodes start from.
pub struct NewRun<'a> {
    pub organization_id: Uuid,
    pub actor_user_id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_version_id: Uuid,
    pub version: i32,
    pub input: &'a Value,
    pub nodes: &'a [PlannedNode],
    /// The run this one repeats.
    pub rerun_of: Option<Uuid>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RunSummaryRecord {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub version: i32,
    pub status: String,
    pub trigger_source: String,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub cancel_requested: bool,
}

/// A run with its pinned graph and its nodes in plan order.
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub summary: RunSummaryRecord,
    pub workflow_name: String,
    pub input: Value,
    pub last_event_seq: i64,
    #[sqlx(json)]
    pub graph: GraphSnapshot,
    #[sqlx(skip)]
    pub nodes: Vec<NodeRunRecord>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct NodeRunRecord {
    pub node_id: String,
    pub state: String,
    pub attempt: i32,
    pub max_attempts: i32,
    pub executor: String,
    pub input: Option<Value>,
    pub output: Option<Value>,
    pub error: Option<Value>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct RunEventRecord {
    pub seq: i64,
    pub payload: Value,
}

const RUN_SUMMARY: &str = "r.id, r.workflow_id, r.version, r.status, r.trigger_source,
     r.created_at, r.started_at, r.finished_at,
     r.cancel_requested_at IS NOT NULL AS cancel_requested";

/// Creates a queued run: roots start queued, and every other node waits blocked for its
/// distinct parents. Its first event is `run.state queued`; the snapshot holds the node
/// states. A plan without nodes has nothing to wait for and settles at once.
pub async fn create_run(
    conn: &mut PgConnection,
    run: &NewRun<'_>,
) -> Result<RunSummaryRecord, sqlx::Error> {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO runs(
            organization_id, workflow_id, workflow_version_id, version, input, rerun_of,
            created_by
         ) VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id",
    )
    .bind(run.organization_id)
    .bind(run.workflow_id)
    .bind(run.workflow_version_id)
    .bind(run.version)
    .bind(Json(run.input))
    .bind(run.rerun_of)
    .bind(run.actor_user_id)
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO node_runs(
            organization_id, run_id, node_id, executor, state,
            max_attempts, timeout_seconds, pending_parents, children
         )
         SELECT $1, $2, node_id, executor,
                CASE WHEN pending_parents = 0 THEN 'queued' ELSE 'blocked' END,
                max_attempts, timeout_seconds, pending_parents, children
         FROM jsonb_to_recordset($3) AS node(
             node_id text, executor text, max_attempts integer, timeout_seconds integer,
             pending_parents integer, children text[]
         )",
    )
    .bind(run.organization_id)
    .bind(id)
    .bind(Json(run.nodes))
    .execute(&mut *conn)
    .await?;
    append_run_event(conn, id, &json!({"type": "run.state", "state": "queued"})).await?;
    settle_run(conn, id).await?;
    let (action, metadata) = match run.rerun_of {
        None => ("run.started", json!({"runId": id, "version": run.version})),
        Some(source) => (
            "run.rerun",
            json!({"runId": id, "sourceRunId": source, "version": run.version}),
        ),
    };
    audit(
        conn,
        run.organization_id,
        run.actor_user_id,
        action,
        id,
        metadata,
    )
    .await?;
    summary(conn, id).await
}

/// The workflow's runs, newest first.
// ponytail: the 50 newest, without paging.
pub async fn list_runs(
    conn: &mut PgConnection,
    workflow_id: Uuid,
) -> Result<Vec<RunSummaryRecord>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {RUN_SUMMARY} FROM runs r
         WHERE r.workflow_id = $1
         ORDER BY r.created_at DESC, r.id DESC
         LIMIT 50"
    ))
    .bind(workflow_id)
    .fetch_all(conn)
    .await
}

/// Reads the run before its nodes, never the other way round: the node states are then at
/// least as new as `last_event_seq`, so a client that applies the later events stays current.
pub async fn get_run(
    conn: &mut PgConnection,
    run_id: Uuid,
) -> Result<Option<RunRecord>, sqlx::Error> {
    let run: Option<RunRecord> = sqlx::query_as(&format!(
        "SELECT {RUN_SUMMARY}, w.name AS workflow_name, r.input, r.last_event_seq, v.graph
         FROM runs r
         JOIN workflows w ON w.id = r.workflow_id
         JOIN workflow_versions v ON v.id = r.workflow_version_id
         WHERE r.id = $1"
    ))
    .bind(run_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(mut run) = run else {
        return Ok(None);
    };
    run.nodes = sqlx::query_as(
        "SELECT node_id, state, attempt, max_attempts, executor, input, output, error,
                started_at, finished_at
         FROM node_runs
         WHERE run_id = $1
         ORDER BY id",
    )
    .bind(run_id)
    .fetch_all(conn)
    .await?;
    Ok(Some(run))
}

/// Requests the cancellation of an unfinished run. Its blocked, queued and retry_wait nodes
/// are canceled now; running nodes are left to their workers, which see the request on their
/// next heartbeat. The run settles as canceled once no node can change.
pub async fn cancel_run(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    run_id: Uuid,
) -> Result<RunSummaryRecord, CancelError> {
    // The runs row first, then the node rows in one statement ordered by id: every writer of
    // a run locks in this order, so a cancel cannot deadlock with a completing worker.
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM runs WHERE id = $1 FOR UPDATE")
            .bind(run_id)
            .fetch_optional(&mut *conn)
            .await?;
    match status.as_deref() {
        None => return Err(CancelError::NotFound),
        Some("queued" | "running") => {}
        Some(_) => return Err(CancelError::Finished),
    }
    sqlx::query(
        "WITH canceled AS (
            UPDATE node_runs AS n
            SET state = 'canceled', finished_at = now()
            FROM (
                SELECT id FROM node_runs
                WHERE run_id = $1 AND state IN ('blocked', 'queued', 'retry_wait')
                ORDER BY id
                FOR UPDATE
            ) AS waiting
            WHERE n.id = waiting.id
            RETURNING n.id, n.node_id, n.attempt
         )
         SELECT corqty.append_run_event($1, jsonb_build_object(
             'type', 'node.state', 'nodeId', node_id, 'state', 'canceled', 'attempt', attempt
         ))
         FROM canceled
         ORDER BY id",
    )
    .bind(run_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE runs SET cancel_requested_at = coalesce(cancel_requested_at, now())
         WHERE id = $1",
    )
    .bind(run_id)
    .execute(&mut *conn)
    .await?;
    settle_run(conn, run_id).await?;
    let metadata = json!({"runId": run_id});
    audit(
        conn,
        organization_id,
        actor_user_id,
        "run.canceled",
        run_id,
        metadata,
    )
    .await?;
    Ok(summary(conn, run_id).await?)
}

/// Up to `limit` of the run's events after seq `after`, in order.
pub async fn events_after(
    conn: &mut PgConnection,
    run_id: Uuid,
    after: i64,
    limit: i64,
) -> Result<Vec<RunEventRecord>, sqlx::Error> {
    sqlx::query_as(
        "SELECT seq, payload FROM run_events
         WHERE run_id = $1 AND seq > $2
         ORDER BY seq
         LIMIT $3",
    )
    .bind(run_id)
    .bind(after)
    .bind(limit)
    .fetch_all(conn)
    .await
}

/// Appends an event through `corqty.append_run_event`, which numbers it. None means the run
/// is not visible to the caller.
pub async fn append_run_event(
    conn: &mut PgConnection,
    run_id: Uuid,
    payload: &Value,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar("SELECT corqty.append_run_event($1, $2)")
        .bind(run_id)
        .bind(Json(payload))
        .fetch_one(conn)
        .await
}

/// Finishes the run through `corqty.settle_run` if none of its nodes can change any more.
pub async fn settle_run(conn: &mut PgConnection, run_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT corqty.settle_run($1)")
        .bind(run_id)
        .execute(conn)
        .await?;
    Ok(())
}

async fn summary(conn: &mut PgConnection, run_id: Uuid) -> Result<RunSummaryRecord, sqlx::Error> {
    sqlx::query_as(&format!("SELECT {RUN_SUMMARY} FROM runs r WHERE r.id = $1"))
        .bind(run_id)
        .fetch_one(conn)
        .await
}

async fn audit(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    action: &str,
    run_id: Uuid,
    metadata: Value,
) -> Result<(), sqlx::Error> {
    crate::audit::insert_in_transaction(
        conn,
        organization_id,
        actor_user_id,
        action,
        "run",
        &run_id.to_string(),
        metadata,
    )
    .await
}
```

Replace the whole of `crates/persistence/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod audit;
pub mod auth;
pub mod catalog;
pub mod members;
pub mod migrate;
pub mod models;
pub mod organizations;
pub mod runs;
pub mod store;
pub mod tenant;
pub mod workflows;
```

Replace the whole of `crates/persistence/src/store.rs`:

```rust
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Clone)]
pub struct PgStore {
    pub(crate) pool: PgPool,
}

impl PgStore {
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The pool itself, for work outside a request such as background loops.
    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn begin_tenant(
        &self,
        user_id: Uuid,
        organization_id: Uuid,
    ) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
        crate::tenant::begin_tenant(&self.pool, user_id, organization_id).await
    }
}
```

The cancel statement runs the node UPDATE in a CTE whose `FROM` locks the rows with `ORDER BY id FOR UPDATE`. The
outer `SELECT` appends one event per canceled node. Since PostgreSQL 9.6, volatile functions in the target list are
evaluated after the sort, so the events follow node id order, which is plan order.

- [ ] **Step 7: Verify GREEN**

```text
cargo test -p corqty-persistence --test run_store
```

Expected: all 8 tests pass in about 2 s.

Three negative controls show that the tests catch the mistakes they target. Each one replaced a function in the
test database with `CREATE OR REPLACE FUNCTION` through `docker exec ... psql -U corqty_owner -d corqty_test` and
restored it afterwards.

- Numbering events with `max(seq) + 1` instead of the runs row makes
  `concurrent_appends_number_the_events_of_a_run_without_gaps` fail with 23505 on `run_events_pkey`.
- The literal `jsonb_strip_nulls(payload || seq)` from the design removes `"tags": null` inside the output, and
  `appended_events_drop_only_their_own_null_fields` fails.
- Checking the cancel request before failures makes `settling_gives_a_finished_run_the_status_the_domain_aggregates`
  fail with `("canceled", true)` against `("failed", true)`.

- [ ] **Step 8: Run the full backend verification**

```text
cargo fmt --check
```

Expected: no output, exit code 0.

```text
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: `Finished` with no warnings.

```text
cargo test --workspace
```

Expected: every suite passes. corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 4,
`catalog_http` 4, `members_http` 3, `organizations_http` 1, `problems_http` 2, `publish_http` 2, `shutdown` 1,
`workflows_http` 4. corqty-domain: `account_and_rbac` 6, `catalog_rules` 7, `graph_rules` 8, `publish_rules` 10,
`run_rules` 8, `runtime_rules` 12. corqty-executor-mock: `contract` 11. corqty-executor-shell: `contract` 0 on
Windows and macOS, 15 on Linux. corqty-persistence: `auth_store` 2, `catalog_guards` 3, `final_admin` 2,
`migrations` 1, `run_store` 8, `tenant_isolation` 1, `test_reset` 1, `workflow_catalog` 6. corqty-plugin-host:
`registry` 8. That is 123 tests on Windows and 138 on Linux (CI). `catalog_guards.rs` and `test_reset.rs` pass
unchanged: the new tables have forced row-level security and a policy, `corqty_worker` has no table privilege, and
reset truncates the new tables like any other.

- [ ] **Step 9: Commit**

```text
git add migrations/202610260001_runs.sql crates/domain/src/run.rs crates/persistence crates/test-support/src/lib.rs
git commit -m "feat: store runs, node runs, run events, and worker instances"
```

Expected: `git show --stat HEAD` lists exactly the 7 files above (1405 insertions, 5 deletions).

## Task 9: Claim, start, heartbeat, log, and complete node runs through worker-only functions

**Files:**

- Create: `migrations/202610260002_worker_functions.sql`
- Create: `crates/persistence/src/worker_queue.rs`
- Modify: `crates/persistence/src/lib.rs`
- Modify: `crates/persistence/tests/catalog_guards.rs`
- Test: `crates/persistence/tests/worker_queue.rs`

Task 8 stored runs. This task gives workers their only way into them (design §3.4). `corqty_worker` has no table
privileges, so migration `202610260002_worker_functions.sql` adds six SECURITY DEFINER functions and grants them to
that role alone:

- `register_worker(worker, executors, host_contracts, capacity)` upserts the process's `worker_instances` row. The
  executors are `"<key>@<version>"` strings (D3). Host contracts stay an empty array until Milestone 4.
- `claim_node_runs(worker, limit, lease_seconds)` leases up to `limit` due node runs whose executor the worker
  registered, oldest `available_at` first. A due retry is claimed straight from `retry_wait` (D6). Each claim adds
  one to `attempt` and to `lease_token`. Rows are locked with `FOR UPDATE OF n SKIP LOCKED`, so workers claim side
  by side, and the function takes no runs lock. Nothing of a run with `cancel_requested_at` is handed out. Each
  call, `limit` 0 included, refreshes `last_heartbeat_at` (D11).
- `start_node_run(worker, node_run, token)` sets the attempt's `started_at`. On the run's first start it moves the
  run to `running` and appends `run.state running`. Then it appends `node.state running {attempt}` and returns the
  job payload:
  `{executionId, attempt, maxAttempts, timeoutSeconds, workflowId, runId, nodeId, cancelRequested, runInput, graph,
  definition, upstreamOutputs}`. The definition is the system one or the one of the run's own organization; the
  catalog forbids organization keys that start with `corqty.`, so exactly one row matches. `upstreamOutputs` maps
  each parent node id to its stored output object.
- `heartbeat(worker, node_runs[], tokens[], lease_seconds)` extends every lease the worker still holds. It answers
  one row per lease asked about, in order: `alive`, and `cancel_requested` for the run. It also refreshes the
  worker's `last_heartbeat_at`, and it locks only the worker's own running rows, in id order, and no runs row.
- `append_node_log(worker, node_run, token, stream, text)` stores one batch as a `node.log` event. A node run keeps
  at most 10 MiB of log text (`MAX_LOG_BYTES_PER_NODE_RUN`, counted in bytes). The batch that crosses the cap keeps
  its longest prefix that does not split a UTF-8 character. One `node.log_truncated` event follows, and later text
  is dropped. The function returns true while the lease holds, whether or not it stored the text, and false only
  once the lease is gone. The stream must be `stdout`, `stderr` or `system`; anything else raises 22023.
- `complete_node_run(worker, node_run, token, outcome)` records how the attempt ended through `finish_node_run`.
  The outcome is `{"status":"succeeded","input":{..},"output":{..}}`,
  `{"status":"failed","input"?:{..},"error":{code,message,retryable}}` or `{"status":"canceled"}`.

Every call after the claim is fenced. The node run must be `running`, leased by the calling worker id, under the
token of the claim. Any other worker id, an older or newer token, or another organization's node run id changes
nothing. Start returns NULL, append and complete return false, and heartbeat reports `alive = false`. The spec
requires the worker id as well as the token (M3-96). A token alone is a small counter that anyone could guess.

Two SECURITY INVOKER helpers are granted to no role. Only the definer functions call them, as the owner:

- `lock_lease(worker, node_run, token)` follows the lock order of D9. It reads the run id without a lock, which
  `node_runs.run_id` never changes, and a caller without the lease locks nothing. Then it locks the runs row, then
  checks the fence again on the node row `FOR UPDATE`. It returns the run id, or NULL.
- `finish_node_run(node_run, outcome)` is the whole transition of D10. The caller holds the runs row lock; Task 12's
  lease recovery will call it too.
  - A success stores the output and releases each child. In one statement that locks the blocked children in id
    order, it decrements `pending_parents`, and a child that reaches 0 becomes `queued`.
  - A retryable failure with attempts left goes to `retry_wait` with
    `available_at = now() + least(300, 2 * 2^(attempt-1) * (1 + 0.2 * random()))` seconds, keeping the error.
    If the run was asked to cancel, the failure cancels the node instead, still keeping the error. Otherwise the
    node fails.
  - A failed or canceled node skips its blocked descendants (a recursive walk over `children`, then one statement
    in id order).
  - The lease is cleared, `finished_at` is set for every state but `retry_wait`, and `input` keeps the previous
    value when the outcome has none.
  - Every node change is one `node.state` event: the node's own (with `error` or `output`), then each released or
    skipped node in id order. `corqty.settle_run` from Task 8 then appends the terminal `run.state` if the run is
    done.

Every writer of a run locks its runs row first, so these functions and Task 8's `cancel_run` take turns on a run
and keep its event sequence gapless. Claim and heartbeat are the two exceptions that D9 allows. Claim skips locked
rows and never waits. Heartbeat locks only the worker's running rows, in id order, and never a runs row. A writer
that waits for one of those rows holds no other node row yet, so the two cannot deadlock.

The migration also adds `corqty.live_executors()`, granted to `corqty_api` only. It returns the sorted executors
of the workers heard from in the last 60 seconds. Task 10 derives `queueReason` from it.

`corqty_persistence::worker_queue` wraps the six functions for Task 14's worker. `WorkerDb::new(pool, worker_id)`
holds the worker pool and the process's worker id, with `register`, `claim`, `start`, `heartbeat`, `append_log`
and `complete`. `ClaimedJob`, `Job`, `LeaseStatus`, `Outcome` and `NodeError` type the rows, the payload and the
outcome. `Job` deserializes the payload into the domain's `GraphSnapshot` and `NodeDefinitionSpec`, and
`upstream_outputs` is the map that `runtime::resolve_inputs` takes.

The tests run with the worker pool (`corqty_worker`). They set up data with the API pool and read it back with the
owner pool. Organizations A and B each have an admin. Each test publishes the graph it needs as version 1 of a
workflow and starts runs with `runs::create_run`, either from `plan_run` or from a hand-written plan.

- [ ] **Step 1: Write the failing tests**

Create `crates/persistence/tests/worker_queue.rs`:

```rust
use std::collections::{BTreeMap, HashMap, HashSet};

use corqty_domain::{
    catalog::NewNodeDefinition,
    graph::GraphSnapshot,
    publish::canonical_checksum,
    run::{PlannedNode, plan_run},
};
use corqty_persistence::{
    catalog,
    runs::{self, NewRun},
    tenant::begin_tenant,
    worker_queue::{ClaimedJob, Job, LeaseStatus, NodeError, Outcome, WorkerDb},
    workflows,
};
use corqty_test_support::TestDatabase;
use serde_json::{Map, Value, json};
use serial_test::serial;
use sqlx::{Postgres, Transaction};
use tokio::task::JoinSet;
use uuid::Uuid;

/// An organization and its admin, who may edit and run workflows.
#[derive(Clone, Copy)]
struct Tenant {
    organization_id: Uuid,
    admin: Uuid,
}

/// Version 1 of a workflow, with the plan its runs start from.
struct Published {
    tenant: Tenant,
    workflow_id: Uuid,
    version_id: Uuid,
    plan: Vec<PlannedNode>,
}

/// Organizations A and B, each with an admin.
struct Fixture {
    database: TestDatabase,
    a: Tenant,
    b: Tenant,
}

impl Fixture {
    async fn new() -> Self {
        let database = TestDatabase::reset().await;
        let [org_a, org_b, admin_a, admin_b] = std::array::from_fn(|_| Uuid::new_v4());
        sqlx::query(
            "INSERT INTO users(id, email)
             SELECT id, id || '@example.com' FROM unnest($1::uuid[]) AS id",
        )
        .bind(vec![admin_a, admin_b])
        .execute(&database.owner)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO organizations(id, name, slug, created_by)
             VALUES ($1, 'A', 'a', $3), ($2, 'B', 'b', $4)",
        )
        .bind(org_a)
        .bind(org_b)
        .bind(admin_a)
        .bind(admin_b)
        .execute(&database.owner)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships(organization_id, user_id, role)
             VALUES ($1, $3, 'admin'), ($2, $4, 'admin')",
        )
        .bind(org_a)
        .bind(org_b)
        .bind(admin_a)
        .bind(admin_b)
        .execute(&database.owner)
        .await
        .unwrap();

        let a = Tenant {
            organization_id: org_a,
            admin: admin_a,
        };
        let b = Tenant {
            organization_id: org_b,
            admin: admin_b,
        };
        Self { database, a, b }
    }

    async fn tx(&self, tenant: Tenant) -> Transaction<'static, Postgres> {
        begin_tenant(&self.database.api, tenant.admin, tenant.organization_id)
            .await
            .unwrap()
    }

    /// The simulated infrastructure blueprint: vpc, then rds and deploy.
    async fn infrastructure(&self) -> GraphSnapshot {
        let id = "00000000-0000-7000-8000-000000000101".parse().unwrap();
        let mut tx = self.tx(self.a).await;
        let blueprint = catalog::get_blueprint(&mut tx, id).await.unwrap();
        tx.commit().await.unwrap();
        blueprint.unwrap().graph
    }

    /// Publishes `graph` as version 1 of a new workflow of the tenant.
    async fn publish(&self, tenant: Tenant, graph: &GraphSnapshot) -> Published {
        let (organization_id, admin) = (tenant.organization_id, tenant.admin);
        let slug = Uuid::new_v4().to_string();
        let mut tx = self.tx(tenant).await;
        let workflow =
            workflows::insert_workflow(&mut tx, organization_id, admin, "Flow", &slug, graph, None)
                .await
                .unwrap();
        let checksum = canonical_checksum(graph);
        let version = workflows::insert_version(
            &mut tx,
            organization_id,
            admin,
            workflow.id,
            1,
            graph,
            &checksum,
        )
        .await
        .unwrap();
        let definitions = catalog::definitions_map(&mut tx).await.unwrap();
        tx.commit().await.unwrap();
        Published {
            tenant,
            workflow_id: workflow.id,
            version_id: version.id,
            plan: plan_run(graph, &definitions).unwrap(),
        }
    }

    /// Starts a run of the version with `nodes` as its plan and returns its id.
    async fn run(&self, published: &Published, nodes: &[PlannedNode]) -> Uuid {
        let tenant = published.tenant;
        let input = json!({"region": "eu-west-1"});
        let new = NewRun {
            organization_id: tenant.organization_id,
            actor_user_id: tenant.admin,
            workflow_id: published.workflow_id,
            workflow_version_id: published.version_id,
            version: 1,
            input: &input,
            nodes,
            rerun_of: None,
        };
        let mut tx = self.tx(tenant).await;
        let run = runs::create_run(&mut tx, &new).await.unwrap();
        tx.commit().await.unwrap();
        run.id
    }

    /// A new worker process that registered `executors`.
    async fn worker(&self, executors: &[&str]) -> WorkerDb {
        let worker = WorkerDb::new(self.database.worker.clone(), Uuid::now_v7());
        let executors: Vec<String> = executors.iter().map(|&key| key.to_owned()).collect();
        worker.register(&executors, 4).await.unwrap();
        worker
    }

    /// Claims every due node run the worker can take, by node id.
    async fn claim(&self, worker: &WorkerDb) -> BTreeMap<String, ClaimedJob> {
        let mut claimed = BTreeMap::new();
        for job in worker.claim(100, 30).await.unwrap() {
            let node_id: String = sqlx::query_scalar("SELECT node_id FROM node_runs WHERE id = $1")
                .bind(job.node_run_id)
                .fetch_one(&self.database.owner)
                .await
                .unwrap();
            claimed.insert(node_id, job);
        }
        claimed
    }

    /// The run's event payloads in sequence order.
    async fn events(&self, run_id: Uuid) -> Vec<Value> {
        sqlx::query_scalar("SELECT payload FROM run_events WHERE run_id = $1 ORDER BY seq")
            .bind(run_id)
            .fetch_all(&self.database.owner)
            .await
            .unwrap()
    }

    /// Each node of the run with its state, in plan order.
    async fn states(&self, run_id: Uuid) -> Vec<(String, String)> {
        sqlx::query_as("SELECT node_id, state FROM node_runs WHERE run_id = $1 ORDER BY id")
            .bind(run_id)
            .fetch_all(&self.database.owner)
            .await
            .unwrap()
    }

    /// Every node run, run and event, to show that a call changed nothing.
    async fn snapshot(&self) -> Value {
        sqlx::query_scalar(
            "SELECT jsonb_build_array(
                 (SELECT jsonb_agg(n ORDER BY n.id) FROM node_runs n),
                 (SELECT jsonb_agg(r ORDER BY r.id) FROM runs r),
                 (SELECT jsonb_agg(e ORDER BY e.run_id, e.seq) FROM run_events e)
             )",
        )
        .fetch_one(&self.database.owner)
        .await
        .unwrap()
    }

    /// The executors of the workers that claimed or heartbeated in the last minute, as the
    /// API sees them.
    async fn live_executors(&self) -> Vec<String> {
        sqlx::query_scalar("SELECT corqty.live_executors()")
            .fetch_one(&self.database.api)
            .await
            .unwrap()
    }
}

fn code(error: &sqlx::Error) -> Option<String> {
    let code = error.as_database_error().and_then(|error| error.code());
    code.map(String::from)
}

/// A node of a hand-written plan on the mock executor, with one attempt.
fn planned(node_id: &str, children: &[&str], pending_parents: i32) -> PlannedNode {
    PlannedNode {
        node_id: node_id.to_owned(),
        executor: "mock@1.0.0".to_owned(),
        max_attempts: 1,
        timeout_seconds: 60,
        children: children.iter().map(|&child| child.to_owned()).collect(),
        pending_parents,
    }
}

/// A `corqty.mock.task` node with up to `max_attempts` attempts.
fn task(id: &str, max_attempts: u32) -> Value {
    json!({
        "id": id,
        "definition": {"key": "corqty.mock.task", "version": 1},
        "displayName": id,
        "position": {"x": 0, "y": 0},
        "config": {"mode": "fixed", "output": {"result": id}, "delayMs": 0, "failureMessage": null},
        "inputs": {},
        "retry": {"maxAttempts": max_attempts},
        "timeoutSeconds": 60
    })
}

/// An edge from the result of `source` into the `port` input of `target`. Publish would
/// reject ports the definition lacks; a plan only reads which nodes an edge joins.
fn edge(source: &str, target: &str, port: &str) -> Value {
    json!({
        "id": format!("{source}-{target}-{port}"),
        "source": {"nodeId": source, "port": "result"},
        "target": {"nodeId": target, "port": port}
    })
}

fn graph(nodes: &[Value], edges: &[Value]) -> GraphSnapshot {
    serde_json::from_value(json!({
        "schemaVersion": 1,
        "workflowInputSchema": {"type": "object"},
        "nodes": nodes,
        "edges": edges,
        "viewport": {"x": 0, "y": 0, "zoom": 1}
    }))
    .unwrap()
}

fn object(value: Value) -> Map<String, Value> {
    let Value::Object(object) = value else {
        panic!("not an object: {value}");
    };
    object
}

fn succeeded(output: Value) -> Outcome {
    Outcome::Succeeded {
        input: Map::new(),
        output: object(output),
    }
}

fn failure(code: &str, retryable: bool) -> NodeError {
    NodeError {
        code: code.to_owned(),
        message: format!("The mock failed with {code}."),
        retryable,
    }
}

fn failed(code: &str, retryable: bool) -> Outcome {
    Outcome::Failed {
        input: None,
        error: failure(code, retryable),
    }
}

fn run_state(state: &str, seq: i64) -> Value {
    json!({"type": "run.state", "state": state, "seq": seq})
}

fn node_state(node_id: &str, state: &str, attempt: i32, seq: i64) -> Value {
    json!({"type": "node.state", "nodeId": node_id, "state": state, "attempt": attempt, "seq": seq})
}

/// `event` with one more field.
fn with_field(mut event: Value, key: &str, value: Value) -> Value {
    event[key] = value;
    event
}

/// Starts the claimed attempt and completes it with `outcome`.
async fn execute(worker: &WorkerDb, job: &ClaimedJob, outcome: Outcome) {
    let (id, token) = (job.node_run_id, job.lease_token);
    assert!(worker.start(id, token).await.unwrap().is_some());
    assert!(worker.complete(id, token, &outcome).await.unwrap());
}

/// Makes every fenced call with one lease, in a worker's order, and reports which held:
/// (started, renewed, logged, completed).
async fn fenced_calls(worker: &WorkerDb, id: Uuid, token: i64) -> (bool, bool, bool, bool) {
    let started = worker.start(id, token).await.unwrap().is_some();
    let renewed = worker.heartbeat(&[(id, token)], 30).await.unwrap()[0].alive;
    let logged = worker
        .append_log(id, token, "stdout", "hello")
        .await
        .unwrap();
    let outcome = succeeded(json!({"vpcId": "vpc-1"}));
    let completed = worker.complete(id, token, &outcome).await.unwrap();
    (started, renewed, logged, completed)
}

#[tokio::test]
#[serial]
async fn concurrent_claimers_never_share_a_node_run() {
    let fixture = Fixture::new().await;
    let published = fixture
        .publish(fixture.a, &fixture.infrastructure().await)
        .await;
    let nodes: Vec<PlannedNode> = (0..40)
        .map(|index| planned(&format!("n{index}"), &[], 0))
        .collect();
    for _ in 0..5 {
        fixture.run(&published, &nodes).await;
    }

    // Eight workers claim at the same time, each on a connection of its own, until nothing
    // is left.
    let mut claimers = JoinSet::new();
    for _ in 0..8 {
        let worker = fixture.worker(&["mock@1.0.0"]).await;
        claimers.spawn(async move {
            let mut claimed = Vec::new();
            loop {
                let jobs = worker.claim(5, 30).await.unwrap();
                if jobs.is_empty() {
                    return claimed;
                }
                let worker_id = worker.worker_id();
                claimed.extend(jobs.into_iter().map(|job| (job.node_run_id, worker_id)));
            }
        });
    }
    let claimed: Vec<(Uuid, Uuid)> = claimers.join_all().await.concat();
    let distinct: HashSet<Uuid> = claimed.iter().map(|&(node_run, _)| node_run).collect();
    assert_eq!((claimed.len(), distinct.len()), (200, 200));

    // Each node run is leased, at its first attempt, to the worker that claimed it.
    let rows: Vec<(Uuid, String, Option<Uuid>, i32, i64)> =
        sqlx::query_as("SELECT id, state, leased_by, attempt, lease_token FROM node_runs")
            .fetch_all(&fixture.database.owner)
            .await
            .unwrap();
    let leases: HashMap<Uuid, (String, Option<Uuid>, i32, i64)> = rows
        .into_iter()
        .map(|(id, state, leased_by, attempt, token)| (id, (state, leased_by, attempt, token)))
        .collect();
    for (node_run, worker) in claimed {
        let expected = ("running".to_owned(), Some(worker), 1, 1);
        assert_eq!(leases[&node_run], expected);
    }
}

#[tokio::test]
#[serial]
async fn only_the_lease_holder_can_start_renew_log_or_complete_a_node_run() {
    let fixture = Fixture::new().await;
    let graph = fixture.infrastructure().await;
    let in_a = fixture.publish(fixture.a, &graph).await;
    let in_b = fixture.publish(fixture.b, &graph).await;
    fixture.run(&in_a, &in_a.plan).await;
    fixture.run(&in_b, &in_b.plan).await;
    // The holder claims A's vpc, which was queued first, and another worker claims B's.
    let holder = fixture.worker(&["mock@1.0.0"]).await;
    let other = fixture.worker(&["mock@1.0.0"]).await;
    let mine = holder.claim(1, 30).await.unwrap().remove(0);
    let theirs = other.claim(1, 30).await.unwrap().remove(0);
    assert_eq!(
        (mine.organization_id, theirs.organization_id),
        (fixture.a.organization_id, fixture.b.organization_id)
    );

    let before = fixture.snapshot().await;
    for (worker, id, token) in [
        // Another worker with the right token.
        (&other, mine.node_run_id, mine.lease_token),
        // The holder with an older or a newer token.
        (&holder, mine.node_run_id, mine.lease_token - 1),
        (&holder, mine.node_run_id, mine.lease_token + 1),
        // Another organization's node run, with the token it was leased under.
        (&holder, theirs.node_run_id, theirs.lease_token),
    ] {
        let calls = fenced_calls(worker, id, token).await;
        assert_eq!(calls, (false, false, false, false), "{id} {token}");
    }
    assert_eq!(fixture.snapshot().await, before);

    // The holder's lease works for every call, and completing the node ends it.
    let (id, token) = (mine.node_run_id, mine.lease_token);
    let calls = fenced_calls(&holder, id, token).await;
    assert_eq!(calls, (true, true, true, true));
    let calls = fenced_calls(&holder, id, token).await;
    assert_eq!(calls, (false, false, false, false));
}

#[tokio::test]
#[serial]
async fn parents_that_finish_together_release_their_child_once() {
    let fixture = Fixture::new().await;
    let published = fixture
        .publish(fixture.a, &fixture.infrastructure().await)
        .await;
    let mut nodes: Vec<PlannedNode> = ["p0", "p1", "p2", "p3"]
        .iter()
        .map(|parent| planned(parent, &["child"], 0))
        .collect();
    // No worker here runs the child's executor, so released children stay queued.
    nodes.push(PlannedNode {
        executor: "other@1.0.0".to_owned(),
        ..planned("child", &[], 4)
    });
    let worker = fixture.worker(&["mock@1.0.0"]).await;

    for _ in 0..50 {
        let run_id = fixture.run(&published, &nodes).await;
        let jobs = worker.claim(10, 30).await.unwrap();
        assert_eq!(jobs.len(), 4);
        let mut completions = JoinSet::new();
        for job in jobs {
            let worker = worker.clone();
            completions.spawn(async move {
                let outcome = succeeded(json!({}));
                let (id, token) = (job.node_run_id, job.lease_token);
                worker.complete(id, token, &outcome).await.unwrap()
            });
        }
        assert_eq!(completions.join_all().await, [true; 4]);

        let child: (String, i32) = sqlx::query_as(
            "SELECT state, pending_parents FROM node_runs WHERE run_id = $1 AND node_id = 'child'",
        )
        .bind(run_id)
        .fetch_one(&fixture.database.owner)
        .await
        .unwrap();
        assert_eq!(child, ("queued".to_owned(), 0));
        // Events 2 to 5 are the parents succeeding; the last one releases the child.
        let releases: Vec<Value> = fixture
            .events(run_id)
            .await
            .into_iter()
            .filter(|event| event["nodeId"] == "child")
            .collect();
        assert_eq!(releases, [node_state("child", "queued", 0, 6)]);
    }
}

#[tokio::test]
#[serial]
async fn a_child_waits_once_for_each_distinct_parent_and_the_run_succeeds() {
    let fixture = Fixture::new().await;
    // split feeds two ports of join, so join waits for two parents, not three edges.
    let graph = graph(
        &[task("split", 1), task("other", 1), task("join", 1)],
        &[
            edge("split", "join", "input"),
            edge("split", "join", "extra"),
            edge("other", "join", "more"),
        ],
    );
    let published = fixture.publish(fixture.a, &graph).await;
    let run_id = fixture.run(&published, &published.plan).await;
    let worker = fixture.worker(&["mock@1.0.0"]).await;
    let output = |node_id: &str| json!({"result": node_id});

    let roots = fixture.claim(&worker).await;
    assert_eq!(roots.keys().collect::<Vec<_>>(), ["other", "split"]);
    execute(&worker, &roots["split"], succeeded(output("split"))).await;
    // One parent is not enough.
    assert!(worker.claim(10, 30).await.unwrap().is_empty());
    execute(&worker, &roots["other"], succeeded(output("other"))).await;

    let join = fixture.claim(&worker).await.remove("join").unwrap();
    let job = worker.start(join.node_run_id, join.lease_token).await;
    let job = job.unwrap().unwrap();
    assert_eq!(
        job.upstream_outputs,
        HashMap::from([
            ("split".to_owned(), object(output("split"))),
            ("other".to_owned(), object(output("other"))),
        ])
    );
    let outcome = succeeded(output("join"));
    let completed = worker.complete(join.node_run_id, join.lease_token, &outcome);
    assert!(completed.await.unwrap());

    assert_eq!(
        fixture.events(run_id).await,
        [
            run_state("queued", 1),
            run_state("running", 2),
            node_state("split", "running", 1, 3),
            with_field(
                node_state("split", "succeeded", 1, 4),
                "output",
                output("split")
            ),
            node_state("other", "running", 1, 5),
            with_field(
                node_state("other", "succeeded", 1, 6),
                "output",
                output("other")
            ),
            node_state("join", "queued", 0, 7),
            node_state("join", "running", 1, 8),
            with_field(
                node_state("join", "succeeded", 1, 9),
                "output",
                output("join")
            ),
            run_state("succeeded", 10),
        ]
    );
    // Each node keeps its input and output, and no lease is left behind.
    let nodes: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object(
             'nodeId', node_id, 'input', input, 'output', output, 'error', error,
             'leased', leased_by IS NOT NULL OR lease_expires_at IS NOT NULL,
             'timed', started_at IS NOT NULL AND finished_at IS NOT NULL
         )
         FROM node_runs WHERE run_id = $1 ORDER BY id",
    )
    .bind(run_id)
    .fetch_all(&fixture.database.owner)
    .await
    .unwrap();
    let node = |node_id: &str| {
        json!({
            "nodeId": node_id, "input": {}, "output": output(node_id), "error": null,
            "leased": false, "timed": true
        })
    };
    assert_eq!(nodes, [node("split"), node("other"), node("join")]);
    let run: (String, bool, bool) = sqlx::query_as(
        "SELECT status, started_at IS NOT NULL, finished_at IS NOT NULL FROM runs WHERE id = $1",
    )
    .bind(run_id)
    .fetch_one(&fixture.database.owner)
    .await
    .unwrap();
    assert_eq!(run, ("succeeded".to_owned(), true, true));
}

#[tokio::test]
#[serial]
async fn a_failure_skips_only_its_descendants_and_fails_the_run() {
    let fixture = Fixture::new().await;
    // a -> b -> c, and u on its own. A failure the executor calls final is not retried,
    // although a has a second attempt.
    let graph = graph(
        &[task("a", 2), task("b", 1), task("c", 1), task("u", 1)],
        &[edge("a", "b", "input"), edge("b", "c", "input")],
    );
    let published = fixture.publish(fixture.a, &graph).await;
    let run_id = fixture.run(&published, &published.plan).await;
    let worker = fixture.worker(&["mock@1.0.0"]).await;

    let roots = fixture.claim(&worker).await;
    let (a, u) = (&roots["a"], &roots["u"]);
    let outcome = failed("mock_failure", false);
    assert!(
        worker
            .complete(a.node_run_id, a.lease_token, &outcome)
            .await
            .unwrap()
    );
    // The unrelated branch carries on.
    let state = |node_id: &str, state: &str| (node_id.to_owned(), state.to_owned());
    assert_eq!(
        fixture.states(run_id).await,
        [
            state("a", "failed"),
            state("u", "running"),
            state("b", "skipped"),
            state("c", "skipped"),
        ]
    );
    let outcome = succeeded(json!({"result": "u"}));
    assert!(
        worker
            .complete(u.node_run_id, u.lease_token, &outcome)
            .await
            .unwrap()
    );

    let error = json!(failure("mock_failure", false));
    assert_eq!(
        fixture.events(run_id).await,
        [
            run_state("queued", 1),
            with_field(node_state("a", "failed", 1, 2), "error", error),
            node_state("b", "skipped", 0, 3),
            node_state("c", "skipped", 0, 4),
            with_field(
                node_state("u", "succeeded", 1, 5),
                "output",
                json!({"result": "u"})
            ),
            run_state("failed", 6),
        ]
    );
}

#[tokio::test]
#[serial]
async fn retryable_failures_wait_with_backoff_until_the_attempts_run_out() {
    let fixture = Fixture::new().await;
    let published = fixture
        .publish(fixture.a, &graph(&[task("flaky", 2)], &[]))
        .await;
    let run_id = fixture.run(&published, &published.plan).await;
    let worker = fixture.worker(&["mock@1.0.0"]).await;
    let owner = &fixture.database.owner;
    let outcome = failed("mock_retryable_failure", true);
    let error = json!(failure("mock_retryable_failure", true));

    let first = fixture.claim(&worker).await.remove("flaky").unwrap();
    let completed = worker.complete(first.node_run_id, first.lease_token, &outcome);
    assert!(completed.await.unwrap());
    // Attempt 1 waits 2 s plus up to 20% jitter, counted from the completion that
    // scheduled the retry. The error stays, and the lease is gone.
    let (state, stored, leased_by, delay): (String, Value, Option<Uuid>, f64) = sqlx::query_as(
        "SELECT n.state, n.error, n.leased_by,
                extract(epoch FROM n.available_at - e.created_at)::float8
         FROM node_runs n
         JOIN run_events e ON e.run_id = n.run_id AND e.payload ->> 'state' = 'retry_wait'
         WHERE n.id = $1",
    )
    .bind(first.node_run_id)
    .fetch_one(owner)
    .await
    .unwrap();
    assert_eq!(
        (state.as_str(), &stored, leased_by),
        ("retry_wait", &error, None)
    );
    assert!((2.0..2.4).contains(&delay), "{delay}");
    assert!(worker.claim(10, 30).await.unwrap().is_empty());

    // Once due, the retry is claimed straight from retry_wait as attempt 2.
    sqlx::query("UPDATE node_runs SET available_at = now() WHERE id = $1")
        .bind(first.node_run_id)
        .execute(owner)
        .await
        .unwrap();
    let second = fixture.claim(&worker).await.remove("flaky").unwrap();
    assert_eq!(
        (second.node_run_id, second.attempt, second.lease_token),
        (first.node_run_id, 2, 2)
    );
    let completed = worker.complete(second.node_run_id, second.lease_token, &outcome);
    assert!(completed.await.unwrap());

    assert_eq!(
        fixture.events(run_id).await,
        [
            run_state("queued", 1),
            with_field(
                node_state("flaky", "retry_wait", 1, 2),
                "error",
                error.clone()
            ),
            with_field(node_state("flaky", "failed", 2, 3), "error", error),
            run_state("failed", 4),
        ]
    );
}

#[tokio::test]
#[serial]
async fn after_a_cancel_request_a_retryable_failure_cancels_the_node_and_the_run() {
    let fixture = Fixture::new().await;
    let graph = graph(
        &[task("long", 3), task("after", 1)],
        &[edge("long", "after", "input")],
    );
    let published = fixture.publish(fixture.a, &graph).await;
    let run_id = fixture.run(&published, &published.plan).await;
    let worker = fixture.worker(&["mock@1.0.0"]).await;
    let long = fixture.claim(&worker).await.remove("long").unwrap();
    let (id, token) = (long.node_run_id, long.lease_token);

    let tenant = fixture.a;
    let mut tx = fixture.tx(tenant).await;
    runs::cancel_run(&mut tx, tenant.organization_id, tenant.admin, run_id)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    // The worker learns about the request when it starts the attempt and on each heartbeat.
    let job = worker.start(id, token).await.unwrap().unwrap();
    assert!(job.cancel_requested);
    let leases = worker.heartbeat(&[(id, token)], 30).await.unwrap();
    let lease = LeaseStatus {
        node_run_id: id,
        alive: true,
        cancel_requested: true,
    };
    assert_eq!(leases, [lease]);

    // Two attempts are left, but a run that was asked to cancel is not retried.
    let outcome = failed("mock_retryable_failure", true);
    assert!(worker.complete(id, token, &outcome).await.unwrap());
    let error = json!(failure("mock_retryable_failure", true));
    assert_eq!(
        fixture.events(run_id).await,
        [
            run_state("queued", 1),
            node_state("after", "canceled", 0, 2),
            run_state("running", 3),
            node_state("long", "running", 1, 4),
            with_field(node_state("long", "canceled", 1, 5), "error", error),
            run_state("canceled", 6),
        ]
    );

    // Its nodes are not handed out even when they are queued again.
    sqlx::query("UPDATE node_runs SET state = 'queued', available_at = now() WHERE run_id = $1")
        .bind(run_id)
        .execute(&fixture.database.owner)
        .await
        .unwrap();
    assert!(worker.claim(10, 30).await.unwrap().is_empty());
}

#[tokio::test]
#[serial]
async fn heartbeats_renew_held_leases_and_report_lost_ones_in_order() {
    let fixture = Fixture::new().await;
    let published = fixture
        .publish(fixture.a, &fixture.infrastructure().await)
        .await;
    fixture.run(&published, &published.plan).await;
    let worker = fixture.worker(&["mock@1.0.0"]).await;
    let owner = &fixture.database.owner;
    let vpc = fixture.claim(&worker).await.remove("vpc").unwrap();
    sqlx::query("UPDATE worker_instances SET last_heartbeat_at = now() - interval '1 hour'")
        .execute(owner)
        .await
        .unwrap();

    let stranger = Uuid::now_v7();
    let leases = worker
        .heartbeat(&[(stranger, 1), (vpc.node_run_id, vpc.lease_token)], 120)
        .await
        .unwrap();
    let lease = |node_run_id, alive| LeaseStatus {
        node_run_id,
        alive,
        cancel_requested: false,
    };
    assert_eq!(
        leases,
        [lease(stranger, false), lease(vpc.node_run_id, true)]
    );
    // The 30 s lease of the claim now runs for 120 s, and the worker is seen alive.
    let (remaining, silence): (f64, f64) = sqlx::query_as(
        "SELECT extract(epoch FROM n.lease_expires_at - now())::float8,
                extract(epoch FROM now() - w.last_heartbeat_at)::float8
         FROM node_runs n JOIN worker_instances w ON w.id = n.leased_by
         WHERE n.id = $1",
    )
    .bind(vpc.node_run_id)
    .fetch_one(owner)
    .await
    .unwrap();
    assert!((119.0..=120.0).contains(&remaining), "{remaining}");
    assert!(silence < 60.0, "{silence}");
}

#[tokio::test]
#[serial]
async fn logs_stop_at_the_cap_on_a_character_boundary() {
    let fixture = Fixture::new().await;
    let published = fixture
        .publish(fixture.a, &fixture.infrastructure().await)
        .await;
    let run_id = fixture.run(&published, &published.plan).await;
    let worker = fixture.worker(&["mock@1.0.0"]).await;
    let owner = &fixture.database.owner;
    let vpc = fixture.claim(&worker).await.remove("vpc").unwrap();
    let (id, token) = (vpc.node_run_id, vpc.lease_token);
    assert!(worker.start(id, token).await.unwrap().is_some());

    assert!(
        worker
            .append_log(id, token, "stdout", "hello")
            .await
            .unwrap()
    );
    let logged: i64 = sqlx::query_scalar("SELECT log_bytes FROM node_runs WHERE id = $1")
        .bind(id)
        .fetch_one(owner)
        .await
        .unwrap();
    assert_eq!(logged, 5);
    // As if all but 5 bytes of the 10 MiB had been logged. "abc€" needs 6 bytes, and the
    // 5th would split the 3-byte "€", so only "abc" is stored.
    sqlx::query("UPDATE node_runs SET log_bytes = 10485760 - 5 WHERE id = $1")
        .bind(id)
        .execute(owner)
        .await
        .unwrap();
    assert!(
        worker
            .append_log(id, token, "stderr", "abc€")
            .await
            .unwrap()
    );
    // Later text is dropped, and the lease still holds.
    assert!(
        worker
            .append_log(id, token, "stdout", "more")
            .await
            .unwrap()
    );

    let log = |stream: &str, text: &str, seq: i64| json!({"type": "node.log", "nodeId": "vpc", "stream": stream, "text": text, "seq": seq});
    assert_eq!(
        fixture.events(run_id).await[3..],
        [
            log("stdout", "hello", 4),
            log("stderr", "abc", 5),
            json!({"type": "node.log_truncated", "nodeId": "vpc", "seq": 6}),
        ]
    );
    let stored: (i64, bool) =
        sqlx::query_as("SELECT log_bytes, log_truncated FROM node_runs WHERE id = $1")
            .bind(id)
            .fetch_one(owner)
            .await
            .unwrap();
    assert_eq!(stored, (10_485_760 - 2, true));

    // Logs have three streams.
    let error = worker
        .append_log(id, token, "stdin", "x")
        .await
        .unwrap_err();
    assert_eq!(code(&error).as_deref(), Some("22023"), "{error}");
}

#[tokio::test]
#[serial]
async fn workers_claim_only_the_executors_they_advertise() {
    let fixture = Fixture::new().await;
    let published = fixture
        .publish(fixture.a, &fixture.infrastructure().await)
        .await;
    fixture.run(&published, &published.plan).await;

    // A worker that never registered finds nothing, and neither does a shell-only worker.
    let unregistered = WorkerDb::new(fixture.database.worker.clone(), Uuid::now_v7());
    assert!(unregistered.claim(10, 30).await.unwrap().is_empty());
    let shell = fixture.worker(&["shell@1.0.0"]).await;
    assert!(shell.claim(10, 30).await.unwrap().is_empty());
    assert_eq!(fixture.live_executors().await, ["shell@1.0.0"]);

    // The API sees an executor as live while a worker that runs it was heard from in the
    // last minute. A claim with no free slot counts too.
    let mock = fixture.worker(&["mock@1.0.0"]).await;
    let both = ["mock@1.0.0", "shell@1.0.0"];
    assert_eq!(fixture.live_executors().await, both);
    sqlx::query(
        "UPDATE worker_instances SET last_heartbeat_at = now() - interval '61 seconds'
         WHERE id = $1",
    )
    .bind(mock.worker_id())
    .execute(&fixture.database.owner)
    .await
    .unwrap();
    assert_eq!(fixture.live_executors().await, ["shell@1.0.0"]);
    assert!(mock.claim(0, 30).await.unwrap().is_empty());
    assert_eq!(fixture.live_executors().await, both);

    let claimed = fixture.claim(&mock).await;
    assert_eq!(claimed.keys().collect::<Vec<_>>(), ["vpc"]);
}

#[tokio::test]
#[serial]
async fn each_job_carries_the_definition_of_its_own_organization() {
    let fixture = Fixture::new().await;
    let mut node = task("task", 1);
    node["definition"]["key"] = json!("acme.task");
    let graph = graph(&[node], &[]);
    let worker = fixture.worker(&["mock@1.0.0"]).await;

    // Both organizations define acme.task version 1, with different result types.
    for (tenant, result_type) in [(fixture.a, "string"), (fixture.b, "number")] {
        let definition: NewNodeDefinition = serde_json::from_value(json!({
            "key": "acme.task", "displayName": "Task", "category": "Testing",
            "inputs": [],
            "outputs": [{
                "name": "result", "dataType": format!("core.{result_type}"),
                "schema": {"type": result_type}
            }],
            "configSchema": {"type": "object"}, "uiSchema": {}, "defaults": {},
            "executor": {"key": "mock", "version": "1.0.0"}
        }))
        .unwrap();
        let (organization_id, admin) = (tenant.organization_id, tenant.admin);
        let mut tx = fixture.tx(tenant).await;
        catalog::insert_node_definition(&mut tx, organization_id, admin, &definition)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let published = fixture.publish(tenant, &graph).await;
        let run_id = fixture.run(&published, &published.plan).await;

        let claimed = worker.claim(10, 30).await.unwrap().remove(0);
        let job = worker.start(claimed.node_run_id, claimed.lease_token).await;
        let expected = Job {
            execution_id: claimed.node_run_id,
            attempt: 1,
            max_attempts: 1,
            timeout_seconds: 60,
            workflow_id: published.workflow_id,
            run_id,
            node_id: "task".to_owned(),
            cancel_requested: false,
            run_input: json!({"region": "eu-west-1"}),
            graph: graph.clone(),
            definition: definition.spec,
            upstream_outputs: HashMap::new(),
        };
        assert_eq!(job.unwrap(), Some(expected));
    }
}
```

The eleven tests:

- `concurrent_claimers_never_share_a_node_run`: eight workers claim 200 queued node runs at the same time, five at
  a time, on the pool's separate connections. Every node run is claimed once, at attempt 1 and token 1, by the
  worker that got it.
- `only_the_lease_holder_can_start_renew_log_or_complete_a_node_run`: four bad leases make every fenced call
  return NULL/false: another worker with the right token, an older token, a newer token, and organization B's node
  run with its own token. Node runs, runs and events stay exactly the same. The real lease then works for all
  four calls, and after the completion it works for none.
- `parents_that_finish_together_release_their_child_once`: 50 rounds of four parents completing concurrently.
  Their child ends `queued` with `pending_parents` 0 and exactly one release event (seq 6).
- `a_child_waits_once_for_each_distinct_parent_and_the_run_succeeds`: `join` has three edges from two parents.
  `split` feeds two of its ports. One finished parent does not release it, and the second releases it once. Its
  start payload carries both parents' outputs. The whole event log is checked, including `run.state running` on
  the first start only. So are the stored input and output of each node, the cleared leases and the run's
  `succeeded` status.
- `a_failure_skips_only_its_descendants_and_fails_the_run`: `a -> b -> c` plus `u`. A non-retryable failure of
  `a`, which has a second attempt, fails it and skips `b` and `c`. `u` keeps running, and the run ends `failed`
  after `u` succeeds.
- `retryable_failures_wait_with_backoff_until_the_attempts_run_out`: attempt 1 waits in `retry_wait` with its
  error and no lease. Its `available_at` is 2.0 to 2.4 s after the completion, measured against the `created_at`
  of the `retry_wait` event, which shares the completion's `now()`. It is not claimable before then. Claimed again
  as attempt 2 with token 2, the second failure exhausts the attempts: the node and the run fail.
- `after_a_cancel_request_a_retryable_failure_cancels_the_node_and_the_run`: the start payload and the heartbeat
  both report `cancelRequested`. A retryable failure with two attempts left cancels the node, keeps its error and
  settles the run `canceled`. The run's nodes, even when set back to `queued`, are not claimable.
- `heartbeats_renew_held_leases_and_report_lost_ones_in_order`: a batch with an unknown lease and a held one
  answers in order. The held lease now runs for 120 s, and the worker's heartbeat time is fresh.
- `logs_stop_at_the_cap_on_a_character_boundary`: 5 bytes of `hello` are counted. With 5 bytes left, `abc€` keeps
  `abc`, because the fifth byte would split the 3-byte `€`. One `node.log_truncated` follows, the next batch is
  dropped, and every call still returns true. An unknown stream raises 22023.
- `workers_claim_only_the_executors_they_advertise`: an unregistered worker and a `shell@1.0.0` worker claim no
  mock node. `live_executors()`, called as `corqty_api`, follows the workers heard from in the last minute. A
  claim with no free slot counts as a heartbeat.
- `each_job_carries_the_definition_of_its_own_organization`: A and B both define `acme.task` version 1 with
  different result types. Each start payload equals the complete expected `Job`, including its own organization's
  spec.

In `crates/persistence/tests/catalog_guards.rs`:

Replace:

```rust
            "table guard_probe grants privileges to corqty_worker",
            "table guard_probe lacks forced row-level security or a policy",
        ]
    );
}
```

With:

```rust
            "table guard_probe grants privileges to corqty_worker",
            "table guard_probe lacks forced row-level security or a policy",
        ]
    );
}

#[tokio::test]
#[serial]
async fn the_worker_role_executes_only_the_worker_functions() {
    let database = TestDatabase::reset().await;

    // Only definer functions run with more rights than the caller's, and the worker role
    // has no table privileges of its own.
    let functions: Vec<String> = sqlx::query_scalar(
        "SELECT p.proname::text
         FROM pg_proc p
         WHERE p.pronamespace = 'corqty'::regnamespace
           AND p.prosecdef
           AND has_function_privilege('corqty_worker', p.oid, 'EXECUTE')
         ORDER BY 1",
    )
    .fetch_all(&database.owner)
    .await
    .unwrap();

    assert_eq!(
        functions,
        [
            "append_node_log",
            "claim_node_runs",
            "complete_node_run",
            "heartbeat",
            "register_worker",
            "start_node_run",
        ]
    );
}
```

The guard lists definer functions only. `corqty.current_user_id()` and `corqty.current_organization_id()` keep
PUBLIC's default EXECUTE, but they are SECURITY INVOKER and only read settings. An invoker function runs with the
worker's own rights, and the worker has none on tables.

- [ ] **Step 2: Run the tests and verify RED**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-persistence --test worker_queue
```

Expected: the test does not compile, because the repository module does not exist yet:

```text
error[E0432]: unresolved import `corqty_persistence::worker_queue`
  --> crates\persistence\tests\worker_queue.rs:13:5
   |
13 |     worker_queue::{ClaimedJob, Job, LeaseStatus, NodeError, Outcome, WorkerDb},
   |     ^^^^^^^^^^^^ could not find `worker_queue` in `corqty_persistence`
```

The last line is ``error: could not compile `corqty-persistence` (test "worker_queue") due to 1 previous error``.

```text
cargo test -p corqty-persistence --test catalog_guards
```

Expected: 3 passed and 1 failed. So far the worker role may execute no definer function:

```text
---- the_worker_role_executes_only_the_worker_functions stdout ----

thread 'the_worker_role_executes_only_the_worker_functions' (34720) panicked at crates\persistence\tests\catalog_guards.rs:141:5:
assertion `left == right` failed
  left: []
 right: ["append_node_log", "claim_node_runs", "complete_node_run", "heartbeat", "register_worker", "start_node_run"]
```

- [ ] **Step 3: Add the worker functions**

Create `migrations/202610260002_worker_functions.sql`:

```sql
-- Workers reach run data only through the definer functions below, granted to corqty_worker
-- one by one. Every call after the claim is fenced: the node run must be running under the
-- caller's worker id and the lease token of its claim. A worker that lost its lease, or that
-- names another worker's or another organization's node run, changes nothing.

CREATE TYPE corqty.claimed_job AS (
    node_run_id uuid,
    run_id uuid,
    organization_id uuid,
    executor text,
    attempt integer,
    lease_token bigint
);

-- Records or refreshes a worker process and the executors it runs, as "<key>@<version>".
CREATE FUNCTION corqty.register_worker(
    p_worker_id uuid,
    p_executors text[],
    p_host_contracts text[],
    p_capacity integer
)
RETURNS void
LANGUAGE sql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    INSERT INTO worker_instances(id, executors, host_contracts, capacity)
    VALUES (p_worker_id, p_executors, p_host_contracts, p_capacity)
    ON CONFLICT (id) DO UPDATE
    SET executors = EXCLUDED.executors,
        host_contracts = EXCLUDED.host_contracts,
        capacity = EXCLUDED.capacity,
        last_heartbeat_at = now()
$$;

-- Leases up to p_limit due node runs of the worker's executors, oldest first. A due retry is
-- claimed straight from retry_wait, and each claim consumes an attempt. Nothing of a run that
-- was asked to cancel is handed out. Every call, even with p_limit 0, counts as a heartbeat.
-- SKIP LOCKED lets workers claim side by side; no runs row is locked.
CREATE FUNCTION corqty.claim_node_runs(p_worker_id uuid, p_limit integer, p_lease_seconds integer)
RETURNS SETOF corqty.claimed_job
LANGUAGE sql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    WITH worker AS (
        UPDATE worker_instances
        SET last_heartbeat_at = now()
        WHERE id = p_worker_id
        RETURNING executors
    ),
    due AS (
        SELECT n.id
        FROM node_runs AS n
        JOIN runs AS r ON r.id = n.run_id
        JOIN worker AS w ON n.executor = ANY (w.executors)
        WHERE n.state IN ('queued', 'retry_wait')
          AND n.available_at <= now()
          AND r.cancel_requested_at IS NULL
        ORDER BY n.available_at, n.created_at
        LIMIT p_limit
        FOR UPDATE OF n SKIP LOCKED
    )
    UPDATE node_runs AS n
    SET state = 'running',
        attempt = n.attempt + 1,
        lease_token = n.lease_token + 1,
        leased_by = p_worker_id,
        lease_expires_at = now() + make_interval(secs => p_lease_seconds)
    FROM due
    WHERE n.id = due.id
    RETURNING n.id, n.run_id, n.organization_id, n.executor, n.attempt, n.lease_token
$$;

-- Locks the run, then the node run, if the worker holds the node run's lease, and returns
-- the run id; NULL when the fence does not match. Every writer of a run takes its runs row
-- first, so writers of one run never deadlock.
CREATE FUNCTION corqty.lock_lease(p_worker_id uuid, p_node_run_id uuid, p_lease_token bigint)
RETURNS uuid
LANGUAGE plpgsql
SET search_path = public, pg_temp
AS $$
DECLARE
    v_run uuid;
BEGIN
    -- node_runs.run_id never changes, so it is read without a lock, and a caller without the
    -- lease locks nothing.
    SELECT run_id INTO v_run
    FROM node_runs
    WHERE id = p_node_run_id
      AND leased_by = p_worker_id
      AND lease_token = p_lease_token
      AND state = 'running';
    IF NOT FOUND THEN
        RETURN NULL;
    END IF;
    PERFORM FROM runs WHERE id = v_run FOR UPDATE;
    -- The fence again under the lock: the lease may have ended in the meantime.
    PERFORM FROM node_runs
    WHERE id = p_node_run_id
      AND leased_by = p_worker_id
      AND lease_token = p_lease_token
      AND state = 'running'
    FOR UPDATE;
    IF NOT FOUND THEN
        RETURN NULL;
    END IF;
    RETURN v_run;
END
$$;

-- Moves a running node run on after an attempt, inside the caller's lock on its run.
-- p_outcome is {"status": "succeeded", "input": {..}, "output": {..}},
-- {"status": "failed", "input"?: {..}, "error": {code, message, retryable}} or
-- {"status": "canceled"}. A retryable failure with attempts left waits in retry_wait with
-- exponential backoff from 2 s (20% jitter, at most 300 s), unless the run was asked to
-- cancel: then the failure, like a canceled attempt, cancels the node and keeps the error.
-- A success releases each child whose parents have all succeeded; a failed or canceled node
-- skips its blocked descendants. Each change is one node.state event, then the run settles.
CREATE FUNCTION corqty.finish_node_run(p_node_run_id uuid, p_outcome jsonb)
RETURNS void
LANGUAGE plpgsql
SET search_path = public, pg_temp
AS $$
DECLARE
    v_run uuid;
    v_state text;
    v_children text[];
    v_event jsonb;
BEGIN
    SELECT n.run_id,
           CASE
               WHEN p_outcome ->> 'status' = 'succeeded' THEN 'succeeded'
               WHEN p_outcome ->> 'status' = 'canceled' OR r.cancel_requested_at IS NOT NULL
                   THEN 'canceled'
               WHEN (p_outcome #>> '{error,retryable}')::boolean AND n.attempt < n.max_attempts
                   THEN 'retry_wait'
               ELSE 'failed'
           END
    INTO v_run, v_state
    FROM node_runs AS n
    JOIN runs AS r ON r.id = n.run_id
    WHERE n.id = p_node_run_id;

    UPDATE node_runs
    SET state = v_state,
        input = coalesce(p_outcome -> 'input', input),
        output = CASE WHEN v_state = 'succeeded' THEN p_outcome -> 'output' END,
        error = p_outcome -> 'error',
        available_at = CASE
            WHEN v_state = 'retry_wait' THEN now() + make_interval(
                secs => least(300, 2 * power(2, attempt - 1) * (1 + 0.2 * random()))
            )
            ELSE available_at
        END,
        leased_by = NULL,
        lease_expires_at = NULL,
        finished_at = CASE WHEN v_state = 'retry_wait' THEN NULL ELSE now() END
    WHERE id = p_node_run_id
    RETURNING children,
              jsonb_build_object(
                  'type', 'node.state', 'nodeId', node_id, 'state', state,
                  'attempt', attempt, 'error', error, 'output', output
              )
    INTO v_children, v_event;
    PERFORM corqty.append_run_event(v_run, v_event);

    IF v_state = 'succeeded' THEN
        -- One statement locks the children in id order. Each parent decrements a child once,
        -- and the child is queued when no parent is left.
        FOR v_event IN
            WITH released AS (
                UPDATE node_runs AS child
                SET pending_parents = child.pending_parents - 1,
                    state = CASE WHEN child.pending_parents = 1 THEN 'queued' ELSE 'blocked' END,
                    available_at = now()
                FROM (
                    SELECT id
                    FROM node_runs
                    WHERE run_id = v_run AND node_id = ANY (v_children) AND state = 'blocked'
                    ORDER BY id
                    FOR UPDATE
                ) AS waiting
                WHERE child.id = waiting.id
                RETURNING child.id, child.node_id, child.state, child.attempt
            )
            SELECT jsonb_build_object(
                       'type', 'node.state', 'nodeId', node_id, 'state', state,
                       'attempt', attempt
                   )
            FROM released
            WHERE state = 'queued'
            ORDER BY id
        LOOP
            PERFORM corqty.append_run_event(v_run, v_event);
        END LOOP;
    ELSIF v_state IN ('failed', 'canceled') THEN
        FOR v_event IN
            WITH RECURSIVE below(node_id) AS (
                SELECT unnest(v_children)
                UNION
                SELECT child
                FROM below
                JOIN node_runs AS n ON n.run_id = v_run AND n.node_id = below.node_id
                CROSS JOIN unnest(n.children) AS child
            ),
            skipped AS (
                UPDATE node_runs AS descendant
                SET state = 'skipped', finished_at = now()
                FROM (
                    SELECT id
                    FROM node_runs
                    WHERE run_id = v_run
                      AND node_id IN (SELECT node_id FROM below)
                      AND state = 'blocked'
                    ORDER BY id
                    FOR UPDATE
                ) AS waiting
                WHERE descendant.id = waiting.id
                RETURNING descendant.id, descendant.node_id, descendant.attempt
            )
            SELECT jsonb_build_object(
                       'type', 'node.state', 'nodeId', node_id, 'state', 'skipped',
                       'attempt', attempt
                   )
            FROM skipped
            ORDER BY id
        LOOP
            PERFORM corqty.append_run_event(v_run, v_event);
        END LOOP;
    END IF;

    PERFORM corqty.settle_run(v_run);
END
$$;

-- Starts a claimed attempt and returns its job, or NULL when the fence does not match. The
-- first start of a run moves it to running. The definition is the system one or the run's
-- own organization's: organization keys never start with "corqty.", so one row matches.
CREATE FUNCTION corqty.start_node_run(p_worker_id uuid, p_node_run_id uuid, p_lease_token bigint)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
    v_run uuid := corqty.lock_lease(p_worker_id, p_node_run_id, p_lease_token);
    v_event jsonb;
    v_job jsonb;
BEGIN
    IF v_run IS NULL THEN
        RETURN NULL;
    END IF;
    UPDATE runs SET status = 'running', started_at = now() WHERE id = v_run AND status = 'queued';
    IF FOUND THEN
        PERFORM corqty.append_run_event(v_run, '{"type": "run.state", "state": "running"}');
    END IF;
    UPDATE node_runs
    SET started_at = now()
    WHERE id = p_node_run_id
    RETURNING jsonb_build_object(
        'type', 'node.state', 'nodeId', node_id, 'state', 'running', 'attempt', attempt
    )
    INTO v_event;
    PERFORM corqty.append_run_event(v_run, v_event);

    SELECT jsonb_build_object(
               'executionId', n.id,
               'attempt', n.attempt,
               'maxAttempts', n.max_attempts,
               'timeoutSeconds', n.timeout_seconds,
               'workflowId', r.workflow_id,
               'runId', r.id,
               'nodeId', n.node_id,
               'cancelRequested', r.cancel_requested_at IS NOT NULL,
               'runInput', r.input,
               'graph', v.graph,
               'definition', (
                   SELECT d.spec
                   FROM jsonb_array_elements(v.graph -> 'nodes') AS node
                   JOIN node_definitions AS d
                     ON d.key = node #>> '{definition,key}'
                    AND d.version = (node #>> '{definition,version}')::integer
                   WHERE node ->> 'id' = n.node_id
                     AND (d.organization_id IS NULL OR d.organization_id = n.organization_id)
               ),
               'upstreamOutputs', (
                   SELECT coalesce(jsonb_object_agg(parent.node_id, parent.output), '{}')
                   FROM node_runs AS parent
                   WHERE parent.run_id = n.run_id
                     AND n.node_id = ANY (parent.children)
                     AND jsonb_typeof(parent.output) = 'object'
               )
           )
    INTO v_job
    FROM node_runs AS n
    JOIN runs AS r ON r.id = n.run_id
    JOIN workflow_versions AS v ON v.id = r.workflow_version_id
    WHERE n.id = p_node_run_id;
    RETURN v_job;
END
$$;

-- Extends each lease the worker still holds and says whether its run was asked to cancel.
-- One row per lease asked about, in order; alive is false for a lease that is gone. It locks
-- only the worker's own running rows, in id order, and no runs row.
CREATE FUNCTION corqty.heartbeat(
    p_worker_id uuid,
    p_node_run_ids uuid[],
    p_lease_tokens bigint[],
    p_lease_seconds integer
)
RETURNS TABLE (node_run_id uuid, alive boolean, cancel_requested boolean)
LANGUAGE sql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    WITH worker AS (
        UPDATE worker_instances SET last_heartbeat_at = now() WHERE id = p_worker_id
    ),
    renewed AS (
        UPDATE node_runs AS n
        SET lease_expires_at = now() + make_interval(secs => p_lease_seconds)
        FROM (
            SELECT own.id
            FROM node_runs AS own
            JOIN unnest(p_node_run_ids, p_lease_tokens) AS lease(id, token)
              ON lease.id = own.id AND lease.token = own.lease_token
            WHERE own.leased_by = p_worker_id AND own.state = 'running'
            ORDER BY own.id
            FOR UPDATE OF own
        ) AS held
        WHERE n.id = held.id
        RETURNING n.id, n.run_id
    )
    SELECT lease.id, renewed.id IS NOT NULL, r.cancel_requested_at IS NOT NULL
    FROM unnest(p_node_run_ids) WITH ORDINALITY AS lease(id, position)
    LEFT JOIN renewed ON renewed.id = lease.id
    LEFT JOIN runs AS r ON r.id = renewed.run_id
    ORDER BY lease.position
$$;

-- Stores a batch of log text as one node.log event. A node run keeps at most 10 MiB of log
-- text (MAX_LOG_BYTES_PER_NODE_RUN): the batch that crosses the cap keeps its longest prefix
-- that fits without splitting a character, then one node.log_truncated event follows and
-- later text is dropped. True while the lease holds, stored or not; false once it is lost.
CREATE FUNCTION corqty.append_node_log(
    p_worker_id uuid,
    p_node_run_id uuid,
    p_lease_token bigint,
    p_stream text,
    p_text text
)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
    v_run uuid;
    v_node text;
    v_logged bigint;
    v_truncated boolean;
    v_bytes bytea := convert_to(p_text, 'UTF8');
    v_kept integer;
BEGIN
    IF p_stream NOT IN ('stdout', 'stderr', 'system') THEN
        RAISE EXCEPTION 'unknown log stream %', p_stream USING ERRCODE = 'invalid_parameter_value';
    END IF;
    v_run := corqty.lock_lease(p_worker_id, p_node_run_id, p_lease_token);
    IF v_run IS NULL THEN
        RETURN false;
    END IF;
    SELECT node_id, log_bytes, log_truncated INTO v_node, v_logged, v_truncated
    FROM node_runs
    WHERE id = p_node_run_id;
    IF v_truncated THEN
        RETURN true;
    END IF;

    v_kept := least(length(v_bytes), 10485760 - v_logged);
    -- Back off while the first byte left out continues a UTF-8 character (0b10xxxxxx).
    WHILE v_kept < length(v_bytes) AND get_byte(v_bytes, v_kept) & 192 = 128 LOOP
        v_kept := v_kept - 1;
    END LOOP;
    UPDATE node_runs
    SET log_bytes = log_bytes + v_kept,
        log_truncated = v_kept < length(v_bytes)
    WHERE id = p_node_run_id;
    IF v_kept > 0 THEN
        PERFORM corqty.append_run_event(v_run, jsonb_build_object(
            'type', 'node.log', 'nodeId', v_node, 'stream', p_stream,
            'text', convert_from(substring(v_bytes FROM 1 FOR v_kept), 'UTF8')
        ));
    END IF;
    IF v_kept < length(v_bytes) THEN
        PERFORM corqty.append_run_event(v_run, jsonb_build_object(
            'type', 'node.log_truncated', 'nodeId', v_node
        ));
    END IF;
    RETURN true;
END
$$;

-- Records how an attempt ended (see finish_node_run). False when the fence does not match.
CREATE FUNCTION corqty.complete_node_run(
    p_worker_id uuid,
    p_node_run_id uuid,
    p_lease_token bigint,
    p_outcome jsonb
)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
BEGIN
    IF corqty.lock_lease(p_worker_id, p_node_run_id, p_lease_token) IS NULL THEN
        RETURN false;
    END IF;
    PERFORM corqty.finish_node_run(p_node_run_id, p_outcome);
    RETURN true;
END
$$;

-- The executors that a worker heard from in the last minute runs, so the API can tell a
-- queued node that no live worker can take. The API cannot read worker_instances itself.
CREATE FUNCTION corqty.live_executors()
RETURNS text[]
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    SELECT coalesce(array_agg(DISTINCT executor ORDER BY executor), '{}')
    FROM worker_instances, unnest(executors) AS executor
    WHERE last_heartbeat_at > now() - interval '60 seconds'
$$;

-- lock_lease and finish_node_run are SECURITY INVOKER helpers of the definer functions,
-- which call them as the owner. No role may call them directly.
REVOKE ALL ON FUNCTION
    corqty.register_worker(uuid, text[], text[], integer),
    corqty.claim_node_runs(uuid, integer, integer),
    corqty.lock_lease(uuid, uuid, bigint),
    corqty.finish_node_run(uuid, jsonb),
    corqty.start_node_run(uuid, uuid, bigint),
    corqty.heartbeat(uuid, uuid[], bigint[], integer),
    corqty.append_node_log(uuid, uuid, bigint, text, text),
    corqty.complete_node_run(uuid, uuid, bigint, jsonb),
    corqty.live_executors()
FROM PUBLIC;
GRANT EXECUTE ON FUNCTION
    corqty.register_worker(uuid, text[], text[], integer),
    corqty.claim_node_runs(uuid, integer, integer),
    corqty.start_node_run(uuid, uuid, bigint),
    corqty.heartbeat(uuid, uuid[], bigint[], integer),
    corqty.append_node_log(uuid, uuid, bigint, text, text),
    corqty.complete_node_run(uuid, uuid, bigint, jsonb)
TO corqty_worker;
GRANT EXECUTE ON FUNCTION corqty.live_executors() TO corqty_api;
```

Notes on the SQL:

- `claim_node_runs` and `heartbeat` are single statements, so they are `LANGUAGE sql`. The others need several
  statements and are plpgsql, with `v_` locals and `p_` parameters, so that no name clashes with a column.
- `claim_node_runs` updates `worker_instances` in a CTE that also yields the registered executors. A worker that
  never registered matches no executor and claims nothing.
- Release and skip events come from `FOR v_event IN WITH ... UPDATE ... RETURNING ... SELECT ... ORDER BY id LOOP`.
  plpgsql runs a data-modifying query in a `FOR` loop to completion, and the `ORDER BY id` puts the events in plan
  order.
- `append_node_log` cuts on bytes: it backs off while the first byte left out is a UTF-8 continuation byte
  (`0b10xxxxxx`). That is at most three steps, so `convert_from` always gets whole characters.
- `start_node_run` keeps only parent outputs that are objects (`jsonb_typeof(..) = 'object'`). Every released
  node's parents succeeded with an object output anyway, so the payload always deserializes into `Job`.

- [ ] **Step 4: Add the worker queue repository**

Create `crates/persistence/src/worker_queue.rs`:

```rust
//! The worker side of the run queue. The worker role has no table privileges: each call is
//! one `corqty` definer function, and every call after the claim is fenced by the worker id
//! and the lease token of the claim.

use std::collections::HashMap;

use corqty_domain::{catalog::NodeDefinitionSpec, graph::GraphSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::{PgPool, types::Json};
use uuid::Uuid;

/// A node run leased to this worker. Its id and lease token identify it in every later call.
#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct ClaimedJob {
    pub node_run_id: Uuid,
    pub run_id: Uuid,
    pub organization_id: Uuid,
    /// `"<key>@<version>"`, as the worker advertised it.
    pub executor: String,
    pub attempt: i32,
    pub lease_token: i64,
}

/// What a started attempt needs to resolve its inputs and run.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    /// The node run id, the same on every attempt.
    pub execution_id: Uuid,
    pub attempt: i32,
    pub max_attempts: i32,
    pub timeout_seconds: i32,
    pub workflow_id: Uuid,
    pub run_id: Uuid,
    pub node_id: String,
    pub cancel_requested: bool,
    pub run_input: Value,
    /// The pinned version's graph.
    pub graph: GraphSnapshot,
    /// The spec of the node's definition: a system one or the run's organization's own.
    pub definition: NodeDefinitionSpec,
    /// The output of each parent, by node id.
    pub upstream_outputs: HashMap<String, Map<String, Value>>,
}

/// One answer of a heartbeat. A lease that is not alive is gone: the worker stops the attempt
/// and reports nothing more about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct LeaseStatus {
    pub node_run_id: Uuid,
    pub alive: bool,
    pub cancel_requested: bool,
}

/// How an attempt ended.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    Succeeded {
        input: Map<String, Value>,
        output: Map<String, Value>,
    },
    /// `input` is None when the inputs could not be resolved.
    Failed {
        #[serde(skip_serializing_if = "Option::is_none")]
        input: Option<Map<String, Value>>,
        error: NodeError,
    },
    Canceled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NodeError {
    pub code: String,
    pub message: String,
    /// Whether another attempt may succeed. The database retries only while attempts are
    /// left and the run was not asked to cancel.
    pub retryable: bool,
}

/// One worker process's handle on the queue. The worker id keys its leases and its
/// `worker_instances` row.
#[derive(Clone, Debug)]
pub struct WorkerDb {
    pool: PgPool,
    worker_id: Uuid,
}

impl WorkerDb {
    #[must_use]
    pub const fn new(pool: PgPool, worker_id: Uuid) -> Self {
        Self { pool, worker_id }
    }

    #[must_use]
    pub const fn worker_id(&self) -> Uuid {
        self.worker_id
    }

    /// Records the executors this worker runs, as `"<key>@<version>"`, and its capacity.
    pub async fn register(&self, executors: &[String], capacity: i32) -> Result<(), sqlx::Error> {
        // Host contracts stay empty until plugins are loaded at run time.
        sqlx::query("SELECT corqty.register_worker($1, $2, '{}', $3)")
            .bind(self.worker_id)
            .bind(executors)
            .bind(capacity)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Leases up to `limit` due node runs of the registered executors for `lease_seconds`.
    /// Every call, `limit` 0 included, also counts as the worker's heartbeat.
    pub async fn claim(
        &self,
        limit: i32,
        lease_seconds: i32,
    ) -> Result<Vec<ClaimedJob>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM corqty.claim_node_runs($1, $2, $3)")
            .bind(self.worker_id)
            .bind(limit)
            .bind(lease_seconds)
            .fetch_all(&self.pool)
            .await
    }

    /// Starts a claimed attempt and returns its job, or None when the lease is gone.
    pub async fn start(
        &self,
        node_run_id: Uuid,
        lease_token: i64,
    ) -> Result<Option<Job>, sqlx::Error> {
        let job: Option<Json<Job>> = sqlx::query_scalar("SELECT corqty.start_node_run($1, $2, $3)")
            .bind(self.worker_id)
            .bind(node_run_id)
            .bind(lease_token)
            .fetch_one(&self.pool)
            .await?;
        Ok(job.map(|Json(job)| job))
    }

    /// Extends the `(node run id, lease token)` leases the worker still holds to
    /// `lease_seconds` from now. One status per lease, in order.
    pub async fn heartbeat(
        &self,
        leases: &[(Uuid, i64)],
        lease_seconds: i32,
    ) -> Result<Vec<LeaseStatus>, sqlx::Error> {
        let (node_run_ids, lease_tokens): (Vec<Uuid>, Vec<i64>) = leases.iter().copied().unzip();
        sqlx::query_as("SELECT * FROM corqty.heartbeat($1, $2, $3, $4)")
            .bind(self.worker_id)
            .bind(node_run_ids)
            .bind(lease_tokens)
            .bind(lease_seconds)
            .fetch_all(&self.pool)
            .await
    }

    /// Appends log text from `stream` (`stdout`, `stderr` or `system`). False only when the
    /// lease is gone: text past the node run's cap is dropped and still returns true.
    pub async fn append_log(
        &self,
        node_run_id: Uuid,
        lease_token: i64,
        stream: &str,
        text: &str,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar("SELECT corqty.append_node_log($1, $2, $3, $4, $5)")
            .bind(self.worker_id)
            .bind(node_run_id)
            .bind(lease_token)
            .bind(stream)
            .bind(text)
            .fetch_one(&self.pool)
            .await
    }

    /// Records how the attempt ended. False when the lease is gone.
    pub async fn complete(
        &self,
        node_run_id: Uuid,
        lease_token: i64,
        outcome: &Outcome,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar("SELECT corqty.complete_node_run($1, $2, $3, $4)")
            .bind(self.worker_id)
            .bind(node_run_id)
            .bind(lease_token)
            .bind(Json(outcome))
            .fetch_one(&self.pool)
            .await
    }
}
```

Replace the whole of `crates/persistence/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod audit;
pub mod auth;
pub mod catalog;
pub mod members;
pub mod migrate;
pub mod models;
pub mod organizations;
pub mod runs;
pub mod store;
pub mod tenant;
pub mod worker_queue;
pub mod workflows;
```

`Outcome` serializes with the `status` tag, and `Failed` leaves out an absent `input`. The SQL then keeps the
previous attempt's input instead of storing a JSON null. The wrappers do not retry on 40P01 or 40001. Each call is
a single statement on the pool, so Task 14's worker can retry it as it is (D11).

- [ ] **Step 5: Verify GREEN**

```text
cargo test -p corqty-persistence --test worker_queue --test catalog_guards
```

Expected: `catalog_guards` 4 passed, and `worker_queue` 11 passed in about 4 s.

Nine negative controls show that the tests catch the mistakes they target. Each one replaced one function in the
test database with a changed copy (`CREATE OR REPLACE FUNCTION` through `docker exec -i
wt-backend-postgres-test-1 psql -U corqty_owner -d corqty_test`), ran the named test, and restored the function
from the migration text:

- `claim_node_runs` without `FOR UPDATE OF n SKIP LOCKED`: `concurrent_claimers_never_share_a_node_run` counts
  705 claims of 200 node runs.
- `lock_lease` without `AND leased_by = p_worker_id`: another worker with the right token gets
  `(true, false, true, true)` instead of all false. Heartbeat has its own fence.
- `finish_node_run` without `OR r.cancel_requested_at IS NOT NULL`: the canceled run's node goes to `retry_wait`.
- `claim_node_runs` without `AND r.cancel_requested_at IS NULL`: the canceled run's requeued nodes are claimed.
- `finish_node_run` without `AND n.attempt < n.max_attempts`: attempt 2 goes to `retry_wait` again instead of
  `failed`.
- `append_node_log` with the backoff test replaced by `false`: `convert_from` gets half a `€`, and the append
  fails.
- `start_node_run` without the organization filter on the definition: the second organization's start fails,
  because the subquery returns more than one row.
- `finish_node_run` queuing a child at its first decrement: the fan-in test finds `join` claimable after one
  parent.
- `finish_node_run` skipping only direct children: `c` stays `blocked`.

Recreate the test database afterwards, so that no changed function is left behind:

```text
docker compose -f docker-compose.test.yml down -v
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: the container is removed, then reported `Healthy`.

- [ ] **Step 6: Run the full backend verification**

```text
cargo fmt --check
```

Expected: no output, exit code 0.

```text
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: `Finished` with no warnings.

```text
cargo test --workspace
```

Expected: every suite passes. corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 4,
`catalog_http` 4, `members_http` 3, `organizations_http` 1, `problems_http` 2, `publish_http` 2, `shutdown` 1,
`workflows_http` 4. corqty-domain: `account_and_rbac` 6, `catalog_rules` 7, `graph_rules` 8, `publish_rules` 10,
`run_rules` 8, `runtime_rules` 12. corqty-executor-mock: `contract` 11. corqty-executor-shell: `contract` 0 on
Windows and macOS, 15 on Linux. corqty-persistence: `auth_store` 2, `catalog_guards` 4, `final_admin` 2,
`migrations` 1, `run_store` 8, `tenant_isolation` 1, `test_reset` 1, `worker_queue` 11, `workflow_catalog` 6.
corqty-plugin-host: `registry` 8. That is 135 tests on Windows and 150 on Linux (CI). `run_store.rs` and
`test_reset.rs` pass unchanged: the worker role still reaches no table, and the migration adds no table.

- [ ] **Step 7: Commit**

```text
git add migrations/202610260002_worker_functions.sql crates/persistence
git commit -m "feat: claim, start, heartbeat, log, and complete node runs through worker-only functions"
```

Expected: `git show --stat HEAD` lists exactly the 5 files above (1651 insertions).

## Task 10: Start, inspect, cancel, and rerun runs over the API

**Files:**

- Create: `crates/api/src/runs.rs`
- Modify: `crates/api/src/lib.rs`
- Modify: `crates/api/src/router.rs`
- Modify: `crates/persistence/src/runs.rs`
- Modify: `crates/api/tests/support/mod.rs`
- Modify: `crates/api/tests/authorization_matrix.rs`
- Modify: `crates/persistence/tests/run_store.rs`
- Test: `crates/api/tests/runs_http.rs`

This task puts the Task 8 run repository behind HTTP (design D14 and D16, §4). Every route lives under
`/api/v1/orgs/{organization_id}`, runs in one tenant transaction, and goes through `authorize`, so a non-member gets
404 and a member without the capability gets 403:

| route | capability | answer |
|---|---|---|
| `POST /workflows/{workflow_id}/runs` `{input?, version?}` | RunWorkflows | 201 Run |
| `GET /workflows/{workflow_id}/runs` | ViewOrganization | 200 `{items: RunSummary[]}`, the 50 newest |
| `GET /runs/{run_id}` | ViewOrganization | 200 Run |
| `POST /runs/{run_id}/rerun` `{version?}` | RunWorkflows | 201 Run |
| `POST /runs/{run_id}/cancel` (no body) | RunWorkflows | 200 RunSummary |

Starting a run follows these rules:

- The input defaults to `{}`, and the body must be an object (otherwise 422 `invalid_json`).
- The input's compact serialized JSON may be at most `MAX_RUN_INPUT_BYTES` (1 MiB). A larger one gets 422
  `run_input_too_large`. The check runs right after authorization, before anything is read, so validation never
  sees more than 1 MiB. axum's 2 MiB body limit alone would let a 1.5 MiB input through.
- The version defaults to the workflow's `published_version`. With nothing published the answer is 409
  `workflow_not_published`. An unknown workflow or version is 404.
- The input is validated against the pinned version's `workflowInputSchema` on a blocking thread. Each schema error
  becomes a diagnostic `invalid_input` whose pointer is the instance path ("" for a missing required property) and
  whose message is the masked jsonschema text. The answer is then 422 `run_input_invalid` with those diagnostics.
- The plan comes from `plan_run` over the organization's visible definitions. Publish has already rejected every
  graph that has no plan, so a `None` here is a 500.
- `runs::create_run` then writes the run, its node runs, the `run.state queued` event and the `run.started` audit row
  in the same transaction (Task 8). The handler answers with `runs::get_run`, so a 201 has exactly the shape of
  `GET /runs/{run_id}`.

A rerun reads the source run (404 if invisible), keeps its input and its workflow, and uses the source version
unless the body names another. It goes through the same checks, so a version whose schema the copied input does not
match gives 422 `run_input_invalid`. The rerun is stored with `rerun_of` and audited as `run.rerun`. The web client
sends `{}`.

Cancel takes no extractor for its body. The web client sends no body and no content type, and a JSON extractor
would answer 415. The handler maps `CancelError::NotFound` to 404 and `Finished` to 409 `run_finished`. A
`Database` error with code 40P01 (deadlock) or 40001 (serialization failure) rolls the transaction back, and the
handler tries the whole transaction again, up to three more times. Each try authorizes again. Task 8 already writes
the `run.canceled` audit row inside `cancel_run`.

`get_run` gains the derived `queueReason` (D16). The node SELECT joins `corqty.live_executors()` once, as a function
in FROM. A node in `queued` or `retry_wait` whose executor is not in that list gets
`"waiting_for_compatible_worker"`. Every other node gets null. `NodeRunRecord.queue_reason` is never stored. No Rust
wrapper for `live_executors` is added: the SQL is its only caller. The existing `run_store.rs` shape assertion now
expects the field.

The API tests need the database for what the API cannot do: read events, take row locks, register workers. So
`AuthFixture` keeps the owner and worker pools of the `TestDatabase` that `registered` creates. Fixtures from
`register_another` and `login` share them. `AuthFixture::post(path)` sends a POST without body or content type,
the way the web client cancels.

The tests in `runs_http.rs` publish the simulated-infrastructure blueprint's graph unchanged, through a draft save
and a publish. The node ids are then `vpc`, `rds` and `deploy`, not the fresh UUIDs that creating a workflow from
a blueprint would assign. The tests cover:

- the full 201 Run and its node plan;
- `lastEventSeq` 1 with only `run.state queued` evented;
- the list, newest first;
- 409 and 404 when no version can be pinned;
- 422 `run_input_invalid` with pointers;
- the exact 1 MiB boundary;
- 422 `unsupported_character` for U+0000, through the existing 22P05 mapping;
- the rerun defaults, and the version 2 schema check;
- the exact `run.started`, `run.rerun` and `run.canceled` audit rows;
- the cancel events and the 409 for a second cancel;
- a cancel that loses a real deadlock and is tried again;
- the queue reason with no worker, with a shell-only worker, with a mock worker, and with a stale worker;
- 404 on every run route for another organization's workflow and run ids.

The capability matrix gains `run_routes_enforce_capabilities`.

- [ ] **Step 1: Write the failing tests**

Create `crates/api/tests/runs_http.rs`:

```rust
mod support;

use std::pin::pin;

use axum::{
    body::to_bytes,
    http::{Method, StatusCode},
    response::Response,
};
use corqty_domain::runtime::MAX_RUN_INPUT_BYTES;
use corqty_persistence::worker_queue::WorkerDb;
use serde_json::{Map, Value, json};
use serial_test::serial;
use support::AuthFixture;
use uuid::Uuid;

const INFRASTRUCTURE_BLUEPRINT_ID: &str = "00000000-0000-7000-8000-000000000101";
const INPUT: &str = r#"{"input":{"region":"eu-west-1"}}"#;
const NO_WORKER: &str = "waiting_for_compatible_worker";

/// Asserts the status and returns the JSON body. A run repeats its input, up to 1 MiB.
async fn body(response: Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    let body = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// Registers `<role>@example.com` and adds it to the admin's organization with that role.
async fn member_of(admin: &AuthFixture, role: &str) -> AuthFixture {
    let invitations = format!("/api/v1/orgs/{}/invitations", admin.organization_id);
    let email = format!("{role}@example.com");
    let request = json!({ "email": email, "role": role }).to_string();
    let invitation = body(
        admin.post_json(&invitations, &request).await,
        StatusCode::CREATED,
    )
    .await;
    let member = admin.register_another(&email, "Personal").await;
    let accept = json!({ "token": invitation["token"] }).to_string();
    let accepted = member
        .post_json("/api/v1/invitations/accept", &accept)
        .await;
    assert_eq!(accepted.status(), StatusCode::OK);
    member
}

/// A workflow of the organization whose version 1 is the simulated-infrastructure blueprint
/// with the blueprint's own node ids: vpc, then rds and deploy, which wait for vpc. Returns
/// the workflow's URL.
async fn infrastructure_workflow(member: &AuthFixture, organization_id: Uuid) -> String {
    let org = format!("/api/v1/orgs/{organization_id}");
    let blueprints = body(
        member.get(&format!("{org}/blueprints")).await,
        StatusCode::OK,
    )
    .await;
    let blueprint = blueprints["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|blueprint| blueprint["id"] == INFRASTRUCTURE_BLUEPRINT_ID)
        .unwrap();
    let created = body(
        member
            .post_json(&format!("{org}/workflows"), r#"{"name":"Provision"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let workflow = format!("{org}/workflows/{}", created["id"].as_str().unwrap());
    let draft = json!({ "revision": 1, "graph": blueprint["graph"] }).to_string();
    let saved = member
        .request_json(Method::PUT, &format!("{workflow}/draft"), &draft)
        .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let published = member
        .post_json(&format!("{workflow}/publish"), r#"{"revision":2}"#)
        .await;
    assert_eq!(published.status(), StatusCode::CREATED);
    workflow
}

fn run_id(run: &Value) -> Uuid {
    run["id"].as_str().unwrap().parse().unwrap()
}

/// The URL of a run of the admin's organization.
fn run_path(admin: &AuthFixture, run: &Value) -> String {
    format!(
        "/api/v1/orgs/{}/runs/{}",
        admin.organization_id,
        run_id(run)
    )
}

/// The RunSummary fields of a Run.
fn summary(run: &Value) -> Value {
    let fields = [
        "id",
        "workflowId",
        "version",
        "status",
        "triggerSource",
        "createdAt",
        "startedAt",
        "finishedAt",
        "cancelRequested",
    ];
    let summary: Map<String, Value> = fields
        .into_iter()
        .map(|field| (field.to_owned(), run[field].clone()))
        .collect();
    Value::Object(summary)
}

/// A node run that no worker has claimed yet.
fn unclaimed(node_id: &str, state: &str, queue_reason: Option<&str>) -> Value {
    json!({
        "nodeId": node_id, "state": state, "attempt": 0, "maxAttempts": 1,
        "executor": "mock@1.0.0", "queueReason": queue_reason,
        "input": null, "output": null, "error": null, "startedAt": null, "finishedAt": null,
    })
}

/// Each node's queue reason, in plan order.
async fn queue_reasons(admin: &AuthFixture, run: &Value) -> Vec<Value> {
    let run = body(admin.get(&run_path(admin, run)).await, StatusCode::OK).await;
    run["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["queueReason"].clone())
        .collect()
}

/// The run's events in seq order.
async fn events(admin: &AuthFixture, run: &Value) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM run_events WHERE run_id = $1 ORDER BY seq")
        .bind(run_id(run))
        .fetch_all(&admin.owner)
        .await
        .unwrap()
}

/// The admin's organization's run audit events, newest first, as
/// [actorUserId, action, resourceId, metadata].
async fn run_audit(admin: &AuthFixture) -> Vec<Value> {
    let path = format!("/api/v1/orgs/{}/audit-events", admin.organization_id);
    let events = body(admin.get(&path).await, StatusCode::OK).await;
    events["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["resourceType"] == "run")
        .map(|event| {
            json!([
                event["actorUserId"],
                event["action"],
                event["resourceId"],
                event["metadata"],
            ])
        })
        .collect()
}

#[tokio::test]
#[serial]
async fn runners_start_the_published_version_with_its_plan() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let runner = member_of(&admin, "runner").await;
    let workflow = infrastructure_workflow(&admin, admin.organization_id).await;
    let version = body(
        admin.get(&format!("{workflow}/versions/1")).await,
        StatusCode::OK,
    )
    .await;
    let runs = format!("{workflow}/runs");

    let run = body(runner.post_json(&runs, INPUT).await, StatusCode::CREATED).await;
    assert!(run["createdAt"].is_string());
    assert_eq!(
        run,
        json!({
            "id": run["id"],
            "workflowId": workflow.rsplit('/').next().unwrap(),
            "version": 1,
            "status": "queued",
            "triggerSource": "manual",
            "createdAt": run["createdAt"],
            "startedAt": null,
            "finishedAt": null,
            "cancelRequested": false,
            "workflowName": "Provision",
            "input": { "region": "eu-west-1" },
            "lastEventSeq": 1,
            "graph": version["graph"],
            "nodes": [
                unclaimed("vpc", "queued", Some(NO_WORKER)),
                unclaimed("rds", "blocked", None),
                unclaimed("deploy", "blocked", None),
            ],
        })
    );
    // Only the run is evented at creation; the snapshot holds the node states.
    assert_eq!(
        events(&admin, &run).await,
        [json!({ "type": "run.state", "state": "queued", "seq": 1 })]
    );
    assert_eq!(
        body(runner.get(&run_path(&admin, &run)).await, StatusCode::OK).await,
        run
    );

    // The list holds summaries, newest first.
    let second = body(
        admin
            .post_json(&runs, r#"{"input":{"region":"us-east-1"}}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(
        body(runner.get(&runs).await, StatusCode::OK).await,
        json!({ "items": [summary(&second), summary(&run)] })
    );
}

#[tokio::test]
#[serial]
async fn runs_start_only_from_a_published_version() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let org = format!("/api/v1/orgs/{}", admin.organization_id);
    let created = body(
        admin
            .post_json(&format!("{org}/workflows"), r#"{"name":"Draft only"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let draft_only = format!("{org}/workflows/{}", created["id"].as_str().unwrap());
    let unpublished = body(
        admin.post_json(&format!("{draft_only}/runs"), "{}").await,
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(unpublished["code"], "workflow_not_published");

    // A version that does not exist is not found, whether the workflow is published or not,
    // and so is a workflow that does not exist.
    let workflow = infrastructure_workflow(&admin, admin.organization_id).await;
    let unknown = format!("{org}/workflows/{}", Uuid::new_v4());
    for (path, request) in [
        (
            &workflow,
            r#"{"input":{"region":"eu-west-1"},"version":99}"#,
        ),
        (&draft_only, r#"{"version":1}"#),
        (&unknown, INPUT),
    ] {
        let response = admin.post_json(&format!("{path}/runs"), request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path} {request}");
    }
    let missing = admin.get(&format!("{unknown}/runs")).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    // Nothing was started.
    assert_eq!(
        body(admin.get(&format!("{workflow}/runs")).await, StatusCode::OK).await,
        json!({ "items": [] })
    );
    assert_eq!(run_audit(&admin).await, Vec::<Value>::new());
}

#[tokio::test]
#[serial]
async fn run_inputs_must_match_the_schema_and_fit_in_one_mebibyte() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let workflow = infrastructure_workflow(&admin, admin.organization_id).await;
    let runs = format!("{workflow}/runs");

    let invalid = body(
        admin.post_json(&runs, r#"{"input":{"region":5}}"#).await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(
        invalid,
        json!({
            "type": "about:blank",
            "title": "Validation failed",
            "status": 422,
            "code": "run_input_invalid",
            "diagnostics": [{
                "code": "invalid_input",
                "pointer": "/region",
                "message": "value is not of type \"string\"",
            }],
        })
    );
    // A missing input is the empty object, which lacks the required region.
    let missing = body(
        admin.post_json(&runs, "{}").await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(missing["code"], "run_input_invalid");
    assert_eq!(
        missing["diagnostics"],
        json!([{
            "code": "invalid_input",
            "pointer": "",
            "message": "\"region\" is a required property",
        }])
    );

    // The limit applies to the input's serialized JSON: exactly 1 MiB passes, a byte more
    // does not.
    let room = MAX_RUN_INPUT_BYTES - r#"{"region":""}"#.len();
    let request = |length: usize| json!({ "input": { "region": "a".repeat(length) } }).to_string();
    let too_large = body(
        admin.post_json(&runs, &request(room + 1)).await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(too_large["code"], "run_input_too_large");
    let largest = body(
        admin.post_json(&runs, &request(room)).await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(largest["input"]["region"].as_str().unwrap().len(), room);

    // An input the database cannot store, and one that is not an object.
    let nul = body(
        admin
            .post_json(&runs, r#"{"input":{"region":"eu\u0000west"}}"#)
            .await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(nul["code"], "unsupported_character");
    let list = body(
        admin.post_json(&runs, r#"{"input":["eu-west-1"]}"#).await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(list["code"], "invalid_json");

    // Only the 1 MiB run was started.
    assert_eq!(
        body(admin.get(&runs).await, StatusCode::OK).await,
        json!({ "items": [summary(&largest)] })
    );
}

#[tokio::test]
#[serial]
async fn reruns_copy_the_input_and_default_to_the_source_version() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let runner = member_of(&admin, "runner").await;
    let workflow = infrastructure_workflow(&admin, admin.organization_id).await;
    let first = body(
        runner.post_json(&format!("{workflow}/runs"), INPUT).await,
        StatusCode::CREATED,
    )
    .await;

    // Version 2 also requires a replica count, which the first run's input lacks.
    let stored = body(admin.get(&workflow).await, StatusCode::OK).await;
    let mut graph = stored["draft"]["graph"].clone();
    graph["workflowInputSchema"]["properties"]["replicas"] = json!({ "type": "integer" });
    graph["workflowInputSchema"]["required"] = json!(["region", "replicas"]);
    let save = json!({ "revision": 2, "graph": graph }).to_string();
    let saved = admin
        .request_json(Method::PUT, &format!("{workflow}/draft"), &save)
        .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let published = admin
        .post_json(&format!("{workflow}/publish"), r#"{"revision":3}"#)
        .await;
    assert_eq!(published.status(), StatusCode::CREATED);

    let rerun_path = format!("{}/rerun", run_path(&admin, &first));
    let rerun = body(
        runner.post_json(&rerun_path, "{}").await,
        StatusCode::CREATED,
    )
    .await;
    assert_ne!(rerun["id"], first["id"]);
    assert_eq!(
        (&rerun["version"], &rerun["status"], &rerun["input"]),
        (&json!(1), &json!("queued"), &first["input"])
    );
    assert_eq!(
        (&rerun["graph"], &rerun["nodes"]),
        (&first["graph"], &first["nodes"])
    );
    // On version 2 the copied input is checked against version 2's schema.
    let invalid = body(
        runner.post_json(&rerun_path, r#"{"version":2}"#).await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(
        (&invalid["code"], &invalid["diagnostics"]),
        (
            &json!("run_input_invalid"),
            &json!([{
                "code": "invalid_input",
                "pointer": "",
                "message": "\"replicas\" is a required property",
            }])
        )
    );
    let unknown = runner.post_json(&rerun_path, r#"{"version":99}"#).await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    // A new run without a version starts the published version, now 2.
    let latest = body(
        runner
            .post_json(
                &format!("{workflow}/runs"),
                r#"{"input":{"region":"eu-west-1","replicas":2}}"#,
            )
            .await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(latest["version"], 2);

    // Each new run audited how it started, with ids and the version only.
    assert_eq!(
        run_audit(&admin).await,
        [
            json!([
                runner.user_id,
                "run.started",
                latest["id"],
                { "runId": latest["id"], "version": 2 },
            ]),
            json!([
                runner.user_id,
                "run.rerun",
                rerun["id"],
                { "runId": rerun["id"], "sourceRunId": first["id"], "version": 1 },
            ]),
            json!([
                runner.user_id,
                "run.started",
                first["id"],
                { "runId": first["id"], "version": 1 },
            ]),
        ]
    );
}

#[tokio::test]
#[serial]
async fn cancel_ends_a_queued_run_once() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let runner = member_of(&admin, "runner").await;
    let workflow = infrastructure_workflow(&admin, admin.organization_id).await;
    let run = body(
        runner.post_json(&format!("{workflow}/runs"), INPUT).await,
        StatusCode::CREATED,
    )
    .await;
    let cancel = format!("{}/cancel", run_path(&admin, &run));

    // The web app sends a cancel without a body or a content type.
    let canceled = body(runner.post(&cancel).await, StatusCode::OK).await;
    assert!(canceled["finishedAt"].is_string());
    let mut expected = summary(&run);
    expected["status"] = json!("canceled");
    expected["finishedAt"] = canceled["finishedAt"].clone();
    expected["cancelRequested"] = json!(true);
    assert_eq!(canceled, expected);

    // Every node was waiting, so every node is canceled and the run is over.
    let stored = body(runner.get(&run_path(&admin, &run)).await, StatusCode::OK).await;
    assert_eq!(summary(&stored), canceled);
    let nodes: Vec<Value> = stored["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| {
            json!([
                node["nodeId"],
                node["state"],
                node["queueReason"],
                node["finishedAt"].is_string()
            ])
        })
        .collect();
    assert_eq!(
        nodes,
        [
            json!(["vpc", "canceled", null, true]),
            json!(["rds", "canceled", null, true]),
            json!(["deploy", "canceled", null, true]),
        ]
    );
    let node_canceled = |node_id: &str, seq: i64| {
        json!({
            "type": "node.state", "nodeId": node_id, "state": "canceled", "attempt": 0,
            "seq": seq,
        })
    };
    assert_eq!(
        events(&admin, &run).await,
        [
            json!({ "type": "run.state", "state": "queued", "seq": 1 }),
            node_canceled("vpc", 2),
            node_canceled("rds", 3),
            node_canceled("deploy", 4),
            json!({ "type": "run.state", "state": "canceled", "seq": 5 }),
        ]
    );
    assert_eq!(stored["lastEventSeq"], 5);

    // A finished run cannot be canceled again, and only the first cancel was audited.
    let again = body(runner.post(&cancel).await, StatusCode::CONFLICT).await;
    assert_eq!(again["code"], "run_finished");
    assert_eq!(
        run_audit(&admin).await,
        [
            json!([
                runner.user_id,
                "run.canceled",
                run["id"],
                { "runId": run["id"] },
            ]),
            json!([
                runner.user_id,
                "run.started",
                run["id"],
                { "runId": run["id"], "version": 1 },
            ]),
        ]
    );
}

#[tokio::test]
#[serial]
async fn a_cancel_that_deadlocks_is_tried_again() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let workflow = infrastructure_workflow(&admin, admin.organization_id).await;
    let run = body(
        admin.post_json(&format!("{workflow}/runs"), INPUT).await,
        StatusCode::CREATED,
    )
    .await;

    // Another transaction locks a node of the run and then the run itself, the reverse of the
    // order every writer of a run follows. The cancel, which waits first, loses the deadlock.
    let mut other = admin.owner.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM node_runs WHERE run_id = $1 AND node_id = 'vpc' FOR UPDATE")
        .bind(run_id(&run))
        .execute(&mut *other)
        .await
        .unwrap();
    let cancel = format!("{}/cancel", run_path(&admin, &run));
    let mut response = pin!(admin.post(&cancel));
    let waiting =
        "SELECT EXISTS (SELECT 1 FROM pg_locks WHERE locktype = 'transactionid' AND NOT granted)";
    tokio::select! {
        response = &mut response => panic!("the cancel did not wait: {}", response.status()),
        () = async {
            while !sqlx::query_scalar::<_, bool>(waiting).fetch_one(&admin.owner).await.unwrap() {}
        } => {}
    }
    let deadlock = async {
        sqlx::query("SELECT 1 FROM runs WHERE id = $1 FOR UPDATE")
            .bind(run_id(&run))
            .execute(&mut *other)
            .await
            .unwrap();
        other.commit().await.unwrap();
    };
    let (response, ()) = tokio::join!(response, deadlock);

    let canceled = body(response, StatusCode::OK).await;
    assert_eq!(canceled["status"], "canceled");
    // The failed attempt rolled back completely: one set of events and one audit row.
    assert_eq!(events(&admin, &run).await.len(), 5);
    assert_eq!(
        run_audit(&admin).await,
        [
            json!([
                admin.user_id,
                "run.canceled",
                run["id"],
                { "runId": run["id"] },
            ]),
            json!([
                admin.user_id,
                "run.started",
                run["id"],
                { "runId": run["id"], "version": 1 },
            ]),
        ]
    );
}

#[tokio::test]
#[serial]
async fn queued_nodes_wait_for_a_worker_that_runs_their_executor() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let workflow = infrastructure_workflow(&admin, admin.organization_id).await;
    let run = body(
        admin.post_json(&format!("{workflow}/runs"), INPUT).await,
        StatusCode::CREATED,
    )
    .await;
    // deploy waits for its retry the way vpc waits for its first attempt. rds waits for vpc,
    // not for a worker.
    sqlx::query(
        "UPDATE node_runs SET state = 'retry_wait' WHERE run_id = $1 AND node_id = 'deploy'",
    )
    .bind(run_id(&run))
    .execute(&admin.owner)
    .await
    .unwrap();
    let waiting = [json!(NO_WORKER), Value::Null, json!(NO_WORKER)];
    assert_eq!(queue_reasons(&admin, &run).await, waiting);

    // A worker without the mock executor changes nothing; one with it is enough.
    let shell = WorkerDb::new(admin.worker.clone(), Uuid::now_v7());
    shell
        .register(&["shell@1.0.0".to_owned()], 4)
        .await
        .unwrap();
    assert_eq!(queue_reasons(&admin, &run).await, waiting);
    let mock = WorkerDb::new(admin.worker.clone(), Uuid::now_v7());
    mock.register(&["mock@1.0.0".to_owned()], 4).await.unwrap();
    assert_eq!(
        queue_reasons(&admin, &run).await,
        [Value::Null, Value::Null, Value::Null]
    );

    // A worker that has not been heard from for a minute no longer counts.
    sqlx::query("UPDATE worker_instances SET last_heartbeat_at = now() - interval '61 seconds'")
        .execute(&admin.owner)
        .await
        .unwrap();
    assert_eq!(queue_reasons(&admin, &run).await, waiting);
}

#[tokio::test]
#[serial]
async fn run_routes_do_not_reach_another_organizations_runs() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let beta = body(
        admin
            .post_json("/api/v1/organizations", r#"{"name":"Beta"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let beta: Uuid = beta["organizationId"].as_str().unwrap().parse().unwrap();
    let beta_workflow = infrastructure_workflow(&admin, beta).await;
    let beta_runs = format!("{beta_workflow}/runs");
    let beta_run = body(
        admin.post_json(&beta_runs, INPUT).await,
        StatusCode::CREATED,
    )
    .await;

    // Through Acme's URLs, Beta's workflow and run are not found, just like unknown ids.
    let acme = format!("/api/v1/orgs/{}", admin.organization_id);
    let foreign = (
        beta_workflow.rsplit('/').next().unwrap().to_owned(),
        run_id(&beta_run).to_string(),
    );
    let unknown = (Uuid::new_v4().to_string(), Uuid::new_v4().to_string());
    for (workflow, run) in [foreign, unknown] {
        for (method, path, request) in [
            (Method::GET, format!("{acme}/workflows/{workflow}/runs"), ""),
            (
                Method::POST,
                format!("{acme}/workflows/{workflow}/runs"),
                INPUT,
            ),
            (Method::GET, format!("{acme}/runs/{run}"), ""),
            (Method::POST, format!("{acme}/runs/{run}/cancel"), ""),
            (Method::POST, format!("{acme}/runs/{run}/rerun"), "{}"),
        ] {
            let response = admin.request_json(method.clone(), &path, request).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {path}");
        }
    }

    // Beta's run was neither canceled nor rerun, and Acme audited nothing.
    let beta_run_path = format!("/api/v1/orgs/{beta}/runs/{}", run_id(&beta_run));
    assert_eq!(
        body(admin.get(&beta_run_path).await, StatusCode::OK).await,
        beta_run
    );
    assert_eq!(
        body(admin.get(&beta_runs).await, StatusCode::OK).await,
        json!({ "items": [summary(&beta_run)] })
    );
    assert_eq!(run_audit(&admin).await, Vec::<Value>::new());
}
```

In `crates/api/tests/authorization_matrix.rs`:

Replace:

```rust
#[tokio::test]
#[serial]
async fn org_access_commit_reports_the_deferred_final_admin_check_as_conflict() {
```

With:

```rust
#[tokio::test]
#[serial]
async fn run_routes_enforce_capabilities() {
    let matrix = Matrix::new().await;
    let org = format!("/api/v1/orgs/{}", matrix.organization_id);
    let (_, admin) = &matrix.members[0];
    let request = serde_json::json!({
        "name": "Matrix",
        "blueprintId": "00000000-0000-7000-8000-000000000101",
    });
    let created = admin
        .post_json(&format!("{org}/workflows"), &request.to_string())
        .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let body = to_bytes(created.into_body(), 64 * 1024).await.unwrap();
    let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let workflow = format!("{org}/workflows/{}", created["id"].as_str().unwrap());
    let published = admin
        .post_json(&format!("{workflow}/publish"), r#"{"revision":1}"#)
        .await;
    assert_eq!(published.status(), StatusCode::CREATED);
    // Start a run, so every role has a run to read.
    let input = r#"{"input":{"region":"eu-west-1"}}"#;
    let started = admin.post_json(&format!("{workflow}/runs"), input).await;
    assert_eq!(started.status(), StatusCode::CREATED);
    let body = to_bytes(started.into_body(), 1024 * 1024).await.unwrap();
    let started: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let run = format!("{org}/runs/{}", started["id"].as_str().unwrap());
    for (method, path, body, capability) in [
        (
            Method::GET,
            format!("{workflow}/runs"),
            "",
            Capability::ViewOrganization,
        ),
        (
            Method::POST,
            format!("{workflow}/runs"),
            input,
            Capability::RunWorkflows,
        ),
        (Method::GET, run.clone(), "", Capability::ViewOrganization),
        (
            Method::POST,
            format!("{run}/rerun"),
            "{}",
            Capability::RunWorkflows,
        ),
        (
            Method::POST,
            format!("{run}/cancel"),
            "",
            Capability::RunWorkflows,
        ),
    ] {
        matrix.assert_row(method, &path, body, capability).await;
    }
}

#[tokio::test]
#[serial]
async fn org_access_commit_reports_the_deferred_final_admin_check_as_conflict() {
```

The rerun row sends `{}`, because the rerun's JSON extractor runs before `authorize`. An empty body would answer
400, not 403. The cancel row's body is ignored.

In `crates/persistence/tests/run_store.rs`:

Replace:

```rust
    // Every field is present, with null for a missing value.
    let node = |node_id: &str, state: &str| {
        json!({
            "nodeId": node_id, "state": state, "attempt": 0, "maxAttempts": 1,
            "executor": "mock@1.0.0", "input": null, "output": null, "error": null,
            "startedAt": null, "finishedAt": null
        })
    };
```

With:

```rust
    // Every field is present, with null for a missing value. No worker runs mock@1.0.0, so
    // the queued node waits for one.
    let node = |node_id: &str, state: &str| {
        let queue_reason = (state == "queued").then_some("waiting_for_compatible_worker");
        json!({
            "nodeId": node_id, "state": state, "attempt": 0, "maxAttempts": 1,
            "executor": "mock@1.0.0", "queueReason": queue_reason, "input": null,
            "output": null, "error": null, "startedAt": null, "finishedAt": null
        })
    };
```

- [ ] **Step 2: Give the API test fixture the owner and worker pools**

In `crates/api/tests/support/mod.rs`:

Replace:

```rust
use corqty_test_support::TestDatabase;
use tower::ServiceExt;
```

With:

```rust
use corqty_test_support::TestDatabase;
use sqlx::PgPool;
use tower::ServiceExt;
```

Replace:

```rust
    pub organization_id: Uuid,
    cookies: String,
}
```

With:

```rust
    pub organization_id: Uuid,
    /// The test database's owner and worker pools, for what the API itself cannot do.
    pub owner: PgPool,
    pub worker: PgPool,
    cookies: String,
}
```

Replace:

```rust
            ApiConfig::for_tests(),
        ));
        Self::register_on(app, email, organization_name).await
    }

    pub async fn register_another(&self, email: &str, organization_name: &str) -> Self {
        Self::register_on(self.app.clone(), email, organization_name).await
    }
```

With:

```rust
            ApiConfig::for_tests(),
        ));
        Self::register_on(
            app,
            database.owner,
            database.worker,
            email,
            organization_name,
        )
        .await
    }

    pub async fn register_another(&self, email: &str, organization_name: &str) -> Self {
        Self::register_on(
            self.app.clone(),
            self.owner.clone(),
            self.worker.clone(),
            email,
            organization_name,
        )
        .await
    }
```

Replace:

```rust
        assert_eq!(response.status(), StatusCode::OK);
        Self::from_auth_response(self.app.clone(), response).await
    }

    async fn register_on(app: Router, email: &str, organization_name: &str) -> Self {
```

With:

```rust
        assert_eq!(response.status(), StatusCode::OK);
        Self::from_auth_response(
            self.app.clone(),
            self.owner.clone(),
            self.worker.clone(),
            response,
        )
        .await
    }

    async fn register_on(
        app: Router,
        owner: PgPool,
        worker: PgPool,
        email: &str,
        organization_name: &str,
    ) -> Self {
```

Replace:

```rust
        assert_eq!(response.status(), StatusCode::CREATED);
        Self::from_auth_response(app, response).await
    }

    async fn from_auth_response(app: Router, response: Response) -> Self {
```

With:

```rust
        assert_eq!(response.status(), StatusCode::CREATED);
        Self::from_auth_response(app, owner, worker, response).await
    }

    async fn from_auth_response(
        app: Router,
        owner: PgPool,
        worker: PgPool,
        response: Response,
    ) -> Self {
```

Replace:

```rust
                .parse()
                .unwrap(),
            cookies,
        }
    }
```

With:

```rust
                .parse()
                .unwrap(),
            owner,
            worker,
            cookies,
        }
    }
```

Replace:

```rust
    pub async fn post_json(&self, path: &str, json: &str) -> Response {
```

With:

```rust
    /// A POST without a body or a content type, the way the web app cancels a run.
    pub async fn post(&self, path: &str) -> Response {
        self.app
            .clone()
            .oneshot(
                Request::post(path)
                    .header(header::COOKIE, &self.cookies)
                    .header("x-csrf-token", &self.csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    pub async fn post_json(&self, path: &str, json: &str) -> Response {
```

- [ ] **Step 3: Run the tests and verify RED**

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-api --test runs_http
```

Expected: the tests compile, and all 8 fail because no run route exists yet. Each one stops at its first run request,
inside the `body` helper:

```text
thread 'runners_start_the_published_version_with_its_plan' panicked at crates\api\tests\runs_http.rs:23:5:
  left: 404
 right: 201
thread 'runs_start_only_from_a_published_version' panicked at crates\api\tests\runs_http.rs:23:5:
  left: 404
 right: 409
thread 'run_inputs_must_match_the_schema_and_fit_in_one_mebibyte' panicked at crates\api\tests\runs_http.rs:23:5:
  left: 404
 right: 422
test result: FAILED. 0 passed; 8 failed; 0 ignored; 0 measured; 0 filtered out
```

The other five fail the same way, with `left: 404` and `right: 201`.

```text
cargo test -p corqty-api --test authorization_matrix run_routes
```

Expected: `run_routes_enforce_capabilities` fails at `authorization_matrix.rs:269:5` (`left: 404`, `right: 201`), where
it starts its run.

```text
cargo test -p corqty-persistence --test run_store
```

Expected: 7 pass. `runners_create_runs_from_the_plan_and_viewers_cannot` fails at `run_store.rs:352:5`: the stored
nodes have no `queueReason` key, and the expected vpc node has `String("waiting_for_compatible_worker")`.

- [ ] **Step 4: Derive the queue reason when a run is read**

In `crates/persistence/src/runs.rs`:

Replace:

```rust
    pub max_attempts: i32,
    pub executor: String,
    pub input: Option<Value>,
```

With:

```rust
    pub max_attempts: i32,
    pub executor: String,
    /// `waiting_for_compatible_worker` while the node waits for a worker and no live worker
    /// runs its executor. Derived on read, never stored.
    pub queue_reason: Option<String>,
    pub input: Option<Value>,
```

Replace:

```rust
/// Reads the run before its nodes, never the other way round: the node states are then at
/// least as new as `last_event_seq`, so a client that applies the later events stays current.
```

With:

```rust
/// Reads the run before its nodes, never the other way round: the node states are then at
/// least as new as `last_event_seq`, so a client that applies the later events stays current.
/// A queued or retrying node whose executor no worker has advertised within the last minute
/// gets a queue reason.
```

Replace:

```rust
    run.nodes = sqlx::query_as(
        "SELECT node_id, state, attempt, max_attempts, executor, input, output, error,
                started_at, finished_at
         FROM node_runs
         WHERE run_id = $1
         ORDER BY id",
    )
```

With:

```rust
    run.nodes = sqlx::query_as(
        "SELECT n.node_id, n.state, n.attempt, n.max_attempts, n.executor,
                CASE WHEN n.state IN ('queued', 'retry_wait')
                          AND NOT n.executor = ANY (live.executors)
                     THEN 'waiting_for_compatible_worker'
                END AS queue_reason,
                n.input, n.output, n.error, n.started_at, n.finished_at
         FROM node_runs n, corqty.live_executors() AS live(executors)
         WHERE n.run_id = $1
         ORDER BY n.id",
    )
```

`corqty.live_executors()` is the Task 9 definer function that `corqty_api` may execute. Called in FROM, it runs once
per read, not once per node.

- [ ] **Step 5: Add the run handlers and their routes**

Create `crates/api/src/runs.rs`:

```rust
use axum::{Extension, Json, extract::State, http::StatusCode};
use corqty_domain::{
    diagnostic::Diagnostic,
    rbac::Capability,
    run::plan_run,
    runtime::MAX_RUN_INPUT_BYTES,
    schema::{compile, instance_errors},
};
use corqty_persistence::{
    catalog,
    models::SessionRecord,
    runs::{self, CancelError, NewRun, RunRecord, RunSummaryRecord},
    workflows,
};
use serde::Deserialize;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{
    authz::{OrgAccess, authorize},
    catalog::{Items, blocking},
    error::ApiError,
    extract::{ApiJson, ApiPath},
    state::AppState,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartRun {
    #[serde(default)]
    input: Map<String, Value>,
    version: Option<i32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rerun {
    version: Option<i32>,
}

pub async fn list_runs(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, workflow_id)): ApiPath<(Uuid, Uuid)>,
) -> Result<Json<Items<RunSummaryRecord>>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    if !workflows::workflow_exists(&mut access.tx, workflow_id).await? {
        return Err(ApiError::NotFound);
    }
    let items = runs::list_runs(&mut access.tx, workflow_id).await?;
    access.commit().await?;
    Ok(Json(Items { items }))
}

/// Starts the published version, or the version the request names, with an input that is
/// at most 1 MiB of JSON and matches that version's workflow input schema.
pub async fn start_run(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, workflow_id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(request): ApiJson<StartRun>,
) -> Result<(StatusCode, Json<RunRecord>), ApiError> {
    let mut access = authorize(&state, &session, organization_id, Capability::RunWorkflows).await?;
    let input = Value::Object(request.input);
    if input.to_string().len() > MAX_RUN_INPUT_BYTES {
        return Err(ApiError::Validation("run_input_too_large"));
    }
    let workflow = workflows::get_workflow(&mut access.tx, workflow_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let version = request
        .version
        .or(workflow.published_version)
        .ok_or(ApiError::Conflict("workflow_not_published"))?;
    let run = create(&mut access, workflow_id, version, input, None).await?;
    access.commit().await?;
    Ok((StatusCode::CREATED, Json(run)))
}

pub async fn get_run(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, run_id)): ApiPath<(Uuid, Uuid)>,
) -> Result<Json<RunRecord>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let run = runs::get_run(&mut access.tx, run_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    access.commit().await?;
    Ok(Json(run))
}

/// Starts the run's workflow again with the run's input, on the run's version unless the
/// request names another one, whose schema the input must then match.
pub async fn rerun(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, run_id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(request): ApiJson<Rerun>,
) -> Result<(StatusCode, Json<RunRecord>), ApiError> {
    let mut access = authorize(&state, &session, organization_id, Capability::RunWorkflows).await?;
    let source = runs::get_run(&mut access.tx, run_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let version = request.version.unwrap_or(source.summary.version);
    let run = create(
        &mut access,
        source.summary.workflow_id,
        version,
        source.input,
        Some(run_id),
    )
    .await?;
    access.commit().await?;
    Ok((StatusCode::CREATED, Json(run)))
}

/// Cancels the run's waiting nodes now; running nodes stop when their workers see the
/// request. The cancel has no body. A deadlock or serialization failure rolls the attempt
/// back, and the cancel is tried again up to three times.
pub async fn cancel_run(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, run_id)): ApiPath<(Uuid, Uuid)>,
) -> Result<Json<RunSummaryRecord>, ApiError> {
    let mut retries = 0;
    loop {
        let mut access =
            authorize(&state, &session, organization_id, Capability::RunWorkflows).await?;
        match runs::cancel_run(&mut access.tx, organization_id, access.user_id, run_id).await {
            Ok(run) => {
                access.commit().await?;
                return Ok(Json(run));
            }
            Err(CancelError::NotFound) => return Err(ApiError::NotFound),
            Err(CancelError::Finished) => return Err(ApiError::Conflict("run_finished")),
            Err(CancelError::Database(error)) if retries < 3 && is_transient(&error) => {
                retries += 1;
            }
            Err(CancelError::Database(error)) => return Err(error.into()),
        }
    }
}

/// Creates a run of the workflow's `version` from its plan and reads it back. `input` must
/// match that version's workflow input schema.
async fn create(
    access: &mut OrgAccess,
    workflow_id: Uuid,
    version: i32,
    input: Value,
    rerun_of: Option<Uuid>,
) -> Result<RunRecord, ApiError> {
    let version = workflows::get_version(&mut access.tx, workflow_id, version)
        .await?
        .ok_or(ApiError::NotFound)?;
    let ((version, input), diagnostics) = blocking((version, input), |(version, input)| {
        input_diagnostics(&version.graph.workflow_input_schema, input)
    })
    .await?;
    // Publish compiled this schema, so a failure here is the server's.
    let diagnostics = diagnostics.map_err(ApiError::internal)?;
    if !diagnostics.is_empty() {
        return Err(ApiError::Invalid {
            code: "run_input_invalid",
            diagnostics,
        });
    }
    let definitions = catalog::definitions_map(&mut access.tx).await?;
    let nodes = plan_run(&version.graph, &definitions)
        .ok_or_else(|| ApiError::internal("a published version has no run plan"))?;
    let run = runs::create_run(
        &mut access.tx,
        &NewRun {
            organization_id: access.organization_id,
            actor_user_id: access.user_id,
            workflow_id,
            workflow_version_id: version.summary.id,
            version: version.summary.version,
            input: &input,
            nodes: &nodes,
            rerun_of,
        },
    )
    .await?;
    runs::get_run(&mut access.tx, run.id)
        .await?
        .ok_or_else(|| ApiError::internal("a new run is not visible"))
}

/// Where and how `input` breaks `schema`, one diagnostic per error.
fn input_diagnostics(schema: &Value, input: &Value) -> Result<Vec<Diagnostic>, String> {
    let validator = compile(schema)?;
    Ok(instance_errors(&validator, input)
        .into_iter()
        .map(|(pointer, message)| Diagnostic::new("invalid_input", pointer, message))
        .collect())
}

/// A deadlock or serialization failure: the transaction failed only because of its timing.
fn is_transient(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .is_some_and(|code| code == "40P01" || code == "40001")
}
```

In `crates/api/src/lib.rs`:

Replace:

```rust
pub mod router;
pub mod state;
```

With:

```rust
pub mod router;
pub mod runs;
pub mod state;
```

In `crates/api/src/router.rs`:

Replace:

```rust
    error::ApiError,
    organizations,
    state::AppState,
```

With:

```rust
    error::ApiError,
    organizations, runs,
    state::AppState,
```

Replace:

```rust
            get(workflows::get_version),
        )
```

With:

```rust
            get(workflows::get_version),
        )
        .route(
            "/orgs/{organization_id}/workflows/{workflow_id}/runs",
            get(runs::list_runs).post(runs::start_run),
        )
        .route("/orgs/{organization_id}/runs/{run_id}", get(runs::get_run))
        .route(
            "/orgs/{organization_id}/runs/{run_id}/cancel",
            post(runs::cancel_run),
        )
        .route(
            "/orgs/{organization_id}/runs/{run_id}/rerun",
            post(runs::rerun),
        )
```

- [ ] **Step 6: Verify GREEN**

```text
cargo test -p corqty-api --test runs_http
```

Expected: all 8 tests pass in about 7.5 s. The deadlock test alone takes about 1.7 s, because PostgreSQL detects the
deadlock after `deadlock_timeout` (1 s).

```text
cargo test -p corqty-api --test authorization_matrix
```

Expected: all 5 tests pass.

```text
cargo test -p corqty-persistence --test run_store
```

Expected: all 8 tests pass.

Negative control: with the retry guard changed from `retries < 3` to `retries < 0`,
`a_cancel_that_deadlocks_is_tried_again` fails with `left: 500`, `right: 200`. The deadlock then surfaces as an internal
error. Five repeated runs of the real test all passed, in about 1.7 s each.

- [ ] **Step 7: Run the full backend verification**

```text
cargo fmt --check
```

Expected: no output, exit code 0.

```text
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: `Finished` with no warnings.

```text
cargo test --workspace
```

Expected: every suite passes, with these counts:

- corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 5, `catalog_http` 4, `members_http` 3,
  `organizations_http` 1, `problems_http` 2, `publish_http` 2, `runs_http` 8, `shutdown` 1, `workflows_http` 4.
- corqty-domain: `account_and_rbac` 6, `catalog_rules` 7, `graph_rules` 8, `publish_rules` 10, `run_rules` 8,
  `runtime_rules` 12.
- corqty-executor-mock: `contract` 11.
- corqty-executor-shell: `contract` 0 on Windows and macOS, 15 on Linux.
- corqty-persistence: `auth_store` 2, `catalog_guards` 4, `final_admin` 2, `migrations` 1, `run_store` 8,
  `tenant_isolation` 1, `test_reset` 1, `worker_queue` 11, `workflow_catalog` 6.
- corqty-plugin-host: `registry` 8.

That is 144 tests on Windows and 159 on Linux (CI).

- [ ] **Step 8: Commit**

```text
git add crates/api crates/persistence
git commit -m "feat: start, inspect, cancel, and rerun runs over the API"
```

Expected: `git show --stat HEAD` lists exactly the 8 files above (1074 insertions, 15 deletions).

## Task 11: Let admins enable the trusted shell executor

**Files:**

- Create: `migrations/202610260003_shell_executor.sql`
- Create: `crates/api/src/organizations/settings.rs`
- Modify: `crates/api/src/organizations/mod.rs`
- Modify: `crates/api/src/router.rs`
- Modify: `crates/api/src/runs.rs`
- Modify: `crates/api/src/workflows.rs`
- Modify: `crates/domain/src/catalog.rs`
- Modify: `crates/domain/src/publish.rs`
- Modify: `crates/persistence/src/organizations.rs`
- Modify: `crates/api/tests/authorization_matrix.rs`
- Modify: `crates/api/tests/catalog_http.rs`
- Modify: `crates/domain/tests/catalog_rules.rs`
- Modify: `crates/domain/tests/publish_rules.rs`
- Modify: `crates/persistence/tests/test_reset.rs`
- Modify: `crates/persistence/tests/workflow_catalog.rs`
- Modify: `scripts/smoke-foundation.ps1`
- Test: `crates/api/tests/settings_http.rs`

This task makes the shell executor from Task 7 usable, but only in organizations whose admin opts in (design D15,
spec §6.2 and §10.4). An organization stores the opt-in as `organizations.shell_enabled_at`, which is NULL until an
admin enables shell. Two routes expose it under `/api/v1/orgs/{organization_id}`:

| route | capability | answer |
|---|---|---|
| `GET /settings` | ViewOrganization | 200 `{shellEnabled}` |
| `PUT /settings/shell` `{enabled}` | EnableShellExecutor (admins only) | 200 `{shellEnabled}` |

The PUT is one conditional `UPDATE` in the tenant transaction. It writes `shell_enabled_at = now()` or NULL only when
the value changes. Only then does it audit `organization.shell_enabled` or `organization.shell_disabled`, with
resource type `organization`, the organization id and metadata `{}`. Two concurrent PUTs with the same value
re-check the row after the lock, so only one of them writes an audit event. The existing `organizations_update`
policy already limits row updates to admins.

A shell node is any node whose resolved definition, system or organization, has `executor.key == "shell"`. Keying on
the executor matters: an editor can bind an organization definition to `shell@1.0.0`, because `BUILTIN_EXECUTORS`
now lists `("shell", "1.0.0")`. A check on the key `corqty.shell.command` alone would let that definition bypass the
opt-in. The domain function `publish::shell_diagnostics(graph, definitions)` lists one `executor_disabled` diagnostic
at `/nodes/{i}/definition` for each shell node. It skips unknown definitions and is capped like `validate_graph`.
While an organization has not enabled shell, the API applies it at three points:

- Publish answers 422 `graph_invalid`, with the shell diagnostics after the publish rules' diagnostics. The studio
  already shows `graph_invalid` diagnostics on their nodes.
- `POST /validate` reports the same diagnostics, so Validate never passes for a graph that Publish then refuses.
- Run creation (start and rerun, both through `runs::create`) answers 422 `executor_disabled` with the same
  diagnostics. The check runs after the input validation and before `plan_run`. A version that was published while
  shell was enabled can therefore not start after shell is disabled.

Disabling never touches runs that already exist: their queued shell nodes stay claimable by a worker that runs
`shell@1.0.0`. Run creation reads the setting without a row lock, because a runner may not lock the organization row
(`FOR SHARE` needs the admin-only update policy). A run that starts while an admin disables shell therefore counts as
started just before the change.

Blueprint creation keeps using `validate_graph` alone. A blueprint is a template, and publish and run creation still
check every workflow made from it.

Migration `202610260003` adds the column and replaces `corqty.seed_system_catalog()` with `CREATE OR REPLACE`. The
function body is the one from `202610250001`: all six mock definitions and both blueprints are unchanged, and only
one comment is reworded. It also inserts the preset `corqty.shell.command` v1 with id
`00000000-0000-7000-8000-000000000007`, display name `Shell command`, category `Shell (trusted)`, and the D15
description, ports, config schema, uiSchema, defaults and executor. The migration ends with
`SELECT corqty.seed_system_catalog();`, which seeds existing databases. `TestDatabase::reset()` calls the replaced
function, so every test database gets 7 definitions. `CREATE OR REPLACE` keeps the function's ACL, so PUBLIC still
has no EXECUTE.

Tests that pin the catalog change with it. The reset test and the smoke script now count 7 system definitions. The
catalog lists in `workflow_catalog.rs` and `catalog_http.rs` gain `corqty.shell.command`. The seed test checks the
default outputs of mock definitions only, since the shell defaults have no `output`. It pins the preset exactly. It
also requires both seeding migrations to end with the `SELECT`. The domain unknown-executor case now uses the key
`python`, and a definition bound to `shell` is well formed. The unknown-executor message names both executors.

`settings_http.rs` covers these cases:

- GET and PUT, idempotent repeats with no extra audit, the exact audit rows, and 422 `invalid_json` bodies.
- Publish and validate while disabled, for both `corqty.shell.command` and an organization definition bound to
  `shell@1.0.0`. The diagnostics are at `/nodes/1/definition` and `/nodes/2/definition`, after a mock node at index 0.
- Publish and a run start once enabled.
- 422 `executor_disabled` for a start while disabled, and for a rerun after disabling.
- A run created while enabled reads the same after disabling, and a shell-only worker still claims both of its
  shell nodes.

The capability matrix gains `settings_routes_enforce_capabilities`: GET is 200 for every role, PUT is 403 for the
editor, the runner and the viewer, and an outsider gets 404 for both.

- [ ] **Step 1: Write the failing tests**

Create `crates/api/tests/settings_http.rs`:

```rust
mod support;

use axum::{
    body::to_bytes,
    http::{Method, StatusCode},
    response::Response,
};
use corqty_persistence::worker_queue::WorkerDb;
use serde_json::{Value, json};
use serial_test::serial;
use support::AuthFixture;
use uuid::Uuid;

const SHELL_DISABLED: &str =
    "An admin must enable the shell executor before this node can be published or run.";

/// Asserts the status and returns the JSON body.
async fn body(response: Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// Registers `<role>@example.com` and adds it to the admin's organization with that role.
async fn member_of(admin: &AuthFixture, role: &str) -> AuthFixture {
    let invitations = format!("/api/v1/orgs/{}/invitations", admin.organization_id);
    let email = format!("{role}@example.com");
    let request = json!({ "email": email, "role": role }).to_string();
    let invitation = body(
        admin.post_json(&invitations, &request).await,
        StatusCode::CREATED,
    )
    .await;
    let member = admin.register_another(&email, "Personal").await;
    let accept = json!({ "token": invitation["token"] }).to_string();
    let accepted = member
        .post_json("/api/v1/invitations/accept", &accept)
        .await;
    assert_eq!(accepted.status(), StatusCode::OK);
    member
}

/// Turns the shell executor of the admin's organization on or off, and returns the answer.
async fn set_shell(admin: &AuthFixture, enabled: bool) -> Value {
    let path = format!("/api/v1/orgs/{}/settings/shell", admin.organization_id);
    let request = json!({ "enabled": enabled }).to_string();
    let response = admin.request_json(Method::PUT, &path, &request).await;
    body(response, StatusCode::OK).await
}

/// The audit events about the admin's organization itself, newest first, as
/// [actorUserId, action, resourceId, metadata].
async fn organization_audit(admin: &AuthFixture) -> Vec<Value> {
    let path = format!("/api/v1/orgs/{}/audit-events", admin.organization_id);
    let events = body(admin.get(&path).await, StatusCode::OK).await;
    events["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["resourceType"] == "organization")
        .map(|event| {
            json!([
                event["actorUserId"],
                event["action"],
                event["resourceId"],
                event["metadata"],
            ])
        })
        .collect()
}

/// A node with no inputs, one attempt and a 60 s timeout.
fn node(id: &str, key: &str, config: Value) -> Value {
    json!({
        "id": id,
        "definition": { "key": key, "version": 1 },
        "displayName": id,
        "position": { "x": 0, "y": 0 },
        "config": config,
        "inputs": {},
        "retry": { "maxAttempts": 1 },
        "timeoutSeconds": 60,
    })
}

#[tokio::test]
#[serial]
async fn admins_turn_the_shell_executor_on_and_off() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let viewer = member_of(&admin, "viewer").await;
    let settings = format!("/api/v1/orgs/{}/settings", admin.organization_id);
    let (enabled, disabled) = (
        json!({ "shellEnabled": true }),
        json!({ "shellEnabled": false }),
    );
    assert_eq!(
        body(viewer.get(&settings).await, StatusCode::OK).await,
        disabled
    );

    assert_eq!(set_shell(&admin, true).await, enabled);
    // Setting the value the organization already has changes nothing and audits nothing.
    assert_eq!(set_shell(&admin, true).await, enabled);
    assert_eq!(
        body(viewer.get(&settings).await, StatusCode::OK).await,
        enabled
    );
    assert_eq!(set_shell(&admin, false).await, disabled);
    assert_eq!(set_shell(&admin, false).await, disabled);
    assert_eq!(
        body(viewer.get(&settings).await, StatusCode::OK).await,
        disabled
    );

    let shell = format!("{settings}/shell");
    for request in ["{}", r#"{"enabled":"yes"}"#] {
        let response = admin.request_json(Method::PUT, &shell, request).await;
        let problem = body(response, StatusCode::UNPROCESSABLE_ENTITY).await;
        assert_eq!(problem["code"], "invalid_json", "{request}");
    }

    let event = |action: &str, metadata: Value| {
        json!([admin.user_id, action, admin.organization_id, metadata])
    };
    assert_eq!(
        organization_audit(&admin).await,
        [
            event("organization.shell_disabled", json!({})),
            event("organization.shell_enabled", json!({})),
            event("organization.created", json!({ "name": "Acme" })),
        ]
    );
}

#[tokio::test]
#[serial]
async fn shell_nodes_publish_and_start_only_while_shell_is_enabled() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let editor = member_of(&admin, "editor").await;
    let runner = member_of(&admin, "runner").await;
    let org = format!("/api/v1/orgs/{}", admin.organization_id);

    // An organization definition bound to shell counts exactly like the system preset.
    let script = json!({
        "key": "acme.script",
        "displayName": "Script",
        "category": "Scripts",
        "inputs": [],
        "outputs": [{ "name": "result", "dataType": "core.any", "schema": {} }],
        "configSchema": { "type": "object" },
        "uiSchema": {},
        "defaults": {},
        "executor": { "key": "shell", "version": "1.0.0" },
    });
    let definitions = format!("{org}/node-definitions");
    let created = editor.post_json(&definitions, &script.to_string()).await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let mock = json!({
        "mode": "fixed", "output": { "result": 1 }, "delayMs": 0, "failureMessage": null,
    });
    let graph = json!({
        "schemaVersion": 1,
        "workflowInputSchema": { "type": "object" },
        "nodes": [
            node("mock", "corqty.mock.task", mock),
            node("preset", "corqty.shell.command", json!({ "command": "true", "env": {} })),
            node("script", "acme.script", json!({})),
        ],
        "edges": [],
        "viewport": { "x": 0, "y": 0, "zoom": 1 },
    });
    let workflow = body(
        editor
            .post_json(&format!("{org}/workflows"), r#"{"name":"Scripts"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let workflow = format!("{org}/workflows/{}", workflow["id"].as_str().unwrap());
    let draft = json!({ "revision": 1, "graph": graph }).to_string();
    let saved = editor
        .request_json(Method::PUT, &format!("{workflow}/draft"), &draft)
        .await;
    assert_eq!(saved.status(), StatusCode::OK);

    let shell_nodes = json!([
        { "code": "executor_disabled", "pointer": "/nodes/1/definition", "message": SHELL_DISABLED },
        { "code": "executor_disabled", "pointer": "/nodes/2/definition", "message": SHELL_DISABLED },
    ]);
    let validate = format!("{workflow}/validate");
    let request = json!({ "graph": graph }).to_string();
    assert_eq!(
        body(editor.post_json(&validate, &request).await, StatusCode::OK).await,
        json!({ "valid": false, "diagnostics": shell_nodes }),
    );
    let publish = format!("{workflow}/publish");
    let refused = body(
        editor.post_json(&publish, r#"{"revision":2}"#).await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(
        (&refused["code"], &refused["diagnostics"]),
        (&json!("graph_invalid"), &shell_nodes)
    );

    set_shell(&admin, true).await;
    assert_eq!(
        body(editor.post_json(&validate, &request).await, StatusCode::OK).await,
        json!({ "valid": true, "diagnostics": [] }),
    );
    let published = editor.post_json(&publish, r#"{"revision":2}"#).await;
    assert_eq!(published.status(), StatusCode::CREATED);

    // A published version still needs shell enabled whenever a run of it starts.
    set_shell(&admin, false).await;
    let runs = format!("{workflow}/runs");
    let refused = body(
        runner.post_json(&runs, "{}").await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(
        (&refused["code"], &refused["diagnostics"]),
        (&json!("executor_disabled"), &shell_nodes)
    );
    set_shell(&admin, true).await;
    let run = body(runner.post_json(&runs, "{}").await, StatusCode::CREATED).await;
    let run_id: Uuid = run["id"].as_str().unwrap().parse().unwrap();
    let path = format!("{org}/runs/{run_id}");

    // Disabling stops new runs and reruns, but the run that exists keeps its shell nodes.
    set_shell(&admin, false).await;
    let rerun = body(
        runner.post_json(&format!("{path}/rerun"), "{}").await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(
        (&rerun["code"], &rerun["diagnostics"]),
        (&json!("executor_disabled"), &shell_nodes)
    );
    assert_eq!(body(runner.get(&path).await, StatusCode::OK).await, run);
    let worker = WorkerDb::new(admin.worker.clone(), Uuid::now_v7());
    worker
        .register(&["shell@1.0.0".to_owned()], 4)
        .await
        .unwrap();
    let claimed: Vec<_> = worker
        .claim(10, 30)
        .await
        .unwrap()
        .into_iter()
        .map(|job| (job.run_id, job.executor))
        .collect();
    let shell_job = (run_id, "shell@1.0.0".to_owned());
    assert_eq!(claimed, [shell_job.clone(), shell_job]);
}
```

In `crates/api/tests/authorization_matrix.rs`:

Replace:

```rust
#[tokio::test]
#[serial]
async fn org_access_commit_reports_the_deferred_final_admin_check_as_conflict() {
```

With:

```rust
#[tokio::test]
#[serial]
async fn settings_routes_enforce_capabilities() {
    let matrix = Matrix::new().await;
    let settings = format!("/api/v1/orgs/{}/settings", matrix.organization_id);
    matrix
        .assert_row(Method::GET, &settings, "", Capability::ViewOrganization)
        .await;
    matrix
        .assert_row(
            Method::PUT,
            &format!("{settings}/shell"),
            r#"{"enabled":true}"#,
            Capability::EnableShellExecutor,
        )
        .await;
}

#[tokio::test]
#[serial]
async fn org_access_commit_reports_the_deferred_final_admin_check_as_conflict() {
```

`assert_row` sends the PUT only for roles without `EnableShellExecutor`, so shell stays disabled.

In `crates/api/tests/catalog_http.rs`:

Replace:

```rust
            json!(["system", "corqty.mock.task", 1, false]),
```

With:

```rust
            json!(["system", "corqty.mock.task", 1, false]),
            json!(["system", "corqty.shell.command", 1, false]),
```

In `crates/domain/tests/catalog_rules.rs`:

Replace:

```rust
        ("/outputs/0/schema", schema_of(MAX_SCHEMA_BYTES)),
    ] {
```

With:

```rust
        ("/outputs/0/schema", schema_of(MAX_SCHEMA_BYTES)),
        ("/executor/key", json!("shell")),
    ] {
```

Replace:

```rust
            "/executor/key",
            json!("shell"),
            "unknown_executor",
```

With:

```rust
            "/executor/key",
            json!("python"),
            "unknown_executor",
```

In `crates/domain/tests/publish_rules.rs`:

Replace:

```rust
    publish::{Definitions, canonical_checksum, validate_graph},
```

With:

```rust
    publish::{Definitions, canonical_checksum, shell_diagnostics, validate_graph},
```

Replace:

```rust
#[test]
fn a_graph_over_the_limits_gets_only_the_limit_diagnostics() {
```

With:

```rust
#[test]
fn nodes_that_run_on_the_shell_executor_are_reported_on_their_definitions() {
    let mut definitions = definitions();
    assert!(shell_diagnostics(&with(&[]), &definitions).is_empty());

    let database = definitions.get_mut(&("acme.db".to_owned(), 1)).unwrap();
    database.executor.key = "shell".to_owned();
    // Shell is built in, so the publish rules alone accept the graph.
    assert!(validate_graph(&with(&[]), &definitions).is_empty());
    // A node whose definition is unknown runs on nothing.
    let graph = with(&[
        ("/nodes/2", node("ghost")),
        ("/nodes/2/definition/key", json!("acme.ghost")),
    ]);
    assert_eq!(
        shell_diagnostics(&graph, &definitions),
        [Diagnostic::new(
            "executor_disabled",
            "/nodes/1/definition",
            "An admin must enable the shell executor before this node can be published or run.",
        )]
    );
}

#[test]
fn a_graph_over_the_limits_gets_only_the_limit_diagnostics() {
```

Replace the whole of `crates/persistence/tests/test_reset.rs`:

```rust
use corqty_test_support::TestDatabase;
use serial_test::serial;

/// Rows that `TestDatabase::reset()` puts back after truncating a table.
fn seeded_rows(table: &str) -> i64 {
    match table {
        "node_definitions" => 7,
        "blueprints" => 2,
        _ => 0,
    }
}

#[tokio::test]
#[serial]
async fn reset_truncates_every_table_in_public() {
    let database = TestDatabase::reset().await;
    // reset_probe stands in for a table that a later migration adds: reset()
    // has to find it in the catalog rather than in a list of known tables.
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS public.reset_probe (id integer);
         INSERT INTO reset_probe VALUES (1);
         INSERT INTO users(id, email)
         VALUES (gen_random_uuid(), 'admin@example.com');
         INSERT INTO organizations(id, name, slug, created_by)
         SELECT gen_random_uuid(), 'Acme', 'acme', id FROM users;
         INSERT INTO memberships(organization_id, user_id, role)
         SELECT organizations.id, users.id, 'admin' FROM organizations, users;
         INSERT INTO invitations(
             id, organization_id, email, role, token_hash, invited_by, expires_at
         )
         SELECT gen_random_uuid(), organizations.id, 'viewer@example.com',
                'viewer', 'token'::bytea, users.id, now() + interval '1 day'
         FROM organizations, users;
         INSERT INTO audit_events(
             organization_id, actor_user_id, action, resource_type, resource_id
         )
         SELECT organizations.id, users.id, 'invitation.created', 'invitation',
                'viewer@example.com'
         FROM organizations, users;",
    )
    .execute(&database.owner)
    .await
    .unwrap();

    let database = TestDatabase::reset().await;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT tablename::text FROM pg_tables
         WHERE schemaname = 'public' AND tablename <> '_sqlx_migrations'
         ORDER BY tablename",
    )
    .fetch_all(&database.owner)
    .await
    .unwrap();
    let mut rows = Vec::new();
    for table in &tables {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&database.owner)
            .await
            .unwrap();
        rows.push((table.as_str(), count));
    }
    sqlx::query("DROP TABLE reset_probe")
        .execute(&database.owner)
        .await
        .unwrap();

    let expected: Vec<_> = tables
        .iter()
        .map(|table| (table.as_str(), seeded_rows(table)))
        .collect();
    assert!(tables.iter().any(|table| table == "reset_probe"));
    assert_eq!(rows, expected);
}
```

In `crates/persistence/tests/workflow_catalog.rs`:

Replace:

```rust
    catalog::{NewNodeDefinition, validate_new_definition},
```

With:

```rust
    catalog::{NewNodeDefinition, NodeDefinitionSpec, validate_new_definition},
```

Replace:

```rust
    assert_eq!(definitions.len(), 8);
```

With:

```rust
    assert_eq!(definitions.len(), 9);
```

Replace:

```rust
    assert_eq!(catalog::definitions_map(&mut tx).await.unwrap().len(), 8);
```

With:

```rust
    assert_eq!(catalog::definitions_map(&mut tx).await.unwrap().len(), 9);
```

Replace:

```rust
    assert_eq!(definitions.len(), 6);
    assert_eq!(catalog::definitions_map(&mut tx).await.unwrap().len(), 6);
```

With:

```rust
    assert_eq!(definitions.len(), 7);
    assert_eq!(catalog::definitions_map(&mut tx).await.unwrap().len(), 7);
```

Replace:

```rust
    let migration = include_str!("../../../migrations/202610250001_workflow_catalog.sql");
    assert!(
        migration
            .trim_end()
            .ends_with("SELECT corqty.seed_system_catalog();")
    );
```

With:

```rust
    // Each migration that replaces the seed function also runs it.
    for migration in [
        include_str!("../../../migrations/202610250001_workflow_catalog.sql"),
        include_str!("../../../migrations/202610260003_shell_executor.sql"),
    ] {
        assert!(
            migration
                .trim_end()
                .ends_with("SELECT corqty.seed_system_catalog();")
        );
    }
```

Replace:

```rust
    assert_eq!(counts, (6, 2));
```

With:

```rust
    assert_eq!(counts, (7, 2));
```

Replace:

```rust
            "corqty.mock.task",
            "corqty.simulated.create-rds",
            "corqty.simulated.create-vpc",
            "corqty.simulated.deploy-service",
        ]
    );
```

With:

```rust
            "corqty.mock.task",
            "corqty.shell.command",
            "corqty.simulated.create-rds",
            "corqty.simulated.create-vpc",
            "corqty.simulated.deploy-service",
        ]
    );
    let shell = &definitions[3];
    let preset: NodeDefinitionSpec = serde_json::from_value(json!({
        "inputs": [{"name": "input", "dataType": "core.any", "schema": {}}],
        "outputs": [{"name": "result", "dataType": "core.any", "schema": {}}],
        "configSchema": {
            "type": "object",
            "properties": {
                "command": {"type": "string", "minLength": 1, "maxLength": 16384, "title": "Command"},
                "env": {
                    "type": "object",
                    "additionalProperties": {"type": "string"},
                    "title": "Environment"
                }
            },
            "required": ["command"],
            "additionalProperties": false
        },
        "uiSchema": {"command": {"widget": "textarea"}, "env": {"widget": "keyValue"}},
        "defaults": {"command": "jq '{result: .}' \"$CORQTY_INPUT\" > \"$CORQTY_OUTPUT\"", "env": {}},
        "executor": {"key": "shell", "version": "1.0.0"}
    }))
    .unwrap();
    assert_eq!(
        (
            shell.id.to_string(),
            shell.display_name.as_str(),
            shell.description.as_str(),
            shell.category.as_str(),
            &shell.spec,
        ),
        (
            "00000000-0000-7000-8000-000000000007".to_owned(),
            "Shell command",
            "Trusted: runs a shell command inside the worker container. Not a security boundary.",
            "Shell (trusted)",
            &preset,
        )
    );
```

Replace:

```rust
        assert_eq!(codes, [("reserved_key", "/key")], "{}", definition.key);
        for port in &definition.spec.outputs {
```

With:

```rust
        assert_eq!(codes, [("reserved_key", "/key")], "{}", definition.key);
        // A mock definition's fixed output fills each of its output ports.
        if definition.spec.executor.key != "mock" {
            continue;
        }
        for port in &definition.spec.outputs {
```

In `scripts/smoke-foundation.ps1`:

Replace:

```powershell
    @($definitions.Body.items | Where-Object { $_.scope -eq "system" }).Count -ne 6 -or
    @($blueprints.Body.items | Where-Object { $_.scope -eq "system" }).Count -ne 2
) {
    throw "System catalog does not list 6 node definitions and 2 blueprints"
```

With:

```powershell
    @($definitions.Body.items | Where-Object { $_.scope -eq "system" }).Count -ne 7 -or
    @($blueprints.Body.items | Where-Object { $_.scope -eq "system" }).Count -ne 2
) {
    throw "System catalog does not list 7 node definitions and 2 blueprints"
```

- [ ] **Step 2: Run the tests and verify RED**

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-domain --test catalog_rules
```

Expected: 6 pass, and `a_well_formed_definition_has_no_diagnostics` fails because shell is not a built-in executor
yet:

```text
thread 'a_well_formed_definition_has_no_diagnostics' panicked at crates\domain\tests\catalog_rules.rs:104:9:
/executor/key: [Diagnostic { code: "unknown_executor", pointer: "/executor", message: "The executor must be mock 1.0.0." }]
test result: FAILED. 6 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
```

```text
cargo test -p corqty-domain --test publish_rules
```

Expected: the test does not compile:

```text
error[E0432]: unresolved import `corqty_domain::publish::shell_diagnostics`
 --> crates\domain\tests\publish_rules.rs:5:48
```

```text
cargo test -p corqty-persistence --test test_reset
```

Expected: `reset_truncates_every_table_in_public` fails at `test_reset.rs:72:5`: the reset seeds
`("node_definitions", 6)`, and the test expects 7.

```text
cargo test -p corqty-persistence --test workflow_catalog
```

Expected: the test does not compile, because the migration does not exist yet:

```text
error: couldn't read `crates\persistence\tests\../../../migrations/202610260003_shell_executor.sql`: The system cannot find the file specified. (os error 2)
   --> crates\persistence\tests\workflow_catalog.rs:504:9
```

```text
cargo test -p corqty-api --test settings_http
```

Expected: both tests fail. The settings routes do not exist, and an organization definition cannot use shell yet:

```text
thread 'admins_turn_the_shell_executor_on_and_off' panicked at crates\api\tests\settings_http.rs:19:5:
  left: 404
 right: 200
thread 'shell_nodes_publish_and_start_only_while_shell_is_enabled' panicked at crates\api\tests\settings_http.rs:157:5:
  left: 422
 right: 201
test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
```

```text
cargo test -p corqty-api --test catalog_http
```

Expected: 3 pass. `editors_add_and_deprecate_node_definition_versions` fails at `catalog_http.rs:213:5`, because the
listed system definitions lack `corqty.shell.command`.

```text
cargo test -p corqty-api --test authorization_matrix settings
```

Expected: `settings_routes_enforce_capabilities` fails at `authorization_matrix.rs:74:13` with
`admin GET /api/v1/orgs/<id>/settings`, `left: 404`, `right: 200`.

- [ ] **Step 3: Add the shell setting and the shell preset**

Create `migrations/202610260003_shell_executor.sql`:

```sql
-- Organizations opt into the trusted shell executor. While shell_enabled_at is NULL, publish
-- and run creation refuse every node whose definition runs on shell. Disabling it never
-- touches runs that already exist.
ALTER TABLE organizations ADD COLUMN shell_enabled_at timestamptz;

-- The system catalog gains the trusted shell preset. The six mock definitions and both
-- blueprints are the ones 202610250001 seeds, unchanged.
CREATE OR REPLACE FUNCTION corqty.seed_system_catalog()
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
    -- Every mock definition runs on the mock executor with this config form.
    mock_config_schema jsonb := '{
        "type": "object",
        "properties": {
            "mode": {"type": "string", "enum": ["fixed", "echo"], "title": "Mode"},
            "output": {"type": "object", "title": "Fixed output"},
            "delayMs": {"type": "integer", "minimum": 0, "maximum": 60000, "title": "Delay (ms)"},
            "failureMessage": {"type": ["string", "null"], "maxLength": 500, "title": "Fail with message"}
        },
        "required": ["mode", "output", "delayMs"],
        "additionalProperties": false
    }';
    mock_ui_schema jsonb := '{
        "mode": {"widget": "select"},
        "output": {"widget": "code"},
        "delayMs": {"widget": "number"},
        "failureMessage": {"widget": "text"}
    }';
BEGIN
    INSERT INTO node_definitions(
        id, scope, key, version, display_name, description, category, spec
    )
    SELECT seed.id::uuid, 'system', seed.key, 1, seed.display_name, seed.description,
           seed.category,
           jsonb_build_object(
               'inputs', seed.inputs::jsonb,
               'outputs', seed.outputs::jsonb,
               'configSchema', mock_config_schema,
               'uiSchema', mock_ui_schema,
               'defaults', jsonb_build_object(
                   'mode', 'fixed',
                   'output', seed.default_output::jsonb,
                   'delayMs', seed.delay_ms,
                   'failureMessage', NULL
               ),
               'executor', '{"key": "mock", "version": "1.0.0"}'::jsonb
           )
    FROM (VALUES
        ('00000000-0000-7000-8000-000000000001', 'corqty.mock.task', 'Mock task',
         'Returns a fixed JSON result or echoes its input.', 'Testing',
         '[{"name": "input", "dataType": "core.any", "schema": {}}]',
         '[{"name": "result", "dataType": "core.any", "schema": {}}]',
         '{"result": {"ok": true}}', 0),
        ('00000000-0000-7000-8000-000000000002', 'corqty.simulated.create-vpc',
         'Create VPC (simulated)', 'Simulated: pretends to create a VPC and returns its id.',
         'Infrastructure (simulated)',
         '[{"name": "region", "dataType": "core.string", "schema": {"type": "string", "minLength": 1}, "required": true}]',
         '[{"name": "vpcId", "dataType": "aws.vpc-id", "schema": {"type": "string", "pattern": "^vpc-"}}]',
         '{"vpcId": "vpc-0simulated"}', 500),
        ('00000000-0000-7000-8000-000000000003', 'corqty.simulated.create-rds',
         'Create RDS (simulated)', 'Simulated: pretends to create a database inside a VPC.',
         'Infrastructure (simulated)',
         '[{"name": "vpcId", "dataType": "aws.vpc-id", "schema": {"type": "string", "pattern": "^vpc-"}, "required": true}]',
         '[{"name": "endpoint", "dataType": "core.string", "schema": {"type": "string"}}]',
         '{"endpoint": "db.simulated.internal:5432"}', 500),
        ('00000000-0000-7000-8000-000000000004', 'corqty.simulated.deploy-service',
         'Deploy service (simulated)', 'Simulated: pretends to deploy a service into a VPC.',
         'Infrastructure (simulated)',
         '[{"name": "vpcId", "dataType": "aws.vpc-id", "schema": {"type": "string", "pattern": "^vpc-"}, "required": true}]',
         '[{"name": "url", "dataType": "core.string", "schema": {"type": "string"}}]',
         '{"url": "https://service.simulated.internal"}', 500),
        ('00000000-0000-7000-8000-000000000005', 'corqty.mock.normalize-payload',
         'Normalize payload', 'Normalizes a JSON payload.', 'Data processing',
         '[{"name": "payload", "dataType": "core.object", "schema": {"type": "object"}, "required": true}]',
         '[{"name": "normalized", "dataType": "core.object", "schema": {"type": "object"}}]',
         '{"normalized": {"records": 3}}', 500),
        ('00000000-0000-7000-8000-000000000006', 'corqty.mock.produce-summary',
         'Produce summary', 'Produces a text summary of normalized data.', 'Data processing',
         '[{"name": "normalized", "dataType": "core.object", "schema": {"type": "object"}, "required": true}]',
         '[{"name": "summary", "dataType": "core.string", "schema": {"type": "string"}}]',
         '{"summary": "3 records processed"}', 500)
    ) AS seed(id, key, display_name, description, category, inputs, outputs, default_output, delay_ms)
    ON CONFLICT (id) DO NOTHING;

    INSERT INTO blueprints(id, scope, key, version, display_name, description, graph)
    VALUES
        ('00000000-0000-7000-8000-000000000101', 'system', 'corqty.simulated-infrastructure', 1,
         'Simulated infrastructure',
         'Simulated: every step uses the mock executor. Creates a VPC, then a database and a service in parallel.',
         '{
             "schemaVersion": 1,
             "workflowInputSchema": {
                 "type": "object",
                 "properties": {"region": {"type": "string", "minLength": 1, "default": "us-east-1"}},
                 "required": ["region"],
                 "additionalProperties": false
             },
             "nodes": [
                 {
                     "id": "vpc",
                     "definition": {"key": "corqty.simulated.create-vpc", "version": 1},
                     "displayName": "Create VPC (simulated)",
                     "position": {"x": 0, "y": 0},
                     "config": {
                         "mode": "fixed", "output": {"vpcId": "vpc-0simulated"},
                         "delayMs": 500, "failureMessage": null
                     },
                     "inputs": {"region": {"kind": "workflowInput", "pointer": "/region"}},
                     "retry": {"maxAttempts": 1},
                     "timeoutSeconds": 900
                 },
                 {
                     "id": "rds",
                     "definition": {"key": "corqty.simulated.create-rds", "version": 1},
                     "displayName": "Create RDS (simulated)",
                     "position": {"x": 300, "y": -100},
                     "config": {
                         "mode": "fixed", "output": {"endpoint": "db.simulated.internal:5432"},
                         "delayMs": 500, "failureMessage": null
                     },
                     "inputs": {},
                     "retry": {"maxAttempts": 1},
                     "timeoutSeconds": 900
                 },
                 {
                     "id": "deploy",
                     "definition": {"key": "corqty.simulated.deploy-service", "version": 1},
                     "displayName": "Deploy service (simulated)",
                     "position": {"x": 300, "y": 100},
                     "config": {
                         "mode": "fixed", "output": {"url": "https://service.simulated.internal"},
                         "delayMs": 500, "failureMessage": null
                     },
                     "inputs": {},
                     "retry": {"maxAttempts": 1},
                     "timeoutSeconds": 900
                 }
             ],
             "edges": [
                 {
                     "id": "vpc-rds",
                     "source": {"nodeId": "vpc", "port": "vpcId"},
                     "target": {"nodeId": "rds", "port": "vpcId"}
                 },
                 {
                     "id": "vpc-deploy",
                     "source": {"nodeId": "vpc", "port": "vpcId"},
                     "target": {"nodeId": "deploy", "port": "vpcId"}
                 }
             ],
             "viewport": {"x": 0, "y": 0, "zoom": 1}
         }'),
        ('00000000-0000-7000-8000-000000000102', 'system', 'corqty.data-processing', 1,
         'Data processing',
         'Normalizes a JSON payload and produces a summary with the mock executor.',
         '{
             "schemaVersion": 1,
             "workflowInputSchema": {
                 "type": "object",
                 "properties": {"payload": {"type": "object"}},
                 "required": ["payload"],
                 "additionalProperties": false
             },
             "nodes": [
                 {
                     "id": "normalize",
                     "definition": {"key": "corqty.mock.normalize-payload", "version": 1},
                     "displayName": "Normalize payload",
                     "position": {"x": 0, "y": 0},
                     "config": {
                         "mode": "fixed", "output": {"normalized": {"records": 3}},
                         "delayMs": 500, "failureMessage": null
                     },
                     "inputs": {"payload": {"kind": "workflowInput", "pointer": "/payload"}},
                     "retry": {"maxAttempts": 1},
                     "timeoutSeconds": 900
                 },
                 {
                     "id": "summary",
                     "definition": {"key": "corqty.mock.produce-summary", "version": 1},
                     "displayName": "Produce summary",
                     "position": {"x": 300, "y": 0},
                     "config": {
                         "mode": "fixed", "output": {"summary": "3 records processed"},
                         "delayMs": 500, "failureMessage": null
                     },
                     "inputs": {},
                     "retry": {"maxAttempts": 1},
                     "timeoutSeconds": 900
                 }
             ],
             "edges": [
                 {
                     "id": "normalize-summary",
                     "source": {"nodeId": "normalize", "port": "normalized"},
                     "target": {"nodeId": "summary", "port": "normalized"}
                 }
             ],
             "viewport": {"x": 0, "y": 0, "zoom": 1}
         }')
    ON CONFLICT (id) DO NOTHING;

    INSERT INTO node_definitions(
        id, scope, key, version, display_name, description, category, spec
    )
    VALUES (
        '00000000-0000-7000-8000-000000000007', 'system', 'corqty.shell.command', 1,
        'Shell command',
        'Trusted: runs a shell command inside the worker container. Not a security boundary.',
        'Shell (trusted)',
        '{
            "inputs": [{"name": "input", "dataType": "core.any", "schema": {}}],
            "outputs": [{"name": "result", "dataType": "core.any", "schema": {}}],
            "configSchema": {
                "type": "object",
                "properties": {
                    "command": {"type": "string", "minLength": 1, "maxLength": 16384, "title": "Command"},
                    "env": {"type": "object", "additionalProperties": {"type": "string"}, "title": "Environment"}
                },
                "required": ["command"],
                "additionalProperties": false
            },
            "uiSchema": {"command": {"widget": "textarea"}, "env": {"widget": "keyValue"}},
            "defaults": {
                "command": "jq ''{result: .}'' \"$CORQTY_INPUT\" > \"$CORQTY_OUTPUT\"",
                "env": {}
            },
            "executor": {"key": "shell", "version": "1.0.0"}
        }'
    )
    ON CONFLICT (id) DO NOTHING;
END;
$$;

SELECT corqty.seed_system_catalog();
```

- [ ] **Step 4: Accept the shell executor and list shell nodes in the domain**

In `crates/domain/src/catalog.rs`:

Replace:

```rust
// ponytail: a static compiled registry; Milestone 3 adds executors that workers advertise.
pub const BUILTIN_EXECUTORS: &[(&str, &str)] = &[("mock", "1.0.0")];
```

With:

```rust
// ponytail: the executors compiled into every worker; Milestone 4 adds installed plugins.
pub const BUILTIN_EXECUTORS: &[(&str, &str)] = &[("mock", "1.0.0"), ("shell", "1.0.0")];
```

Replace:

```rust
            "The executor must be mock 1.0.0.",
```

With:

```rust
            "The executor must be mock 1.0.0 or shell 1.0.0.",
```

In `crates/domain/src/publish.rs`:

Replace:

```rust
/// The lowercase hex SHA-256 of the graph as JSON with sorted object keys. The same graph
```

With:

```rust
/// An `executor_disabled` diagnostic for each node whose definition runs on the trusted shell
/// executor, which an organization's admin must enable before such a node is published or run.
/// Nodes with an unknown definition are skipped. Capped at `MAX_DIAGNOSTICS`.
#[must_use]
pub fn shell_diagnostics(graph: &GraphSnapshot, definitions: &Definitions) -> Vec<Diagnostic> {
    let diagnostics = graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| {
            let reference = (node.definition.key.clone(), node.definition.version);
            definitions
                .get(&reference)
                .is_some_and(|spec| spec.executor.key == "shell")
        })
        .map(|(index, _)| {
            Diagnostic::new(
                "executor_disabled",
                format!("/nodes/{index}/definition"),
                "An admin must enable the shell executor before this node can be published or run.",
            )
        })
        .collect();
    capped(diagnostics)
}

/// The lowercase hex SHA-256 of the graph as JSON with sorted object keys. The same graph
```

- [ ] **Step 5: Read and change the setting in the repository**

Replace the whole of `crates/persistence/src/organizations.rs`:

```rust
use corqty_domain::rbac::Role;
use serde_json::json;
use sqlx::{Error, PgConnection};
use uuid::Uuid;

use crate::{models::OrganizationMembershipRecord, store::PgStore, tenant::begin_tenant};

impl PgStore {
    pub async fn create_organization(
        &self,
        user_id: Uuid,
        organization_id: Uuid,
        name: &str,
        slug: &str,
    ) -> Result<OrganizationMembershipRecord, Error> {
        let mut tx = begin_tenant(&self.pool, user_id, organization_id).await?;
        sqlx::query(
            "INSERT INTO organizations(id, name, slug, created_by)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(organization_id)
        .bind(name)
        .bind(slug)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO memberships(organization_id, user_id, role)
             VALUES ($1, $2, 'admin')",
        )
        .bind(organization_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
        crate::audit::insert_in_transaction(
            &mut tx,
            organization_id,
            user_id,
            "organization.created",
            "organization",
            &organization_id.to_string(),
            serde_json::json!({ "name": name }),
        )
        .await?;
        tx.commit().await?;
        Ok(OrganizationMembershipRecord {
            organization_id,
            name: name.to_owned(),
            slug: slug.to_owned(),
            role: "admin".to_owned(),
        })
    }
}

pub async fn member_role(
    conn: &mut PgConnection,
    organization_id: Uuid,
    user_id: Uuid,
) -> Result<Option<Role>, Error> {
    let role: Option<String> = sqlx::query_scalar(
        "SELECT role
         FROM memberships
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(user_id)
    .fetch_optional(conn)
    .await?;
    role.map(|role| role.parse().map_err(|error| Error::Decode(Box::new(error))))
        .transpose()
}

/// Whether the organization's admins have enabled the trusted shell executor.
pub async fn shell_enabled(conn: &mut PgConnection, organization_id: Uuid) -> Result<bool, Error> {
    sqlx::query_scalar("SELECT shell_enabled_at IS NOT NULL FROM organizations WHERE id = $1")
        .bind(organization_id)
        .fetch_one(conn)
        .await
}

/// Enables or disables the trusted shell executor and audits the change. Setting the value
/// the organization already has changes nothing and writes no audit event.
pub async fn set_shell_enabled(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    enabled: bool,
) -> Result<(), Error> {
    let changed = sqlx::query(
        "UPDATE organizations
         SET shell_enabled_at = CASE WHEN $2 THEN now() END
         WHERE id = $1 AND (shell_enabled_at IS NOT NULL) <> $2",
    )
    .bind(organization_id)
    .bind(enabled)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if changed == 1 {
        let action = if enabled {
            "organization.shell_enabled"
        } else {
            "organization.shell_disabled"
        };
        crate::audit::insert_in_transaction(
            conn,
            organization_id,
            actor_user_id,
            action,
            "organization",
            &organization_id.to_string(),
            json!({}),
        )
        .await?;
    }
    Ok(())
}
```

- [ ] **Step 6: Add the settings routes and refuse shell nodes while shell is disabled**

Create `crates/api/src/organizations/settings.rs`:

```rust
use axum::{Extension, Json, extract::State};
use corqty_domain::rbac::Capability;
use corqty_persistence::{models::SessionRecord, organizations};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    authz::authorize,
    error::ApiError,
    extract::{ApiJson, ApiPath},
    state::AppState,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    shell_enabled: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShellSetting {
    enabled: bool,
}

pub async fn get_settings(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(organization_id): ApiPath<Uuid>,
) -> Result<Json<Settings>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let shell_enabled = organizations::shell_enabled(&mut access.tx, organization_id).await?;
    access.commit().await?;
    Ok(Json(Settings { shell_enabled }))
}

/// Allows or refuses shell nodes in new publishes and runs. Runs that already exist keep
/// their shell nodes either way.
pub async fn set_shell(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(organization_id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<ShellSetting>,
) -> Result<Json<Settings>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::EnableShellExecutor,
    )
    .await?;
    organizations::set_shell_enabled(
        &mut access.tx,
        organization_id,
        access.user_id,
        request.enabled,
    )
    .await?;
    access.commit().await?;
    Ok(Json(Settings {
        shell_enabled: request.enabled,
    }))
}
```

Replace the whole of `crates/api/src/organizations/mod.rs`:

```rust
pub mod handlers;
pub mod members;
pub mod service;
pub mod settings;
```

In `crates/api/src/router.rs`:

Replace:

```rust
        .route("/orgs/{organization_id}/audit-events", get(audit::list))
```

With:

```rust
        .route("/orgs/{organization_id}/audit-events", get(audit::list))
        .route(
            "/orgs/{organization_id}/settings",
            get(organizations::settings::get_settings),
        )
        .route(
            "/orgs/{organization_id}/settings/shell",
            put(organizations::settings::set_shell),
        )
```

In `crates/api/src/workflows.rs`:

Replace:

```rust
use corqty_domain::{
    graph::{GraphSnapshot, check_limits},
    publish::{canonical_checksum, validate_graph},
    rbac::Capability,
    slug::slugify,
};
use corqty_persistence::{
    catalog,
    models::SessionRecord,
    workflows::{self, VersionRecord, VersionSummaryRecord, WorkflowRecord, WorkflowSummaryRecord},
};
```

With:

```rust
use corqty_domain::{
    diagnostic::Diagnostic,
    graph::{GraphSnapshot, check_limits},
    publish::{Definitions, canonical_checksum, shell_diagnostics, validate_graph},
    rbac::Capability,
    slug::slugify,
};
use corqty_persistence::{
    catalog,
    models::SessionRecord,
    organizations,
    workflows::{self, VersionRecord, VersionSummaryRecord, WorkflowRecord, WorkflowSummaryRecord},
};
```

Replace:

```rust
/// Checks a graph as a publish would, without storing anything.
```

With:

```rust
/// Checks a graph as a publish would, shell nodes included, without storing anything.
```

Replace:

```rust
    let definitions = catalog::definitions_map(&mut access.tx).await?;
    let (_, diagnostics) =
        blocking(body.graph, move |graph| validate_graph(graph, &definitions)).await?;
```

With:

```rust
    let shell_enabled = organizations::shell_enabled(&mut access.tx, organization_id).await?;
    let definitions = catalog::definitions_map(&mut access.tx).await?;
    let (_, diagnostics) = blocking(body.graph, move |graph| {
        publish_diagnostics(graph, &definitions, shell_enabled)
    })
    .await?;
```

Replace:

```rust
    let definitions = catalog::definitions_map(&mut access.tx).await?;
    let (graph, diagnostics) = blocking(draft.graph, move |graph| {
        validate_graph(graph, &definitions)
    })
    .await?;
```

With:

```rust
    let shell_enabled = organizations::shell_enabled(&mut access.tx, organization_id).await?;
    let definitions = catalog::definitions_map(&mut access.tx).await?;
    let (graph, diagnostics) = blocking(draft.graph, move |graph| {
        publish_diagnostics(graph, &definitions, shell_enabled)
    })
    .await?;
```

Replace:

```rust
/// The unique key that keeps a draft revision from becoming two versions. The workflow
```

With:

```rust
/// Every reason the organization cannot publish `graph`: the publish rules, then each shell
/// node while the organization has not enabled the shell executor.
fn publish_diagnostics(
    graph: &GraphSnapshot,
    definitions: &Definitions,
    shell_enabled: bool,
) -> Vec<Diagnostic> {
    let mut diagnostics = validate_graph(graph, definitions);
    if !shell_enabled {
        diagnostics.extend(shell_diagnostics(graph, definitions));
    }
    diagnostics
}

/// The unique key that keeps a draft revision from becoming two versions. The workflow
```

In `crates/api/src/runs.rs`:

Replace:

```rust
    diagnostic::Diagnostic,
    rbac::Capability,
    run::plan_run,
```

With:

```rust
    diagnostic::Diagnostic,
    publish::shell_diagnostics,
    rbac::Capability,
    run::plan_run,
```

Replace:

```rust
    models::SessionRecord,
    runs::{self, CancelError, NewRun, RunRecord, RunSummaryRecord},
```

With:

```rust
    models::SessionRecord,
    organizations,
    runs::{self, CancelError, NewRun, RunRecord, RunSummaryRecord},
```

Replace:

```rust
/// Creates a run of the workflow's `version` from its plan and reads it back. `input` must
/// match that version's workflow input schema.
```

With:

```rust
/// Creates a run of the workflow's `version` from its plan and reads it back. `input` must
/// match that version's workflow input schema, and a version with shell nodes runs only while
/// the organization has the shell executor enabled.
```

Replace:

```rust
    let definitions = catalog::definitions_map(&mut access.tx).await?;
    let nodes = plan_run(&version.graph, &definitions)
```

With:

```rust
    let definitions = catalog::definitions_map(&mut access.tx).await?;
    // ponytail: no row lock, so a run that starts while an admin disables shell counts as
    // started just before; disabling never touches existing runs anyway.
    if !organizations::shell_enabled(&mut access.tx, access.organization_id).await? {
        let diagnostics = shell_diagnostics(&version.graph, &definitions);
        if !diagnostics.is_empty() {
            return Err(ApiError::Invalid {
                code: "executor_disabled",
                diagnostics,
            });
        }
    }
    let nodes = plan_run(&version.graph, &definitions)
```

- [ ] **Step 7: Verify GREEN**

```text
cargo test -p corqty-domain --test catalog_rules --test publish_rules
```

Expected: 7 and 11 tests pass.

```text
cargo test -p corqty-persistence --test test_reset --test workflow_catalog
```

Expected: 1 and 6 tests pass. The first reset applies migration `202610260003`.

```text
cargo test -p corqty-api --test settings_http --test catalog_http --test authorization_matrix
```

Expected: `authorization_matrix` 6, `catalog_http` 4 and `settings_http` 2 tests pass.

Negative controls: with the run-creation check skipped (`if false && !organizations::shell_enabled(...)`),
`shell_nodes_publish_and_start_only_while_shell_is_enabled` fails at `settings_http.rs:19:5` with `left: 201`,
`right: 422`. With the `AND (shell_enabled_at IS NOT NULL) <> $2` condition removed from the UPDATE,
`admins_turn_the_shell_executor_on_and_off` fails at `settings_http.rs:125:5`: the audit then lists each repeated
PUT, two `shell_enabled` and two `shell_disabled` events.

- [ ] **Step 8: Run the full backend verification**

```text
cargo fmt --check
```

Expected: no output, exit code 0.

```text
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: `Finished` with no warnings.

```text
cargo test --workspace
```

Expected: every suite passes, with these counts:

- corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 6, `catalog_http` 4, `members_http` 3,
  `organizations_http` 1, `problems_http` 2, `publish_http` 2, `runs_http` 8, `settings_http` 2, `shutdown` 1,
  `workflows_http` 4.
- corqty-domain: `account_and_rbac` 6, `catalog_rules` 7, `graph_rules` 8, `publish_rules` 11, `run_rules` 8,
  `runtime_rules` 12.
- corqty-executor-mock: `contract` 11.
- corqty-executor-shell: `contract` 0 on Windows and macOS, 15 on Linux.
- corqty-persistence: `auth_store` 2, `catalog_guards` 4, `final_admin` 2, `migrations` 1, `run_store` 8,
  `tenant_isolation` 1, `test_reset` 1, `worker_queue` 11, `workflow_catalog` 6.
- corqty-plugin-host: `registry` 8.

That is 148 tests on Windows and 163 on Linux (CI). No unix-only code changed in this task.

- [ ] **Step 9: Commit**

```text
git add crates migrations scripts
git commit -m "feat: let admins enable the trusted shell executor"
```

Expected: `git show --stat HEAD` lists exactly the 17 files above (798 insertions, 26 deletions).

## Task 12: Recover expired leases in a leader-elected API loop

**Files:**

- Create: `migrations/202610260004_lease_recovery.sql`
- Create: `crates/api/src/loops.rs`
- Modify: `crates/api/src/lib.rs`
- Modify: `crates/api/src/main.rs`
- Modify: `crates/persistence/src/worker_queue.rs`
- Modify: `crates/persistence/tests/worker_queue.rs`
- Test: `crates/api/tests/loops.rs`

A worker renews the lease of each node run it executes through `corqty.heartbeat` (Task 9). A worker that crashes,
or loses its database connection, stops renewing. Its node runs then stay `running` past `lease_expires_at`, and
without help the run would never finish. This task gives the API process its first background loop, which recovers
those leases (spec M3-05, M3-06, M3-113).

Migration `202610260004_lease_recovery.sql` adds `corqty.recover_expired_leases() RETURNS integer`. It is SECURITY
DEFINER, so it reaches every organization's node runs, and it is granted to `corqty_api` only. One call works like
this:

- It first takes `pg_try_advisory_xact_lock(7243, 1)`. Every API replica runs the loop, and the replica that gets
  the lock does the pass. Any other replica gets 0 at once and changes nothing. The lock is transaction-scoped, so
  it never stays behind on a pooled connection, and it ends when the pass commits or rolls back.
- It visits the runs that have an expired lease (`state = 'running' AND lease_expires_at < now()`), in `run_id`
  order. For each run it follows the lock order of D9: first `SELECT ... FROM runs ... FOR UPDATE`, then one
  statement that locks that run's expired node runs in id order and bumps their `lease_token`. The new token fences
  out the old worker, should it come back: its start, heartbeat, log and complete calls all fail.
- It then calls Task 9's `corqty.finish_node_run` for each of those node runs, in id order, with the outcome
  `{"status":"failed","error":{"code":"lease_expired","message":"The worker stopped renewing its lease.",
  "retryable":true}}`. That applies D10 exactly as a worker's own failure would:
  - While attempts are left, the node waits in `retry_wait` with the usual backoff, and the error stays.
  - When no attempt is left, it fails, its blocked descendants are skipped, and `settle_run` ends the run with a
    terminal `run.state` event.
  - When the run was asked to cancel, the node is canceled and keeps the error.
  - Each change is a `node.state` event. `finish_node_run` also clears `leased_by` and `lease_expires_at`.
- The claim already counted the attempt, so recovery does not count it again: the retry is claimed as the next
  attempt.
- It returns how many node runs it recovered.

Recovery re-reads the expired rows under the runs lock, so a lease that a worker completed or renewed in the meantime
no longer matches. A completion takes the runs lock first and has finished by the time recovery reads the rows. A
heartbeat takes no runs lock, but it locks the row it renews, so recovery waits for it and then re-checks
`lease_expires_at`. Within one run, recovery and a heartbeat both lock node rows in id order. Across runs, a cycle
is possible: a heartbeat for leases in two runs against a pass that recovers both. PostgreSQL then aborts one side
with 40P01, and the loop logs a WARN and tries again on its next pass.

The worker role still executes only its six definer functions, so `catalog_guards.rs` passes unchanged.

`corqty_persistence::worker_queue::recover_expired_leases(pool)` wraps the function and returns its count as `i32`.
The API calls it with its own pool.

`crates/api/src/loops.rs` holds the API's background loops, behind one function: `loops::spawn(&state)` returns a
`JoinSet<()>`. Later loops (cron and retention, Milestone 5) join the same set, and the module can move to another
service without touching the domain (spec M3-07). The recovery loop runs its first pass at once, then waits 5 s
between passes. A failed pass is logged as WARN, and a pass that recovered leases is logged as INFO with the count.
The loop ends as soon as `state.shutdown` is cancelled, even in the middle of its wait. `main.rs` starts the loops
right after it builds `AppState` and waits for them after `serve` returns.

The persistence tests extend `worker_queue.rs` and reuse its fixture. A small helper, `expire`, moves a claimed
lease's `lease_expires_at` one second into the past through the owner pool, as if its worker had died. The API test
`crates/api/tests/loops.rs` drives the real loop against the test database.

- [ ] **Step 1: Write the failing tests**

In `crates/persistence/tests/worker_queue.rs`:

Replace:

```rust
    worker_queue::{ClaimedJob, Job, LeaseStatus, NodeError, Outcome, WorkerDb},
    workflows,
};
use corqty_test_support::TestDatabase;
use serde_json::{Map, Value, json};
use serial_test::serial;
use sqlx::{Postgres, Transaction};
```

With:

```rust
    worker_queue::{
        ClaimedJob, Job, LeaseStatus, NodeError, Outcome, WorkerDb, recover_expired_leases,
    },
    workflows,
};
use corqty_test_support::{API_URL, TestDatabase};
use serde_json::{Map, Value, json};
use serial_test::serial;
use sqlx::{Postgres, Transaction, postgres::PgPoolOptions};
```

Replace:

```rust
        assert_eq!(job.unwrap(), Some(expected));
    }
}
```

With:

```rust
        assert_eq!(job.unwrap(), Some(expected));
    }
}

/// Ends a claimed lease a second ago, as if its worker had stopped renewing it.
async fn expire(fixture: &Fixture, job: &ClaimedJob) {
    sqlx::query(
        "UPDATE node_runs SET lease_expires_at = now() - interval '1 second' WHERE id = $1",
    )
    .bind(job.node_run_id)
    .execute(&fixture.database.owner)
    .await
    .unwrap();
}

/// The error that recovery records for an expired lease.
fn lease_expired() -> Value {
    json!({
        "code": "lease_expired",
        "message": "The worker stopped renewing its lease.",
        "retryable": true
    })
}

#[tokio::test]
#[serial]
async fn an_expired_lease_waits_for_a_retry_and_fences_out_its_worker() {
    let fixture = Fixture::new().await;
    let graph = graph(&[task("lost", 2), task("kept", 1)], &[]);
    let published = fixture.publish(fixture.a, &graph).await;
    let run_id = fixture.run(&published, &published.plan).await;
    let worker = fixture.worker(&["mock@1.0.0"]).await;
    let roots = fixture.claim(&worker).await;
    let (lost, kept) = (&roots["lost"], &roots["kept"]);
    // lost's worker stopped renewing its lease; kept's lease still runs.
    expire(&fixture, lost).await;
    let (api, owner) = (&fixture.database.api, &fixture.database.owner);

    assert_eq!(recover_expired_leases(api).await.unwrap(), 1);
    assert_eq!(recover_expired_leases(api).await.unwrap(), 0);

    // The claim counted the attempt, so it stays 1. The retry waits 2 s plus up to 20%
    // jitter, as after any retryable failure, and a new token ends the old lease.
    let (state, attempt, token, leased, error, delay): (String, i32, i64, bool, Value, f64) =
        sqlx::query_as(
            "SELECT n.state, n.attempt, n.lease_token,
                    n.leased_by IS NOT NULL OR n.lease_expires_at IS NOT NULL, n.error,
                    extract(epoch FROM n.available_at - e.created_at)::float8
             FROM node_runs n
             JOIN run_events e ON e.run_id = n.run_id AND e.payload ->> 'state' = 'retry_wait'
             WHERE n.id = $1",
        )
        .bind(lost.node_run_id)
        .fetch_one(owner)
        .await
        .unwrap();
    assert_eq!(
        (state.as_str(), attempt, token, leased, &error),
        ("retry_wait", 1, 2, false, &lease_expired())
    );
    assert!((2.0..2.4).contains(&delay), "{delay}");
    let kept_lease: (String, Option<Uuid>, i64) =
        sqlx::query_as("SELECT state, leased_by, lease_token FROM node_runs WHERE id = $1")
            .bind(kept.node_run_id)
            .fetch_one(owner)
            .await
            .unwrap();
    assert_eq!(
        kept_lease,
        ("running".to_owned(), Some(worker.worker_id()), 1)
    );
    assert_eq!(
        fixture.events(run_id).await,
        [
            run_state("queued", 1),
            with_field(
                node_state("lost", "retry_wait", 1, 2),
                "error",
                lease_expired()
            ),
        ]
    );

    // The old lease can no longer start, renew, log or complete the attempt.
    let before = fixture.snapshot().await;
    let calls = fenced_calls(&worker, lost.node_run_id, lost.lease_token).await;
    assert_eq!(calls, (false, false, false, false));
    assert_eq!(fixture.snapshot().await, before);

    // Once due, the retry is claimed as attempt 2.
    sqlx::query("UPDATE node_runs SET available_at = now() WHERE id = $1")
        .bind(lost.node_run_id)
        .execute(owner)
        .await
        .unwrap();
    let retry = fixture.claim(&worker).await.remove("lost").unwrap();
    assert_eq!((retry.attempt, retry.lease_token), (2, 3));
}

#[tokio::test]
#[serial]
async fn an_expired_lease_without_attempts_left_fails_the_node_and_the_run() {
    let fixture = Fixture::new().await;
    let published = fixture
        .publish(fixture.a, &fixture.infrastructure().await)
        .await;
    let run_id = fixture.run(&published, &published.plan).await;
    let worker = fixture.worker(&["mock@1.0.0"]).await;
    let vpc = fixture.claim(&worker).await.remove("vpc").unwrap();
    let job = worker.start(vpc.node_run_id, vpc.lease_token).await;
    assert!(job.unwrap().is_some());
    expire(&fixture, &vpc).await;

    let api = &fixture.database.api;
    assert_eq!(recover_expired_leases(api).await.unwrap(), 1);

    // vpc had its only attempt: it fails, its descendants are skipped, and the run ends.
    assert_eq!(
        fixture.events(run_id).await,
        [
            run_state("queued", 1),
            run_state("running", 2),
            node_state("vpc", "running", 1, 3),
            with_field(node_state("vpc", "failed", 1, 4), "error", lease_expired()),
            node_state("rds", "skipped", 0, 5),
            node_state("deploy", "skipped", 0, 6),
            run_state("failed", 7),
        ]
    );
    let run: (String, bool) =
        sqlx::query_as("SELECT status, finished_at IS NOT NULL FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(&fixture.database.owner)
            .await
            .unwrap();
    assert_eq!(run, ("failed".to_owned(), true));
}

#[tokio::test]
#[serial]
async fn an_expired_lease_of_a_run_asked_to_cancel_cancels_the_node() {
    let fixture = Fixture::new().await;
    let graph = graph(
        &[task("long", 3), task("after", 1)],
        &[edge("long", "after", "input")],
    );
    let published = fixture.publish(fixture.a, &graph).await;
    let run_id = fixture.run(&published, &published.plan).await;
    let worker = fixture.worker(&["mock@1.0.0"]).await;
    let long = fixture.claim(&worker).await.remove("long").unwrap();
    let tenant = fixture.a;
    let mut tx = fixture.tx(tenant).await;
    runs::cancel_run(&mut tx, tenant.organization_id, tenant.admin, run_id)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    expire(&fixture, &long).await;

    let api = &fixture.database.api;
    assert_eq!(recover_expired_leases(api).await.unwrap(), 1);

    // Two attempts are left, but a run that was asked to cancel is not retried.
    assert_eq!(
        fixture.events(run_id).await,
        [
            run_state("queued", 1),
            node_state("after", "canceled", 0, 2),
            with_field(
                node_state("long", "canceled", 1, 3),
                "error",
                lease_expired()
            ),
            run_state("canceled", 4),
        ]
    );
}

#[tokio::test]
#[serial]
async fn one_recovery_pass_at_a_time_recovers_each_expired_lease_once() {
    let fixture = Fixture::new().await;
    let published = fixture
        .publish(fixture.a, &graph(&[task("task", 2)], &[]))
        .await;
    for _ in 0..20 {
        fixture.run(&published, &published.plan).await;
    }
    let worker = fixture.worker(&["mock@1.0.0"]).await;
    assert_eq!(worker.claim(100, 30).await.unwrap().len(), 20);
    let owner = &fixture.database.owner;
    sqlx::query("UPDATE node_runs SET lease_expires_at = now() - interval '1 second'")
        .execute(owner)
        .await
        .unwrap();

    // While another API replica holds the recovery lock, a pass recovers nothing.
    let before = fixture.snapshot().await;
    let mut leader = owner.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(7243, 1)")
        .execute(&mut *leader)
        .await
        .unwrap();
    let api = &fixture.database.api;
    assert_eq!(recover_expired_leases(api).await.unwrap(), 0);
    leader.rollback().await.unwrap();
    assert_eq!(fixture.snapshot().await, before);

    // Four passes at the same time, each on a connection of its own, recover each lease
    // once: one new token and one event per node run.
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(API_URL)
        .await
        .unwrap();
    let mut passes = JoinSet::new();
    for _ in 0..4 {
        let pool = pool.clone();
        passes.spawn(async move { recover_expired_leases(&pool).await.unwrap() });
    }
    assert_eq!(passes.join_all().await.iter().sum::<i32>(), 20);
    let recovered: Vec<(String, i32, i64, i64)> = sqlx::query_as(
        "SELECT n.state, n.attempt, n.lease_token,
                (SELECT count(*) FROM run_events e WHERE e.run_id = n.run_id)
         FROM node_runs n",
    )
    .fetch_all(owner)
    .await
    .unwrap();
    assert_eq!(recovered, vec![("retry_wait".to_owned(), 1, 2, 2); 20]);
}
```

The four new tests:

- `an_expired_lease_waits_for_a_retry_and_fences_out_its_worker`: two roots, `lost` with 2 attempts and `kept` with
  1. Only `lost`'s lease expires. The first pass recovers 1 and the second recovers 0. `lost` is then `retry_wait` at
  attempt 1 under token 2. It holds no lease, its error is `lease_expired`, and the retry waits 2.0 to 2.4 s from the
  event. `kept` is still running under its worker and token 1. The only new event is `node.state retry_wait` with
  the error. With the old token, start, heartbeat, log and complete all fail and change nothing. Once due, the retry
  is claimed as attempt 2 under token 3.
- `an_expired_lease_without_attempts_left_fails_the_node_and_the_run`: in the simulated infrastructure blueprint,
  vpc (1 attempt) has started when its lease expires. Recovery fails vpc with `lease_expired`, skips rds and deploy,
  and ends the run `failed` with `finished_at` set. The test checks the whole event log, seq 1 to 7.
- `an_expired_lease_of_a_run_asked_to_cancel_cancels_the_node`: `long` has 3 attempts and `after` waits for it.
  Cancel is requested while `long` is claimed, then its lease expires. Recovery cancels `long`, keeping the error,
  and the run ends `canceled`.
- `one_recovery_pass_at_a_time_recovers_each_expired_lease_once`: 20 runs each have one expired lease. While an
  owner transaction holds `pg_advisory_xact_lock(7243, 1)`, as another replica's pass would, a pass returns 0 and
  the snapshot of node runs, runs and events is unchanged. Then four passes run at the same time, each on its own
  connection of a 4-connection `corqty_api` pool, and together recover 20. Every node run ends `retry_wait` at
  attempt 1 under token 2, with exactly two events in its run (queued, then retry_wait).

Create `crates/api/tests/loops.rs`:

```rust
mod support;

use std::time::Duration;

use axum::{body::to_bytes, http::StatusCode, response::Response};
use corqty_api::{config::ApiConfig, loops, state::AppState};
use corqty_persistence::{store::PgStore, worker_queue::WorkerDb};
use corqty_test_support::API_URL;
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use support::AuthFixture;
use uuid::Uuid;

/// Asserts the status and returns the JSON body.
async fn body(response: Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
#[serial]
async fn the_recovery_loop_recovers_expired_leases_until_shutdown() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let org = format!("/api/v1/orgs/{}", admin.organization_id);
    let request = json!({
        "name": "Provision",
        "blueprintId": "00000000-0000-7000-8000-000000000101",
    });
    let created = body(
        admin
            .post_json(&format!("{org}/workflows"), &request.to_string())
            .await,
        StatusCode::CREATED,
    )
    .await;
    let workflow = format!("{org}/workflows/{}", created["id"].as_str().unwrap());
    let published = admin
        .post_json(&format!("{workflow}/publish"), r#"{"revision":1}"#)
        .await;
    assert_eq!(published.status(), StatusCode::CREATED);
    let input = r#"{"input":{"region":"eu-west-1"}}"#;
    let run = body(
        admin.post_json(&format!("{workflow}/runs"), input).await,
        StatusCode::CREATED,
    )
    .await;
    let run = format!("{org}/runs/{}", run["id"].as_str().unwrap());

    // A worker claims the first node, then stops renewing its lease.
    let worker = WorkerDb::new(admin.worker.clone(), Uuid::now_v7());
    worker
        .register(&["mock@1.0.0".to_owned()], 1)
        .await
        .unwrap();
    let job = worker.claim(1, 30).await.unwrap().remove(0);
    sqlx::query(
        "UPDATE node_runs SET lease_expires_at = now() - interval '1 second' WHERE id = $1",
    )
    .bind(job.node_run_id)
    .execute(&admin.owner)
    .await
    .unwrap();

    // The loop's first pass runs at once. The node had its only attempt, so it fails and
    // the run fails with it.
    let pool = PgPool::connect(API_URL).await.unwrap();
    let state = AppState::new(PgStore::new(pool), ApiConfig::for_tests());
    let mut background = loops::spawn(&state);
    let recovered = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let run = body(admin.get(&run).await, StatusCode::OK).await;
            if run["status"] != "queued" {
                return run;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the first pass recovers the lease within 2 s");
    let states: Vec<&str> = recovered["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["state"].as_str().unwrap())
        .collect();
    assert_eq!(recovered["status"], "failed");
    assert_eq!(states, ["failed", "skipped", "skipped"]);
    assert_eq!(recovered["nodes"][0]["error"]["code"], "lease_expired");

    // The loops end on the shutdown token, although the next pass is 5 s away.
    state.shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(result) = background.join_next().await {
            result.unwrap();
        }
    })
    .await
    .expect("the loops stop within 2 s");
}
```

The API test starts a run of the simulated infrastructure blueprint over HTTP. A worker registered on the worker pool
claims vpc, then the owner pool expires the lease. The test builds an `AppState` on a pool of its own (`API_URL`) and
starts the loops. Within 2 s the run, read through `GET /runs/{run}`, has failed: vpc failed with `lease_expired`,
and the two other nodes were skipped. Cancelling `state.shutdown` then ends the loops within 2 s, although the next
pass is 5 s away.

- [ ] **Step 2: Run the tests and verify RED**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-persistence --test worker_queue
```

Expected: the test does not compile, because the repository function does not exist yet:

```text
error[E0432]: unresolved import `corqty_persistence::worker_queue::recover_expired_leases`
  --> crates\persistence\tests\worker_queue.rs:14:69
   |
14 |         ClaimedJob, Job, LeaseStatus, NodeError, Outcome, WorkerDb, recover_expired_leases,
   |                                                                     ^^^^^^^^^^^^^^^^^^^^^^ no `recover_expired_leases` in `worker_queue`
```

The last line is ``error: could not compile `corqty-persistence` (test "worker_queue") due to 1 previous error``.

```text
cargo test -p corqty-api --test loops
```

Expected: the test does not compile, because the module does not exist yet:

```text
error[E0432]: unresolved import `corqty_api::loops`
 --> crates\api\tests\loops.rs:6:37
  |
6 | use corqty_api::{config::ApiConfig, loops, state::AppState};
  |                                     ^^^^^ no `loops` in the root
```

The last line is ``error: could not compile `corqty-api` (test "loops") due to 1 previous error``.

- [ ] **Step 3: Add the repository function and the loop**

In `crates/persistence/src/worker_queue.rs`:

Replace:

```rust
            .bind(Json(outcome))
            .fetch_one(&self.pool)
            .await
    }
}
```

With:

```rust
            .bind(Json(outcome))
            .fetch_one(&self.pool)
            .await
    }
}

/// Recovers the node runs whose worker stopped renewing the lease, on the API's pool, and
/// returns how many it recovered. Each counts as a retryable `lease_expired` failure of its
/// attempt. 0 also when another API replica is recovering at the same time.
pub async fn recover_expired_leases(pool: &PgPool) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar("SELECT corqty.recover_expired_leases()")
        .fetch_one(pool)
        .await
}
```

Create `crates/api/src/loops.rs`:

```rust
//! Background loops of the API process. Every replica runs them, and each pass elects its
//! leader in the database, so replicas never repeat each other's work.

use std::time::Duration;

use corqty_persistence::worker_queue::recover_expired_leases;
use tokio::task::JoinSet;

use crate::state::AppState;

/// How long the recovery loop waits between passes.
const RECOVERY_INTERVAL: Duration = Duration::from_secs(5);

/// Starts the background loops. Each ends once `state.shutdown` is cancelled.
#[must_use = "dropping the JoinSet aborts the loops"]
pub fn spawn(state: &AppState) -> JoinSet<()> {
    let mut loops = JoinSet::new();
    let pool = state.store.pool().clone();
    let shutdown = state.shutdown.clone();
    loops.spawn(async move {
        loop {
            match recover_expired_leases(&pool).await {
                Ok(0) => {}
                Ok(recovered) => tracing::info!(recovered, "recovered expired worker leases"),
                // A deadlock with a heartbeat, or a lost connection: the next pass tries again.
                Err(error) => tracing::warn!(error = %error, "lease recovery failed"),
            }
            tokio::select! {
                () = shutdown.cancelled() => break,
                () = tokio::time::sleep(RECOVERY_INTERVAL) => {}
            }
        }
    });
    loops
}
```

In `crates/api/src/lib.rs`:

Replace:

```rust
pub mod extract;
pub mod organizations;
```

With:

```rust
pub mod extract;
pub mod loops;
pub mod organizations;
```

In `crates/api/src/main.rs`:

Replace:

```rust
use corqty_api::{config::ApiConfig, serve, state::AppState};
```

With:

```rust
use corqty_api::{config::ApiConfig, loops, serve, state::AppState};
```

Replace:

```rust
    let state = AppState::new(PgStore::new(pool), config);
    let shutdown = state.shutdown.clone();
```

With:

```rust
    let state = AppState::new(PgStore::new(pool), config);
    let background = loops::spawn(&state);
    let shutdown = state.shutdown.clone();
```

Replace:

```rust
    serve(listener, state).await?;
    Ok(())
```

With:

```rust
    serve(listener, state).await?;
    background.join_all().await;
    Ok(())
```

`spawn` is `#[must_use]` because dropping a `JoinSet` aborts its tasks. The pass itself is not raced against the
shutdown token: it is one short statement, and the token is checked again as soon as it returns. When `serve`
returns an error, `main` returns early and the dropped `JoinSet` aborts the loop.

- [ ] **Step 4: Run the tests and verify RED at the database**

```text
cargo test -p corqty-persistence --test worker_queue
```

Expected: 11 passed and 4 failed. Each new test fails on its first pass, because the SQL function does not exist yet:

```text
thread 'an_expired_lease_waits_for_a_retry_and_fences_out_its_worker' (82248) panicked at crates\persistence\tests\worker_queue.rs:1001:50:
called `Result::unwrap()` on an `Err` value: Database(PgDatabaseError { severity: Error, code: "42883", message: "function corqty.recover_expired_leases() does not exist", detail: None, hint: Some("No function matches the given name and argument types. You might need to add explicit type casts."), position: Some(Original(8)), where: None, schema: None, table: None, column: None, data_type: None, constraint: None, file: Some("parse_func.c"), line: Some(629), routine: Some("ParseFuncOrColumn") })
```

The three others panic the same way, at lines 1077, 1122 and 1166.

```text
cargo test -p corqty-api --test loops
```

Expected: 1 failed. Each pass of the loop fails, so the run stays queued:

```text
thread 'the_recovery_loop_recovers_expired_leases_until_shutdown' (33420) panicked at crates\api\tests\loops.rs:81:6:
the first pass recovers the lease within 2 s: Elapsed(())
```

- [ ] **Step 5: Add the recovery function**

Create `migrations/202610260004_lease_recovery.sql`:

```sql
-- A worker renews the lease of each node run it executes (corqty.heartbeat). One that crashed
-- or lost its connection stops renewing, and its node runs stay running past
-- lease_expires_at. Every API replica calls corqty.recover_expired_leases() every few
-- seconds. The replica that takes the advisory lock recovers them; the others skip the pass.

-- Ends each expired lease as an attempt that failed with a retryable lease_expired error (see
-- corqty.finish_node_run). The node waits for a retry while attempts are left, fails and
-- skips its descendants once none are, and is canceled when its run was asked to cancel.
-- The claim already counted the attempt, so it is not counted again. A new lease token fences
-- out the old worker, should it come back. Returns how many node runs it recovered, and 0
-- when another caller holds the lock.
CREATE FUNCTION corqty.recover_expired_leases()
RETURNS integer
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
    v_run uuid;
    v_node_run uuid;
    v_recovered integer := 0;
BEGIN
    -- One caller at a time. The lock ends with the transaction.
    IF NOT pg_try_advisory_xact_lock(7243, 1) THEN
        RETURN 0;
    END IF;
    FOR v_run IN
        SELECT DISTINCT run_id
        FROM node_runs
        WHERE state = 'running' AND lease_expires_at < now()
        ORDER BY run_id
    LOOP
        -- The run first, then its expired node runs in one statement in id order. Under the
        -- locks, a lease that was completed or renewed in the meantime no longer matches.
        PERFORM FROM runs WHERE id = v_run FOR UPDATE;
        FOR v_node_run IN
            WITH fenced AS (
                UPDATE node_runs AS n
                SET lease_token = n.lease_token + 1
                FROM (
                    SELECT id
                    FROM node_runs
                    WHERE run_id = v_run AND state = 'running' AND lease_expires_at < now()
                    ORDER BY id
                    FOR UPDATE
                ) AS expired
                WHERE n.id = expired.id
                RETURNING n.id
            )
            SELECT id FROM fenced ORDER BY id
        LOOP
            PERFORM corqty.finish_node_run(v_node_run, '{
                "status": "failed",
                "error": {
                    "code": "lease_expired",
                    "message": "The worker stopped renewing its lease.",
                    "retryable": true
                }
            }');
            v_recovered := v_recovered + 1;
        END LOOP;
    END LOOP;
    RETURN v_recovered;
END
$$;

REVOKE ALL ON FUNCTION corqty.recover_expired_leases() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.recover_expired_leases() TO corqty_api;
```

Notes on the SQL:

- The outer query reads the run ids without a lock. `node_runs.run_id` never changes, and the inner statement
  selects the expired rows again under the runs lock.
- `FOR v_node_run IN WITH fenced AS (UPDATE ... RETURNING n.id) SELECT id FROM fenced ORDER BY id LOOP` bumps every
  token in one statement, then finishes the node runs in id order, so the events are in plan order.
- `finish_node_run` does not check the node's state and does not change `lease_token`, which is why the bump comes
  first, under the row locks.
- `pg_try_advisory_xact_lock` is in `pg_catalog`, which every search path includes.

- [ ] **Step 6: Verify GREEN**

```text
cargo test -p corqty-persistence --test worker_queue --test catalog_guards
```

Expected: `catalog_guards` 4 passed, and `worker_queue` 15 passed in about 5 s.

```text
cargo test -p corqty-api --test loops
```

Expected: 1 passed in about 1 s.

Five negative controls show that the tests catch the mistakes they target. Four replaced
`corqty.recover_expired_leases()` in the test database with a changed copy (`CREATE OR REPLACE FUNCTION` through
`docker exec -i wt-backend-postgres-test-1 psql -U corqty_owner -d corqty_test`), ran the named test, and restored
the function from the migration text:

- Without the advisory lock: `one_recovery_pass_at_a_time_recovers_each_expired_lease_once` recovers 20 while
  another replica holds the lock, instead of 0.
- Without the token bump: `an_expired_lease_waits_for_a_retry_and_fences_out_its_worker` finds token 1, not 2.
- Without `AND lease_expires_at < now()` in the inner statement: the same test recovers 2 node runs, because `kept`
  is recovered too.
- With `"retryable": false`: the same test finds no `retry_wait` event, because `lost` failed.

The fifth removed the `shutdown.cancelled()` arm from the loop's `select!`. The API test then fails with
`the loops stop within 2 s: Elapsed(())`.

Recreate the test database afterwards, so that no changed function is left behind:

```text
docker compose -f docker-compose.test.yml down -v
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: the container is removed, then reported `Healthy`.

- [ ] **Step 7: Run the full backend verification**

```text
cargo fmt --check
```

Expected: no output, exit code 0.

```text
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: `Finished` with no warnings.

```text
cargo test --workspace
```

Expected: every suite passes. corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 6,
`catalog_http` 4, `loops` 1, `members_http` 3, `organizations_http` 1, `problems_http` 2, `publish_http` 2,
`runs_http` 8, `settings_http` 2, `shutdown` 1, `workflows_http` 4. corqty-domain: `account_and_rbac` 6,
`catalog_rules` 7, `graph_rules` 8, `publish_rules` 11, `run_rules` 8, `runtime_rules` 12. corqty-executor-mock:
`contract` 11. corqty-executor-shell: `contract` 0 on Windows and macOS, 15 on Linux. corqty-persistence:
`auth_store` 2, `catalog_guards` 4, `final_admin` 2, `migrations` 1, `run_store` 8, `tenant_isolation` 1,
`test_reset` 1, `worker_queue` 15, `workflow_catalog` 6. corqty-plugin-host: `registry` 8. That is 153 tests on
Windows and 168 on Linux (CI). No unix-only code changed, so the Docker run of Task 7 is not repeated.

- [ ] **Step 8: Commit**

```text
git add migrations/202610260004_lease_recovery.sql crates/api crates/persistence
git commit -m "feat: recover expired leases in a leader-elected API loop"
```

Expected: `git show --stat HEAD` lists exactly the 7 files above (449 insertions, 4 deletions).

## Task 13: Stream run events over SSE with replay

**Files:**

- Create: `crates/api/src/events.rs`
- Modify: `Cargo.lock`
- Modify: `crates/api/Cargo.toml`
- Modify: `crates/api/src/lib.rs`
- Modify: `crates/api/src/main.rs`
- Modify: `crates/api/src/router.rs`
- Modify: `crates/api/src/state.rs`
- Modify: `crates/api/tests/authorization_matrix.rs`
- Test: `crates/api/tests/events_http.rs`

Every change to a run is already a `run_events` row with a gapless per-run `seq` (Tasks 8, 9 and 12), and the
trigger `run_events_notify` sends the run id on the channel `corqty_run_events` once such a row commits. This task
serves those events to the browser as server-sent events (D13) on `GET /api/v1/orgs/{org}/runs/{run}/events`, for
every role that can view the organization (spec M3-17, M3-100). The database stays the only source of the events:
a notification carries nothing but the run id, and a stream that hears about its run reads the new rows itself.

The handler works like this:

- The cursor is the `Last-Event-ID` header, which the browser sends when it reconnects on its own, else `?after=`,
  which the web app puts on the URL of a new `EventSource`, else 0. A header that is not a number is ignored.
- It authorizes `ViewOrganization` and reads the run with `runs::get_run` in a short tenant transaction. Another
  organization's run, or a malformed id, is 404 (spec M3-92). A finished run (status other than queued or running)
  whose `last_event_seq` the cursor has reached answers 204. Per the HTML spec, 204 stops `EventSource` from
  reconnecting.
- Otherwise it subscribes to the process's wake-up broadcast and returns the stream. The subscription exists
  before the stream's first read, so an event committed after any read still wakes the stream.
- The first frame is `retry: 2000`. Then the stream reads the events after the cursor in pages of 500, each page in
  its own tenant transaction, so an open stream never holds a pool connection. Each event is unnamed, with
  `id: <seq>` and `data: <payload>`, and the payload repeats `seq` and `type` (spec M3-102, M3-103).
- Once it has sent everything, the stream waits for a wake-up for its run, a `Resync`, or a lagged receiver, and
  then reads again. It also reads again 30 s after its last read, in case a notification was ever lost.
- It ends after the run's terminal `run.state` event, when `state.shutdown` is cancelled, after 10 minutes (the
  browser then reconnects from its last id), or when a read fails (logged at WARN). axum's graceful shutdown waits
  for open responses with no time limit, so the shutdown end is what lets the API stop while streams are open.
- The headers are `Cache-Control: no-cache, no-transform`, which replaces the `no-cache` that `Sse` sets (spec
  M3-115: Next.js does not compress a response marked `no-transform`, so the rewrite does not buffer the stream),
  and `X-Accel-Buffering: no`. A keep-alive comment goes out every 15 s.

`events::spawn_listener(database_url, wake, shutdown)` owns one `PgListener` per process, on a connection of its own.
It turns each notification into `Wake::Run(run_id)` on the broadcast. After every connect it sends `Wake::Resync`,
because notifications sent while nobody listened are lost, and every stream then reads again. sqlx reconnects on its
own when the connection drops, and `try_recv` then returns `Ok(None)`, which also sends `Resync`. Any other error
rebuilds the listener; a failed connect is retried with backoff from 250 ms up to 10 s. The listener stops on the
shutdown token. `AppState` gains `wake: broadcast::Sender<Wake>` with capacity 1024, created in `AppState::new`, so
tests that start no listener still build. `main.rs` starts the listener with the API's own `DATABASE_URL_API`, and
waits for it after `serve` returns. LISTEN needs no grant, so `corqty_api` listens.

The stream is a `futures_util::stream::unfold` over a small `Follower`, cut by `take_until` on the shutdown token or
the 10-minute timer. So `corqty-api` gains the workspace dependency `futures-util`, which the tests also use to read
`Body::into_data_stream()` (D1).

- [ ] **Step 1: Write the failing tests**

Replace the whole of `crates/api/Cargo.toml`:

```toml
[package]
name = "corqty-api"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
argon2.workspace = true
axum.workspace = true
base64.workspace = true
chrono.workspace = true
corqty-domain = { path = "../domain" }
corqty-persistence = { path = "../persistence" }
futures-util.workspace = true
getrandom.workspace = true
serde.workspace = true
serde_json.workspace = true
sha2.workspace = true
sqlx.workspace = true
thiserror.workspace = true
tokio.workspace = true
tokio-util.workspace = true
tower-http.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
uuid.workspace = true

[dev-dependencies]
corqty-test-support = { path = "../test-support" }
serial_test = "3"
tower.workspace = true

[lints]
workspace = true
```

The next cargo command updates `Cargo.lock`: `corqty-api` gains `futures-util`, which the lockfile already holds.

Create `crates/api/tests/events_http.rs`:

```rust
mod support;

use std::time::Duration;

use axum::{
    Router,
    body::{Body, BodyDataStream, to_bytes},
    http::{Request, StatusCode, header},
    response::Response,
};
use corqty_api::{config::ApiConfig, events, router::build_router, serve, state::AppState};
use corqty_persistence::store::PgStore;
use corqty_test_support::API_URL;
use futures_util::StreamExt;
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use support::AuthFixture;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tower::ServiceExt;
use uuid::Uuid;

const INFRASTRUCTURE_BLUEPRINT_ID: &str = "00000000-0000-7000-8000-000000000101";

/// An event stream, read one frame at a time.
struct Frames {
    body: BodyDataStream,
    buffer: String,
}

impl Frames {
    fn new(response: Response) -> Self {
        assert_eq!(response.status(), StatusCode::OK);
        Self {
            body: response.into_body().into_data_stream(),
            buffer: String::new(),
        }
    }

    /// The next frame without its closing blank line, or None once the stream has ended.
    /// Fails unless one of them comes within 2 s.
    async fn next(&mut self) -> Option<String> {
        let frame = async {
            loop {
                if let Some(end) = self.buffer.find("\n\n") {
                    let frame = self.buffer[..end].to_owned();
                    self.buffer.drain(..end + 2);
                    return Some(frame);
                }
                let chunk = self.body.next().await?.unwrap();
                self.buffer.push_str(std::str::from_utf8(&chunk).unwrap());
            }
        };
        tokio::time::timeout(Duration::from_secs(2), frame)
            .await
            .expect("a frame or the end within 2 s")
    }

    /// The next frame, which must be an unnamed event, as its id and its JSON data.
    async fn event(&mut self) -> (i64, Value) {
        let frame = self.next().await.expect("an event");
        let (id, data) = frame.split_once('\n').unwrap();
        (
            id.strip_prefix("id: ").unwrap().parse().unwrap(),
            serde_json::from_str(data.strip_prefix("data: ").unwrap()).unwrap(),
        )
    }

    async fn events(&mut self, count: usize) -> Vec<(i64, Value)> {
        let mut events = Vec::new();
        for _ in 0..count {
            events.push(self.event().await);
        }
        events
    }
}

/// Asserts the status and returns the JSON body.
async fn body(response: Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// Starts a run of a new workflow made from the simulated-infrastructure blueprint and
/// returns the run's URL and the run. Nothing executes it, so it stays queued with the single
/// event `run.state queued`.
async fn start_run(admin: &AuthFixture) -> (String, Value) {
    let org = format!("/api/v1/orgs/{}", admin.organization_id);
    let request = json!({ "name": "Provision", "blueprintId": INFRASTRUCTURE_BLUEPRINT_ID });
    let created = body(
        admin
            .post_json(&format!("{org}/workflows"), &request.to_string())
            .await,
        StatusCode::CREATED,
    )
    .await;
    let workflow = format!("{org}/workflows/{}", created["id"].as_str().unwrap());
    let published = admin
        .post_json(&format!("{workflow}/publish"), r#"{"revision":1}"#)
        .await;
    assert_eq!(published.status(), StatusCode::CREATED);
    let input = r#"{"input":{"region":"eu-west-1"}}"#;
    let run = body(
        admin.post_json(&format!("{workflow}/runs"), input).await,
        StatusCode::CREATED,
    )
    .await;
    (format!("{org}/runs/{}", run["id"].as_str().unwrap()), run)
}

/// Opens the event stream at `path` through `app` as `member`, with `Last-Event-ID` when
/// given, as the browser sends it when it reconnects.
async fn open(
    app: &Router,
    member: &AuthFixture,
    path: &str,
    last_event_id: Option<&str>,
) -> Response {
    let mut request = Request::get(path).header(header::COOKIE, member.cookie_header());
    if let Some(last_event_id) = last_event_id {
        request = request.header("last-event-id", last_event_id);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

/// Appends a log line to the run's events through the owner pool, the way the worker
/// functions do, and returns the event as the stream sends it.
async fn append(admin: &AuthFixture, run: &Value, text: &str) -> (i64, Value) {
    let mut payload =
        json!({ "type": "node.log", "nodeId": "vpc", "stream": "stdout", "text": text });
    let run_id: Uuid = run["id"].as_str().unwrap().parse().unwrap();
    let seq: i64 = sqlx::query_scalar("SELECT corqty.append_run_event($1, $2)")
        .bind(run_id)
        .bind(&payload)
        .fetch_one(&admin.owner)
        .await
        .unwrap();
    payload["seq"] = json!(seq);
    (seq, payload)
}

fn queued() -> (i64, Value) {
    (
        1,
        json!({ "type": "run.state", "state": "queued", "seq": 1 }),
    )
}

/// The events that cancelling the queued run appends after seq `last`: each node canceled,
/// in plan order, then the run.
fn cancel_events(run: &Value, last: i64) -> Vec<(i64, Value)> {
    let mut seq = last;
    let mut events: Vec<(i64, Value)> = run["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| {
            seq += 1;
            let event = json!({
                "type": "node.state", "nodeId": node["nodeId"], "state": "canceled",
                "attempt": 0, "seq": seq,
            });
            (seq, event)
        })
        .collect();
    let event = json!({ "type": "run.state", "state": "canceled", "seq": seq + 1 });
    events.push((seq + 1, event));
    events
}

#[tokio::test]
#[serial]
async fn streams_replay_the_events_after_the_cursor() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let (path, run) = start_run(&admin).await;
    let events = format!("{path}/events");
    let second = append(&admin, &run, "first line").await;
    let third = append(&admin, &run, "second line").await;

    let response = open(&admin.app, &admin, &format!("{events}?after=1"), None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers();
    assert_eq!(headers[header::CONTENT_TYPE], "text/event-stream");
    // A single value, with `no-transform`, which keeps proxies such as Next.js from
    // compressing, and so buffering, the stream.
    let cache_control: Vec<_> = headers.get_all(header::CACHE_CONTROL).iter().collect();
    assert_eq!(cache_control, ["no-cache, no-transform"]);
    assert_eq!(headers["x-accel-buffering"], "no");
    let mut frames = Frames::new(response);
    assert_eq!(frames.next().await.as_deref(), Some("retry: 2000"));
    assert_eq!(frames.events(2).await, [second.clone(), third.clone()]);

    // The browser sends Last-Event-ID when it reconnects, and it wins over the ?after= of
    // the URL it reconnects to.
    let mut frames =
        Frames::new(open(&admin.app, &admin, &format!("{events}?after=1"), Some("2")).await);
    assert_eq!(frames.next().await.as_deref(), Some("retry: 2000"));
    assert_eq!(frames.event().await, third);

    // Without a cursor the stream starts at the first event.
    let mut frames = Frames::new(open(&admin.app, &admin, &events, None).await);
    assert_eq!(frames.next().await.as_deref(), Some("retry: 2000"));
    assert_eq!(frames.events(3).await, [queued(), second, third]);
}

#[tokio::test]
#[serial]
async fn open_streams_follow_the_run_live_until_it_finishes() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let (path, run) = start_run(&admin).await;
    // An API process of its own, whose listener wakes its streams.
    let pool = PgPool::connect(API_URL).await.unwrap();
    let state = AppState::new(PgStore::new(pool.clone()), ApiConfig::for_tests());
    let listener = events::spawn_listener(
        API_URL.to_owned(),
        state.wake.clone(),
        state.shutdown.clone(),
    );
    let app = build_router(state.clone());
    let mut frames = Frames::new(open(&app, &admin, &format!("{path}/events"), None).await);
    assert_eq!(frames.next().await.as_deref(), Some("retry: 2000"));
    assert_eq!(frames.event().await, queued());

    // Committed after the replay: the notification wakes the stream.
    let live = append(&admin, &run, "live").await;
    assert_eq!(frames.event().await, live);

    // The listener's connection drops. The listener reconnects and wakes every stream, so
    // an event committed in the meantime still arrives.
    let terminated: Vec<bool> = sqlx::query_scalar(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity
         WHERE usename = current_user AND query LIKE 'LISTEN %'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(terminated, [true]);
    let after_reconnect = append(&admin, &run, "after the reconnect").await;
    assert_eq!(frames.event().await, after_reconnect);

    // The cancel finishes the run, and the stream ends after the run's terminal event.
    let canceled = admin.post(&format!("{path}/cancel")).await;
    assert_eq!(canceled.status(), StatusCode::OK);
    assert_eq!(frames.events(4).await, cancel_events(&run, 3));
    assert_eq!(frames.next().await, None);

    state.shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(2), listener)
        .await
        .expect("the listener stops within 2 s")
        .unwrap();
}

#[tokio::test]
#[serial]
async fn finished_runs_end_their_streams_and_answer_204_once_replayed() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let (path, run) = start_run(&admin).await;
    let canceled = admin.post(&format!("{path}/cancel")).await;
    assert_eq!(canceled.status(), StatusCode::OK);
    let events = format!("{path}/events");

    // A replay that reaches the terminal event of a finished run ends there.
    let mut frames =
        Frames::new(open(&admin.app, &admin, &format!("{events}?after=3"), None).await);
    assert_eq!(frames.next().await.as_deref(), Some("retry: 2000"));
    assert_eq!(frames.events(2).await, cancel_events(&run, 1)[2..]);
    assert_eq!(frames.next().await, None);

    // A client that already has every event gets 204, which stops the browser from
    // reconnecting.
    for (query, last_event_id) in [("?after=5", None), ("?after=0", Some("5"))] {
        let response = open(
            &admin.app,
            &admin,
            &format!("{events}{query}"),
            last_event_id,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    // To another organization's member the run does not exist, under either organization.
    let outsider = admin
        .register_another("outsider@example.com", "Outside")
        .await;
    let elsewhere = format!(
        "/api/v1/orgs/{}/runs/{}/events",
        outsider.organization_id,
        run["id"].as_str().unwrap()
    );
    for path in [&events, &elsewhere] {
        let response = open(&admin.app, &outsider, path, None).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
#[serial]
async fn the_server_stops_on_shutdown_although_a_stream_is_open() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let (path, _) = start_run(&admin).await;
    let pool = PgPool::connect(API_URL).await.unwrap();
    let state = AppState::new(PgStore::new(pool), ApiConfig::for_tests());
    let shutdown = state.shutdown.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(listener, state));

    let mut stream = TcpStream::connect(address).await.unwrap();
    let request = format!(
        "GET {path}/events HTTP/1.1\r\nHost: localhost\r\nCookie: {}\r\n\r\n",
        admin.cookie_header()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut received = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !String::from_utf8_lossy(&received).contains(r#""run.state""#) {
            let mut chunk = [0; 4096];
            let read = stream.read(&mut chunk).await.unwrap();
            assert_ne!(read, 0, "the stream ended early");
            received.extend_from_slice(&chunk[..read]);
        }
    })
    .await
    .expect("the first event within 2 s");

    // axum's graceful shutdown waits for open responses, so the stream must end on its own.
    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .expect("serve returns within 2 s")
        .unwrap()
        .unwrap();
    // The response ended cleanly, with the last chunk of its chunked body.
    stream.read_to_end(&mut received).await.unwrap();
    assert!(String::from_utf8_lossy(&received).ends_with("0\r\n\r\n"));
}
```

In `crates/api/tests/authorization_matrix.rs`:

Replace:

```rust
        (Method::GET, run.clone(), "", Capability::ViewOrganization),
        (
            Method::POST,
            format!("{run}/rerun"),
```

With:

```rust
        (Method::GET, run.clone(), "", Capability::ViewOrganization),
        // The run stays queued, since only the viewer tries to cancel it, so its event
        // stream opens with 200 rather than 204. The test drops it unread.
        (
            Method::GET,
            format!("{run}/events"),
            "",
            Capability::ViewOrganization,
        ),
        (
            Method::POST,
            format!("{run}/rerun"),
```

The test helper `Frames` reads a response body with `Body::into_data_stream()` and splits it into frames at the blank
line. `next` returns a frame without its blank line, or None once the stream has ended, and fails unless one of them
arrives within 2 s. `event` requires an unnamed event, exactly `id: <seq>` then `data: <json>`. The runs come from the
simulated-infrastructure blueprint. Nothing executes them, so each stays queued with the single event
`run.state queued`, and `append` adds `node.log` events through the owner pool the way the worker functions do.

- `streams_replay_the_events_after_the_cursor` checks the headers, the `retry: 2000` first frame, a replay after
  `?after=1`, that `Last-Event-ID: 2` wins over `?after=1`, and that no cursor replays from the first event.
- `open_streams_follow_the_run_live_until_it_finishes` builds an `AppState` on a pool of its own (`API_URL`) and starts
  the listener with `API_URL`, as `main.rs` does. An event appended after the replay arrives within 2 s. Then the test
  terminates the listener's connection with `pg_terminate_backend`; `corqty_api` may signal its own role's sessions,
  and the listener is the only one whose last query is `LISTEN`. An event appended right afterwards still arrives.
  Cancelling the run over HTTP appends `node.state canceled` for each node in plan order and then `run.state
  canceled`, and the stream ends after that event. On the shutdown token the listener stops within 2 s.
- `finished_runs_end_their_streams_and_answer_204_once_replayed` cancels a run (events 1 to 5), replays from 3 and sees
  the stream end after event 5. `?after=5` and `Last-Event-ID: 5` both get 204. An outsider gets 404 for the run under
  the admin's organization and under the outsider's own.
- `the_server_stops_on_shutdown_although_a_stream_is_open` runs the real `serve` on 127.0.0.1:0 and opens the stream
  over raw TCP with the admin's cookie. Once the first event has arrived it cancels `state.shutdown`. `serve` must
  return within 2 s, and the response must end with the last chunk of its chunked body, `0\r\n\r\n`.

The matrix row checks that all four roles get 200 for the stream and the outsider 404 (spec M3-17). `assert_row`
reads only the status, so the stream is dropped unread.

- [ ] **Step 2: Run the tests and verify RED**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-api --test events_http
```

Expected: the test does not compile, because the module and the field do not exist yet:

```text
error[E0432]: unresolved import `corqty_api::events`
  --> crates\api\tests\events_http.rs:11:37
   |
11 | use corqty_api::{config::ApiConfig, events, router::build_router, serve, state::AppState};
   |                                     ^^^^^^ no `events` in the root

error[E0609]: no field `wake` on type `AppState`
   --> crates\api\tests\events_http.rs:223:15
    |
223 |         state.wake.clone(),
    |               ^^^^ unknown field
    |
    = note: available fields are: `store`, `config`, `dummy_password_hash`, `shutdown`
```

The last error line is ``error: could not compile `corqty-api` (test "events_http") due to 2 previous errors``.

```text
cargo test -p corqty-api --test authorization_matrix run_routes_enforce_capabilities
```

Expected: 1 failed. The route does not exist, so the router's fallback answers 404:

```text
thread 'run_routes_enforce_capabilities' (25068) panicked at crates\api\tests\authorization_matrix.rs:74:13:
assertion `left == right` failed: admin GET /api/v1/orgs/d787ca5a-f54d-4f1d-87b4-db64a4fe0f24/runs/01a17773-8d5a-7d18-9e9b-c6fda06f3787/events
  left: 404
 right: 200
```

- [ ] **Step 3: Add the event stream and the listener**

Create `crates/api/src/events.rs`:

```rust
//! Run events as server-sent events. A stream replays the run's events after the client's
//! cursor, then follows the run live: one listener per process turns the database's
//! `corqty_run_events` notifications into wake-ups, and a wake-up for a run makes that run's
//! streams read again. The database stays the only source of the events.

use std::{collections::VecDeque, convert::Infallible, time::Duration};

use axum::{
    Extension,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use corqty_domain::rbac::Capability;
use corqty_persistence::{models::SessionRecord, runs, store::PgStore};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use sqlx::postgres::PgListener;
use tokio::{
    sync::broadcast::{self, error::RecvError},
    task::JoinHandle,
    time::Instant,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{authz::authorize, error::ApiError, extract::ApiPath, state::AppState};

/// The channel that `corqty.notify_run_event()` notifies with the run id.
const CHANNEL: &str = "corqty_run_events";
/// Events read per query.
const PAGE: i64 = 500;
/// How long the browser waits before it reconnects.
const RETRY: Duration = Duration::from_secs(2);
const KEEP_ALIVE: Duration = Duration::from_secs(15);
/// A stream reads again this long after its last read even without a wake-up, in case a
/// notification was lost.
const SAFETY_READ: Duration = Duration::from_secs(30);
/// How long a stream stays open. The browser then reconnects from its last event.
const MAX_STREAM: Duration = Duration::from_secs(600);
const MIN_BACKOFF: Duration = Duration::from_millis(250);
const MAX_BACKOFF: Duration = Duration::from_secs(10);

/// What the listener tells the open streams.
#[derive(Clone, Copy, Debug)]
pub enum Wake {
    /// The run has new events.
    Run(Uuid),
    /// Notifications may have been missed, so every stream reads again.
    Resync,
}

#[derive(Deserialize)]
pub struct EventsQuery {
    after: Option<i64>,
}

/// Streams the run's events after the cursor: the `Last-Event-ID` header that the browser
/// sends when it reconnects, else `?after=`, else 0. Each event is unnamed, with its seq as
/// the id and its payload as the data. A finished run whose events the client already has
/// answers 204, which stops the browser from reconnecting.
pub async fn stream_events(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, run_id)): ApiPath<(Uuid, Uuid)>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let cursor = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse().ok())
        .or(query.after)
        .unwrap_or(0);
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let run = runs::get_run(&mut access.tx, run_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    access.commit().await?;
    if is_finished(&run.summary.status) && cursor >= run.last_event_seq {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    // Subscribed before the first read, so an event committed after that read wakes the
    // stream.
    let follower = Follower {
        store: state.store.clone(),
        user_id: session.user_id,
        organization_id,
        run_id,
        last_seq: cursor,
        wake: state.wake.subscribe(),
        ready: VecDeque::from([Event::default().retry(RETRY)]),
        stale: true,
        read_again_at: Instant::now(),
        finished: false,
    };
    // axum's graceful shutdown waits for open responses, so every stream ends on the token.
    let shutdown = state.shutdown.clone();
    let end = async move {
        tokio::select! {
            () = shutdown.cancelled() => {}
            () = tokio::time::sleep(MAX_STREAM) => {}
        }
    };
    let stream = futures_util::stream::unfold(follower, Follower::advance).take_until(end);
    Ok((
        // Replaces Sse's `no-cache`. `no-transform` keeps proxies such as Next.js from
        // compressing, and so buffering, the stream; `x-accel-buffering` does it for nginx.
        [
            ("cache-control", "no-cache, no-transform"),
            ("x-accel-buffering", "no"),
        ],
        Sse::new(stream).keep_alive(KeepAlive::new().interval(KEEP_ALIVE)),
    )
        .into_response())
}

/// Listens for `corqty_run_events` on a connection of its own and wakes the streams of each
/// notified run. After every (re)connect it wakes all streams, because notifications sent
/// while it was not listening are lost. It retries a failed connect with backoff and stops
/// on `shutdown`.
pub fn spawn_listener(
    database_url: String,
    wake: broadcast::Sender<Wake>,
    shutdown: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = MIN_BACKOFF;
        loop {
            let connected = tokio::select! {
                () = shutdown.cancelled() => return,
                connected = listen(&database_url) => connected,
            };
            let mut listener = match connected {
                Ok(listener) => listener,
                Err(error) => {
                    tracing::warn!(error = %error, "the run event listener could not connect");
                    tokio::select! {
                        () = shutdown.cancelled() => return,
                        () = tokio::time::sleep(backoff) => {}
                    }
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                    continue;
                }
            };
            backoff = MIN_BACKOFF;
            // A send fails only while no stream is open.
            let _ = wake.send(Wake::Resync);
            loop {
                let received = tokio::select! {
                    () = shutdown.cancelled() => return,
                    received = listener.try_recv() => received,
                };
                match received {
                    Ok(Some(notification)) => {
                        if let Ok(run_id) = notification.payload().parse() {
                            let _ = wake.send(Wake::Run(run_id));
                        }
                    }
                    // The connection dropped, and the listener has already reconnected.
                    Ok(None) => {
                        let _ = wake.send(Wake::Resync);
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "the run event listener lost its connection");
                        break;
                    }
                }
            }
        }
    })
}

async fn listen(database_url: &str) -> Result<PgListener, sqlx::Error> {
    let mut listener = PgListener::connect(database_url).await?;
    listener.listen(CHANNEL).await?;
    Ok(listener)
}

/// One stream's place in its run's events.
// ponytail: the session is checked when the stream opens, so a stream outlives a logout by
// at most MAX_STREAM. Every read still checks the membership, through RLS.
struct Follower {
    store: PgStore,
    user_id: Uuid,
    organization_id: Uuid,
    run_id: Uuid,
    last_seq: i64,
    wake: broadcast::Receiver<Wake>,
    /// Frames read but not sent yet.
    ready: VecDeque<Event>,
    /// Whether the run may have events that the stream has not read.
    stale: bool,
    read_again_at: Instant,
    /// Whether the run's terminal `run.state` has been read.
    finished: bool,
}

impl Follower {
    /// The next frame, reading and waiting as needed, or None once the stream is over.
    async fn advance(mut self) -> Option<(Result<Event, Infallible>, Self)> {
        loop {
            if let Some(frame) = self.ready.pop_front() {
                return Some((Ok(frame), self));
            }
            if self.finished {
                return None;
            }
            if self.stale {
                if let Err(error) = self.read().await {
                    // The browser reconnects from the last event it received.
                    tracing::warn!(error = %error, "reading run events failed");
                    return None;
                }
                continue;
            }
            tokio::select! {
                () = tokio::time::sleep_until(self.read_again_at) => self.stale = true,
                wake = self.wake.recv() => match wake {
                    Ok(Wake::Run(run_id)) => self.stale = run_id == self.run_id,
                    Ok(Wake::Resync) | Err(RecvError::Lagged(_)) => self.stale = true,
                    Err(RecvError::Closed) => return None,
                },
            }
        }
    }

    /// Reads the next page of events in a short tenant transaction, so an open stream holds
    /// no connection. After a full page the stream stays stale and reads again at once.
    async fn read(&mut self) -> Result<(), sqlx::Error> {
        let mut tx = self
            .store
            .begin_tenant(self.user_id, self.organization_id)
            .await?;
        let events = runs::events_after(&mut tx, self.run_id, self.last_seq, PAGE).await?;
        tx.commit().await?;
        self.stale = events.len() == PAGE as usize;
        self.read_again_at = Instant::now() + SAFETY_READ;
        for event in events {
            self.finished |= ends_run(&event.payload);
            self.last_seq = event.seq;
            self.ready.push_back(
                Event::default()
                    .id(event.seq.to_string())
                    .data(event.payload.to_string()),
            );
        }
        Ok(())
    }
}

/// Whether a run in this status can change no more.
fn is_finished(status: &str) -> bool {
    !matches!(status, "queued" | "running")
}

/// Whether the event is the run's terminal `run.state`, its last event.
fn ends_run(payload: &Value) -> bool {
    payload["type"] == "run.state" && payload["state"].as_str().is_some_and(is_finished)
}
```

In `crates/api/src/lib.rs`:

Replace:

```rust
pub mod error;
pub mod extract;
```

With:

```rust
pub mod error;
pub mod events;
pub mod extract;
```

In `crates/api/src/state.rs`:

Replace:

```rust
use corqty_persistence::store::PgStore;
use tokio_util::sync::CancellationToken;

use crate::{auth::crypto::PasswordEngine, config::ApiConfig};
```

With:

```rust
use corqty_persistence::store::PgStore;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::{auth::crypto::PasswordEngine, config::ApiConfig, events::Wake};
```

Replace:

```rust
    pub shutdown: CancellationToken,
}
```

With:

```rust
    pub shutdown: CancellationToken,
    /// Wakes the open run event streams. `events::spawn_listener` feeds it from the database.
    pub wake: broadcast::Sender<Wake>,
}
```

Replace:

```rust
            shutdown: CancellationToken::new(),
```

With:

```rust
            shutdown: CancellationToken::new(),
            // A stream that falls 1024 wake-ups behind reads again.
            wake: broadcast::channel(1024).0,
```

In `crates/api/src/router.rs`:

Replace:

```rust
    error::ApiError,
    organizations, runs,
```

With:

```rust
    error::ApiError,
    events, organizations, runs,
```

Replace:

```rust
            post(runs::rerun),
        )
```

With:

```rust
            post(runs::rerun),
        )
        .route(
            "/orgs/{organization_id}/runs/{run_id}/events",
            get(events::stream_events),
        )
```

In `crates/api/src/main.rs`:

Replace:

```rust
use corqty_api::{config::ApiConfig, loops, serve, state::AppState};
```

With:

```rust
use corqty_api::{config::ApiConfig, events, loops, serve, state::AppState};
```

Replace:

```rust
    let background = loops::spawn(&state);
```

With:

```rust
    let background = loops::spawn(&state);
    let run_events = events::spawn_listener(
        state.config.database_url.clone(),
        state.wake.clone(),
        state.shutdown.clone(),
    );
```

Replace:

```rust
    background.join_all().await;
    Ok(())
```

With:

```rust
    background.join_all().await;
    run_events.await?;
    Ok(())
```

Notes on the code:

- The stream is `unfold(follower, Follower::advance)`. `advance` sends what the last read produced, ends after the
  terminal event, reads when the stream is stale, and otherwise waits in a `select!` for the 30 s re-read deadline
  or a wake-up. The deadline is an `Instant` set by each read, so wake-ups for other runs do not keep pushing it back.
  A full page leaves the stream stale, so the next page is read at once.
- The handler reuses `runs::get_run` for the 404 and the 204 decision rather than adding a query: it runs once per
  connection, and it already reads the runs row and checks visibility through RLS.
- Each read is a tenant transaction for the stream's user and organization, so RLS still checks the membership on
  every page. The session itself is checked only when the stream opens; a `// ponytail:` comment records that a
  stream outlives a logout by at most the 10-minute cap.
- `take_until` polls the end future before the inner stream, so a cancelled token ends the stream at its next poll,
  and the token's waker triggers that poll while hyper waits on the body.
- The `cache-control` pair replaces the header that `Sse` sets, because `IntoResponseParts` for header arrays inserts
  rather than appends; the test asserts a single value.
- A failed broadcast send only means that no stream is open, so sends ignore the result.

- [ ] **Step 4: Verify GREEN**

```text
cargo test -p corqty-api --test events_http --test authorization_matrix
```

Expected: `authorization_matrix` 6 passed in about 9 s, and `events_http` 4 passed in about 3.5 s. Five repeated runs of
`events_http` all passed.

Negative controls, each a temporary change to `events.rs` with one named test run, then restored:

- Without `.take_until(end)`: `the_server_stops_on_shutdown_although_a_stream_is_open` fails with
  `serve returns within 2 s: Elapsed(())`.
- Without the `Resync` on `Ok(None)`: `open_streams_follow_the_run_live_until_it_finishes` fails with
  `a frame or the end within 2 s: Elapsed(())`. The event appended right after the connection was terminated is lost
  with its notification, and only the reconnect's `Resync` brings it.
- Ignoring `Wake::Run`: the same test fails the same way at the first live event.
- Without the 204 branch: `finished_runs_end_their_streams_and_answer_204_once_replayed` gets 200, not 204.
- Without the end after the terminal event: the same test times out waiting for the end of the stream.
- Without the `cache-control` pair: `streams_replay_the_events_after_the_cursor` finds `["no-cache"]`.
- With `?after=` winning over `Last-Event-ID`: the same test receives event 2 where it expects 3.

Turning the `break` on a listener error into a `return` does not fail any test: `pg_terminate_backend` reaches the
listener as `Ok(None)`, after sqlx has reconnected. The `Err` branch handles failures such as a database restart,
which Windows reports as `ConnectionReset`.

- [ ] **Step 5: Run the full backend verification**

```text
cargo fmt --check
```

Expected: no output, exit code 0.

```text
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: `Finished` with no warnings.

```text
cargo test --workspace
```

Expected: every suite passes. corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 6,
`catalog_http` 4, `events_http` 4, `loops` 1, `members_http` 3, `organizations_http` 1, `problems_http` 2,
`publish_http` 2, `runs_http` 8, `settings_http` 2, `shutdown` 1, `workflows_http` 4. corqty-domain:
`account_and_rbac` 6, `catalog_rules` 7, `graph_rules` 8, `publish_rules` 11, `run_rules` 8, `runtime_rules` 12.
corqty-executor-mock: `contract` 11. corqty-executor-shell: `contract` 0 on Windows and macOS, 15 on Linux.
corqty-persistence: `auth_store` 2, `catalog_guards` 4, `final_admin` 2, `migrations` 1, `run_store` 8,
`tenant_isolation` 1, `test_reset` 1, `worker_queue` 15, `workflow_catalog` 6. corqty-plugin-host: `registry` 8. That
is 157 tests on Windows and 172 on Linux (CI). No unix-only code changed, so the Docker run of Task 7 is not repeated.

- [ ] **Step 6: Commit**

```text
git add Cargo.lock crates/api
git commit -m "feat: stream run events over SSE with replay"
```

Expected: `git show --stat HEAD` lists exactly the 9 files above (644 insertions, 3 deletions).

## Task 14: Run workers that claim, execute, and complete node runs

**Files:**

- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Create: `crates/worker/Cargo.toml`
- Create: `crates/worker/src/lib.rs`
- Create: `crates/worker/src/main.rs`
- Test: `crates/worker/tests/execution.rs`

Runs can be started, cancelled and watched (Tasks 10 to 13), but nothing executes them yet. This task adds the
worker: the crate `corqty-worker`, a library plus the binary `corqty-worker` (spec M3-02, M3-09). The worker reaches
the database only through `WorkerDb` (Task 9), so the worker role's six definer functions are still all it can
call. It never calls the API (M3-03).

`WorkerConfig` holds the D11 timings. `Default` gives the production values, and the tests shorten them:

| field | default | meaning |
|---|---|---|
| `worker_id` | `Uuid::now_v7()` | keys the leases and the `worker_instances` row, new for every process |
| `capacity` | 4 | attempts that run at once (`CORQTY_WORKER_CAPACITY`) |
| `lease_seconds` | 30 | how long a lease lasts without renewal |
| `heartbeat` | 10 s | how often a running attempt renews its lease and checks for a cancel request |
| `poll` | 500 ms | how often the worker claims work |
| `drain` | 25 s | how long running attempts may finish after shutdown begins |
| `cancel_grace` | 5 s | how long an executor has to return once its attempt is canceled |
| `scratch_root` | `std::env::temp_dir()` | where each attempt gets its scratch directory |

The log flush (every 500 ms, or as soon as 8 KiB waits) and the log channel capacity (64 chunks, D2) are constants.

`corqty_worker::run(config, registry, pool, shutdown)` works like this:

1. It registers the registry's `advertised()` executors and the capacity. A failed registration is the only error
   it returns: a worker that is not registered would claim nothing.
2. On every poll tick it reaps finished attempts and calls `claim(capacity - running, lease_seconds)`. With no
   free slot the limit is 0. That call still counts as the worker's heartbeat, so `queueReason` and
   `live_executors()` keep seeing it while it is busy. Each claimed node run becomes a task in a `JoinSet`.
3. Once `shutdown` is cancelled it stops claiming and gives the running attempts `drain` to finish. It then fires a
   private `stop` token and waits for the attempts, which stop without reporting anything (D12 source 3, D17).

One attempt runs these steps:

1. `start_node_run` returns the job. When it returns None, the lease is gone, and the attempt ends there.
2. If the job says `cancelRequested`, the attempt completes `canceled` without running.
3. The executor is looked up by the claimed `key@version`. The worker claims only executors it advertises, so a
   miss cannot normally happen. If it does, the attempt fails with the non-retryable `executor_missing`.
4. `resolve_inputs` builds the inputs. A failure (`input_missing`, `input_invalid`, `input_too_large`,
   `secret_unavailable`) completes `failed` without `input`, and none of them is retried.
5. The attempt gets its own scratch directory, `corqty-node-*` under `scratch_root`. If the worker cannot create
   it, the attempt fails with `scratch_unavailable`, which may be retried.
6. The executor runs with `execution_id` = the node run id (the same on every attempt), the node's `config`, a fresh
   cancellation token and the log channel. `deadline` is `now + timeoutSeconds`, for information only. The
   worker's `select!` loop runs the executor and takes each log chunk as it arrives. It also flushes logs every
   500 ms and renews the lease every `heartbeat`. Each of these ends the attempt early:
   - The deadline, `timeoutSeconds` after the start (D12 source 1).
   - A heartbeat that reports `cancel_requested` (source 2).
   - A heartbeat with `alive = false`, or an `append_node_log` that returns false: the lease is gone.
   - The `stop` token at the end of the drain (source 3).

   The worker then cancels the attempt's token and waits up to `cancel_grace` for the executor, still taking its
   logs so that it never waits on a full channel. Then it drops the future; the shell executor's guard then
   SIGKILLs its process group. Once the worker has fired the token, the executor's own answer is ignored.
7. The scratch directory is removed on a blocking thread (`TempDir::close`). By then the executor has returned or
   been dropped.
8. The outcome follows from how the executor ended:

   | how it ended | outcome |
   |---|---|
   | output, passes `validate_output` | `succeeded` with `input` and `output` |
   | output, fails `validate_output` | `failed` (`output_invalid` / `output_too_large`, not retryable) with `input` |
   | error | `failed` with `input`, retryable = `is_retryable(code, error.retryable)` |
   | deadline | `failed` `timeout` with `input`, retryable = the executor's `timeout_retryable` |
   | cancel request | `canceled` |
   | lease gone, or stopped after the drain | nothing: the lease expires and lease recovery (Task 12) retries it |

9. The remaining logs are flushed, because completing ends the lease that appending needs. Then the attempt
   completes. A flush that finds the lease gone ends the attempt without completing.

Logs: each attempt counts the bytes its executor sends. Consecutive chunks of the same stream are merged into one
`append_node_log` call, and U+0000, which PostgreSQL text cannot hold, becomes U+FFFD. Chunks are kept until the
attempt has sent more than `MAX_LOG_BYTES_PER_NODE_RUN` bytes, and dropped after that, so a flood never reaches the
database. The chunk that passes the cap is kept whole. The database therefore sees the text go on past its cap,
cuts it at a character boundary and records exactly one `node.log_truncated`. A log text that a database error
rejected is dropped with a WARN (`// ponytail:`).

Every `WorkerDb` call goes through `retrying`, which makes the call again up to 3 times on 40P01 (deadlock) or 40001
(serialization failure), as D11 asks. Other errors are logged at WARN, and the worker carries on:

- claiming: the next tick tries again;
- starting, renewing, completing: the lease expires and recovery handles the node run;
- removing a scratch directory: the directory stays.

The binary sets up tracing (`RUST_LOG`, default `corqty_worker=info`). It reads `DATABASE_URL_WORKER` (required)
and `CORQTY_WORKER_CAPACITY` (optional, a whole number of at least 1). It builds the registry from `MockExecutor` and
`ShellExecutor`; a `RegistryError` makes `main` return it, so the process exits with code 1. The pool gets one
connection per slot plus one to claim with. It logs `the worker is ready` with its id, capacity and executors. On
ctrl-c or SIGTERM it cancels the shutdown token. The signal code is the API's `shutdown_signal`, copied.

The tests in `crates/worker/tests/execution.rs` run real workers (`run` in a spawned task) on the test database's
worker pool. They set up runs through the API pool and check them through the owner pool. Each test:

- creates one organization with an admin;
- publishes its graph as version 1 without publish validation (`insert_version`), so a `corqty.mock.task` config
  can carry `failAttempts` and test fields;
- starts the run with `runs::create_run` and the plan of `plan_run`;
- uses a 3 s lease, heartbeats every 200 ms, a claim every 50 ms, no drain and a 1 s cancel grace;
- gives the workers a scratch root of its own (`TempDir`).

A test executor, `Recording`, behaves exactly like the mock, whose identity it borrows. It records each attempt's
`execution_id`, attempt number and scratch directory. It can also write a config `log` text to stdout first.
jsonb cannot hold U+0000, so such a config writes it as U+2400. `timeline(events, node)` turns a node's events into
lines such as `running 1` or `system: Simulating 500 ms of work.`.

- [ ] **Step 1: Add the crate to the workspace**

In `Cargo.toml`:

Replace:

```toml
  "crates/plugin-sdk",
  "crates/test-support",
```

With:

```toml
  "crates/plugin-sdk",
  "crates/test-support",
  "crates/worker",
```

Create `crates/worker/Cargo.toml`:

```toml
[package]
name = "corqty-worker"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
chrono.workspace = true
corqty-domain = { path = "../domain" }
corqty-executor-mock = { path = "../executor-mock" }
corqty-executor-shell = { path = "../executor-shell" }
corqty-persistence = { path = "../persistence" }
corqty-plugin-host = { path = "../plugin-host" }
corqty-plugin-sdk = { path = "../plugin-sdk" }
serde_json.workspace = true
sqlx.workspace = true
tempfile.workspace = true
tokio.workspace = true
tokio-util.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
uuid.workspace = true

[dev-dependencies]
corqty-test-support = { path = "../test-support" }
futures-util.workspace = true
serial_test = "3"

[lints]
workspace = true
```

The library needs `chrono` for the request's deadline, `corqty-domain` for input resolution, output validation and
retry classification, `corqty-persistence` for `WorkerDb`, the two plugin crates for the registry and the executor
contract, and `serde_json`, `sqlx`, `tempfile`, `tokio`, `tokio-util` (the tokens), `tracing` and `uuid`. The binary
adds the two executors and `tracing-subscriber`. The tests need `futures-util` for `BoxFuture`, to implement the
recording executor.

Create `crates/worker/src/lib.rs`:

```rust
#![forbid(unsafe_code)]
```

The next cargo command (Step 3) updates `Cargo.lock`. It adds the package `corqty-worker` with the dependencies
and dev-dependencies listed above. No other entry changes.

- [ ] **Step 2: Write the failing tests**

Create `crates/worker/tests/execution.rs`:

```rust
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use corqty_domain::{
    diagnostic::Diagnostic, graph::GraphSnapshot, publish::canonical_checksum, run::plan_run,
};
use corqty_executor_mock::MockExecutor;
use corqty_executor_shell::ShellExecutor;
use corqty_persistence::{
    catalog,
    runs::{self, NewRun},
    tenant::begin_tenant,
    worker_queue::recover_expired_leases,
    workflows,
};
use corqty_plugin_host::Registry;
use corqty_plugin_sdk::{
    ExecuteRequest, ExecutionContext, Executor, ExecutorError, ExecutorMetadata, LogChunk,
    LogStream,
};
use corqty_test_support::TestDatabase;
use corqty_worker::{WorkerConfig, run};
use futures_util::future::BoxFuture;
use serde_json::{Map, Value, json};
use serial_test::serial;
use sqlx::{Postgres, Transaction};
use tempfile::TempDir;
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep, timeout};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const INFRASTRUCTURE: &str = "00000000-0000-7000-8000-000000000101";
const DATA_PROCESSING: &str = "00000000-0000-7000-8000-000000000102";

/// The mock executor the recording one delegates to.
static MOCK: MockExecutor = MockExecutor;

/// An organization with an admin who runs the workflows, and the root under which the
/// workers create scratch directories.
struct Fixture {
    database: TestDatabase,
    organization_id: Uuid,
    admin: Uuid,
    scratch_root: TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let database = TestDatabase::reset().await;
        let (organization_id, admin) = (Uuid::new_v4(), Uuid::new_v4());
        sqlx::query("INSERT INTO users(id, email) VALUES ($1, 'admin@example.com')")
            .bind(admin)
            .execute(&database.owner)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO organizations(id, name, slug, created_by) VALUES ($1, 'Acme', 'acme', $2)",
        )
        .bind(organization_id)
        .bind(admin)
        .execute(&database.owner)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships(organization_id, user_id, role) VALUES ($1, $2, 'admin')",
        )
        .bind(organization_id)
        .bind(admin)
        .execute(&database.owner)
        .await
        .unwrap();
        Self {
            database,
            organization_id,
            admin,
            scratch_root: TempDir::new().unwrap(),
        }
    }

    async fn tx(&self) -> Transaction<'static, Postgres> {
        begin_tenant(&self.database.api, self.admin, self.organization_id)
            .await
            .unwrap()
    }

    async fn blueprint(&self, id: &str) -> GraphSnapshot {
        let mut tx = self.tx().await;
        let blueprint = catalog::get_blueprint(&mut tx, id.parse().unwrap()).await;
        tx.commit().await.unwrap();
        blueprint.unwrap().unwrap().graph
    }

    /// Publishes `graph` as version 1 of a new workflow, without publish validation, and
    /// starts a run of it with `input`.
    async fn run(&self, graph: &GraphSnapshot, input: Value) -> Uuid {
        let (organization_id, admin) = (self.organization_id, self.admin);
        let slug = Uuid::new_v4().to_string();
        let mut tx = self.tx().await;
        let workflow =
            workflows::insert_workflow(&mut tx, organization_id, admin, "Flow", &slug, graph, None)
                .await
                .unwrap();
        let checksum = canonical_checksum(graph);
        let version = workflows::insert_version(
            &mut tx,
            organization_id,
            admin,
            workflow.id,
            1,
            graph,
            &checksum,
        )
        .await
        .unwrap();
        let definitions = catalog::definitions_map(&mut tx).await.unwrap();
        let nodes = plan_run(graph, &definitions).unwrap();
        let new = NewRun {
            organization_id,
            actor_user_id: admin,
            workflow_id: workflow.id,
            workflow_version_id: version.id,
            version: 1,
            input: &input,
            nodes: &nodes,
            rerun_of: None,
        };
        let run = runs::create_run(&mut tx, &new).await.unwrap();
        tx.commit().await.unwrap();
        run.id
    }

    /// Short timings for tests: a 3 s lease renewed every 200 ms, a claim every 50 ms, no
    /// drain and a 1 s cancel grace.
    fn config(&self, capacity: i32) -> WorkerConfig {
        WorkerConfig {
            capacity,
            lease_seconds: 3,
            heartbeat: Duration::from_millis(200),
            poll: Duration::from_millis(50),
            drain: Duration::ZERO,
            cancel_grace: Duration::from_secs(1),
            scratch_root: self.scratch_root.path().to_owned(),
            ..WorkerConfig::default()
        }
    }

    /// Starts a worker with `executors` on the worker role's pool.
    fn start(&self, config: WorkerConfig, executors: Vec<Arc<dyn Executor>>) -> Running {
        let id = config.worker_id;
        let registry = Registry::new(executors).unwrap();
        let shutdown = CancellationToken::new();
        let pool = self.database.worker.clone();
        let task = tokio::spawn(run(config, registry, pool, shutdown.clone()));
        Running { id, shutdown, task }
    }

    async fn status(&self, run: Uuid) -> String {
        sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
            .bind(run)
            .fetch_one(&self.database.owner)
            .await
            .unwrap()
    }

    /// Waits for the run to finish and returns its status.
    async fn finished(&self, run: Uuid) -> String {
        eventually(|| async move {
            let status = self.status(run).await;
            (!matches!(status.as_str(), "queued" | "running")).then_some(status)
        })
        .await
    }

    /// Waits until a worker has started an attempt of the node.
    async fn started(&self, run: Uuid, node_id: &str) {
        eventually(|| async move {
            let started: bool = sqlx::query_scalar(
                "SELECT state = 'running' AND started_at IS NOT NULL
                 FROM node_runs WHERE run_id = $1 AND node_id = $2",
            )
            .bind(run)
            .bind(node_id)
            .fetch_one(&self.database.owner)
            .await
            .unwrap();
            started.then_some(())
        })
        .await;
    }

    /// Each node's state, attempt, input, output and error, by node id.
    async fn nodes(&self, run: Uuid) -> Value {
        sqlx::query_scalar(
            "SELECT jsonb_object_agg(node_id, jsonb_build_object(
                 'state', state, 'attempt', attempt, 'input', input, 'output', output,
                 'error', error
             ))
             FROM node_runs WHERE run_id = $1",
        )
        .bind(run)
        .fetch_one(&self.database.owner)
        .await
        .unwrap()
    }

    /// The run's event payloads in sequence order.
    async fn events(&self, run: Uuid) -> Vec<Value> {
        sqlx::query_scalar("SELECT payload FROM run_events WHERE run_id = $1 ORDER BY seq")
            .bind(run)
            .fetch_all(&self.database.owner)
            .await
            .unwrap()
    }

    /// How many entries the scratch root holds.
    fn scratch_entries(&self) -> usize {
        std::fs::read_dir(self.scratch_root.path()).unwrap().count()
    }
}

/// A worker running in the background.
struct Running {
    id: Uuid,
    shutdown: CancellationToken,
    task: JoinHandle<Result<(), sqlx::Error>>,
}

impl Running {
    /// Shuts the worker down and waits for it to return.
    async fn stop(self) {
        self.shutdown.cancel();
        let stopped = timeout(Duration::from_secs(5), self.task).await;
        stopped
            .expect("the worker stops within 5 s")
            .unwrap()
            .unwrap();
    }
}

/// What the executor saw of one attempt.
struct Seen {
    execution_id: Uuid,
    attempt: u32,
    scratch_dir: PathBuf,
    scratch_existed: bool,
}

/// The mock executor that records each attempt. A config `log` text is first written to
/// stdout `repeat` times (once by default); jsonb cannot hold U+0000, so the config spells it
/// U+2400.
#[derive(Clone, Default)]
struct Recording {
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Recording {
    fn seen(&self) -> Vec<Seen> {
        std::mem::take(&mut *self.seen.lock().unwrap())
    }
}

impl Executor for Recording {
    fn metadata(&self) -> ExecutorMetadata {
        MOCK.metadata()
    }

    fn validate(&self, config: &Value) -> Vec<Diagnostic> {
        MOCK.validate(config)
    }

    fn execute<'a>(
        &'a self,
        request: ExecuteRequest,
        context: &'a ExecutionContext,
    ) -> BoxFuture<'a, Result<Map<String, Value>, ExecutorError>> {
        self.seen.lock().unwrap().push(Seen {
            execution_id: request.execution_id,
            attempt: request.attempt,
            scratch_dir: context.scratch_dir.clone(),
            scratch_existed: context.scratch_dir.is_dir(),
        });
        Box::pin(async move {
            if let Some(text) = request.config["log"].as_str() {
                for _ in 0..request.config["repeat"].as_u64().unwrap_or(1) {
                    let text = text.replace('\u{2400}', "\0");
                    let chunk = LogChunk {
                        stream: LogStream::Stdout,
                        text,
                    };
                    let _ = context.logs.send(chunk).await;
                }
            }
            MOCK.execute(request, context).await
        })
    }
}

fn mock() -> Vec<Arc<dyn Executor>> {
    vec![Arc::new(MockExecutor)]
}

/// Polls `check` every 20 ms until it returns a value, for up to 10 s.
async fn eventually<T, F>(mut check: impl FnMut() -> F) -> T
where
    F: Future<Output = Option<T>>,
{
    let polling = async {
        loop {
            if let Some(value) = check().await {
                return value;
            }
            sleep(Duration::from_millis(20)).await;
        }
    };
    timeout(Duration::from_secs(10), polling)
        .await
        .expect("the condition holds within 10 s")
}

/// `base` with the top-level fields of `changes` replaced.
fn merge(mut base: Value, changes: Value) -> Value {
    let Value::Object(changes) = changes else {
        panic!("not an object: {changes}");
    };
    base.as_object_mut().unwrap().extend(changes);
    base
}

/// A `corqty.mock.task` node with one attempt whose mock returns `{"result": <id>}` at once,
/// with `config` replacing fields of that mock config.
fn task(id: &str, config: Value) -> Value {
    let mock = json!({
        "mode": "fixed",
        "output": {"result": id},
        "delayMs": 0,
        "failureMessage": null
    });
    json!({
        "id": id,
        "definition": {"key": "corqty.mock.task", "version": 1},
        "displayName": id,
        "position": {"x": 0, "y": 0},
        "config": merge(mock, config),
        "inputs": {},
        "retry": {"maxAttempts": 1},
        "timeoutSeconds": 60
    })
}

/// An edge from the result of `source` into the input of `target`.
fn edge(source: &str, target: &str) -> Value {
    json!({
        "id": format!("{source}-{target}"),
        "source": {"nodeId": source, "port": "result"},
        "target": {"nodeId": target, "port": "input"}
    })
}

fn graph(nodes: &[Value], edges: &[Value]) -> GraphSnapshot {
    serde_json::from_value(json!({
        "schemaVersion": 1,
        "workflowInputSchema": {"type": "object"},
        "nodes": nodes,
        "edges": edges,
        "viewport": {"x": 0, "y": 0, "zoom": 1}
    }))
    .unwrap()
}

/// What happened to one node, in event order: `<state> <attempt>` for a state change,
/// `<stream>: <text>` for log text, and the event type otherwise.
fn timeline(events: &[Value], node_id: &str) -> Vec<String> {
    let text = |event: &Value, key: &str| event[key].as_str().unwrap().to_owned();
    events
        .iter()
        .filter(|event| event["nodeId"] == node_id)
        .map(|event| match event["type"].as_str().unwrap() {
            "node.state" => format!("{} {}", text(event, "state"), event["attempt"]),
            "node.log" => format!("{}: {}", text(event, "stream"), text(event, "text")),
            other => other.to_owned(),
        })
        .collect()
}

/// The state changes in a timeline, without its log lines.
fn changes(timeline: &[String]) -> Vec<&str> {
    timeline
        .iter()
        .map(String::as_str)
        .filter(|entry| !entry.contains(": "))
        .collect()
}

#[tokio::test]
#[serial]
async fn both_starter_blueprints_run_to_completion() {
    let fixture = Fixture::new().await;
    let infrastructure = fixture.blueprint(INFRASTRUCTURE).await;
    let infrastructure = fixture
        .run(&infrastructure, json!({"region": "eu-west-1"}))
        .await;
    let data = fixture.blueprint(DATA_PROCESSING).await;
    let data = fixture.run(&data, json!({"payload": {"a": 1}})).await;

    let worker = fixture.start(fixture.config(4), mock());
    assert_eq!(fixture.finished(infrastructure).await, "succeeded");
    assert_eq!(fixture.finished(data).await, "succeeded");
    worker.stop().await;

    // vpc's output reaches the inputs of rds and deploy.
    assert_eq!(
        fixture.nodes(infrastructure).await,
        json!({
            "vpc": {
                "state": "succeeded", "attempt": 1, "input": {"region": "eu-west-1"},
                "output": {"vpcId": "vpc-0simulated"}, "error": null
            },
            "rds": {
                "state": "succeeded", "attempt": 1, "input": {"vpcId": "vpc-0simulated"},
                "output": {"endpoint": "db.simulated.internal:5432"}, "error": null
            },
            "deploy": {
                "state": "succeeded", "attempt": 1, "input": {"vpcId": "vpc-0simulated"},
                "output": {"url": "https://service.simulated.internal"}, "error": null
            }
        })
    );
    assert_eq!(
        fixture.nodes(data).await,
        json!({
            "normalize": {
                "state": "succeeded", "attempt": 1, "input": {"payload": {"a": 1}},
                "output": {"normalized": {"records": 3}}, "error": null
            },
            "summary": {
                "state": "succeeded", "attempt": 1, "input": {"normalized": {"records": 3}},
                "output": {"summary": "3 records processed"}, "error": null
            }
        })
    );

    // Each attempt's log line is flushed before the attempt completes.
    let events = fixture.events(infrastructure).await;
    let log = "system: Simulating 500 ms of work.";
    assert_eq!(timeline(&events, "vpc"), ["running 1", log, "succeeded 1"]);
    for node_id in ["rds", "deploy"] {
        let expected = ["queued 0", "running 1", log, "succeeded 1"];
        assert_eq!(timeline(&events, node_id), expected);
    }
    assert_eq!(
        events.first(),
        Some(&json!({"type": "run.state", "state": "queued", "seq": 1}))
    );
    assert_eq!(events.last().unwrap()["state"], "succeeded");
    assert_eq!(fixture.scratch_entries(), 0);
}

#[tokio::test]
#[serial]
async fn three_workers_with_one_slot_each_run_the_branches_in_parallel() {
    let fixture = Fixture::new().await;
    let graph = fixture.blueprint(INFRASTRUCTURE).await;
    let run = fixture.run(&graph, json!({"region": "eu-west-1"})).await;

    let workers: Vec<Running> = (0..3)
        .map(|_| fixture.start(fixture.config(1), mock()))
        .collect();
    // rds and deploy run at the same time, so two different workers hold their leases.
    let owner = &fixture.database.owner;
    let leased_by: Vec<Uuid> = eventually(|| async move {
        let leased_by: Vec<Uuid> = sqlx::query_scalar(
            "SELECT leased_by FROM node_runs
             WHERE run_id = $1 AND node_id IN ('rds', 'deploy') AND state = 'running'",
        )
        .bind(run)
        .fetch_all(owner)
        .await
        .unwrap();
        (leased_by.len() == 2).then_some(leased_by)
    })
    .await;
    assert_ne!(leased_by[0], leased_by[1]);
    assert!(
        leased_by
            .iter()
            .all(|id| workers.iter().any(|worker| worker.id == *id))
    );
    assert_eq!(fixture.finished(run).await, "succeeded");
    for worker in workers {
        worker.stop().await;
    }

    let overlap: bool = sqlx::query_scalar(
        "SELECT rds.started_at < deploy.finished_at AND deploy.started_at < rds.finished_at
         FROM node_runs rds JOIN node_runs deploy USING (run_id)
         WHERE rds.run_id = $1 AND rds.node_id = 'rds' AND deploy.node_id = 'deploy'",
    )
    .bind(run)
    .fetch_one(&fixture.database.owner)
    .await
    .unwrap();
    assert!(overlap);
    // Each branch was released once and ran once.
    let events = fixture.events(run).await;
    for node_id in ["rds", "deploy"] {
        let entries = timeline(&events, node_id);
        assert_eq!(changes(&entries), ["queued 0", "running 1", "succeeded 1"]);
    }
}

#[tokio::test]
#[serial]
async fn a_failed_node_skips_only_its_descendants() {
    let fixture = Fixture::new().await;
    let broken = task("broken", json!({"failureMessage": "The mock broke."}));
    let nodes = [broken, task("child", json!({})), task("other", json!({}))];
    let run = fixture
        .run(&graph(&nodes, &[edge("broken", "child")]), json!({}))
        .await;

    let worker = fixture.start(fixture.config(4), mock());
    assert_eq!(fixture.finished(run).await, "failed");
    worker.stop().await;

    let error = json!({"code": "mock_failure", "message": "The mock broke.", "retryable": false});
    assert_eq!(
        fixture.nodes(run).await,
        json!({
            "broken": {"state": "failed", "attempt": 1, "input": {}, "output": null, "error": error},
            "child": {"state": "skipped", "attempt": 0, "input": null, "output": null, "error": null},
            "other": {
                "state": "succeeded", "attempt": 1, "input": {}, "output": {"result": "other"},
                "error": null
            }
        })
    );
}

#[tokio::test]
#[serial]
async fn a_retryable_failure_runs_again_with_the_same_execution_id_and_a_new_scratch_directory() {
    let fixture = Fixture::new().await;
    let flaky = merge(
        task("flaky", json!({"failAttempts": 1})),
        json!({"retry": {"maxAttempts": 2}}),
    );
    let run = fixture.run(&graph(&[flaky], &[]), json!({})).await;

    let recording = Recording::default();
    let worker = fixture.start(fixture.config(1), vec![Arc::new(recording.clone())]);
    // The retry waits 2.0 to 2.4 s.
    assert_eq!(fixture.finished(run).await, "succeeded");
    worker.stop().await;

    let node_run_id: Uuid = sqlx::query_scalar("SELECT id FROM node_runs WHERE run_id = $1")
        .bind(run)
        .fetch_one(&fixture.database.owner)
        .await
        .unwrap();
    let seen = recording.seen();
    let attempts: Vec<(Uuid, u32, bool)> = seen
        .iter()
        .map(|seen| (seen.execution_id, seen.attempt, seen.scratch_existed))
        .collect();
    assert_eq!(attempts, [(node_run_id, 1, true), (node_run_id, 2, true)]);
    assert_ne!(seen[0].scratch_dir, seen[1].scratch_dir);
    for seen in &seen {
        assert!(seen.scratch_dir.starts_with(fixture.scratch_root.path()));
        assert!(!seen.scratch_dir.exists());
    }
    assert_eq!(fixture.scratch_entries(), 0);

    let events = fixture.events(run).await;
    let log = "system: Simulating 0 ms of work.";
    let expected = [
        "running 1",
        log,
        "retry_wait 1",
        "running 2",
        log,
        "succeeded 2",
    ];
    assert_eq!(timeline(&events, "flaky"), expected);
    let retry = events.iter().find(|event| event["state"] == "retry_wait");
    assert_eq!(
        retry.unwrap()["error"],
        json!({
            "code": "mock_retryable_failure",
            "message": "Attempt 1 fails because failAttempts is 1.",
            "retryable": true
        })
    );
}

#[tokio::test]
#[serial]
async fn echo_returns_the_resolved_inputs_as_the_result() {
    let fixture = Fixture::new().await;
    let echo = merge(
        task("echo", json!({"mode": "echo", "output": {}})),
        json!({"inputs": {"input": {"kind": "literal", "value": {"name": "corqty"}}}}),
    );
    let run = fixture.run(&graph(&[echo], &[]), json!({})).await;

    let worker = fixture.start(fixture.config(1), mock());
    assert_eq!(fixture.finished(run).await, "succeeded");
    worker.stop().await;

    assert_eq!(
        fixture.nodes(run).await,
        json!({
            "echo": {
                "state": "succeeded", "attempt": 1, "input": {"input": {"name": "corqty"}},
                "output": {"result": {"input": {"name": "corqty"}}}, "error": null
            }
        })
    );
}

#[tokio::test]
#[serial]
async fn a_cancel_request_stops_the_running_attempt() {
    let fixture = Fixture::new().await;
    let slow = task("slow", json!({"delayMs": 10_000}));
    let run = fixture.run(&graph(&[slow], &[]), json!({})).await;

    let worker = fixture.start(fixture.config(1), mock());
    fixture.started(run, "slow").await;
    let canceled = Instant::now();
    let mut tx = fixture.tx().await;
    runs::cancel_run(&mut tx, fixture.organization_id, fixture.admin, run)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    // The next heartbeat reports the request, and the mock returns as soon as it is told.
    assert_eq!(fixture.finished(run).await, "canceled");
    assert!(canceled.elapsed() < Duration::from_secs(2));
    worker.stop().await;

    assert_eq!(
        fixture.nodes(run).await,
        json!({
            "slow": {"state": "canceled", "attempt": 1, "input": null, "output": null, "error": null}
        })
    );
    let events = fixture.events(run).await;
    let expected = [
        "running 1",
        "system: Simulating 10000 ms of work.",
        "canceled 1",
    ];
    assert_eq!(timeline(&events, "slow"), expected);
    assert_eq!(fixture.scratch_entries(), 0);
}

#[tokio::test]
#[serial]
async fn an_attempt_past_its_deadline_fails_with_timeout() {
    let fixture = Fixture::new().await;
    let slow = merge(
        task("slow", json!({"delayMs": 3000})),
        json!({"retry": {"maxAttempts": 2}, "timeoutSeconds": 1}),
    );
    let run = fixture.run(&graph(&[slow], &[]), json!({})).await;

    let worker = fixture.start(fixture.config(1), mock());
    fixture.started(run, "slow").await;
    let started = Instant::now();
    assert_eq!(fixture.finished(run).await, "failed");
    assert!(started.elapsed() < Duration::from_secs(2));
    worker.stop().await;

    // The mock does not retry timeouts, although an attempt is left.
    let error = json!({
        "code": "timeout",
        "message": "The attempt did not finish within its timeout of 1 s.",
        "retryable": false
    });
    assert_eq!(
        fixture.nodes(run).await,
        json!({
            "slow": {"state": "failed", "attempt": 1, "input": {}, "output": null, "error": error}
        })
    );
    let events = fixture.events(run).await;
    let expected = [
        "running 1",
        "system: Simulating 3000 ms of work.",
        "failed 1",
    ];
    assert_eq!(timeline(&events, "slow"), expected);
}

#[tokio::test]
#[serial]
async fn an_output_that_breaks_its_schema_fails_without_retry() {
    let fixture = Fixture::new().await;
    let vpc = merge(
        task("vpc", json!({"output": {"vpcId": "subnet-1"}})),
        json!({
            "definition": {"key": "corqty.simulated.create-vpc", "version": 1},
            "inputs": {"region": {"kind": "literal", "value": "eu-west-1"}},
            "retry": {"maxAttempts": 3}
        }),
    );
    let run = fixture.run(&graph(&[vpc], &[]), json!({})).await;

    let worker = fixture.start(fixture.config(1), mock());
    assert_eq!(fixture.finished(run).await, "failed");
    worker.stop().await;

    let error = json!({
        "code": "output_invalid",
        "message": "The output at /vpcId does not match its schema: value does not match \"^vpc-\".",
        "retryable": false
    });
    assert_eq!(
        fixture.nodes(run).await,
        json!({
            "vpc": {
                "state": "failed", "attempt": 1, "input": {"region": "eu-west-1"},
                "output": null, "error": error
            }
        })
    );
}

#[tokio::test]
#[serial]
async fn a_worker_that_shuts_down_leaves_its_attempt_to_lease_recovery() {
    let fixture = Fixture::new().await;
    let slow = merge(
        task("slow", json!({"delayMs": 2000})),
        json!({"retry": {"maxAttempts": 2}}),
    );
    let run = fixture.run(&graph(&[slow], &[]), json!({})).await;

    let first = fixture.start(fixture.config(1), mock());
    let first_id = first.id;
    fixture.started(run, "slow").await;
    // With no drain, the worker stops the attempt at once and reports nothing about it.
    let stopping = Instant::now();
    first.stop().await;
    assert!(stopping.elapsed() < Duration::from_secs(1));
    let lease: (String, Option<Uuid>, bool) = sqlx::query_as(
        "SELECT state, leased_by, lease_expires_at > now() FROM node_runs WHERE run_id = $1",
    )
    .bind(run)
    .fetch_one(&fixture.database.owner)
    .await
    .unwrap();
    assert_eq!(lease, ("running".to_owned(), Some(first_id), true));
    let events = fixture.events(run).await;
    let reported: Vec<&Value> = events
        .iter()
        .filter(|event| event["type"] != "node.log")
        .collect();
    assert_eq!(
        reported,
        [
            &json!({"type": "run.state", "state": "queued", "seq": 1}),
            &json!({"type": "run.state", "state": "running", "seq": 2}),
            &json!({"type": "node.state", "nodeId": "slow", "state": "running", "attempt": 1, "seq": 3}),
        ]
    );
    assert_eq!(fixture.scratch_entries(), 0);

    // The lease expires, recovery schedules a retry, and another worker runs it.
    let owner = &fixture.database.owner;
    sqlx::query(
        "UPDATE node_runs SET lease_expires_at = now() - interval '1 second' WHERE run_id = $1",
    )
    .bind(run)
    .execute(owner)
    .await
    .unwrap();
    assert_eq!(
        recover_expired_leases(&fixture.database.api).await.unwrap(),
        1
    );
    // Due at once instead of after the backoff.
    sqlx::query("UPDATE node_runs SET available_at = now() WHERE run_id = $1")
        .bind(run)
        .execute(owner)
        .await
        .unwrap();
    let second = fixture.start(fixture.config(1), mock());
    assert_eq!(fixture.finished(run).await, "succeeded");
    second.stop().await;

    assert_eq!(
        fixture.nodes(run).await,
        json!({
            "slow": {
                "state": "succeeded", "attempt": 2, "input": {}, "output": {"result": "slow"},
                "error": null
            }
        })
    );
    let events = fixture.events(run).await;
    let entries = timeline(&events, "slow");
    let expected = ["running 1", "retry_wait 1", "running 2", "succeeded 2"];
    assert_eq!(changes(&entries), expected);
    let retry = events.iter().find(|event| event["state"] == "retry_wait");
    assert_eq!(retry.unwrap()["error"]["code"], "lease_expired");
    assert_eq!(fixture.scratch_entries(), 0);
}

#[tokio::test]
#[serial]
async fn a_worker_without_the_executor_leaves_the_node_queued() {
    let fixture = Fixture::new().await;
    let run = fixture
        .run(&graph(&[task("task", json!({}))], &[]), json!({}))
        .await;

    let worker = fixture.start(fixture.config(1), vec![Arc::new(ShellExecutor)]);
    // The worker is alive and claims every 50 ms, but only shell nodes.
    let api = &fixture.database.api;
    eventually(|| async move {
        let live: Vec<String> = sqlx::query_scalar("SELECT corqty.live_executors()")
            .fetch_one(api)
            .await
            .unwrap();
        (live == ["shell@1.0.0"]).then_some(())
    })
    .await;
    sleep(Duration::from_millis(500)).await;
    worker.stop().await;

    assert_eq!(
        fixture.nodes(run).await,
        json!({
            "task": {"state": "queued", "attempt": 0, "input": null, "output": null, "error": null}
        })
    );
    assert_eq!(
        fixture.events(run).await,
        [json!({"type": "run.state", "state": "queued", "seq": 1})]
    );
}

#[tokio::test]
#[serial]
async fn logs_reach_the_events_in_order_with_nul_replaced() {
    let fixture = Fixture::new().await;
    let chatty = task("chatty", json!({"log": "first\u{2400}line\n"}));
    let run = fixture.run(&graph(&[chatty], &[]), json!({})).await;

    let worker = fixture.start(fixture.config(1), vec![Arc::new(Recording::default())]);
    assert_eq!(fixture.finished(run).await, "succeeded");
    worker.stop().await;

    let events = fixture.events(run).await;
    let expected = [
        "running 1",
        "stdout: first\u{fffd}line\n",
        "system: Simulating 0 ms of work.",
        "succeeded 1",
    ];
    assert_eq!(timeline(&events, "chatty"), expected);
}

#[tokio::test]
#[serial]
async fn a_flood_of_logs_is_cut_at_the_cap_with_one_truncation_event() {
    let fixture = Fixture::new().await;
    // 161 chunks of 64 KiB: one chunk more than the 10 MiB cap.
    let flood = task("flood", json!({"log": "a".repeat(65_536), "repeat": 161}));
    let run = fixture.run(&graph(&[flood], &[]), json!({})).await;

    let worker = fixture.start(fixture.config(1), vec![Arc::new(Recording::default())]);
    assert_eq!(fixture.finished(run).await, "succeeded");
    worker.stop().await;

    let logged: (i64, bool) =
        sqlx::query_as("SELECT log_bytes, log_truncated FROM node_runs WHERE run_id = $1")
            .bind(run)
            .fetch_one(&fixture.database.owner)
            .await
            .unwrap();
    assert_eq!(logged, (10_485_760, true));
    let events = fixture.events(run).await;
    let text: usize = events
        .iter()
        .filter(|event| event["type"] == "node.log")
        .map(|event| event["text"].as_str().unwrap().len())
        .sum();
    assert_eq!(text, 10_485_760);
    let truncations = events
        .iter()
        .filter(|event| event["type"] == "node.log_truncated")
        .count();
    assert_eq!(truncations, 1);
}
```

The tests cover the Task 14 list:

- `both_starter_blueprints_run_to_completion`:
  - vpc's output `vpcId` becomes the input of rds and deploy;
  - the data-processing `summary` outputs `{"summary": "3 records processed"}`;
  - each node's mock log line comes before its completion;
  - the scratch root is empty afterwards.
- `three_workers_with_one_slot_each_run_the_branches_in_parallel`: rds and deploy are running at the same moment
  under two different `leased_by` ids, their `[startedAt, finishedAt]` intervals overlap, and each was released
  (`queued 0`) once.
- `a_failed_node_skips_only_its_descendants`: the failure keeps its error, the child is skipped, and an unrelated
  node succeeds.
- `a_retryable_failure_runs_again_with_the_same_execution_id_and_a_new_scratch_directory`:
  - `failAttempts: 1` with 2 attempts fails once, retryably, and succeeds on attempt 2;
  - the executor saw the node run id as `executionId` both times, and a different scratch directory each time;
  - both directories existed during their attempt and are gone afterwards.
- `echo_returns_the_resolved_inputs_as_the_result`: `{"result": {"input": {"name": "corqty"}}}`.
- `a_cancel_request_stops_the_running_attempt`: a 10 s delay ends `canceled` within 2 s of `cancel_run`, without an
  error, and its log line comes first.
- `an_attempt_past_its_deadline_fails_with_timeout`: `timeoutSeconds` 1 against a 3 s delay with 2 attempts fails
  at attempt 1 with `timeout`, not retryable (the mock's `timeout_retryable` is false), within 2 s.
- `an_output_that_breaks_its_schema_fails_without_retry`: a create-vpc output `{"vpcId": "subnet-1"}` with 3
  attempts fails at once with `output_invalid`, keeping the input.
- `a_worker_that_shuts_down_leaves_its_attempt_to_lease_recovery`:
  - with no drain, `stop` returns within 1 s during a 2 s delay;
  - the node is still `running` under the first worker's unexpired lease, with no event after its `running` (log
    lines aside);
  - after the owner pool expires the lease, `recover_expired_leases` schedules a retry, and a second worker finishes
    it as attempt 2.
- `a_worker_without_the_executor_leaves_the_node_queued`: a shell-only worker is live (`live_executors()`) but
  leaves the mock node queued at attempt 0, with no event.
- `logs_reach_the_events_in_order_with_nul_replaced`: a stdout line with U+0000 arrives as U+FFFD, before the mock's
  system line, before the completion.
- `a_flood_of_logs_is_cut_at_the_cap_with_one_truncation_event`: 161 chunks of 64 KiB, one chunk past the cap, store
  exactly 10,485,760 bytes and write one `node.log_truncated`.

- [ ] **Step 3: Run the tests and verify RED**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-worker --test execution
```

Expected: the test does not compile, because the crate exports nothing yet:

```text
error[E0432]: unresolved imports `corqty_worker::WorkerConfig`, `corqty_worker::run`
  --> crates\worker\tests\execution.rs:23:21
   |
23 | use corqty_worker::{WorkerConfig, run};
   |                     ^^^^^^^^^^^^  ^^^ no `run` in the root
   |                     |
   |                     no `WorkerConfig` in the root
```

The last line is ``error: could not compile `corqty-worker` (test "execution") due to 1 previous error``.

- [ ] **Step 4: Implement the worker**

Replace the whole of `crates/worker/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

//! The worker (spec §10.2). It claims the node runs whose executor it has, runs each attempt
//! under a lease that it keeps renewing, and reports the attempt's logs and outcome. It
//! reaches the database only through the worker role's `corqty` functions.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeDelta, Utc};
use corqty_domain::run::is_retryable;
use corqty_domain::runtime::{
    MAX_LOG_BYTES_PER_NODE_RUN, NodeFailure, resolve_inputs, validate_output,
};
use corqty_persistence::worker_queue::{ClaimedJob, Job, NodeError, Outcome, WorkerDb};
use corqty_plugin_host::Registry;
use corqty_plugin_sdk::{
    ExecuteRequest, ExecutionContext, Executor, ExecutorError, LogChunk, LogStream,
};
use serde_json::{Map, Value};
use sqlx::PgPool;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::{Instant, interval_at, sleep_until, timeout};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// A running attempt's logs go to the database at least this often...
const LOG_FLUSH_INTERVAL: Duration = Duration::from_millis(500);
/// ...and as soon as this much text waits.
const LOG_FLUSH_BYTES: usize = 8 * 1024;
/// How many log chunks an executor may send before it waits for the worker.
const LOG_CHANNEL_CAPACITY: usize = 64;

/// How a worker runs. `Default` holds the production values.
#[derive(Clone, Debug)]
pub struct WorkerConfig {
    /// Keys the worker's leases and its `worker_instances` row: new for every process.
    pub worker_id: Uuid,
    /// How many attempts run at once.
    pub capacity: i32,
    /// How long a lease lasts without renewal.
    pub lease_seconds: i32,
    /// How often a running attempt renews its lease and checks for a cancel request.
    pub heartbeat: Duration,
    /// How often the worker claims work. Every claim, even with no free slot, shows the
    /// worker alive.
    pub poll: Duration,
    /// How long running attempts may go on finishing after shutdown begins.
    pub drain: Duration,
    /// How long an executor has to return once its attempt is canceled. Then it is dropped.
    pub cancel_grace: Duration,
    /// Where each attempt gets a scratch directory of its own.
    pub scratch_root: PathBuf,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            worker_id: Uuid::now_v7(),
            capacity: 4,
            lease_seconds: 30,
            heartbeat: Duration::from_secs(10),
            poll: Duration::from_millis(500),
            drain: Duration::from_secs(25),
            cancel_grace: Duration::from_secs(5),
            scratch_root: std::env::temp_dir(),
        }
    }
}

/// Registers the worker's executors, then claims and runs attempts until `shutdown` is
/// cancelled. It then stops claiming and gives running attempts `config.drain` to finish.
/// Attempts still running after that are stopped and never reported: their leases expire and
/// lease recovery retries them. Only a failed registration is returned; later database
/// errors are logged and tried again.
pub async fn run(
    config: WorkerConfig,
    registry: Registry,
    pool: PgPool,
    shutdown: CancellationToken,
) -> Result<(), sqlx::Error> {
    let db = WorkerDb::new(pool, config.worker_id);
    let executors = registry.advertised();
    retrying(|| db.register(&executors, config.capacity)).await?;
    let worker = Arc::new(Worker {
        config,
        registry,
        db,
        stop: CancellationToken::new(),
    });
    let mut attempts = JoinSet::new();
    let mut poll = tokio::time::interval(worker.config.poll);
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            _ = poll.tick() => {}
        }
        while attempts.try_join_next().is_some() {}
        let running = i32::try_from(attempts.len()).unwrap_or(i32::MAX);
        let free = worker.config.capacity - running;
        match retrying(|| worker.db.claim(free, worker.config.lease_seconds)).await {
            Ok(claimed) => {
                for job in claimed {
                    attempts.spawn(Arc::clone(&worker).attempt(job));
                }
            }
            Err(error) => tracing::warn!(error = %error, "claiming node runs failed"),
        }
    }

    tracing::info!(
        running = attempts.len(),
        "shutting down: finishing running attempts"
    );
    let drain = async { while attempts.join_next().await.is_some() {} };
    if timeout(worker.config.drain, drain).await.is_err() {
        tracing::warn!(
            running = attempts.len(),
            "the drain is over: stopping running attempts without reporting them"
        );
        worker.stop.cancel();
        while attempts.join_next().await.is_some() {}
    }
    Ok(())
}

struct Worker {
    config: WorkerConfig,
    registry: Registry,
    db: WorkerDb,
    /// Fired when the drain is over: running attempts stop and report nothing.
    stop: CancellationToken,
}

/// How an attempt's executor ended.
enum Ended {
    Returned(Result<Map<String, Value>, ExecutorError>),
    /// The attempt reached its timeout.
    Deadline,
    /// The run was asked to cancel.
    Canceled,
    /// The lease is gone, or the worker is shutting down: nothing more is reported.
    Abandoned,
}

impl Worker {
    /// Starts a claimed attempt, runs it and reports its logs and outcome, unless the lease
    /// is gone or the worker shuts down first.
    async fn attempt(self: Arc<Self>, claimed: ClaimedJob) {
        let lease = (claimed.node_run_id, claimed.lease_token);
        let job = match retrying(|| self.db.start(lease.0, lease.1)).await {
            Ok(Some(job)) => job,
            // Lease recovery has taken the node run back.
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(error = %error, "starting a node run failed");
                return;
            }
        };
        let mut logs = Logs {
            db: &self.db,
            lease,
            pending: Vec::new(),
            taken: 0,
        };
        let outcome = if job.cancel_requested {
            Some(Outcome::Canceled)
        } else {
            self.execute(&claimed.executor, &job, &mut logs).await
        };
        let Some(outcome) = outcome else { return };
        // The logs go first: completing ends the lease that appending needs.
        if !logs.flush().await {
            return;
        }
        if let Err(error) = retrying(|| self.db.complete(lease.0, lease.1, &outcome)).await {
            tracing::warn!(error = %error, "completing a node run failed");
        }
    }

    /// Resolves the inputs and runs the executor in a new scratch directory. None when the
    /// outcome must not be reported.
    async fn execute(&self, executor: &str, job: &Job, logs: &mut Logs<'_>) -> Option<Outcome> {
        let found = executor
            .split_once('@')
            .and_then(|(key, version)| self.registry.find(key, version));
        let Some(found) = found else {
            let message = format!("This worker has no executor {executor}.");
            return Some(failed(None, "executor_missing", &message, false));
        };
        let node = job.graph.nodes.iter().find(|node| node.id == job.node_id);
        let node = node.expect("the pinned graph holds every node of its run");
        let resolved = resolve_inputs(
            node,
            &job.definition,
            &job.graph,
            &job.run_input,
            &job.upstream_outputs,
        );
        let inputs = match resolved {
            Ok(inputs) => inputs,
            Err(NodeFailure { code, message }) => return Some(failed(None, code, &message, false)),
        };
        let scratch = tempfile::Builder::new()
            .prefix("corqty-node-")
            .tempdir_in(&self.config.scratch_root);
        let scratch = match scratch {
            Ok(scratch) => scratch,
            Err(error) => {
                let message = format!("The worker could not create a scratch directory: {error}.");
                return Some(failed(Some(inputs), "scratch_unavailable", &message, true));
            }
        };
        let limit = Duration::from_secs(u64::from(job.timeout_seconds.unsigned_abs()));
        let request = ExecuteRequest {
            execution_id: job.execution_id,
            attempt: job.attempt.unsigned_abs(),
            workflow_id: job.workflow_id,
            run_id: job.run_id,
            node_id: job.node_id.clone(),
            inputs: inputs.clone(),
            config: node.config.clone(),
            deadline: Utc::now() + TimeDelta::seconds(i64::from(job.timeout_seconds)),
        };
        let ended = self
            .supervise(
                found.as_ref(),
                request,
                scratch.path().to_owned(),
                limit,
                logs,
            )
            .await;
        // The executor has returned or been dropped, so nothing uses the directory now.
        if let Ok(Err(error)) = tokio::task::spawn_blocking(move || scratch.close()).await {
            tracing::warn!(error = %error, "removing a scratch directory failed");
        }

        Some(match ended {
            Ended::Returned(Ok(output)) => match validate_output(&job.definition, &output) {
                Ok(()) => Outcome::Succeeded {
                    input: inputs,
                    output,
                },
                Err(NodeFailure { code, message }) => failed(Some(inputs), code, &message, false),
            },
            Ended::Returned(Err(error)) => {
                let retryable = is_retryable(&error.code, error.retryable);
                failed(Some(inputs), &error.code, &error.message, retryable)
            }
            Ended::Deadline => {
                let message = format!(
                    "The attempt did not finish within its timeout of {} s.",
                    job.timeout_seconds
                );
                let retryable = found.metadata().timeout_retryable;
                failed(Some(inputs), "timeout", &message, retryable)
            }
            Ended::Canceled => Outcome::Canceled,
            Ended::Abandoned => return None,
        })
    }

    /// Runs the executor while it takes the executor's logs and renews the lease. On the
    /// deadline, a cancel request, a lost lease or the end of the drain, it fires the
    /// attempt's token, gives the executor `cancel_grace` to return, and then drops it. The
    /// executor's own answer is ignored then.
    async fn supervise(
        &self,
        executor: &dyn Executor,
        request: ExecuteRequest,
        scratch_dir: PathBuf,
        limit: Duration,
        logs: &mut Logs<'_>,
    ) -> Ended {
        let (sender, mut receiver) = mpsc::channel(LOG_CHANNEL_CAPACITY);
        let context = ExecutionContext {
            cancel: CancellationToken::new(),
            scratch_dir,
            logs: sender,
        };
        let mut execution = executor.execute(request, &context);
        let start = Instant::now();
        let mut deadline = std::pin::pin!(sleep_until(start + limit));
        let period = self.config.heartbeat;
        let mut heartbeat = interval_at(start + period, period);
        let mut flush = interval_at(start + LOG_FLUSH_INTERVAL, LOG_FLUSH_INTERVAL);
        let leases = [logs.lease];
        let ended = loop {
            tokio::select! {
                result = &mut execution => {
                    while let Ok(chunk) = receiver.try_recv() {
                        logs.push(chunk);
                    }
                    return Ended::Returned(result);
                }
                Some(chunk) = receiver.recv() => {
                    logs.push(chunk);
                    if logs.is_full() && !logs.flush().await {
                        break Ended::Abandoned;
                    }
                }
                _ = flush.tick() => {
                    if !logs.flush().await {
                        break Ended::Abandoned;
                    }
                }
                _ = heartbeat.tick() => {
                    let lease_seconds = self.config.lease_seconds;
                    match retrying(|| self.db.heartbeat(&leases, lease_seconds)).await {
                        Ok(statuses) if statuses.iter().any(|status| !status.alive) => {
                            break Ended::Abandoned;
                        }
                        Ok(statuses) if statuses.iter().any(|status| status.cancel_requested) => {
                            break Ended::Canceled;
                        }
                        Ok(_) => {}
                        Err(error) => tracing::warn!(error = %error, "renewing a lease failed"),
                    }
                }
                () = &mut deadline => break Ended::Deadline,
                () = self.stop.cancelled() => break Ended::Abandoned,
            }
        };
        context.cancel.cancel();
        // Logs are still taken meanwhile, so the executor never waits on a full channel.
        let grace = async {
            loop {
                tokio::select! {
                    _ = &mut execution => break,
                    Some(chunk) = receiver.recv() => logs.push(chunk),
                }
            }
        };
        let _ = timeout(self.config.cancel_grace, grace).await;
        ended
    }
}

/// One attempt's log text on its way to `append_node_log`.
struct Logs<'a> {
    db: &'a WorkerDb,
    lease: (Uuid, i64),
    /// Text not sent yet, in order: one entry per run of chunks from the same stream.
    pending: Vec<(LogStream, String)>,
    /// How many bytes the executor has sent.
    taken: usize,
}

impl Logs<'_> {
    /// Keeps a chunk for the next flush until the attempt has sent more than the log cap,
    /// and drops it after that. The chunk that passes the cap is kept whole, so the database
    /// sees the text go on, cuts it at the cap and records one `node.log_truncated`.
    fn push(&mut self, chunk: LogChunk) {
        if self.taken > MAX_LOG_BYTES_PER_NODE_RUN {
            return;
        }
        self.taken += chunk.text.len();
        let text = printable(&chunk.text);
        match self.pending.last_mut() {
            Some((stream, pending)) if *stream == chunk.stream => pending.push_str(&text),
            _ => self.pending.push((chunk.stream, text)),
        }
    }

    fn is_full(&self) -> bool {
        let waiting: usize = self.pending.iter().map(|(_, text)| text.len()).sum();
        waiting >= LOG_FLUSH_BYTES
    }

    /// Sends the pending text. False when the lease is gone.
    async fn flush(&mut self) -> bool {
        let (node_run_id, lease_token) = self.lease;
        for (stream, text) in std::mem::take(&mut self.pending) {
            let stream = stream.as_str();
            match retrying(|| self.db.append_log(node_run_id, lease_token, stream, &text)).await {
                Ok(true) => {}
                Ok(false) => return false,
                // ponytail: text that a database error rejected is dropped, not sent again.
                Err(error) => tracing::warn!(error = %error, "appending a node log failed"),
            }
        }
        true
    }
}

fn failed(
    input: Option<Map<String, Value>>,
    code: &str,
    message: &str,
    retryable: bool,
) -> Outcome {
    let error = NodeError {
        code: code.to_owned(),
        message: printable(message),
        retryable,
    };
    Outcome::Failed { input, error }
}

/// `text` with U+0000, which PostgreSQL text and jsonb cannot hold, replaced by U+FFFD.
fn printable(text: &str) -> String {
    text.replace('\0', "\u{fffd}")
}

/// Makes a database call, and makes it again up to 3 times when PostgreSQL aborted it for a
/// deadlock (40P01) or a serialization failure (40001).
async fn retrying<T, F>(mut call: impl FnMut() -> F) -> Result<T, sqlx::Error>
where
    F: Future<Output = Result<T, sqlx::Error>>,
{
    let mut retries = 0;
    loop {
        match call().await {
            Err(error) if retries < 3 && is_transient(&error) => retries += 1,
            result => return result,
        }
    }
}

fn is_transient(error: &sqlx::Error) -> bool {
    let code = error.as_database_error().and_then(|error| error.code());
    code.is_some_and(|code| code == "40P01" || code == "40001")
}
```

Notes on the library:

- `tokio::select!` drops the branch futures before it runs the chosen handler, so the handlers may use `logs` and
  `receiver` although other branches borrow them.
- When the executor returns, chunks still in the channel are taken with `try_recv` before the outcome is built, so
  no log line is lost.
- The heartbeat and flush intervals start one period after the attempt starts (`interval_at`). A plain `interval`
  would fire at once.
- The scratch directory is removed as soon as the executor is gone, before the outcome is reported. The
  directory's `Drop` would also remove it on any early return, but synchronously.
- `job.attempt` and `job.timeout_seconds` are positive `i32` (table checks), so `unsigned_abs` converts them without
  a panic path.

Create `crates/worker/src/main.rs`:

```rust
#![forbid(unsafe_code)]

use std::sync::Arc;

use corqty_executor_mock::MockExecutor;
use corqty_executor_shell::ShellExecutor;
use corqty_plugin_host::Registry;
use corqty_plugin_sdk::Executor;
use corqty_worker::{WorkerConfig, run};
use sqlx::postgres::PgPoolOptions;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "corqty_worker=info".into()),
        )
        .init();
    let database_url =
        std::env::var("DATABASE_URL_WORKER").map_err(|_| "DATABASE_URL_WORKER must be set")?;
    let mut config = WorkerConfig::default();
    if let Ok(capacity) = std::env::var("CORQTY_WORKER_CAPACITY") {
        config.capacity = capacity
            .parse()
            .ok()
            .filter(|capacity| *capacity >= 1)
            .ok_or("CORQTY_WORKER_CAPACITY must be a whole number of at least 1")?;
    }
    let executors: Vec<Arc<dyn Executor>> = vec![Arc::new(MockExecutor), Arc::new(ShellExecutor)];
    let registry = Registry::new(executors)?;
    // One connection for each running attempt, and one to claim with.
    let pool = PgPoolOptions::new()
        .max_connections(config.capacity.unsigned_abs() + 1)
        .connect(&database_url)
        .await?;
    tracing::info!(
        worker_id = %config.worker_id,
        capacity = config.capacity,
        executors = ?registry.advertised(),
        "the worker is ready"
    );
    let shutdown = CancellationToken::new();
    let signal = shutdown.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        signal.cancel();
    });
    run(config, registry, pool, shutdown).await?;
    Ok(())
}

/// Resolves on ctrl-c, or on SIGTERM (what `docker compose stop` sends) on unix.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install the ctrl-c handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install the SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}
```

- [ ] **Step 5: Verify GREEN**

```text
cargo test -p corqty-worker --test execution
```

Expected: 12 passed in about 12 s. The retry test waits for the 2.0 to 2.4 s backoff, and the flood test sends
10 MiB through `append_node_log`.

Eight negative controls show that the tests catch the mistakes they target. Each changed one line of `lib.rs`, ran
the named test, and restored the file:

- `>=` instead of `>` in the log cap: `a_flood_of_logs_is_cut_at_the_cap_with_one_truncation_event` finds
  `(10485760, false)`. The worker stops exactly at the cap, so the database never sees the overflow.
- Completing an abandoned attempt as `canceled`: `a_worker_that_shuts_down_leaves_its_attempt_to_lease_recovery`
  fails, because the first worker's shutdown cancels the node and clears its lease.
- No flush before completing: `both_starter_blueprints_run_to_completion` finds no log lines.
- A deadline 100 times later: `an_attempt_past_its_deadline_fails_with_timeout` finds the run `succeeded`.
- Timeouts always retryable: the same test fails its 2 s bound, because the node waits for a retry.
- No U+0000 replacement: `logs_reach_the_events_in_order_with_nul_replaced` loses the stdout line (the database
  rejects it with 22021, and the flush drops it).
- Ignoring a heartbeat's `cancel_requested`: `a_cancel_request_stops_the_running_attempt` times out.
- The scratch root itself as the scratch directory: the retry test finds the same path for both attempts.

- [ ] **Step 6: Check the binary on Linux**

The `#[cfg(unix)]` SIGTERM branch of `main.rs` does not compile on Windows. Run clippy in a Linux container. Then
build the binary, start it against the test database, send SIGTERM after 3 s and print the exit status. The mount
is read-only, and `--locked` keeps cargo from rewriting the lockfile.

```text
docker run --rm -v "${PWD}:/work:ro" -w /work -e CARGO_TARGET_DIR=/tmp/target --add-host=host.docker.internal:host-gateway rust:1.96-bookworm bash -c 'cargo clippy --locked -p corqty-worker --all-targets -- -D warnings && cargo build --locked -p corqty-worker && { DATABASE_URL_WORKER=postgres://corqty_worker:corqty_worker_dev@host.docker.internal:55432/corqty_test /tmp/target/debug/corqty-worker & sleep 3; kill -TERM $!; wait $!; echo exit=$?; }'
```

Expected:

- clippy prints `Finished` with no warnings.
- `corqty_worker: the worker is ready worker_id=<uuid> capacity=4 executors=["mock@1.0.0", "shell@1.0.0"]`.
- `corqty_worker: shutting down: finishing running attempts running=0`.
- `exit=0`.

The first run also prints `info: syncing channel updates for 1.96.0-x86_64-unknown-linux-gnu`: the container downloads
the pinned toolchain's components and the crates. If an earlier test left a due mock node in the database, the
worker runs it first; it finishes well within the 3 s.

- [ ] **Step 7: Run the full backend verification**

```text
cargo fmt --check
```

Expected: no output, exit code 0.

```text
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: `Finished` with no warnings.

```text
cargo test --workspace
```

Expected: every suite passes:

- corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 6, `catalog_http` 4,
  `events_http` 4, `loops` 1, `members_http` 3, `organizations_http` 1, `problems_http` 2, `publish_http` 2,
  `runs_http` 8, `settings_http` 2, `shutdown` 1, `workflows_http` 4.
- corqty-domain: `account_and_rbac` 6, `catalog_rules` 7, `graph_rules` 8, `publish_rules` 11, `run_rules` 8,
  `runtime_rules` 12.
- corqty-executor-mock: `contract` 11.
- corqty-executor-shell: `contract` 0 on Windows and macOS, 15 on Linux.
- corqty-persistence: `auth_store` 2, `catalog_guards` 4, `final_admin` 2, `migrations` 1, `run_store` 8,
  `tenant_isolation` 1, `test_reset` 1, `worker_queue` 15, `workflow_catalog` 6.
- corqty-plugin-host: `registry` 8.
- corqty-worker: `execution` 12.

That is 169 tests on Windows and 184 on Linux (CI).

- [ ] **Step 8: Commit**

```text
git add Cargo.toml Cargo.lock crates/worker
git commit -m "feat: run workers that claim, execute, and complete node runs"
```

Expected: `git show --stat HEAD` lists exactly the 6 files above (1453 insertions).

## Task 15: Start runs from the studio and see recent runs

**Files:**

- Create: `apps/web/src/components/ui/tabs.tsx`
- Create: `apps/web/src/runs/run-input-form.tsx`
- Create: `apps/web/src/runs/run-dialog.tsx`
- Create: `apps/web/src/runs/runs-drawer.tsx`
- Modify: `apps/web/src/api/types.ts`
- Modify: `apps/web/src/api/queries.ts`
- Modify: `apps/web/src/components/status/status-badge.tsx`
- Modify: `apps/web/src/studio/studio.tsx`
- Modify: `apps/web/src/test/handlers.ts`
- Test: `apps/web/src/runs/run-input-form.test.tsx` (new)
- Test: `apps/web/src/studio/studio.test.tsx`

The studio starts runs and lists the workflow's recent runs. Runs start from the published version, never from
the draft (spec §9, M3-34).

- **Run action.** Run sits in the top bar's action group for every role with `run_workflows`, a gate separate
  from `publish_workflows`. A runner therefore sees Run without Validate and Publish, and a viewer sees none of
  them. While `publishedVersion` is null, Run is disabled and described (`aria-describedby`) by the visible text
  `Publish the workflow before running it.` Otherwise Run is the trigger of the `Run workflow` dialog. Run does
  not wait for the autosave, because the draft plays no part in a run.
- **Run dialog** (`src/runs/run-dialog.tsx`, `RunDialog({organizationId, workflowId, version})`). The dialog's
  form is mounted only while the dialog is open. It loads the published version with `useWorkflowVersion` (`GET
  /orgs/{org}/workflows/{wf}/versions/{version}`) and renders `RunInputForm` from that version's
  `workflowInputSchema`. Start run posts `{input, version}` to `POST /orgs/{org}/workflows/{wf}/runs` through
  `useStartRun`, which refreshes the whole organization like every other write, because the API also writes the
  `run.started` audit event. Sending `version` pins the run to the version whose inputs the form showed, even if
  someone publishes again meanwhile.
  - A 201 calls `router.push("/orgs/{org}/runs/{id}")`. Task 16 adds that page. The form stays disabled after a
    success, so a second click cannot start a second run.
  - A problem with diagnostics (422 `run_input_invalid` or `executor_disabled`) lists them as `<pointer>
    <message>`.
  - Any other failure (409 `workflow_not_published`, 422 `run_input_too_large`, 404, a network error shown as
    `network_error`) shows `The run could not be started (<code>).`
  - Both kinds of message appear in one `role="alert"` list.
- **Run input form** (`src/runs/run-input-form.tsx`, adapted from research (d)). It renders one existing widget
  per top-level property: `widgets[widgetFor(property)]`, labelled with `title` or else the property name, and
  with `description` as help. Workflow inputs have no uiSchema, so the secret and file widgets are never chosen.
  - Required fields and fields with a `default` start from `blankValue`.
  - On submit, optional fields that are still `undefined`, `null` (an emptied number field) or `""` are left
    out, as the API expects.
  - A schema without properties is edited as one JSON object (`Inputs (JSON)`).
  - The form does no client-side schema validation: the API is authoritative.
- **Bottom drawer** (`src/runs/runs-drawer.tsx`, `RunsDrawer({organizationId, workflowId})`). The drawer is the
  last row of the studio's column, below the canvas, and it is also there in read-only mode below 1024 px
  (M3-39). It is a `<section aria-label="Runs">` holding a ghost `Runs` button that toggles a panel; the button
  carries `aria-expanded` and `aria-controls`.
  - The panel starts closed. A closed panel is only `hidden`, so it stays mounted and keeps its tab.
  - The panel holds the allowlisted shadcn `tabs` with one `Recent runs` tab; Task 16 adds `Logs`.
  - The tab shows `useRuns` (`GET /orgs/{org}/workflows/{wf}/runs`, newest first) as a table. Each row has the
    creation time (a link to `/orgs/{org}/runs/{id}`), `v<version>` and a status badge.
  - The tab also has loading, error (`role="alert"`) and empty states.
  - A `ponytail:` comment records that the list refreshes on focus and after a run starts; live status is the
    run page's job.
- **Status badges.** The five run statuses (queued, running, succeeded, failed and canceled) become
  `StatusBadge` entries in `components/status/status-badge.tsx`. That file is still the only place for status
  hues (M3-38).
- **Types and keys.** `src/api/types.ts` gains the run contract from D19/§4: `RunStatus`, `NodeRunState`,
  `RunSummary`, `NodeRun` and `Run`. Every field is present, and missing values are `null`. It also gains
  `WorkflowVersion` (`VersionSummary` plus `graph`). `queryKeys` gains `workflowVersion(org, wf, version)` and
  `runs(org, wf)`, both under the workflow's key.
- **Test API.** The test API gains `workflowVersion` (wf-2's v1, published from its draft) and two `runs`
  (run-2 running, run-1 succeeded), with default GET handlers for both, so every studio test has data for the
  drawer.

- [ ] **Step 1: Add the run types and the test fixtures**

In `apps/web/src/api/types.ts`, apply these replacements in order:

Replace:

```ts
export interface VersionSummary {
  version: number
}
```

With:

```ts
export interface VersionSummary {
  version: number
}

export interface WorkflowVersion extends VersionSummary {
  graph: GraphSnapshot
}

// Run payloads list every field; a missing value is null.
export type RunStatus =
  | "queued"
  | "running"
  | "succeeded"
  | "failed"
  | "canceled"

export type NodeRunState =
  | "blocked"
  | "queued"
  | "running"
  | "retry_wait"
  | "succeeded"
  | "failed"
  | "canceled"
  | "skipped"

export interface RunSummary {
  id: string
  workflowId: string
  version: number
  status: RunStatus
  triggerSource: string
  createdAt: string
  startedAt: string | null
  finishedAt: string | null
  cancelRequested: boolean
}

export interface NodeRun {
  nodeId: string
  state: NodeRunState
  attempt: number
  maxAttempts: number
  executor: string
  queueReason: "waiting_for_compatible_worker" | null
  input: Record<string, unknown> | null
  output: Record<string, unknown> | null
  error: { code: string; message: string; retryable: boolean } | null
  startedAt: string | null
  finishedAt: string | null
}

export type Run = RunSummary & {
  workflowName: string
  input: Record<string, unknown>
  lastEventSeq: number
  graph: GraphSnapshot
  nodes: NodeRun[]
}
```

In `apps/web/src/test/handlers.ts`, apply these replacements in order:

Replace:

```ts
  Role,
  Workflow,
  WorkflowSummary,
} from "@/api/types"
```

With:

```ts
  Role,
  RunSummary,
  Workflow,
  WorkflowSummary,
  WorkflowVersion,
} from "@/api/types"
```

Replace:

```ts
  draft: { revision: 3, graph: blueprints[1].graph },
}
```

With:

```ts
  draft: { revision: 3, graph: blueprints[1].graph },
}

// Its v1 was published from that draft.
export const workflowVersion: WorkflowVersion = {
  version: 1,
  graph: blueprints[1].graph,
}

// Newest first, as the API lists them.
export const runs: RunSummary[] = [
  {
    id: "run-2",
    workflowId: "wf-2",
    version: 1,
    status: "running",
    triggerSource: "manual",
    createdAt: "2026-10-26T09:00:00Z",
    startedAt: "2026-10-26T09:00:01Z",
    finishedAt: null,
    cancelRequested: false,
  },
  {
    id: "run-1",
    workflowId: "wf-2",
    version: 1,
    status: "succeeded",
    triggerSource: "manual",
    createdAt: "2026-10-25T09:00:00Z",
    startedAt: "2026-10-25T09:00:01Z",
    finishedAt: "2026-10-25T09:00:04Z",
    cancelRequested: false,
  },
]
```

Replace:

```ts
      return HttpResponse.json({ revision: revision + 1 })
    },
  ),
```

With:

```ts
      return HttpResponse.json({ revision: revision + 1 })
    },
  ),
  http.get(
    "/api/v1/orgs/:organizationId/workflows/:workflowId/versions/:version",
    () => HttpResponse.json(workflowVersion),
  ),
  http.get("/api/v1/orgs/:organizationId/workflows/:workflowId/runs", () =>
    HttpResponse.json({ items: runs }),
  ),
```

- [ ] **Step 2: Write the failing run input form tests**

The tests cover these types: a titled required string, a select with a default, an optional integer with help
text that is typed and then cleared (the widget then writes `null`), a required boolean and an optional object.
They also cover the JSON fallback and the disabled state while a run starts.

Create `apps/web/src/runs/run-input-form.test.tsx`:

```tsx
import { fireEvent, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { RunInputForm } from "@/runs/run-input-form"

it("builds the run inputs from the workflow input schema", async () => {
  const onSubmit = vi.fn()
  render(
    <RunInputForm
      schema={{
        type: "object",
        properties: {
          region: { type: "string", title: "Region" },
          size: { type: "string", enum: ["small", "large"], default: "small" },
          replicas: { type: "integer", description: "Leave empty for one." },
          dryRun: { type: "boolean" },
          tags: { type: "object" },
        },
        required: ["region", "dryRun"],
      }}
      pending={false}
      diagnostics={[
        { code: "invalid_value", pointer: "/region", message: "Too short." },
      ]}
      onSubmit={onSubmit}
    />,
  )
  expect(screen.getByText("Leave empty for one.")).toBeInTheDocument()
  await userEvent.type(screen.getByLabelText("Region"), "eu-west-1")
  await userEvent.selectOptions(screen.getByLabelText("size"), "large")
  await userEvent.type(screen.getByLabelText("replicas"), "3")
  await userEvent.clear(screen.getByLabelText("replicas"))
  await userEvent.click(screen.getByRole("button", { name: "Start run" }))

  // Optional fields left empty are omitted; required ones are always sent.
  expect(onSubmit).toHaveBeenCalledWith({
    region: "eu-west-1",
    size: "large",
    dryRun: false,
  })
  expect(screen.getByRole("alert")).toHaveTextContent("/region Too short.")
})

it("edits a schema without properties as one JSON object", async () => {
  const onSubmit = vi.fn()
  render(
    <RunInputForm
      schema={{ type: "object" }}
      pending={false}
      diagnostics={[]}
      onSubmit={onSubmit}
    />,
  )
  const json = screen.getByLabelText("Inputs (JSON)")
  expect(json).toHaveValue("{}")
  fireEvent.change(json, { target: { value: '{"payload": {"a": 1}}' } })
  await userEvent.click(screen.getByRole("button", { name: "Start run" }))
  expect(onSubmit).toHaveBeenCalledWith({ payload: { a: 1 } })
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
})

it("disables the form while the run starts", () => {
  render(
    <RunInputForm
      schema={{ type: "object", properties: { region: { type: "string" } } }}
      pending
      diagnostics={[]}
      onSubmit={vi.fn()}
    />,
  )
  expect(screen.getByLabelText("region")).toBeDisabled()
  expect(screen.getByRole("button", { name: "Starting…" })).toBeDisabled()
})
```

- [ ] **Step 3: Extend the studio tests**

The runner and viewer row now also checks Run. The new tests cover the following:
- Run opens the dialog on a published version whose input schema differs from the draft's, and posts exactly
  `{input: {region: "eu-west-1"}, version: 1}`, leaving out the blank optional `replicas`. Then it navigates.
- A 422 lists its diagnostic, then a 409 without diagnostics shows the generic message.
- An unpublished workflow disables Run with the reason.
- The drawer toggles its hidden panel and lists the runs with their links, versions and statuses.
- At 800 px the read-only studio still offers the drawer and its links.

In `apps/web/src/studio/studio.test.tsx`, apply these replacements in order:

Replace:

```tsx
import { blueprints, meAs, nodeDefinitions, workflow } from "@/test/handlers"
import { renderWithProviders } from "@/test/render"
```

With:

```tsx
import {
  blueprints,
  meAs,
  nodeDefinitions,
  runs,
  workflow,
} from "@/test/handlers"
import { renderWithProviders, router } from "@/test/render"
```

Replace:

```tsx
it.each(["runner", "viewer"] as const)(
  "a %s cannot validate or publish",
  async (role) => {
```

With:

```tsx
it.each([
  { role: "runner" as const, canRun: true },
  { role: "viewer" as const, canRun: false },
])(
  "a $role cannot validate or publish, and runs only with run_workflows",
  async ({ role, canRun }) => {
```

Replace:

```tsx
    expect(
      screen.queryByRole("button", { name: "Publish" }),
    ).not.toBeInTheDocument()
  },
)
```

With:

```tsx
    expect(
      screen.queryByRole("button", { name: "Publish" }),
    ).not.toBeInTheDocument()
    expect(Boolean(screen.queryByRole("button", { name: "Run" }))).toBe(canRun)
  },
)

async function openRunDialog() {
  await userEvent.click(await screen.findByRole("button", { name: "Run" }))
  return screen.findByRole("dialog", { name: "Run workflow" })
}

it("Run starts the published version with its inputs and opens the run", async () => {
  const started: unknown[] = []
  server.use(
    // The published version asks for other inputs than the draft.
    http.get(`${workflowUrl}/versions/:version`, ({ params }) =>
      HttpResponse.json({
        version: Number(params.version),
        graph: {
          ...infrastructure.graph,
          workflowInputSchema: {
            type: "object",
            properties: {
              region: { type: "string", title: "Region", default: "us-east-1" },
              replicas: { type: "integer", title: "Replicas" },
            },
            required: ["region"],
          },
        },
      }),
    ),
    http.post(`${workflowUrl}/runs`, async ({ request }) => {
      started.push(await request.json())
      return HttpResponse.json(
        { ...runs[0], id: "run-3", status: "queued" },
        { status: 201 },
      )
    }),
  )
  renderStudio()
  const dialog = await openRunDialog()
  expect(
    within(dialog).getByText("Starts published version 1 with these inputs."),
  ).toBeInTheDocument()
  const region = await within(dialog).findByLabelText("Region")
  expect(region).toHaveValue("us-east-1")
  expect(within(dialog).getByLabelText("Replicas")).toHaveValue(null)
  await userEvent.clear(region)
  await userEvent.type(region, "eu-west-1")
  await userEvent.click(
    within(dialog).getByRole("button", { name: "Start run" }),
  )

  await waitFor(() =>
    expect(router.push).toHaveBeenCalledWith("/orgs/org-1/runs/run-3"),
  )
  expect(started).toEqual([{ input: { region: "eu-west-1" }, version: 1 }])
})

it("Run shows why a run could not start", async () => {
  server.use(
    http.post(`${workflowUrl}/runs`, () =>
      problem(422, "run_input_invalid", [
        {
          code: "invalid_value",
          pointer: "/region",
          message: "The value is shorter than 1 character.",
        },
      ]),
    ),
  )
  renderStudio()
  const dialog = await openRunDialog()
  const start = await within(dialog).findByRole("button", { name: "Start run" })
  await userEvent.click(start)
  expect(await within(dialog).findByRole("alert")).toHaveTextContent(
    "/region The value is shorter than 1 character.",
  )

  server.use(
    http.post(`${workflowUrl}/runs`, () =>
      problem(409, "workflow_not_published"),
    ),
  )
  await userEvent.click(start)
  await waitFor(() =>
    expect(within(dialog).getByRole("alert")).toHaveTextContent(
      "The run could not be started (workflow_not_published).",
    ),
  )
  expect(router.push).not.toHaveBeenCalled()
})

it("Run waits until the workflow is published", async () => {
  server.use(
    http.get(workflowUrl, () =>
      HttpResponse.json({ ...workflow, publishedVersion: null }),
    ),
  )
  renderStudio()
  const run = await screen.findByRole("button", { name: "Run" })
  expect(run).toBeDisabled()
  expect(run).toHaveAccessibleDescription(
    "Publish the workflow before running it.",
  )
})

it("the runs drawer opens on the recent runs with links to each run", async () => {
  renderStudio()
  const toggle = await screen.findByRole("button", { name: "Runs" })
  const panel = document.getElementById(
    toggle.getAttribute("aria-controls") ?? "",
  )
  if (!panel) throw new Error("No runs panel")
  expect(toggle).toHaveAttribute("aria-expanded", "false")
  expect(panel).not.toBeVisible()

  await userEvent.click(toggle)
  expect(toggle).toHaveAttribute("aria-expanded", "true")
  expect(panel).toBeVisible()
  expect(
    within(panel).getByRole("tab", { name: "Recent runs" }),
  ).toHaveAttribute("aria-selected", "true")
  const rows = (await within(panel).findAllByRole("row")).slice(1)
  expect(
    rows.map((row) => within(row).getByRole("link").getAttribute("href")),
  ).toEqual(["/orgs/org-1/runs/run-2", "/orgs/org-1/runs/run-1"])
  expect(within(rows[0]).getByText("Running")).toBeInTheDocument()
  expect(within(rows[1]).getByText("Succeeded")).toBeInTheDocument()
  expect(within(rows[1]).getByText("v1")).toBeInTheDocument()

  await userEvent.click(toggle)
  expect(panel).not.toBeVisible()
})

it("the read-only studio on a narrow screen still lists the recent runs", async () => {
  Object.assign(window, { innerWidth: 800 })
  renderStudio()
  expect(
    await screen.findByText(
      "Editing needs a screen at least 1024 pixels wide.",
    ),
  ).toBeInTheDocument()
  await userEvent.click(screen.getByRole("button", { name: "Runs" }))
  const drawer = screen.getByRole("region", { name: "Runs" })
  const links = await within(drawer).findAllByRole("link")
  expect(links.map((link) => link.getAttribute("href"))).toEqual([
    "/orgs/org-1/runs/run-2",
    "/orgs/org-1/runs/run-1",
  ])
})
```

- [ ] **Step 4: Run the tests and verify RED**

Run:

```text
pnpm web:test
```

Expected: `Test Files  2 failed | 11 passed (13)` and `Tests  6 failed | 86 passed (92)`:
- `run-input-form.test.tsx` fails to load with `Error: Failed to resolve import "@/runs/run-input-form" from
  "src/runs/run-input-form.test.tsx". Does the file exist?`.
- `a 'runner' cannot validate or publish, and runs only with run_workflows` fails with `AssertionError: expected
  false to be true // Object.is equality`. The viewer row passes.
- The three Run tests fail with `TestingLibraryElementError: Unable to find role="button" and name "Run"`.
- The drawer test fails with `Unable to find role="button" and name "Runs"`, and the narrow-screen test with
  `Unable to find an accessible element with the role "button" and name "Runs"`.

- [ ] **Step 5: Add the tabs component**

This is the shadcn base-nova `tabs` registry file, written by hand with `cn` imported from `@/lib/utils`.
Running `shadcn add tabs` would import `cn` from the `cn` package and add it as a dependency.

Create `apps/web/src/components/ui/tabs.tsx`:

```tsx
"use client"

import { Tabs as TabsPrimitive } from "@base-ui/react/tabs"
import { cva, type VariantProps } from "class-variance-authority"
import { cn } from "@/lib/utils"

function Tabs({
  className,
  orientation = "horizontal",
  ...props
}: TabsPrimitive.Root.Props) {
  return (
    <TabsPrimitive.Root
      data-slot="tabs"
      data-orientation={orientation}
      className={cn(
        "group/tabs flex gap-2 data-horizontal:flex-col",
        className
      )}
      {...props}
    />
  )
}

const tabsListVariants = cva(
  "group/tabs-list inline-flex w-fit items-center justify-center rounded-lg p-[3px] text-muted-foreground group-data-horizontal/tabs:h-8 group-data-vertical/tabs:h-fit group-data-vertical/tabs:flex-col data-[variant=line]:rounded-none",
  {
    variants: {
      variant: {
        default: "bg-muted",
        line: "gap-1 bg-transparent",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  }
)

function TabsList({
  className,
  variant = "default",
  ...props
}: TabsPrimitive.List.Props & VariantProps<typeof tabsListVariants>) {
  return (
    <TabsPrimitive.List
      data-slot="tabs-list"
      data-variant={variant}
      className={cn(tabsListVariants({ variant }), className)}
      {...props}
    />
  )
}

function TabsTrigger({ className, ...props }: TabsPrimitive.Tab.Props) {
  return (
    <TabsPrimitive.Tab
      data-slot="tabs-trigger"
      className={cn(
        "relative inline-flex h-[calc(100%-1px)] flex-1 items-center justify-center gap-1.5 rounded-md border border-transparent px-1.5 py-0.5 text-sm font-medium whitespace-nowrap text-foreground/60 transition-all group-data-vertical/tabs:w-full group-data-vertical/tabs:justify-start hover:text-foreground focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-1 focus-visible:outline-ring disabled:pointer-events-none disabled:opacity-50 has-data-[icon=inline-end]:pr-1 has-data-[icon=inline-start]:pl-1 aria-disabled:pointer-events-none aria-disabled:opacity-50 dark:text-muted-foreground dark:hover:text-foreground group-data-[variant=default]/tabs-list:data-active:shadow-sm group-data-[variant=line]/tabs-list:data-active:shadow-none [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
        "group-data-[variant=line]/tabs-list:bg-transparent group-data-[variant=line]/tabs-list:data-active:bg-transparent dark:group-data-[variant=line]/tabs-list:data-active:border-transparent dark:group-data-[variant=line]/tabs-list:data-active:bg-transparent",
        "data-active:bg-background data-active:text-foreground dark:data-active:border-input dark:data-active:bg-input/30 dark:data-active:text-foreground",
        "after:absolute after:bg-foreground after:opacity-0 after:transition-opacity group-data-horizontal/tabs:after:inset-x-0 group-data-horizontal/tabs:after:bottom-[-5px] group-data-horizontal/tabs:after:h-0.5 group-data-vertical/tabs:after:inset-y-0 group-data-vertical/tabs:after:-right-1 group-data-vertical/tabs:after:w-0.5 group-data-[variant=line]/tabs-list:data-active:after:opacity-100",
        className
      )}
      {...props}
    />
  )
}

function TabsContent({ className, ...props }: TabsPrimitive.Panel.Props) {
  return (
    <TabsPrimitive.Panel
      data-slot="tabs-content"
      className={cn("flex-1 text-sm outline-none", className)}
      {...props}
    />
  )
}

export { Tabs, TabsList, TabsTrigger, TabsContent, tabsListVariants }
```

- [ ] **Step 6: Give run statuses a badge**

In `apps/web/src/components/status/status-badge.tsx`, apply these replacements in order:

Replace:

```tsx
  "read-only": { label: "Read-only", className: "text-muted-foreground" },
}
```

With:

```tsx
  "read-only": { label: "Read-only", className: "text-muted-foreground" },
  // Run statuses (RunStatus).
  queued: { label: "Queued", className: "text-muted-foreground" },
  running: {
    label: "Running",
    className: "border-blue-600/40 text-blue-700 dark:text-blue-400",
  },
  succeeded: {
    label: "Succeeded",
    className: "border-emerald-600/40 text-emerald-700 dark:text-emerald-400",
  },
  failed: {
    label: "Failed",
    className: "border-red-600/40 text-red-700 dark:text-red-400",
  },
  canceled: { label: "Canceled", className: "text-muted-foreground" },
}
```

- [ ] **Step 7: Add the version, runs and start-run queries**

In `apps/web/src/api/queries.ts`, apply these replacements in order:

Replace:

```ts
  Role,
  VersionSummary,
  Workflow,
  WorkflowSummary,
} from "@/api/types"
```

With:

```ts
  Role,
  Run,
  RunSummary,
  VersionSummary,
  Workflow,
  WorkflowSummary,
  WorkflowVersion,
} from "@/api/types"
```

Replace:

```ts
  workflow: (organizationId: string, workflowId: string) =>
    ["orgs", organizationId, "workflows", workflowId] as const,
```

With:

```ts
  workflow: (organizationId: string, workflowId: string) =>
    ["orgs", organizationId, "workflows", workflowId] as const,
  workflowVersion: (
    organizationId: string,
    workflowId: string,
    version: number,
  ) =>
    [
      "orgs",
      organizationId,
      "workflows",
      workflowId,
      "versions",
      version,
    ] as const,
  runs: (organizationId: string, workflowId: string) =>
    ["orgs", organizationId, "workflows", workflowId, "runs"] as const,
```

Replace:

```ts
      { method: "POST", body: JSON.stringify({ revision }) },
    ),
  )
}
```

With:

```ts
      { method: "POST", body: JSON.stringify({ revision }) },
    ),
  )
}

export function useWorkflowVersion(
  organizationId: string,
  workflowId: string,
  version: number,
) {
  return useQuery({
    queryKey: queryKeys.workflowVersion(organizationId, workflowId, version),
    queryFn: ({ signal }) =>
      api<WorkflowVersion>(
        `/orgs/${organizationId}/workflows/${workflowId}/versions/${version}`,
        { signal },
      ),
  })
}

export function useRuns(organizationId: string, workflowId: string) {
  return useQuery({
    queryKey: queryKeys.runs(organizationId, workflowId),
    queryFn: ({ signal }) =>
      api<{ items: RunSummary[] }>(
        `/orgs/${organizationId}/workflows/${workflowId}/runs`,
        { signal },
      ),
  })
}

// Runs the given published version, not the draft.
export function useStartRun(organizationId: string, workflowId: string) {
  return useOrganizationMutation(
    organizationId,
    (body: { input: Record<string, unknown>; version: number }) =>
      api<Run>(`/orgs/${organizationId}/workflows/${workflowId}/runs`, {
        method: "POST",
        body: JSON.stringify(body),
      }),
  )
}
```

- [ ] **Step 8: Add the run input form and the run dialog**

Create `apps/web/src/runs/run-input-form.tsx`:

```tsx
"use client"

import { useState, type FormEvent } from "react"

import type { Diagnostic, JsonSchema } from "@/api/types"
import { Button } from "@/components/ui/button"
import { isRecord } from "@/studio/graph-model"
import { blankValue, widgetFor, widgets } from "@/studio/widgets"

function text(value: unknown) {
  return typeof value === "string" ? value : undefined
}

function filled(value: unknown) {
  return value !== null && value !== undefined && value !== ""
}

// One widget per top-level property of the workflow input schema; any other
// schema is edited as one JSON object. The API validates the inputs.
export function RunInputForm({
  schema,
  pending,
  diagnostics,
  onSubmit,
}: {
  schema: JsonSchema
  pending: boolean
  diagnostics: Diagnostic[]
  onSubmit: (inputs: Record<string, unknown>) => void
}) {
  const properties = Object.entries(
    isRecord(schema.properties) ? schema.properties : {},
  )
  const required = new Set(schema.required ?? [])
  // Required fields and fields with a default start filled.
  const [inputs, setInputs] = useState<Record<string, unknown>>(() =>
    Object.fromEntries(
      properties
        .filter(
          ([name, property]) =>
            required.has(name) || property.default !== undefined,
        )
        .map(([name, property]) => [name, blankValue(property)]),
    ),
  )

  // An optional field left empty is omitted rather than sent as null.
  function submit(event: FormEvent) {
    event.preventDefault()
    onSubmit(
      properties.length === 0
        ? inputs
        : Object.fromEntries(
            Object.entries(inputs).filter(
              ([name, value]) => required.has(name) || filled(value),
            ),
          ),
    )
  }

  return (
    <form onSubmit={submit} aria-label="Run inputs">
      <fieldset disabled={pending} className="grid gap-4">
        {properties.length > 0 ? (
          properties.map(([name, property]) => {
            const Widget = widgets[widgetFor(property)]
            return (
              <Widget
                key={name}
                label={text(property.title) ?? name}
                help={text(property.description)}
                schema={property}
                value={inputs[name]}
                onChange={(value) =>
                  setInputs((current) => ({ ...current, [name]: value }))
                }
              />
            )
          })
        ) : (
          <widgets.code
            label="Inputs (JSON)"
            schema={{ type: "object" }}
            value={inputs}
            onChange={(value) => setInputs(value as Record<string, unknown>)}
          />
        )}
        {diagnostics.length > 0 && (
          <ul role="alert" className="grid gap-1 text-sm text-destructive">
            {diagnostics.map((diagnostic) => (
              <li key={`${diagnostic.pointer}:${diagnostic.code}`}>
                {diagnostic.pointer && (
                  <span className="font-mono">{diagnostic.pointer} </span>
                )}
                {diagnostic.message}
              </li>
            ))}
          </ul>
        )}
        <Button type="submit" className="justify-self-start">
          {pending ? "Starting…" : "Start run"}
        </Button>
      </fieldset>
    </form>
  )
}
```

Create `apps/web/src/runs/run-dialog.tsx`:

```tsx
"use client"

import { useRouter } from "next/navigation"
import { useId, useState } from "react"

import { ApiProblem } from "@/api/client"
import { useStartRun, useWorkflowVersion } from "@/api/queries"
import type { Diagnostic } from "@/api/types"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"
import { RunInputForm } from "@/runs/run-input-form"

// Runs start from the published version, so Run waits for one.
export function RunDialog({
  organizationId,
  workflowId,
  version,
}: {
  organizationId: string
  workflowId: string
  version: number | null
}) {
  const reasonId = useId()
  if (version === null) {
    return (
      <>
        <span id={reasonId} className="text-muted-foreground">
          Publish the workflow before running it.
        </span>
        <Button
          size="sm"
          variant="secondary"
          disabled
          aria-describedby={reasonId}
        >
          Run
        </Button>
      </>
    )
  }
  return (
    <Dialog>
      <DialogTrigger render={<Button size="sm" variant="secondary" />}>
        Run
      </DialogTrigger>
      <DialogContent className="sm:max-w-md">
        <RunForm
          organizationId={organizationId}
          workflowId={workflowId}
          version={version}
        />
      </DialogContent>
    </Dialog>
  )
}

// Mounted only while the dialog is open, so it starts from the defaults each
// time. The form stays disabled after a start until the run page opens.
function RunForm({
  organizationId,
  workflowId,
  version,
}: {
  organizationId: string
  workflowId: string
  version: number
}) {
  const router = useRouter()
  const published = useWorkflowVersion(organizationId, workflowId, version)
  const startRun = useStartRun(organizationId, workflowId)
  const [diagnostics, setDiagnostics] = useState<Diagnostic[]>([])

  async function start(input: Record<string, unknown>) {
    setDiagnostics([])
    try {
      const run = await startRun.mutateAsync({ input, version })
      router.push(`/orgs/${organizationId}/runs/${run.id}`)
    } catch (error) {
      const problem =
        error instanceof ApiProblem ? error : new ApiProblem(0, "network_error")
      setDiagnostics(
        problem.diagnostics.length > 0
          ? problem.diagnostics
          : [
              {
                code: problem.code,
                pointer: "",
                message: `The run could not be started (${problem.code}).`,
              },
            ],
      )
    }
  }

  return (
    <>
      <DialogHeader>
        <DialogTitle>Run workflow</DialogTitle>
        <DialogDescription>
          Starts published version {version} with these inputs.
        </DialogDescription>
      </DialogHeader>
      {published.data ? (
        <RunInputForm
          schema={published.data.graph.workflowInputSchema}
          pending={startRun.isPending || startRun.isSuccess}
          diagnostics={diagnostics}
          onSubmit={(input) => void start(input)}
        />
      ) : published.isError ? (
        <p role="alert" className="text-destructive">
          The published version could not be loaded.
        </p>
      ) : (
        <p className="text-muted-foreground">Loading inputs…</p>
      )}
    </>
  )
}
```

- [ ] **Step 9: Add the runs drawer**

Create `apps/web/src/runs/runs-drawer.tsx`:

```tsx
"use client"

import { ChevronDownIcon, ChevronUpIcon } from "lucide-react"
import Link from "next/link"
import { useId, useState } from "react"

import { useRuns } from "@/api/queries"
import { StatusBadge } from "@/components/status/status-badge"
import { Button } from "@/components/ui/button"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"

// The studio's bottom drawer. The closed panel is only hidden, so it keeps its
// tab and scroll position; it is not a Sheet, which would cover the canvas.
export function RunsDrawer({
  organizationId,
  workflowId,
}: {
  organizationId: string
  workflowId: string
}) {
  const [open, setOpen] = useState(false)
  const panelId = useId()
  return (
    <section aria-label="Runs" className="border-t">
      <Button
        variant="ghost"
        size="sm"
        className="m-1"
        aria-expanded={open}
        aria-controls={panelId}
        onClick={() => setOpen(!open)}
      >
        {open ? <ChevronDownIcon /> : <ChevronUpIcon />}
        Runs
      </Button>
      <div id={panelId} hidden={!open} className="h-72 overflow-y-auto px-4">
        <Tabs defaultValue="runs">
          <TabsList>
            <TabsTrigger value="runs">Recent runs</TabsTrigger>
          </TabsList>
          <TabsContent value="runs">
            <RecentRuns
              organizationId={organizationId}
              workflowId={workflowId}
            />
          </TabsContent>
        </Tabs>
      </div>
    </section>
  )
}

// ponytail: the list refreshes on focus and after a run starts; the run page
// follows a run live.
function RecentRuns({
  organizationId,
  workflowId,
}: {
  organizationId: string
  workflowId: string
}) {
  const runs = useRuns(organizationId, workflowId)
  if (runs.isPending) {
    return <p className="text-muted-foreground">Loading runs…</p>
  }
  if (runs.isError) {
    return (
      <p role="alert" className="text-destructive">
        The runs could not be loaded.
      </p>
    )
  }
  if (runs.data.items.length === 0) {
    return <p className="text-muted-foreground">No runs yet.</p>
  }
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Created</TableHead>
          <TableHead>Version</TableHead>
          <TableHead>Status</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {runs.data.items.map((run) => (
          <TableRow key={run.id}>
            <TableCell>
              <Link
                href={`/orgs/${organizationId}/runs/${run.id}`}
                className="font-medium underline-offset-4 hover:underline"
              >
                {new Date(run.createdAt).toLocaleString()}
              </Link>
            </TableCell>
            <TableCell>v{run.version}</TableCell>
            <TableCell>
              <StatusBadge status={run.status} />
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  )
}
```

- [ ] **Step 10: Put Run and the drawer in the studio**

In `apps/web/src/studio/studio.tsx`, apply these replacements in order:

Replace:

```tsx
import { cn } from "@/lib/utils"
import { mapDiagnostics, type MappedDiagnostic } from "@/studio/diagnostics"
```

With:

```tsx
import { cn } from "@/lib/utils"
import { RunDialog } from "@/runs/run-dialog"
import { RunsDrawer } from "@/runs/runs-drawer"
import { mapDiagnostics, type MappedDiagnostic } from "@/studio/diagnostics"
```

Replace:

```tsx
  const canPublish = useCan(organizationId, "publish_workflows")
```

With:

```tsx
  const canPublish = useCan(organizationId, "publish_workflows")
  const canRun = useCan(organizationId, "run_workflows")
```

Replace:

```tsx
        {canPublish && (
          <div className="ml-auto flex gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={busy || conflict}
              onClick={() => void run(validateDraft)}
            >
              Validate
            </Button>
            <Button
              size="sm"
              disabled={busy || conflict}
              onClick={() => void run(publishDraft)}
            >
              Publish
            </Button>
          </div>
        )}
```

With:

```tsx
        {(canPublish || canRun) && (
          <div className="ml-auto flex items-center gap-2">
            {canPublish && (
              <>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={busy || conflict}
                  onClick={() => void run(validateDraft)}
                >
                  Validate
                </Button>
                <Button
                  size="sm"
                  disabled={busy || conflict}
                  onClick={() => void run(publishDraft)}
                >
                  Publish
                </Button>
              </>
            )}
            {canRun && (
              <RunDialog
                organizationId={organizationId}
                workflowId={workflow.id}
                version={workflow.publishedVersion}
              />
            )}
          </div>
        )}
```

Replace:

```tsx
          onChange={autosave.edit}
        />
      </div>
```

With:

```tsx
          onChange={autosave.edit}
        />
      </div>
      <RunsDrawer organizationId={organizationId} workflowId={workflow.id} />
```

- [ ] **Step 11: Verify GREEN**

Run:

```text
pnpm web:test
```

Expected: `Test Files  13 passed (13)` and `Tests  95 passed (95)` with no unhandled errors. That is studio 24,
run-input-form 3, and the 68 tests of the other eleven files unchanged. On Node 25 or newer, each worker may
print `ExperimentalWarning: localStorage is not available because --localstorage-file was not provided.`; this
is harmless.

- [ ] **Step 12: Run lint and build**

Run:

```text
pnpm web:lint
```

Expected: `✓ Types generated successfully`, then `tsc --noEmit` and `oxlint` report no diagnostics. The command
exits 0.

Run:

```text
pnpm web:build
```

Expected: `✓ Compiled successfully` and `Finished TypeScript`. The route table is unchanged from Milestone 2:
`ƒ /orgs/[organizationId]/workflows/[workflowId]` is still dynamic, and the run page arrives in Task 16. The
command exits 0.

- [ ] **Step 13: Commit**

```text
git add apps/web
git commit -m "feat: start runs from the studio and see recent runs"
```

## Task 16: Watch runs live with graph overlays and logs

**Files:**

- Create: `apps/web/src/test/event-source.ts`
- Create: `apps/web/src/runs/use-run-events.ts`
- Create: `apps/web/src/runs/log-view.tsx`
- Create: `apps/web/src/runs/run-graph.tsx`
- Create: `apps/web/src/runs/run-view.tsx`
- Create: `apps/web/src/app/(app)/orgs/[organizationId]/runs/[runId]/page.tsx`
- Modify: `apps/web/src/test/setup.ts`
- Modify: `apps/web/src/api/types.ts`
- Modify: `apps/web/src/api/queries.ts`
- Modify: `apps/web/src/components/status/status-badge.tsx`
- Modify: `apps/web/src/runs/runs-drawer.tsx`
- Modify: `apps/web/src/test/handlers.ts`
- Test: `apps/web/src/runs/use-run-events.test.tsx` (new)
- Test: `apps/web/src/runs/log-view.test.tsx` (new)
- Test: `apps/web/src/runs/run-view.test.tsx` (new)
- Test: `apps/web/src/studio/studio.test.tsx`

The run page at `/orgs/{org}/runs/{run}` follows one run live. It overlays node-run states on the run's pinned
graph (M3-36), shows the selected node's details, and streams the log through replayable SSE (M3-37, acceptance
8). The studio drawer from Task 15 gains a Logs tab for the workflow's newest run (M3-35). Run monitoring works
on any screen width, unlike editing (M3-39).

- **Route.** `app/(app)/orgs/[organizationId]/runs/[runId]/page.tsx` is a thin server page. It renders the client
  `RunView({organizationId, runId})` keyed by the run id, so a rerun opens with a fresh graph and selection.
- **Snapshot and stream** (`src/runs/use-run-events.ts`, adapted from research (a) to the D13/D19 names).
  - `useRun(org, runId)` lives in `src/api/queries.ts` under `queryKeys.run(org, runId)` =
    `["orgs", org, "runs", runId]`. Its `queryFn` keeps the cached run when the cache has the larger
    `lastEventSeq`, so a refetch never moves overlays back behind events already applied.
  - `useRunEventLog(org, runId)` is a `skipToken` query under `queryKeys.runEvents(org, runId)` (the run key
    plus `"events"`). Only the stream writes it, as `{lastSeq, events}`.
  - `useRunEvents(org, runId, enabled)` opens one native `EventSource` on
    `/api/v1/orgs/{org}/runs/{run}/events?after=<lastSeq>`. Callers pass `run.isSuccess` (and the studio also
    the drawer's `open`), so a 401 or 404 on a run refetch closes the stream.
  - Events are buffered and written once per 100 ms. `mergeEvents` drops `seq <= lastSeq` and keeps the newest
    `EVENT_LIMIT` (2,000) events. `applyEvents` ignores `seq <= run.lastEventSeq`, writes a `run.state` event's
    `state` into `status`, and merges a `node.state` event's `state` and `attempt` into the node, plus `error`
    and `output` when the event carries them.
  - A dropped stream (`readyState` CONNECTING) is the browser's job: it reconnects with `Last-Event-ID`. A
    failed one (CLOSED: a non-200, 204 included) is retried by hand. The hook first refetches the run, then
    reconnects with `?after=` after a backoff of 1 s, doubling up to 30 s.
  - The terminal `run.state` closes the source at once, so the browser does not reconnect when the server ends
    the stream. A terminal run whose events have all arrived opens no stream.
- **Graph overlays** (`src/runs/run-graph.tsx`, `RunGraph({graph, definitions, nodeRuns, onNodeSelect})`).
  - The graph is read once into XYFlow's uncontrolled `defaultNodes`.
  - Node-run states reach the nodes through a context that `RunNodeView` reads. The node view wraps
    `StudioNodeView` and adds a `StatusBadge`. A state change therefore re-renders only the badges: XYFlow keeps
    the node objects and their measured sizes (research (c)).
  - Nothing can be dragged, connected or deleted. Nodes stay selectable by pointer and by keyboard (Enter), and
    `onSelectionChange` reports the one selected node, or null.
  - `RunView` loads the graph through `next/dynamic` with `ssr: false`, like the studio canvas.
- **Node details.** An `<aside aria-label="Node details">` shows the selected node's name and state badge,
  `<attempt> of <maxAttempts>`, the queue reason as `Waiting for a worker that runs <executor>.`, the error
  message and code, and the input and output as formatted JSON (`None` while null). With nothing selected it
  says `Select a node to see its details and only its log lines.`
- **Logs** (`src/runs/log-view.tsx`, `LogView({events, nodeId})`).
  - The container has `role="log"`, label `Logs`, `aria-live="off"` and `tabIndex={0}`.
  - It shows the `node.log` lines of one node, or of every node (each prefixed with its node id) when `nodeId`
    is null.
  - A `node.log_truncated` event becomes a `Log truncated at 10 MiB` row. At the cap, the note
    `Showing the newest 2,000 events.` appears.
  - It follows new lines while the reader stays within 24 px of the bottom.
  - On the run page it sits in a bottom drawer that starts open, with the same toggle as Task 15's: a ghost
    `Logs` button with `aria-expanded` and `aria-controls`, and a panel that is only `hidden` when closed. The
    log is filtered by the node selected in the graph.
- **Header.** The header holds:
  - a breadcrumb to Workflows and the workflow's studio, with the heading `Run <created time>`;
  - the run status in a `role="status"` region labelled `Run status`, so the final state is announced;
  - `v<version>`, and `Cancel requested` while a requested cancel has not finished the run.
  - For `run_workflows` only, two actions. Cancel (`POST /orgs/{org}/runs/{run}/cancel`) shows while the run is
    not terminal and is disabled once a cancel was requested. Rerun (`POST /orgs/{org}/runs/{run}/rerun` with
    `{}`, so the source version) opens the new run.
  - Both go through `useOrganizationMutation`, so the organization refresh also brings `cancelRequested`. A
    failure shows `<Cancel|Rerun> failed (<code>).` in a polite live region.
- **Studio Logs tab.** `RunsDrawer` gets a second tab, `Logs`. It renders `LogView` for the newest run in
  `useRuns`, every node's lines. base-ui unmounts an inactive tab's panel (`keepMounted` is false), so the tab
  streams only while it is active; the drawer's `open` state is passed down, so closing the drawer closes the
  stream too.
- **Badges.** `StatusBadge` gains the node-run states it lacked: `blocked`, `retry_wait` (`Retrying`) and
  `skipped`. Every `NodeRunState` is now a `StatusBadge` status, and the hues stay in `components/status/`
  (M3-38).
- **Test EventSource.** jsdom has no `EventSource`, and `msw/sse` needs one. `src/test/event-source.ts` is the
  verified fetch-based polyfill from research (b). It installs itself only when `EventSource` is undefined, and
  `setup.ts` imports it first. Tests serve streams with `sse()` from `msw/sse` in `server.use`.

- [ ] **Step 1: Add the test EventSource, the event type and the run fixture**

Create `apps/web/src/test/event-source.ts`:

```ts
// ponytail: a test-only EventSource over fetch. jsdom has none, and Node's
// (behind --experimental-eventsource) uses undici internals that MSW never
// sees. msw/sse needs a global EventSource and serves the stream through its
// fetch interceptor, so this one is enough. It follows the WHATWG processing
// model for what the app relies on: open, unnamed and named events, `id:` ->
// lastEventId, `retry:`, Last-Event-ID on reconnect, fail (CLOSED, no retry)
// on a non-200 or non-event-stream response, reconnect when the body ends or
// the network fails. Line splitting ignores a CRLF split across chunks.
export class TestEventSource extends EventTarget {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSED = 2
  readonly CONNECTING = 0
  readonly OPEN = 1
  readonly CLOSED = 2
  readonly url: string
  readonly withCredentials: boolean
  readyState = 0
  onopen: ((event: Event) => void) | null = null
  onmessage: ((event: MessageEvent) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  #lastEventId = ""
  #reconnectionTime = 3000
  #abort = new AbortController()

  constructor(url: string | URL, init?: EventSourceInit) {
    super()
    this.url = new URL(url, location.href).href
    this.withCredentials = init?.withCredentials ?? false
    this.addEventListener("open", (event) => this.onopen?.(event))
    this.addEventListener("message", (event) =>
      this.onmessage?.(event as MessageEvent),
    )
    this.addEventListener("error", (event) => this.onerror?.(event))
    void this.#connect()
  }

  close() {
    this.readyState = this.CLOSED
    this.#abort.abort()
  }

  async #connect() {
    const headers = new Headers({ accept: "text/event-stream" })
    if (this.#lastEventId) headers.set("last-event-id", this.#lastEventId)
    let response: Response
    try {
      response = await fetch(this.url, {
        headers,
        cache: "no-store",
        credentials: this.withCredentials ? "include" : "same-origin",
        signal: this.#abort.signal,
      })
    } catch {
      return this.#reconnect()
    }
    if (this.readyState === this.CLOSED) return
    const type = response.headers.get("content-type")?.split(";")[0].trim()
    if (response.status !== 200 || type !== "text/event-stream") {
      this.readyState = this.CLOSED
      this.dispatchEvent(new Event("error"))
      return
    }
    this.readyState = this.OPEN
    this.dispatchEvent(new Event("open"))
    try {
      await this.#read(response.body!)
    } catch {
      // Aborted by close() or a broken stream: both end the same way.
    }
    this.#reconnect()
  }

  async #read(body: ReadableStream<Uint8Array>) {
    const reader = body.getReader()
    const decoder = new TextDecoder()
    let buffer = ""
    let data: string[] = []
    let event = ""
    for (;;) {
      const { done, value } = await reader.read()
      if (done) return
      buffer += decoder.decode(value, { stream: true })
      const lines = buffer.split(/\r\n|\r|\n/)
      buffer = lines.pop() ?? ""
      for (const line of lines) {
        if (line === "") {
          if (data.length > 0 && this.readyState !== this.CLOSED) {
            this.dispatchEvent(
              new MessageEvent(event || "message", {
                data: data.join("\n"),
                lastEventId: this.#lastEventId,
                origin: new URL(this.url).origin,
              }),
            )
          }
          data = []
          event = ""
          continue
        }
        if (line.startsWith(":")) continue
        const colon = line.indexOf(":")
        const field = colon === -1 ? line : line.slice(0, colon)
        const raw = colon === -1 ? "" : line.slice(colon + 1)
        const text = raw.startsWith(" ") ? raw.slice(1) : raw
        if (field === "data") data.push(text)
        else if (field === "event") event = text
        else if (field === "id" && !text.includes("\0"))
          this.#lastEventId = text
        else if (field === "retry" && /^\d+$/.test(text)) {
          this.#reconnectionTime = Number(text)
        }
      }
    }
  }

  #reconnect() {
    if (this.readyState === this.CLOSED) return
    this.readyState = this.CONNECTING
    this.dispatchEvent(new Event("error"))
    setTimeout(() => {
      if (this.readyState === this.CONNECTING) void this.#connect()
    }, this.#reconnectionTime)
  }
}

if (typeof globalThis.EventSource === "undefined") {
  Object.assign(globalThis, { EventSource: TestEventSource })
}
```

In `apps/web/src/test/setup.ts`, apply these replacements in order:

Replace:

```ts
import "@testing-library/jest-dom/vitest"
import { cleanup } from "@testing-library/react"
```

With:

```ts
// First: msw/sse handlers need a global EventSource when they are created.
import "@/test/event-source"
import "@testing-library/jest-dom/vitest"
import { cleanup } from "@testing-library/react"
```

In `apps/web/src/api/types.ts`, apply these replacements in order:

Replace:

```ts
  graph: GraphSnapshot
  nodes: NodeRun[]
}
```

With:

```ts
  graph: GraphSnapshot
  nodes: NodeRun[]
}

// Event payloads omit absent keys: a node.state carries `error` when the node
// failed or waits to retry, and `output` when it succeeded.
export type RunEvent =
  | { seq: number; type: "run.state"; state: RunStatus }
  | {
      seq: number
      type: "node.state"
      nodeId: string
      state: NodeRunState
      attempt: number
      error?: NonNullable<NodeRun["error"]>
      output?: Record<string, unknown>
    }
  | {
      seq: number
      type: "node.log"
      nodeId: string
      stream: "stdout" | "stderr" | "system"
      text: string
    }
  | { seq: number; type: "node.log_truncated"; nodeId: string }
```

The fixture `run` is run-2 as `GET /runs/{id}` returns it after its first three events: VPC running, RDS and
deploy blocked. A default handler serves it to every test.

In `apps/web/src/test/handlers.ts`, apply these replacements in order:

Replace:

```ts
  NodeDefinition,
  PortSpec,
  Role,
  RunSummary,
```

With:

```ts
  NodeDefinition,
  NodeRun,
  PortSpec,
  Role,
  Run,
  RunSummary,
```

Replace:

```ts
    finishedAt: "2026-10-25T09:00:04Z",
    cancelRequested: false,
  },
]
```

With:

```ts
    finishedAt: "2026-10-25T09:00:04Z",
    cancelRequested: false,
  },
]

function nodeRun(nodeId: string, changes: Partial<NodeRun> = {}): NodeRun {
  return {
    nodeId,
    state: "blocked",
    attempt: 0,
    maxAttempts: 1,
    executor: "mock@1.0.0",
    queueReason: null,
    input: null,
    output: null,
    error: null,
    startedAt: null,
    finishedAt: null,
    ...changes,
  }
}

// run-2 in detail, after its events 1-3: queued, VPC running, run running.
export const run: Run = {
  ...runs[0],
  workflowName: "Provision staging",
  input: { region: "eu-west-1" },
  lastEventSeq: 3,
  graph: workflowVersion.graph,
  nodes: [
    nodeRun("vpc", {
      state: "running",
      attempt: 1,
      input: { region: "eu-west-1" },
      startedAt: "2026-10-26T09:00:01Z",
    }),
    nodeRun("rds"),
    nodeRun("deploy"),
  ],
}
```

Replace:

```ts
  http.get("/api/v1/orgs/:organizationId/workflows/:workflowId/runs", () =>
    HttpResponse.json({ items: runs }),
  ),
```

With:

```ts
  http.get("/api/v1/orgs/:organizationId/workflows/:workflowId/runs", () =>
    HttpResponse.json({ items: runs }),
  ),
  http.get("/api/v1/orgs/:organizationId/runs/:runId", () =>
    HttpResponse.json(run),
  ),
```

- [ ] **Step 2: Write the failing stream tests**

A probe renders `useRun`, `useRunEvents` and `useRunEventLog` for run-2. The tests cover these cases:
- A replay from the start applies each event once. A replayed `run.state queued` (seq 1, below the snapshot's
  `lastEventSeq` 3) does not move the run back.
- A dropped stream reconnects with `Last-Event-ID`, and the URL keeps the first `after`.
- After the browser gives up (CLOSED), the run is read again and the stream reopens with `?after=5`.
- The terminal event closes the source, so a server that ends the stream with `retry: 10` sees one connection.
- `mergeEvents` keeps the newest 2,000 events and returns the same log when nothing is new.

Create `apps/web/src/runs/use-run-events.test.tsx`:

```tsx
import { screen, waitFor, within } from "@testing-library/react"
import { http, HttpResponse } from "msw/http"
import { sse } from "msw/sse"
import { expect, it } from "vitest"

import { useRun } from "@/api/queries"
import type { RunEvent } from "@/api/types"
import {
  EVENT_LIMIT,
  mergeEvents,
  useRunEventLog,
  useRunEvents,
} from "@/runs/use-run-events"
import { run } from "@/test/handlers"
import { renderWithProviders } from "@/test/render"
import { server } from "@/test/server"

const runUrl = "/api/v1/orgs/:organizationId/runs/:runId"
const eventsUrl = `${runUrl}/events`

const log = (seq: number): RunEvent => ({
  seq,
  type: "node.log",
  nodeId: "vpc",
  stream: "stdout",
  text: `line ${seq}`,
})
const vpcSucceeded: RunEvent = {
  seq: 5,
  type: "node.state",
  nodeId: "vpc",
  state: "succeeded",
  attempt: 1,
  output: { vpcId: "vpc-0simulated" },
}
const succeeded: RunEvent = { seq: 9, type: "run.state", state: "succeeded" }

function RunProbe() {
  const query = useRun("org-1", "run-2")
  useRunEvents("org-1", "run-2", query.isSuccess)
  const events = useRunEventLog("org-1", "run-2")
  return (
    <>
      <p>Run {query.data?.status}</p>
      <ul aria-label="Nodes">
        {query.data?.nodes.map((node) => (
          <li key={node.nodeId}>
            {node.nodeId} {node.state}
          </li>
        ))}
      </ul>
      <ol aria-label="Events">
        {events.data?.events.map((event) => (
          <li key={event.seq}>{event.seq}</li>
        ))}
      </ol>
    </>
  )
}

const shownSeqs = () =>
  within(screen.getByRole("list", { name: "Events" }))
    .queryAllByRole("listitem")
    .map((item) => item.textContent)

it("applies each streamed event once, and never behind the snapshot", async () => {
  const afters: (string | null)[] = []
  server.use(
    sse<{ message: RunEvent }>(eventsUrl, ({ client, request }) => {
      afters.push(new URL(request.url).searchParams.get("after"))
      // Replayed from the start: the snapshot already reflects events 1-3.
      client.send({
        id: "1",
        data: { seq: 1, type: "run.state", state: "queued" },
      })
      client.send({ id: "4", data: log(4) })
      client.send({ id: "5", data: vpcSucceeded })
      client.send({ id: "5", data: vpcSucceeded })
      client.send({ id: "6", data: log(6) })
    }),
  )
  renderWithProviders(<RunProbe />)

  await waitFor(() => expect(shownSeqs()).toEqual(["1", "4", "5", "6"]))
  expect(screen.getByText("vpc succeeded")).toBeInTheDocument()
  expect(screen.getByText("Run running")).toBeInTheDocument()
  expect(afters).toEqual(["0"])
})

it("resumes with Last-Event-ID when the stream drops", async () => {
  const reconnects: { lastEventId: string | null; after: string | null }[] = []
  let connections = 0
  server.use(
    sse<{ message: RunEvent }>(eventsUrl, ({ client, request }) => {
      connections += 1
      if (connections === 1) {
        client.send({ retry: 10 })
        client.send({ id: "4", data: log(4) })
        client.send({ id: "5", data: vpcSucceeded })
        // Ending the body makes the browser reconnect after `retry` ms.
        client.close()
        return
      }
      reconnects.push({
        lastEventId: request.headers.get("last-event-id"),
        after: new URL(request.url).searchParams.get("after"),
      })
      client.send({ id: "5", data: vpcSucceeded })
      client.send({ id: "9", data: succeeded })
    }),
  )
  renderWithProviders(<RunProbe />)

  await waitFor(() => expect(screen.getByText("Run succeeded")).toBeVisible())
  expect(shownSeqs()).toEqual(["4", "5", "9"])
  // The URL keeps the first `after`: the server prefers the header.
  expect(reconnects).toEqual([{ lastEventId: "5", after: "0" }])
})

it("reconnects with ?after= once the browser gives up", async () => {
  const afters: (string | null)[] = []
  let connections = 0
  let runReads = 0
  server.use(
    http.get(runUrl, () => {
      runReads += 1
      return HttpResponse.json(run)
    }),
    http.get(eventsUrl, () => {
      connections += 1
      // The browser's own retry fails for good (CLOSED).
      if (connections === 2) {
        return HttpResponse.json({ code: "unavailable" }, { status: 503 })
      }
    }),
    sse<{ message: RunEvent }>(eventsUrl, ({ client, request }) => {
      afters.push(new URL(request.url).searchParams.get("after"))
      if (afters.length === 1) {
        client.send({ retry: 10 })
        client.send({ id: "4", data: log(4) })
        client.send({ id: "5", data: vpcSucceeded })
        client.close()
        return
      }
      client.send({ id: "9", data: succeeded })
    }),
  )
  renderWithProviders(<RunProbe />)

  await waitFor(() => expect(screen.getByText("Run succeeded")).toBeVisible(), {
    timeout: 3000,
  })
  expect(afters).toEqual(["0", "5"])
  expect(shownSeqs()).toEqual(["4", "5", "9"])
  // The run is read again before the new connection.
  expect(runReads).toBe(2)
})

it("closes the stream on the terminal run state", async () => {
  let connections = 0
  server.use(
    sse<{ message: RunEvent }>(eventsUrl, ({ client }) => {
      connections += 1
      client.send({ retry: 10 })
      client.send({ id: "9", data: succeeded })
      // A browser that kept the stream would reconnect after 10 ms.
      client.close()
    }),
  )
  renderWithProviders(<RunProbe />)

  await waitFor(() => expect(screen.getByText("Run succeeded")).toBeVisible())
  await new Promise((resolve) => setTimeout(resolve, 100))
  expect(connections).toBe(1)
})

it("keeps the newest events and drops ones it has", () => {
  const many = Array.from({ length: EVENT_LIMIT + 1 }, (_, index) =>
    log(index + 1),
  )
  const merged = mergeEvents(undefined, many)
  expect(merged.events).toHaveLength(EVENT_LIMIT)
  expect(merged.events[0].seq).toBe(2)
  expect(merged.lastSeq).toBe(EVENT_LIMIT + 1)
  expect(mergeEvents(merged, [log(5)])).toBe(merged)
})
```

- [ ] **Step 3: Write the failing log view tests**

Create `apps/web/src/runs/log-view.test.tsx`:

```tsx
import { fireEvent, render, screen } from "@testing-library/react"
import { expect, it } from "vitest"

import type { RunEvent } from "@/api/types"
import { LogView } from "@/runs/log-view"
import { EVENT_LIMIT } from "@/runs/use-run-events"

const line = (
  seq: number,
  nodeId = "vpc",
  stream: "stdout" | "stderr" | "system" = "stdout",
): RunEvent => ({ seq, type: "node.log", nodeId, stream, text: `line ${seq}` })

const rows = () =>
  screen.getAllByRole("listitem").map((item) => item.textContent)

// jsdom has no layout: give the log a fixed geometry.
function geometry(element: HTMLElement, scrollHeight: number) {
  Object.defineProperties(element, {
    scrollHeight: { configurable: true, value: scrollHeight },
    clientHeight: { configurable: true, value: 100 },
  })
}

it("follows new lines until the reader scrolls up", () => {
  const { rerender } = render(<LogView events={[line(1)]} nodeId="vpc" />)
  const log = screen.getByRole("log", { name: "Logs" })
  geometry(log, 500)
  rerender(<LogView events={[line(1), line(2)]} nodeId="vpc" />)
  expect(log.scrollTop).toBe(500)

  log.scrollTop = 100
  fireEvent.scroll(log)
  geometry(log, 900)
  rerender(<LogView events={[line(1), line(2), line(3)]} nodeId="vpc" />)
  expect(log.scrollTop).toBe(100)
  expect(rows()).toEqual(["line 1", "line 2", "line 3"])
})

it("shows one node's lines and marks a truncated log", () => {
  const events: RunEvent[] = [
    line(1, "vpc"),
    { seq: 2, type: "node.state", nodeId: "rds", state: "running", attempt: 1 },
    line(3, "rds", "stderr"),
    { seq: 4, type: "node.log_truncated", nodeId: "rds" },
  ]
  const { rerender } = render(<LogView events={events} nodeId={null} />)
  // Every node's lines, each with its node.
  expect(rows()).toEqual([
    "vpc line 1",
    "rds line 3",
    "rds Log truncated at 10 MiB",
  ])

  rerender(<LogView events={events} nodeId="rds" />)
  expect(rows()).toEqual(["line 3", "Log truncated at 10 MiB"])
  expect(screen.getByText("line 3")).toHaveAttribute("data-stream", "stderr")

  rerender(<LogView events={events} nodeId="deploy" />)
  expect(screen.getByText("No log output yet.")).toBeInTheDocument()
})

it("says when older events were dropped", () => {
  const events = Array.from({ length: EVENT_LIMIT }, (_, index) =>
    line(index + 1),
  )
  render(<LogView events={events} nodeId="deploy" />)
  expect(
    screen.getByText("Showing the newest 2,000 events."),
  ).toBeInTheDocument()
})
```

- [ ] **Step 4: Write the failing run page tests**

The page renders inside `AuthGate` and `AppShell`, as in the studio tests. The tests cover these cases:
- Events arriving on an open stream change the overlays in place on the pinned graph. The terminal event
  updates the announced status and removes Cancel.
- Selecting a node shows its details and filters the log. The drawer toggle hides the log.
- A runner's Cancel and Rerun send exactly one request each, and Rerun opens the new run. A 409 shows its code.
- A viewer has neither button.
- At 800 px a runner still has the graph, the details, the log and Cancel, and no 1024-pixel notice.

Create `apps/web/src/runs/run-view.test.tsx`:

```tsx
import { fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { http, HttpResponse } from "msw/http"
import { sse } from "msw/sse"
import { expect, it } from "vitest"

import type { Run, RunEvent } from "@/api/types"
import { AuthGate } from "@/auth/auth-gate"
import { AppShell } from "@/layout/app-shell"
// Loaded up front so next/dynamic resolves within the findBy timeouts.
import "@/runs/run-graph"
import { RunView } from "@/runs/run-view"
import { meAs, run, runs } from "@/test/handlers"
import { renderWithProviders, router } from "@/test/render"
import { server } from "@/test/server"

const runUrl = "/api/v1/orgs/:organizationId/runs/:runId"

function renderRun() {
  return renderWithProviders(
    <AuthGate>
      <AppShell organizationId="org-1">
        <RunView organizationId="org-1" runId="run-2" />
      </AppShell>
    </AuthGate>,
    { route: "/orgs/org-1/runs/run-2" },
  )
}

function withRun(changes: Partial<Run>) {
  server.use(http.get(runUrl, () => HttpResponse.json({ ...run, ...changes })))
}

function signedInAs(role: "runner" | "viewer") {
  server.use(http.get("/api/v1/auth/me", () => HttpResponse.json(meAs(role))))
}

// Each connection gets a function that sends it events.
function stream(...initial: RunEvent[]) {
  const connections: ((event: RunEvent) => void)[] = []
  server.use(
    sse<{ message: RunEvent }>(`${runUrl}/events`, ({ client }) => {
      const send = (event: RunEvent) =>
        client.send({ id: String(event.seq), data: event })
      initial.forEach(send)
      connections.push(send)
    }),
  )
  return connections
}

const log = (seq: number, nodeId: string): RunEvent => ({
  seq,
  type: "node.log",
  nodeId,
  stream: "stdout",
  text: `line ${seq}`,
})

const nodeOf = (id: string) => screen.findByTestId(`rf__node-${id}`)

it("overlays node-run states on the pinned graph as events arrive", async () => {
  // The run shows the graph it was started from, not the current draft.
  withRun({
    graph: {
      ...run.graph,
      nodes: run.graph.nodes.map((node) =>
        node.id === "vpc" ? { ...node, displayName: "VPC as published" } : node,
      ),
    },
  })
  const connections = stream()
  renderRun()
  const vpc = await nodeOf("vpc")
  expect(within(vpc).getByText("VPC as published")).toBeInTheDocument()
  expect(within(vpc).getByText("Running")).toBeInTheDocument()
  expect(within(await nodeOf("rds")).getByText("Blocked")).toBeInTheDocument()
  await waitFor(() => expect(connections).toHaveLength(1))

  connections[0]({
    seq: 4,
    type: "node.state",
    nodeId: "vpc",
    state: "succeeded",
    attempt: 1,
    output: { vpcId: "vpc-0simulated" },
  })
  connections[0]({
    seq: 5,
    type: "node.state",
    nodeId: "rds",
    state: "queued",
    attempt: 0,
  })
  await waitFor(() =>
    expect(within(vpc).getByText("Succeeded")).toBeInTheDocument(),
  )
  expect(
    within(screen.getByTestId("rf__node-rds")).getByText("Queued"),
  ).toBeInTheDocument()
  // The overlay changed in place: the node was not rebuilt.
  expect(screen.getByTestId("rf__node-vpc")).toBe(vpc)

  connections[0]({ seq: 9, type: "run.state", state: "succeeded" })
  const status = screen.getByRole("status", { name: "Run status" })
  await waitFor(() => expect(status).toHaveTextContent("Succeeded"))
  expect(
    screen.queryByRole("button", { name: "Cancel" }),
  ).not.toBeInTheDocument()
})

it("shows the selected node's details and only its log", async () => {
  withRun({
    nodes: [
      {
        ...run.nodes[0],
        state: "succeeded",
        output: { vpcId: "vpc-0simulated" },
      },
      {
        ...run.nodes[1],
        state: "failed",
        attempt: 2,
        maxAttempts: 2,
        input: { vpcId: "vpc-0simulated" },
        error: {
          code: "mock_failure",
          message: "The database did not start.",
          retryable: false,
        },
      },
      {
        ...run.nodes[2],
        state: "queued",
        queueReason: "waiting_for_compatible_worker",
      },
    ],
  })
  stream(log(4, "vpc"), log(5, "rds"))
  renderRun()
  const details = await screen.findByRole("complementary", {
    name: "Node details",
  })
  expect(details).toHaveTextContent("Select a node to see its details")
  const logs = screen.getByRole("log", { name: "Logs" })
  expect(await within(logs).findByText("line 5")).toBeInTheDocument()
  expect(within(logs).getByText("line 4")).toBeInTheDocument()

  fireEvent.click(
    await screen.findByRole("group", { name: "Create RDS (simulated)" }),
  )
  expect(
    within(details).getByRole("heading", { name: "Create RDS (simulated)" }),
  ).toBeInTheDocument()
  expect(within(details).getByText("Failed")).toBeInTheDocument()
  expect(within(details).getByText("2 of 2")).toBeInTheDocument()
  expect(details).toHaveTextContent("The database did not start.")
  expect(details).toHaveTextContent("mock_failure")
  const figure = (caption: string) =>
    within(details).getByText(caption).closest("figure")
  expect(figure("Input")).toHaveTextContent('{ "vpcId": "vpc-0simulated" }')
  expect(figure("Output")).toHaveTextContent("None")
  expect(within(logs).getByText("line 5")).toBeInTheDocument()
  expect(within(logs).queryByText("line 4")).not.toBeInTheDocument()

  fireEvent.click(
    screen.getByRole("group", { name: "Deploy service (simulated)" }),
  )
  expect(details).toHaveTextContent(
    "Waiting for a worker that runs mock@1.0.0.",
  )

  // The log drawer folds away.
  const toggle = screen.getByRole("button", { name: "Logs" })
  expect(toggle).toHaveAttribute("aria-expanded", "true")
  await userEvent.click(toggle)
  expect(toggle).toHaveAttribute("aria-expanded", "false")
  expect(logs).not.toBeVisible()
})

it("a runner cancels the run and then reruns it", async () => {
  signedInAs("runner")
  const requests: string[] = []
  let cancelRequested = false
  server.use(
    http.get(runUrl, () => HttpResponse.json({ ...run, cancelRequested })),
    http.post(`${runUrl}/cancel`, ({ params }) => {
      requests.push(`cancel ${params.runId}`)
      cancelRequested = true
      return HttpResponse.json({ ...runs[0], cancelRequested })
    }),
    http.post(`${runUrl}/rerun`, async ({ params, request }) => {
      requests.push(
        `rerun ${params.runId} ${JSON.stringify(await request.json())}`,
      )
      return HttpResponse.json(
        { ...run, id: "run-3", status: "queued", lastEventSeq: 1 },
        { status: 201 },
      )
    }),
  )
  stream()
  renderRun()
  await userEvent.click(await screen.findByRole("button", { name: "Cancel" }))
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled(),
  )
  expect(screen.getByText("Cancel requested")).toBeInTheDocument()

  await userEvent.click(screen.getByRole("button", { name: "Rerun" }))
  await waitFor(() =>
    expect(router.push).toHaveBeenCalledWith("/orgs/org-1/runs/run-3"),
  )
  expect(requests).toEqual(["cancel run-2", "rerun run-2 {}"])
})

it("Cancel says why it failed", async () => {
  server.use(
    http.post(`${runUrl}/cancel`, () =>
      HttpResponse.json({ status: 409, code: "run_finished" }, { status: 409 }),
    ),
  )
  stream()
  renderRun()
  await userEvent.click(await screen.findByRole("button", { name: "Cancel" }))
  expect(
    await screen.findByText("Cancel failed (run_finished)."),
  ).toBeInTheDocument()
})

it("a viewer follows the run without Cancel or Rerun", async () => {
  signedInAs("viewer")
  stream()
  renderRun()
  expect(await nodeOf("vpc")).toBeInTheDocument()
  expect(
    screen.queryByRole("button", { name: "Cancel" }),
  ).not.toBeInTheDocument()
  expect(
    screen.queryByRole("button", { name: "Rerun" }),
  ).not.toBeInTheDocument()
})

it("on a narrow screen a runner still sees the graph, details, log and Cancel", async () => {
  Object.assign(window, { innerWidth: 800 })
  signedInAs("runner")
  stream(log(4, "vpc"))
  renderRun()
  expect(await nodeOf("vpc")).toBeVisible()
  expect(
    screen.getByRole("complementary", { name: "Node details" }),
  ).toBeVisible()
  expect(await screen.findByText("line 4")).toBeVisible()
  expect(screen.getByRole("button", { name: "Cancel" })).toBeVisible()
  expect(
    screen.queryByText("Editing needs a screen at least 1024 pixels wide."),
  ).not.toBeInTheDocument()
})
```

- [ ] **Step 5: Extend the studio tests**

The new test opens the drawer on the Logs tab and sees a streamed line of run-2, the newest run. It then closes
the drawer and checks that the `EventSource` was closed.

In `apps/web/src/studio/studio.test.tsx`, apply these replacements in order:

Replace:

```tsx
import { http, HttpResponse } from "msw/http"
import type { ReactNode } from "react"
import { expect, it } from "vitest"
```

With:

```tsx
import { http, HttpResponse } from "msw/http"
import { sse } from "msw/sse"
import type { ReactNode } from "react"
import { expect, it, vi } from "vitest"
```

Replace:

```tsx
  JsonSchema,
  Workflow,
} from "@/api/types"
```

With:

```tsx
  JsonSchema,
  RunEvent,
  Workflow,
} from "@/api/types"
```

Replace:

```tsx
  expect(links.map((link) => link.getAttribute("href"))).toEqual([
    "/orgs/org-1/runs/run-2",
    "/orgs/org-1/runs/run-1",
  ])
})
```

With:

```tsx
  expect(links.map((link) => link.getAttribute("href"))).toEqual([
    "/orgs/org-1/runs/run-2",
    "/orgs/org-1/runs/run-1",
  ])
})

it("the Logs tab streams the newest run's log while the drawer is open", async () => {
  const streamed: string[] = []
  server.use(
    sse<{ message: RunEvent }>(
      "/api/v1/orgs/:organizationId/runs/:runId/events",
      ({ client, params }) => {
        streamed.push(String(params.runId))
        client.send({
          id: "4",
          data: {
            seq: 4,
            type: "node.log",
            nodeId: "vpc",
            stream: "stdout",
            text: "creating the VPC",
          },
        })
      },
    ),
  )
  const close = vi.spyOn(EventSource.prototype, "close")
  renderStudio()
  const toggle = await screen.findByRole("button", { name: "Runs" })
  await userEvent.click(toggle)
  await userEvent.click(screen.getByRole("tab", { name: "Logs" }))
  const log = await screen.findByRole("log", { name: "Logs" })
  expect(await within(log).findByText("creating the VPC")).toBeInTheDocument()
  expect(streamed).toEqual(["run-2"])
  expect(close).not.toHaveBeenCalled()

  await userEvent.click(toggle)
  expect(close).toHaveBeenCalledTimes(1)
  close.mockRestore()
})
```

- [ ] **Step 6: Run the tests and verify RED**

Run:

```text
pnpm web:test
```

Expected: `Test Files  4 failed | 12 passed (16)` and `Tests  1 failed | 95 passed (96)`:
- The three new files fail to load with `Error: Failed to resolve import "<module>" from "<test file>". Does the
  file exist?`. Vite names the first missing module it reaches:
  - `use-run-events.test.tsx` names `@/runs/use-run-events`;
  - `log-view.test.tsx` names `@/runs/log-view` or `@/runs/use-run-events`;
  - `run-view.test.tsx` names `@/runs/run-graph`.
- `the Logs tab streams the newest run's log while the drawer is open` fails with
  `TestingLibraryElementError: Unable to find an accessible element with the role "tab" and name "Logs"`.

- [ ] **Step 7: Add the run queries and the event stream**

In `apps/web/src/api/queries.ts`, apply these replacements in order:

Replace:

```ts
  runs: (organizationId: string, workflowId: string) =>
    ["orgs", organizationId, "workflows", workflowId, "runs"] as const,
```

With:

```ts
  runs: (organizationId: string, workflowId: string) =>
    ["orgs", organizationId, "workflows", workflowId, "runs"] as const,
  run: (organizationId: string, runId: string) =>
    ["orgs", organizationId, "runs", runId] as const,
  runEvents: (organizationId: string, runId: string) =>
    ["orgs", organizationId, "runs", runId, "events"] as const,
```

Replace:

```ts
      api<Run>(`/orgs/${organizationId}/workflows/${workflowId}/runs`, {
        method: "POST",
        body: JSON.stringify(body),
      }),
  )
}
```

With:

```ts
      api<Run>(`/orgs/${organizationId}/workflows/${workflowId}/runs`, {
        method: "POST",
        body: JSON.stringify(body),
      }),
  )
}

// The run stream also writes this run (runs/use-run-events.ts). A refetch
// never moves it back behind events already applied.
export function useRun(organizationId: string, runId: string) {
  return useQuery({
    queryKey: queryKeys.run(organizationId, runId),
    queryFn: async ({ signal, client, queryKey }) => {
      const fresh = await api<Run>(`/orgs/${organizationId}/runs/${runId}`, {
        signal,
      })
      const cached = client.getQueryData<Run>(queryKey)
      return cached && cached.lastEventSeq > fresh.lastEventSeq ? cached : fresh
    },
  })
}

// The refresh that follows brings the run's cancelRequested flag.
export function useCancelRun(organizationId: string) {
  return useOrganizationMutation(organizationId, (runId: string) =>
    api<RunSummary>(`/orgs/${organizationId}/runs/${runId}/cancel`, {
      method: "POST",
    }),
  )
}

// A rerun starts the source run's version with its input.
export function useRerunRun(organizationId: string) {
  return useOrganizationMutation(organizationId, (runId: string) =>
    api<Run>(`/orgs/${organizationId}/runs/${runId}/rerun`, {
      method: "POST",
      body: JSON.stringify({}),
    }),
  )
}
```

Create `apps/web/src/runs/use-run-events.ts`:

```ts
"use client"

import { skipToken, useQuery, useQueryClient } from "@tanstack/react-query"
import { useEffect } from "react"

import { queryKeys } from "@/api/queries"
import type { Run, RunEvent, RunStatus } from "@/api/types"

export type RunEventLog = { lastSeq: number; events: RunEvent[] }

// ponytail: the newest 2,000 events of a run stay in memory; older ones stay
// on the server.
export const EVENT_LIMIT = 2000

export function isTerminal(status: RunStatus) {
  return status === "succeeded" || status === "failed" || status === "canceled"
}

// Written only by useRunEvents.
export function useRunEventLog(organizationId: string, runId: string) {
  return useQuery<RunEventLog>({
    queryKey: queryKeys.runEvents(organizationId, runId),
    queryFn: skipToken,
  })
}

// Drops events it already has and keeps the newest EVENT_LIMIT.
export function mergeEvents(
  log: RunEventLog | undefined,
  incoming: RunEvent[],
): RunEventLog {
  let lastSeq = log?.lastSeq ?? 0
  const fresh = incoming.filter((event) => {
    if (event.seq <= lastSeq) return false
    lastSeq = event.seq
    return true
  })
  if (fresh.length === 0 && log) return log
  return {
    lastSeq,
    events: [...(log?.events ?? []), ...fresh].slice(-EVENT_LIMIT),
  }
}

// Events the snapshot already reflects (seq <= lastEventSeq) change nothing.
// A node.state carries only the keys it changes.
export function applyEvents(run: Run, events: RunEvent[]): Run {
  const fresh = events.filter((event) => event.seq > run.lastEventSeq)
  if (fresh.length === 0) return run
  let { status, nodes } = run
  for (const event of fresh) {
    if (event.type === "run.state") status = event.state
    if (event.type === "node.state") {
      nodes = nodes.map((node) =>
        node.nodeId === event.nodeId
          ? {
              ...node,
              state: event.state,
              attempt: event.attempt,
              error: event.error ?? node.error,
              output: event.output ?? node.output,
            }
          : node,
      )
    }
  }
  return { ...run, status, nodes, lastEventSeq: fresh.at(-1)!.seq }
}

// Streams a run's events into the query cache while `enabled`. Pass
// `run.isSuccess`, so a 401 or 404 on a run refetch closes the stream. It
// resumes after the newest event in the cache, or replays from the start.
export function useRunEvents(
  organizationId: string,
  runId: string,
  enabled: boolean,
) {
  const queryClient = useQueryClient()
  useEffect(() => {
    if (!enabled) return
    const runKey = queryKeys.run(organizationId, runId)
    const eventsKey = queryKeys.runEvents(organizationId, runId)
    const lastSeq = () =>
      queryClient.getQueryData<RunEventLog>(eventsKey)?.lastSeq ?? 0
    // Nothing is left when the run is terminal and every event it reports
    // has arrived.
    const done = () => {
      const run = queryClient.getQueryData<Run>(runKey)
      return (
        run !== undefined &&
        isTerminal(run.status) &&
        lastSeq() >= run.lastEventSeq
      )
    }
    let source: EventSource | undefined
    let retryTimer: ReturnType<typeof setTimeout> | undefined
    let flushTimer: ReturnType<typeof setTimeout> | undefined
    let pending: RunEvent[] = []
    let failures = 0
    let stopped = false

    // One cache write per 100 ms keeps a log flood from rendering per line.
    function flush() {
      clearTimeout(flushTimer)
      flushTimer = undefined
      const batch = pending
      pending = []
      if (batch.length === 0) return
      queryClient.setQueryData<RunEventLog>(eventsKey, (log) =>
        mergeEvents(log, batch),
      )
      queryClient.setQueryData<Run>(runKey, (run) =>
        run ? applyEvents(run, batch) : run,
      )
    }

    function connect() {
      if (stopped || done()) return
      const current = new EventSource(
        `/api/v1/orgs/${organizationId}/runs/${runId}/events?after=${lastSeq()}`,
      )
      source = current
      current.onopen = () => {
        failures = 0
      }
      current.onmessage = (message) => {
        const event = JSON.parse(message.data) as RunEvent
        pending.push(event)
        flushTimer ??= setTimeout(flush, 100)
        // The server ends the stream after the terminal event; closing first
        // stops the browser from reconnecting.
        if (event.type === "run.state" && isTerminal(event.state)) {
          current.close()
          flush()
        }
      }
      current.onerror = () => {
        // CONNECTING: the browser retries by itself and sends Last-Event-ID.
        if (current.readyState !== EventSource.CLOSED) return
        // CLOSED: a non-200 (204 included) that the browser never retries.
        // The status is hidden, so read the run first: a 401 or 404 turns
        // `enabled` off, and a finished run ends in done().
        flush()
        void queryClient
          .invalidateQueries({ queryKey: runKey, exact: true })
          .then(() => {
            if (stopped) return
            const delay = Math.min(1000 * 2 ** failures++, 30_000)
            retryTimer = setTimeout(connect, delay)
          })
      }
    }

    connect()
    return () => {
      stopped = true
      source?.close()
      clearTimeout(retryTimer)
      flush()
    }
  }, [enabled, organizationId, queryClient, runId])
}
```

- [ ] **Step 8: Give every node-run state a badge**

In `apps/web/src/components/status/status-badge.tsx`, apply these replacements in order:

Replace:

```tsx
  canceled: { label: "Canceled", className: "text-muted-foreground" },
}
```

With:

```tsx
  canceled: { label: "Canceled", className: "text-muted-foreground" },
  // The other node-run states (NodeRunState).
  blocked: { label: "Blocked", className: "text-muted-foreground" },
  retry_wait: {
    label: "Retrying",
    className: "border-amber-600/40 text-amber-700 dark:text-amber-400",
  },
  skipped: { label: "Skipped", className: "text-muted-foreground" },
}
```

- [ ] **Step 9: Add the log view**

Create `apps/web/src/runs/log-view.tsx`:

```tsx
"use client"

import { useLayoutEffect, useRef } from "react"

import type { RunEvent } from "@/api/types"
import { EVENT_LIMIT } from "@/runs/use-run-events"

type LogEvent = Extract<RunEvent, { type: "node.log" | "node.log_truncated" }>

// The log lines of one node, or of every node when `nodeId` is null. The
// cache holds at most EVENT_LIMIT events, so the list stays bounded, and rows
// off screen skip layout and paint.
export function LogView({
  events,
  nodeId,
}: {
  events: RunEvent[]
  nodeId: string | null
}) {
  const lines = events.filter(
    (event): event is LogEvent =>
      (event.type === "node.log" || event.type === "node.log_truncated") &&
      (nodeId === null || event.nodeId === nodeId),
  )
  const viewport = useRef<HTMLDivElement>(null)
  // Follow new lines only while the reader stays at the bottom.
  const pinned = useRef(true)
  const last = lines.at(-1)?.seq
  useLayoutEffect(() => {
    const element = viewport.current
    if (element && pinned.current) element.scrollTop = element.scrollHeight
  }, [last])
  return (
    <div
      ref={viewport}
      role="log"
      aria-label="Logs"
      // A flood of lines would drown a screen reader; states are announced
      // elsewhere.
      aria-live="off"
      tabIndex={0}
      onScroll={({ currentTarget: element }) => {
        pinned.current =
          element.scrollHeight - element.scrollTop - element.clientHeight < 24
      }}
      className="h-56 overflow-y-auto rounded-md bg-muted/40 p-2 font-mono text-xs"
    >
      {events.length >= EVENT_LIMIT && (
        <p className="text-muted-foreground">
          Showing the newest {EVENT_LIMIT.toLocaleString("en-US")} events.
        </p>
      )}
      {lines.length === 0 ? (
        <p className="text-muted-foreground">No log output yet.</p>
      ) : (
        <ol>
          {lines.map((line) => (
            <li
              key={line.seq}
              data-stream={line.type === "node.log" ? line.stream : undefined}
              className="break-all whitespace-pre-wrap [contain-intrinsic-size:auto_1rem] [content-visibility:auto] data-[stream=stderr]:text-destructive data-[stream=system]:text-muted-foreground"
            >
              {nodeId === null && (
                <span className="text-muted-foreground">{line.nodeId} </span>
              )}
              {line.type === "node.log" ? (
                line.text
              ) : (
                <em className="text-muted-foreground">
                  Log truncated at 10 MiB
                </em>
              )}
            </li>
          ))}
        </ol>
      )}
    </div>
  )
}
```

- [ ] **Step 10: Add the run graph and the run page**

Create `apps/web/src/runs/run-graph.tsx`:

```tsx
"use client"

import "@xyflow/react/dist/style.css"

import {
  Background,
  Controls,
  ReactFlow,
  type NodeProps,
  type NodeTypes,
  type OnSelectionChangeFunc,
} from "@xyflow/react"
import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
} from "react"

import type { GraphSnapshot, NodeDefinition, NodeRun } from "@/api/types"
import { StatusBadge } from "@/components/status/status-badge"
import { toFlow, type StudioNode } from "@/studio/graph-model"
import { StudioNodeView } from "@/studio/studio-node"

// Node runs by node id. Nodes read them from context, so a state change
// re-renders only the overlays: the node objects, their measured sizes and
// the viewport stay as they are.
const NodeRunsContext = createContext<ReadonlyMap<string, NodeRun>>(new Map())

function RunNodeView(props: NodeProps<StudioNode>) {
  const nodeRun = useContext(NodeRunsContext).get(props.id)
  return (
    <div
      data-selected={props.selected || undefined}
      className="relative rounded-lg data-selected:ring-2 data-selected:ring-ring"
    >
      {nodeRun && (
        <span className="absolute -top-3 right-2 rounded-4xl bg-background">
          <StatusBadge status={nodeRun.state} />
        </span>
      )}
      <StudioNodeView {...props} />
    </div>
  )
}

const nodeTypes = { step: RunNodeView } satisfies NodeTypes

// The run's pinned graph, read once: key it by run. XYFlow keeps the nodes, so
// they stay selectable by pointer and keyboard; the one selected node, or
// null, goes to onNodeSelect.
export function RunGraph({
  graph,
  definitions,
  nodeRuns,
  onNodeSelect,
}: {
  graph: GraphSnapshot
  definitions: NodeDefinition[]
  nodeRuns: NodeRun[]
  onNodeSelect: (nodeId: string | null) => void
}) {
  const [flow] = useState(() => toFlow(graph, definitions))
  const byNode = useMemo(
    () => new Map(nodeRuns.map((nodeRun) => [nodeRun.nodeId, nodeRun])),
    [nodeRuns],
  )
  const onSelectionChange: OnSelectionChangeFunc = useCallback(
    ({ nodes }) => onNodeSelect(nodes.length === 1 ? nodes[0].id : null),
    [onNodeSelect],
  )
  return (
    <NodeRunsContext value={byNode}>
      <ReactFlow
        defaultNodes={flow.nodes}
        defaultEdges={flow.edges}
        nodeTypes={nodeTypes}
        onSelectionChange={onSelectionChange}
        nodesDraggable={false}
        nodesConnectable={false}
        edgesFocusable={false}
        deleteKeyCode={null}
        defaultViewport={graph.viewport}
        colorMode="system"
      >
        <Background />
        <Controls showInteractive={false} />
      </ReactFlow>
    </NodeRunsContext>
  )
}
```

Create `apps/web/src/runs/run-view.tsx`:

```tsx
"use client"

import { ChevronDownIcon, ChevronUpIcon } from "lucide-react"
import dynamic from "next/dynamic"
import Link from "next/link"
import { useRouter } from "next/navigation"
import { useId, useState } from "react"

import { ApiProblem } from "@/api/client"
import {
  useCancelRun,
  useNodeDefinitions,
  useRerunRun,
  useRun,
} from "@/api/queries"
import type { Run, RunEvent } from "@/api/types"
import { useCan } from "@/auth/use-can"
import { StatusBadge } from "@/components/status/status-badge"
import { Button } from "@/components/ui/button"
import { LogView } from "@/runs/log-view"
import { isTerminal, useRunEventLog, useRunEvents } from "@/runs/use-run-events"

// XYFlow measures the DOM, so the graph renders only in the browser.
const RunGraph = dynamic(
  () => import("@/runs/run-graph").then((module) => module.RunGraph),
  {
    ssr: false,
    loading: () => (
      <p className="p-4 text-sm text-muted-foreground">Loading graph…</p>
    ),
  },
)

function RunHeader({
  organizationId,
  run,
}: {
  organizationId: string
  run: Run
}) {
  const canRun = useCan(organizationId, "run_workflows")
  const router = useRouter()
  const cancel = useCancelRun(organizationId)
  const rerun = useRerunRun(organizationId)
  const [notice, setNotice] = useState("")
  const terminal = isTerminal(run.status)

  async function act(action: string, task: () => Promise<void>) {
    setNotice("")
    try {
      await task()
    } catch (error) {
      const code = error instanceof ApiProblem ? error.code : "network_error"
      setNotice(`${action} failed (${code}).`)
    }
  }

  return (
    <header className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b px-4 py-3 text-sm">
      <nav aria-label="Breadcrumb">
        <ol className="flex flex-wrap items-center gap-2">
          <li>
            <Link
              href={`/orgs/${organizationId}/workflows`}
              className="text-muted-foreground hover:text-foreground"
            >
              Workflows
            </Link>
          </li>
          <li aria-hidden="true" className="text-muted-foreground">
            /
          </li>
          <li>
            <Link
              href={`/orgs/${organizationId}/workflows/${run.workflowId}`}
              className="text-muted-foreground hover:text-foreground"
            >
              {run.workflowName}
            </Link>
          </li>
          <li aria-hidden="true" className="text-muted-foreground">
            /
          </li>
          <li aria-current="page">
            <h1 className="text-lg font-semibold tracking-tight">
              Run {new Date(run.createdAt).toLocaleString()}
            </h1>
          </li>
        </ol>
      </nav>
      <span role="status" aria-label="Run status">
        <StatusBadge status={run.status} />
      </span>
      <span className="text-muted-foreground">v{run.version}</span>
      {run.cancelRequested && !terminal && (
        <span className="text-muted-foreground">Cancel requested</span>
      )}
      <span aria-live="polite">{notice}</span>
      {canRun && (
        <div className="ml-auto flex items-center gap-2">
          {!terminal && (
            <Button
              variant="outline"
              size="sm"
              disabled={cancel.isPending || run.cancelRequested}
              onClick={() =>
                void act("Cancel", async () => {
                  await cancel.mutateAsync(run.id)
                })
              }
            >
              Cancel
            </Button>
          )}
          <Button
            variant="secondary"
            size="sm"
            disabled={rerun.isPending || rerun.isSuccess}
            onClick={() =>
              void act("Rerun", async () => {
                const next = await rerun.mutateAsync(run.id)
                router.push(`/orgs/${organizationId}/runs/${next.id}`)
              })
            }
          >
            Rerun
          </Button>
        </div>
      )}
    </header>
  )
}

function Json({ label, value }: { label: string; value: unknown }) {
  return (
    <figure className="grid gap-1">
      <figcaption className="font-medium">{label}</figcaption>
      {value === null ? (
        <p className="text-muted-foreground">None</p>
      ) : (
        <pre className="overflow-x-auto rounded-md bg-muted/40 p-2 font-mono text-xs">
          {JSON.stringify(value, null, 2)}
        </pre>
      )}
    </figure>
  )
}

function NodeDetails({ run, nodeId }: { run: Run; nodeId: string | null }) {
  const node = run.graph.nodes.find(({ id }) => id === nodeId)
  const nodeRun = run.nodes.find((candidate) => candidate.nodeId === nodeId)
  if (!node || !nodeRun) {
    return (
      <p className="text-muted-foreground">
        Select a node to see its details and only its log lines.
      </p>
    )
  }
  return (
    <div className="grid gap-3">
      <h2 className="font-medium">{node.displayName}</h2>
      <dl className="grid grid-cols-[auto_1fr] items-center gap-x-3 gap-y-1">
        <dt className="text-muted-foreground">State</dt>
        <dd>
          <StatusBadge status={nodeRun.state} />
        </dd>
        <dt className="text-muted-foreground">Attempt</dt>
        <dd>
          {nodeRun.attempt} of {nodeRun.maxAttempts}
        </dd>
      </dl>
      {nodeRun.queueReason === "waiting_for_compatible_worker" && (
        <p>Waiting for a worker that runs {nodeRun.executor}.</p>
      )}
      {nodeRun.error && (
        <p className="text-destructive">
          {nodeRun.error.message}{" "}
          <span className="font-mono text-xs">{nodeRun.error.code}</span>
        </p>
      )}
      <Json label="Input" value={nodeRun.input} />
      <Json label="Output" value={nodeRun.output} />
    </div>
  )
}

// The run page's bottom drawer starts open; closed, it is only hidden.
function LogDrawer({
  events,
  nodeId,
}: {
  events: RunEvent[]
  nodeId: string | null
}) {
  const [open, setOpen] = useState(true)
  const panelId = useId()
  return (
    <section aria-label="Logs" className="border-t">
      <Button
        variant="ghost"
        size="sm"
        className="m-1"
        aria-expanded={open}
        aria-controls={panelId}
        onClick={() => setOpen(!open)}
      >
        {open ? <ChevronDownIcon /> : <ChevronUpIcon />}
        Logs
      </Button>
      <div id={panelId} hidden={!open} className="px-4 pb-4">
        <LogView events={events} nodeId={nodeId} />
      </div>
    </section>
  )
}

// Follows one run live: overlays on its pinned graph, the selected node's
// details, and its log. Unlike editing, this works on any screen width.
export function RunView({
  organizationId,
  runId,
}: {
  organizationId: string
  runId: string
}) {
  const run = useRun(organizationId, runId)
  const definitions = useNodeDefinitions(organizationId)
  useRunEvents(organizationId, runId, run.isSuccess)
  const events = useRunEventLog(organizationId, runId)
  const [selected, setSelected] = useState<string | null>(null)

  if (!run.data || !definitions.data) {
    return run.isError || definitions.isError ? (
      <p role="alert" className="p-4 text-sm text-destructive md:p-6">
        The run could not be loaded.
      </p>
    ) : (
      <p className="p-4 text-sm text-muted-foreground md:p-6">Loading run…</p>
    )
  }

  return (
    <section>
      <RunHeader organizationId={organizationId} run={run.data} />
      <div className="grid lg:grid-cols-[1fr_20rem]">
        <div className="h-112">
          <RunGraph
            graph={run.data.graph}
            definitions={definitions.data.items}
            nodeRuns={run.data.nodes}
            onNodeSelect={setSelected}
          />
        </div>
        <aside
          aria-label="Node details"
          className="border-t p-4 text-sm lg:max-h-112 lg:overflow-y-auto lg:border-t-0 lg:border-l"
        >
          <NodeDetails run={run.data} nodeId={selected} />
        </aside>
      </div>
      <LogDrawer events={events.data?.events ?? []} nodeId={selected} />
    </section>
  )
}
```

Create `apps/web/src/app/(app)/orgs/[organizationId]/runs/[runId]/page.tsx`:

```tsx
import { RunView } from "@/runs/run-view"

export default async function RunPage({
  params,
}: PageProps<"/orgs/[organizationId]/runs/[runId]">) {
  const { organizationId, runId } = await params
  // A new run (a rerun) starts with its own graph and selection.
  return <RunView key={runId} organizationId={organizationId} runId={runId} />
}
```

- [ ] **Step 11: Add the Logs tab to the studio drawer**

In `apps/web/src/runs/runs-drawer.tsx`, apply these replacements in order:

Replace:

```tsx
import { useRuns } from "@/api/queries"
```

With:

```tsx
import { useRun, useRuns } from "@/api/queries"
```

Replace:

```tsx
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"

// The studio's bottom drawer. The closed panel is only hidden, so it keeps its
// tab and scroll position; it is not a Sheet, which would cover the canvas.
```

With:

```tsx
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { LogView } from "@/runs/log-view"
import { useRunEventLog, useRunEvents } from "@/runs/use-run-events"

// The studio's bottom drawer. The closed panel is only hidden, so it keeps its
// tab and scroll position; it is not a Sheet, which would cover the canvas.
// An inactive tab's panel unmounts, so the Logs tab streams only while the
// drawer is open on it.
```

Replace:

```tsx
            <TabsTrigger value="runs">Recent runs</TabsTrigger>
          </TabsList>
          <TabsContent value="runs">
            <RecentRuns
              organizationId={organizationId}
              workflowId={workflowId}
            />
          </TabsContent>
```

With:

```tsx
            <TabsTrigger value="runs">Recent runs</TabsTrigger>
            <TabsTrigger value="logs">Logs</TabsTrigger>
          </TabsList>
          <TabsContent value="runs">
            <RecentRuns
              organizationId={organizationId}
              workflowId={workflowId}
            />
          </TabsContent>
          <TabsContent value="logs">
            <NewestRunLogs
              organizationId={organizationId}
              workflowId={workflowId}
              streaming={open}
            />
          </TabsContent>
```

Replace:

```tsx
      </TableBody>
    </Table>
  )
}
```

With:

```tsx
      </TableBody>
    </Table>
  )
}

// The log of the workflow's newest run, every node's lines.
function NewestRunLogs({
  organizationId,
  workflowId,
  streaming,
}: {
  organizationId: string
  workflowId: string
  streaming: boolean
}) {
  const runs = useRuns(organizationId, workflowId)
  const newest = runs.data?.items[0]
  if (!newest) {
    return (
      <p className="text-muted-foreground">
        {runs.isPending
          ? "Loading runs…"
          : runs.isError
            ? "The runs could not be loaded."
            : "No runs yet."}
      </p>
    )
  }
  return (
    <RunLogs
      organizationId={organizationId}
      runId={newest.id}
      streaming={streaming}
    />
  )
}

function RunLogs({
  organizationId,
  runId,
  streaming,
}: {
  organizationId: string
  runId: string
  streaming: boolean
}) {
  const run = useRun(organizationId, runId)
  useRunEvents(organizationId, runId, streaming && run.isSuccess)
  const events = useRunEventLog(organizationId, runId)
  return <LogView events={events.data?.events ?? []} nodeId={null} />
}
```

- [ ] **Step 12: Verify GREEN**

Run:

```text
pnpm web:test
```

Expected: `Test Files  16 passed (16)` and `Tests  110 passed (110)` with no unhandled errors. That is
use-run-events 5, log-view 3, run-view 6, studio 25, and the 71 tests of the other twelve files unchanged.
The CLOSED-path test waits for the 1 s backoff, so it takes about 1.1 s. On Node 25 or newer, each worker may
print `ExperimentalWarning: localStorage is not available because --localstorage-file was not provided.`; this
is harmless.

- [ ] **Step 13: Run lint and build**

Run:

```text
pnpm web:lint
```

Expected: `✓ Types generated successfully`, then `tsc --noEmit` and `oxlint` report no diagnostics. The command
exits 0. `next typegen` generates `PageProps<"/orgs/[organizationId]/runs/[runId]">` for the new page.

Run:

```text
pnpm web:build
```

Expected: `✓ Compiled successfully` and `Finished TypeScript`. The route table gains
`ƒ /orgs/[organizationId]/runs/[runId]`, so the studio's run links and the run dialog's navigation now resolve.
The command exits 0.

- [ ] **Step 14: Commit**

```text
git add apps/web
git commit -m "feat: watch runs live with graph overlays and logs"
```

## Task 17: Enable the shell executor in organization settings

**Files:**

- Create: `apps/web/src/components/ui/alert.tsx`
- Create: `apps/web/src/components/shell-warning.tsx`
- Create: `apps/web/src/views/settings-view.tsx`
- Create: `apps/web/src/app/(app)/orgs/[organizationId]/settings/page.tsx`
- Modify: `apps/web/src/api/types.ts`
- Modify: `apps/web/src/api/queries.ts`
- Modify: `apps/web/src/layout/app-shell.tsx`
- Modify: `apps/web/src/studio/studio.tsx`
- Modify: `apps/web/src/studio/inspector.tsx`
- Modify: `apps/web/src/test/handlers.ts`
- Test: `apps/web/src/views/settings-view.test.tsx` (new)
- Test: `apps/web/src/studio/inspector.test.tsx`
- Test: `apps/web/src/studio/studio.test.tsx`
- Test: `apps/web/src/views/workflow-views.test.tsx`

An organization admin opts into the trusted shell executor from a settings page (M3-54, acceptance 10). The
trusted-user warning shows where shell is enabled and where a shell node is configured (M3-63), and the studio
palette offers the shell preset only while shell is enabled (M3-65, M3-112). The API (Task 11) stays the
authority: publish and run creation reject shell nodes while shell is disabled, whatever the web shows.

- **Route and navigation.** `app/(app)/orgs/[organizationId]/settings/page.tsx` is a thin server page that renders
  the client `SettingsView({organizationId})`. The sidebar gains a `Settings` link after `Audit log`, shown to
  every role, like the other links.
- **Settings page** (`src/views/settings-view.tsx`). A `Shell executor` section explains the option and shows
  the trusted-user warning to everyone.
  - Members with `enable_shell_executor` (admins) get a native checkbox, `Enable the shell executor`, like the
    studio's boolean widget. A change sends `PUT /orgs/{org}/settings/shell` with `{enabled}`. The checkbox
    stays controlled by `GET /settings` and is disabled while the request and the refresh that follows are
    pending. A failure shows `The shell executor could not be changed (<code>).` with `role="alert"`.
  - Everyone else reads `The shell executor is enabled.` or `... disabled.` and gets no control.
  - Loading and load-error states follow the other views (`Loading settings…`, `The settings could not be
    loaded.` with `role="alert"`).
- **Warning** (`src/components/shell-warning.tsx`, `ShellWarning()`). One shadcn `Alert` (allowlisted) titled
  `Trusted users only`. It says that shell commands run inside the worker container and that the shell executor
  is not a security boundary, so only trusted people should be able to edit workflows. The settings page and the
  inspector render the same component.
- **Alert component.** `src/components/ui/alert.tsx` is the shadcn base-nova `alert` registry file with `cn`
  imported from `@/lib/utils`. Its `Alert` has `role="alert"`.
- **Queries.** `src/api/types.ts` gains `OrganizationSettings {shellEnabled: boolean}`. `src/api/queries.ts`
  gains `queryKeys.settings(org)` = `["orgs", org, "settings"]`, `useSettings(org)` (`GET /orgs/{org}/settings`)
  and `useSetShellEnabled(org)`, whose `mutate(enabled: boolean)` sends the PUT. The mutation goes through
  `useOrganizationMutation`, so the organization refresh brings the new value back and also refreshes the audit
  log (`organization.shell_enabled` / `organization.shell_disabled`).
- **Palette.** The studio's `Editor` reads `useSettings` and hides every definition whose `executor.key` is
  `shell` from the palette unless `shellEnabled` is true. Hidden includes "not loaded yet" and a failed load.
  The canvas and inspector still receive every definition, so a draft that already holds a shell node keeps
  rendering. A `shell` organization definition is hidden too, matching the API's rule from D15.
- **Inspector.** `NodeSettings` shows `ShellWarning` under the node heading when the node's definition has
  `executor.key === "shell"`.
- **Fixtures.** `src/test/handlers.ts` gains `shellCommand`, the `corqty.shell.command` v1 preset from D15
  (`executor {key: "shell", version: "1.0.0"}`), which joins `nodeDefinitions` in API order. A default handler
  answers `GET /orgs/:organizationId/settings` with `{shellEnabled: false}`. The catalog test now counts 8
  definition rows; the palette test still counts 6, because the preset stays hidden by default.

- [ ] **Step 1: Add the shell preset and the settings handler to the test API**

In `apps/web/src/test/handlers.ts`, apply these replacements in order:

Replace:

```ts
// Every seeded definition is version 1 and runs on the mock executor.
function seeded(
```

With:

```ts
// The seeded mock definitions are version 1 and share the mock config.
function seeded(
```

Replace:

```ts
const vpcIdSchema: JsonSchema = { type: "string", pattern: "^vpc-" }

// The seeded system catalog, in API order (key, then version).
```

With:

```ts
const vpcIdSchema: JsonSchema = { type: "string", pattern: "^vpc-" }

// The trusted shell preset: the palette offers it once an admin enables shell.
export const shellCommand: NodeDefinition = {
  id: "00000000-0000-7000-8000-000000000007",
  key: "corqty.shell.command",
  version: 1,
  displayName: "Shell command",
  description:
    "Trusted: runs a shell command inside the worker container. Not a security boundary.",
  category: "Shell (trusted)",
  deprecated: false,
  inputs: [port("input", "core.any", {})],
  outputs: [port("result", "core.any", {})],
  configSchema: {
    type: "object",
    properties: {
      command: {
        type: "string",
        minLength: 1,
        maxLength: 16384,
        title: "Command",
      },
      env: {
        type: "object",
        additionalProperties: { type: "string" },
        title: "Environment",
      },
    },
    required: ["command"],
    additionalProperties: false,
  },
  uiSchema: { command: { widget: "textarea" }, env: { widget: "keyValue" } },
  defaults: {
    command: `jq '{result: .}' "$CORQTY_INPUT" > "$CORQTY_OUTPUT"`,
    env: {},
  },
  executor: { key: "shell", version: "1.0.0" },
}

// The seeded system catalog, in API order (key, then version).
```

Replace:

```ts
    { result: { ok: true } },
    0,
  ),
  seeded(
```

With:

```ts
    { result: { ok: true } },
    0,
  ),
  shellCommand,
  seeded(
```

Replace:

```ts
  http.get("/api/v1/orgs/:organizationId/audit-events", () =>
    HttpResponse.json({ items: auditEvents }),
  ),
```

With:

```ts
  http.get("/api/v1/orgs/:organizationId/audit-events", () =>
    HttpResponse.json({ items: auditEvents }),
  ),
  http.get("/api/v1/orgs/:organizationId/settings", () =>
    HttpResponse.json({ shellEnabled: false }),
  ),
```

The catalog lists every seeded definition, so its row count grows by one.

In `apps/web/src/views/workflow-views.test.tsx`, apply these replacements in order:

Replace:

```ts
  expect(within(definitions).getAllByRole("row")).toHaveLength(1 + 7)
```

With:

```ts
  expect(within(definitions).getAllByRole("row")).toHaveLength(1 + 8)
```

- [ ] **Step 2: Write the failing settings page tests**

The first test keeps the setting on the test API, so each PUT is followed by a GET that returns the new value.

Create `apps/web/src/views/settings-view.test.tsx`:

```tsx
import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { http, HttpResponse } from "msw/http"
import { expect, it } from "vitest"

import { AuthGate } from "@/auth/auth-gate"
import { AppShell } from "@/layout/app-shell"
import { meAs } from "@/test/handlers"
import { renderWithProviders } from "@/test/render"
import { server } from "@/test/server"
import { SettingsView } from "@/views/settings-view"

const settingsUrl = "/api/v1/orgs/:organizationId/settings"

// Mirrors the (app) and orgs/[organizationId] layouts around the page.
function renderSettings() {
  return renderWithProviders(
    <AuthGate>
      <AppShell organizationId="org-1">
        <SettingsView organizationId="org-1" />
      </AppShell>
    </AuthGate>,
    { route: "/orgs/org-1/settings" },
  )
}

it("an admin turns the shell executor on and off below the trusted-user warning", async () => {
  let shellEnabled = false
  const changes: unknown[] = []
  server.use(
    http.get(settingsUrl, () => HttpResponse.json({ shellEnabled })),
    http.put(`${settingsUrl}/shell`, async ({ request }) => {
      const change = (await request.json()) as { enabled: boolean }
      changes.push(change)
      shellEnabled = change.enabled
      return HttpResponse.json({ shellEnabled })
    }),
  )
  renderSettings()
  const toggle = await screen.findByRole("checkbox", {
    name: "Enable the shell executor",
  })
  expect(screen.getByRole("link", { name: "Settings" })).toHaveAttribute(
    "aria-current",
    "page",
  )
  expect(screen.getByRole("alert")).toHaveTextContent(
    "The shell executor is not a security boundary",
  )
  expect(toggle).not.toBeChecked()

  await userEvent.click(toggle)
  await waitFor(() => expect(toggle).toBeChecked())
  await waitFor(() => expect(toggle).toBeEnabled())
  await userEvent.click(toggle)
  await waitFor(() => expect(toggle).not.toBeChecked())
  expect(changes).toEqual([{ enabled: true }, { enabled: false }])
})

it("an editor sees whether shell is enabled but cannot change it", async () => {
  server.use(
    http.get("/api/v1/auth/me", () => HttpResponse.json(meAs("editor"))),
    http.get(settingsUrl, () => HttpResponse.json({ shellEnabled: true })),
  )
  renderSettings()
  expect(
    await screen.findByText("The shell executor is enabled."),
  ).toBeInTheDocument()
  expect(screen.getByRole("alert")).toHaveTextContent("Trusted users only")
  expect(screen.queryByRole("checkbox")).not.toBeInTheDocument()
})

it("says why the shell executor could not be changed", async () => {
  server.use(
    http.put(`${settingsUrl}/shell`, () =>
      HttpResponse.json({ code: "forbidden" }, { status: 403 }),
    ),
  )
  renderSettings()
  const toggle = await screen.findByRole("checkbox", {
    name: "Enable the shell executor",
  })
  await userEvent.click(toggle)
  expect(
    await screen.findByText(
      "The shell executor could not be changed (forbidden).",
    ),
  ).toBeInTheDocument()
  expect(toggle).not.toBeChecked()
})
```

- [ ] **Step 3: Write the failing inspector and palette tests**

A shell node in the inspector shows the warning. The existing test `retry and timeout take whole numbers within
their bounds` already asserts that the VPC node (mock executor) shows no `alert` once its inputs are valid, so
other nodes stay without the warning.

In `apps/web/src/studio/inspector.test.tsx`, apply these replacements in order:

Replace:

```tsx
import { Inspector } from "@/studio/inspector"
import { blueprints, nodeDefinitions } from "@/test/handlers"
```

With:

```tsx
import { newNodeFromDefinition } from "@/studio/graph-model"
import { Inspector } from "@/studio/inspector"
import { blueprints, nodeDefinitions, shellCommand } from "@/test/handlers"
```

Replace:

```tsx
it("shows where a connected input comes from instead of an editor", () => {
```

With:

```tsx
it("warns before a shell node is configured", () => {
  const shell = newNodeFromDefinition(shellCommand, { x: 0, y: 0 })
  const graph = { ...infrastructure, nodes: [shell], edges: [] }
  renderInspector(shell.id, { graph })
  expect(screen.getByRole("alert")).toHaveTextContent("Trusted users only")
  expect(screen.getByLabelText("Command")).toHaveValue(
    `jq '{result: .}' "$CORQTY_INPUT" > "$CORQTY_OUTPUT"`,
  )
})

it("shows where a connected input comes from instead of an editor", () => {
```

The palette test starts with shell disabled, waits until the studio has read the settings, and then enables
shell on the test API. A `visibilitychange` event on `window` is TanStack Query's focus signal, so it refetches
the settings the way returning to the browser tab would.

In `apps/web/src/studio/studio.test.tsx`, apply these replacements in order:

Replace:

```tsx
  expect(within(blueprintList).getByText("Data processing")).toBeInTheDocument()
})
```

With:

```tsx
  expect(within(blueprintList).getByText("Data processing")).toBeInTheDocument()
})

it("the palette offers the shell preset only while shell is enabled", async () => {
  let shellEnabled = false
  let reads = 0
  server.use(
    http.get("/api/v1/orgs/:organizationId/settings", () => {
      reads += 1
      return HttpResponse.json({ shellEnabled })
    }),
  )
  renderStudio()
  const definitions = await screen.findByRole("list", {
    name: "Node definitions",
  })
  await waitFor(() => expect(reads).toBe(1))
  expect(within(definitions).getByText("Mock task")).toBeInTheDocument()
  expect(
    within(definitions).queryByText("Shell command"),
  ).not.toBeInTheDocument()

  // An admin enables shell elsewhere; coming back to the tab refetches.
  shellEnabled = true
  window.dispatchEvent(new Event("visibilitychange"))
  expect(await within(definitions).findByText("Shell command")).toBeVisible()
})
```

- [ ] **Step 4: Run the tests and verify RED**

Run:

```text
pnpm web:test
```

Expected: `Test Files  3 failed | 14 passed (17)` and `Tests  3 failed | 109 passed (112)`:
- `settings-view.test.tsx` fails to load with `Error: Failed to resolve import "@/views/settings-view" from
  "src/views/settings-view.test.tsx". Does the file exist?`.
- `warns before a shell node is configured` fails with `TestingLibraryElementError: Unable to find an
  accessible element with the role "alert"`.
- `the palette lists the latest definitions and blueprints and filters them` fails with
  `AssertionError: expected [ …(7) ] to have a length of 6 but got 7`: nothing hides the shell preset yet.
- `the palette offers the shell preset only while shell is enabled` fails with
  `AssertionError: expected +0 to be 1 // Object.is equality`: the studio never reads the settings.

The catalog test passes with its new count of 8.

- [ ] **Step 5: Add the settings type and queries**

In `apps/web/src/api/types.ts`, apply these replacements in order:

Replace:

```ts
  metadata: Record<string, unknown>
  occurredAt: string
}
```

With:

```ts
  metadata: Record<string, unknown>
  occurredAt: string
}

export interface OrganizationSettings {
  shellEnabled: boolean
}
```

In `apps/web/src/api/queries.ts`, apply these replacements in order:

Replace:

```ts
  Member,
  NodeDefinition,
  Role,
```

With:

```ts
  Member,
  NodeDefinition,
  OrganizationSettings,
  Role,
```

Replace:

```ts
  auditEvents: (organizationId: string) =>
    ["orgs", organizationId, "audit-events"] as const,
```

With:

```ts
  auditEvents: (organizationId: string) =>
    ["orgs", organizationId, "audit-events"] as const,
  settings: (organizationId: string) =>
    ["orgs", organizationId, "settings"] as const,
```

Replace:

```ts
export function useRemoveMember(organizationId: string) {
  return useOrganizationMutation(organizationId, (userId: string) =>
    api<void>(`/orgs/${organizationId}/members/${userId}`, {
      method: "DELETE",
    }),
  )
}
```

With:

```ts
export function useRemoveMember(organizationId: string) {
  return useOrganizationMutation(organizationId, (userId: string) =>
    api<void>(`/orgs/${organizationId}/members/${userId}`, {
      method: "DELETE",
    }),
  )
}

export function useSettings(organizationId: string) {
  return useQuery({
    queryKey: queryKeys.settings(organizationId),
    queryFn: ({ signal }) =>
      api<OrganizationSettings>(`/orgs/${organizationId}/settings`, {
        signal,
      }),
  })
}

// The refresh that follows brings the new value back through useSettings.
export function useSetShellEnabled(organizationId: string) {
  return useOrganizationMutation(organizationId, (enabled: boolean) =>
    api<OrganizationSettings>(`/orgs/${organizationId}/settings/shell`, {
      method: "PUT",
      body: JSON.stringify({ enabled }),
    }),
  )
}
```

- [ ] **Step 6: Add the alert component and the shared warning**

This is the shadcn base-nova `alert` registry file, written by hand with `cn` imported from `@/lib/utils`.
Running `shadcn add alert` would import `cn` from the `cn` package and add it as a dependency.

Create `apps/web/src/components/ui/alert.tsx`:

```tsx
import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { cn } from "@/lib/utils"

const alertVariants = cva(
  "group/alert relative grid w-full gap-0.5 rounded-lg border px-2.5 py-2 text-left text-sm has-data-[slot=alert-action]:relative has-data-[slot=alert-action]:pr-18 has-[>svg]:grid-cols-[auto_1fr] has-[>svg]:gap-x-2 *:[svg]:row-span-2 *:[svg]:translate-y-0.5 *:[svg]:text-current *:[svg:not([class*='size-'])]:size-4",
  {
    variants: {
      variant: {
        default: "bg-card text-card-foreground",
        destructive:
          "bg-card text-destructive *:data-[slot=alert-description]:text-destructive/90 *:[svg]:text-current",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  }
)

function Alert({
  className,
  variant,
  ...props
}: React.ComponentProps<"div"> & VariantProps<typeof alertVariants>) {
  return (
    <div
      data-slot="alert"
      role="alert"
      className={cn(alertVariants({ variant }), className)}
      {...props}
    />
  )
}

function AlertTitle({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="alert-title"
      className={cn(
        "font-medium group-has-[>svg]/alert:col-start-2 [&_a]:underline [&_a]:underline-offset-3 [&_a]:hover:text-foreground",
        className
      )}
      {...props}
    />
  )
}

function AlertDescription({
  className,
  ...props
}: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="alert-description"
      className={cn(
        "text-sm text-balance text-muted-foreground md:text-pretty [&_a]:underline [&_a]:underline-offset-3 [&_a]:hover:text-foreground [&_p:not(:last-child)]:mb-4",
        className
      )}
      {...props}
    />
  )
}

function AlertAction({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="alert-action"
      className={cn("absolute top-2 right-2", className)}
      {...props}
    />
  )
}

export { Alert, AlertTitle, AlertDescription, AlertAction }
```

Create `apps/web/src/components/shell-warning.tsx`:

```tsx
import { TriangleAlertIcon } from "lucide-react"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"

// Shown where shell is enabled and where a shell node is configured.
export function ShellWarning() {
  return (
    <Alert>
      <TriangleAlertIcon />
      <AlertTitle>Trusted users only</AlertTitle>
      <AlertDescription>
        Shell commands run inside the worker container and can use anything it
        can reach. The shell executor is not a security boundary, so only
        trusted people should be able to edit workflows.
      </AlertDescription>
    </Alert>
  )
}
```

- [ ] **Step 7: Add the settings page**

Create `apps/web/src/views/settings-view.tsx`:

```tsx
"use client"

import { useId, useState } from "react"

import { ApiProblem } from "@/api/client"
import { useSetShellEnabled, useSettings } from "@/api/queries"
import { useCan } from "@/auth/use-can"
import { ShellWarning } from "@/components/shell-warning"
import { Label } from "@/components/ui/label"

export function SettingsView({ organizationId }: { organizationId: string }) {
  const canEnableShell = useCan(organizationId, "enable_shell_executor")
  const settings = useSettings(organizationId)
  const setShellEnabled = useSetShellEnabled(organizationId)
  const [error, setError] = useState("")
  const headingId = useId()
  const toggleId = useId()

  async function change(enabled: boolean) {
    setError("")
    try {
      await setShellEnabled.mutateAsync(enabled)
    } catch (problem) {
      const code =
        problem instanceof ApiProblem ? problem.code : "network_error"
      setError(`The shell executor could not be changed (${code}).`)
    }
  }

  return (
    <section className="flex flex-col gap-5 p-4 md:p-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Settings</h1>
        <p className="text-sm text-muted-foreground">
          Options for the whole organization.
        </p>
      </div>
      <section aria-labelledby={headingId} className="grid max-w-2xl gap-3">
        <h2 id={headingId} className="text-lg font-semibold">
          Shell executor
        </h2>
        <p className="text-sm text-muted-foreground">
          Lets workflow nodes run shell commands on the workers. Only an admin
          can change it; runs that already exist are not affected.
        </p>
        <ShellWarning />
        {settings.isPending && (
          <p className="text-sm text-muted-foreground">Loading settings…</p>
        )}
        {settings.isError && (
          <p role="alert" className="text-sm text-destructive">
            The settings could not be loaded.
          </p>
        )}
        {settings.data &&
          (canEnableShell ? (
            <div className="flex items-center gap-2">
              <input
                id={toggleId}
                type="checkbox"
                className="size-4 accent-primary"
                checked={settings.data.shellEnabled}
                disabled={setShellEnabled.isPending}
                onChange={(event) => void change(event.target.checked)}
              />
              <Label htmlFor={toggleId}>Enable the shell executor</Label>
            </div>
          ) : (
            <p className="text-sm">
              The shell executor is{" "}
              {settings.data.shellEnabled ? "enabled" : "disabled"}.
            </p>
          ))}
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
      </section>
    </section>
  )
}
```

Create `apps/web/src/app/(app)/orgs/[organizationId]/settings/page.tsx`:

```tsx
import { SettingsView } from "@/views/settings-view"

export default async function SettingsPage({
  params,
}: PageProps<"/orgs/[organizationId]/settings">) {
  const { organizationId } = await params
  return <SettingsView organizationId={organizationId} />
}
```

In `apps/web/src/layout/app-shell.tsx`, apply these replacements in order:

Replace:

```tsx
  { label: "Audit log", suffix: "/audit" },
]
```

With:

```tsx
  { label: "Audit log", suffix: "/audit" },
  { label: "Settings", suffix: "/settings" },
]
```

- [ ] **Step 8: Gate the palette and warn in the inspector**

In `apps/web/src/studio/studio.tsx`, apply these replacements in order:

Replace:

```tsx
  usePublishWorkflow,
  useValidateWorkflow,
  useWorkflow,
} from "@/api/queries"
```

With:

```tsx
  usePublishWorkflow,
  useSettings,
  useValidateWorkflow,
  useWorkflow,
} from "@/api/queries"
```

Replace:

```tsx
  const validate = useValidateWorkflow(organizationId, workflow.id)
  const publish = usePublishWorkflow(organizationId, workflow.id)
  const queryClient = useQueryClient()
```

With:

```tsx
  const validate = useValidateWorkflow(organizationId, workflow.id)
  const publish = usePublishWorkflow(organizationId, workflow.id)
  const settings = useSettings(organizationId)
  // The shell preset (and any definition on the shell executor) is offered
  // only once an admin enables shell.
  const paletteDefinitions = settings.data?.shellEnabled
    ? definitions
    : definitions.filter(({ executor }) => executor.key !== "shell")
  const queryClient = useQueryClient()
```

Replace:

```tsx
          <Palette definitions={definitions} blueprints={blueprints} />
```

With:

```tsx
          <Palette definitions={paletteDefinitions} blueprints={blueprints} />
```

In `apps/web/src/studio/inspector.tsx`, apply these replacements in order:

Replace:

```tsx
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { findDefinition } from "@/studio/graph-model"
```

With:

```tsx
import { ShellWarning } from "@/components/shell-warning"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { findDefinition } from "@/studio/graph-model"
```

Replace:

```tsx
          {node.definition.key} v{node.definition.version}
        </p>
      </header>
```

With:

```tsx
          {node.definition.key} v{node.definition.version}
        </p>
      </header>
      {definition?.executor.key === "shell" && <ShellWarning />}
```

- [ ] **Step 9: Verify GREEN**

Run:

```text
pnpm web:test
```

Expected: `Test Files  17 passed (17)` and `Tests  115 passed (115)` with no unhandled errors. That is
settings-view 3, inspector 8, studio 26, and the 78 tests of the other fourteen files. On Node 25 or newer, each
worker may print `ExperimentalWarning: localStorage is not available because --localstorage-file was not
provided.`; this is harmless.

- [ ] **Step 10: Run lint and build**

Run:

```text
pnpm web:lint
```

Expected: `✓ Types generated successfully`, then `tsc --noEmit` and `oxlint` report no diagnostics. The command
exits 0. `next typegen` generates `PageProps<"/orgs/[organizationId]/settings">` for the new page.

Run:

```text
pnpm web:build
```

Expected: `✓ Compiled successfully` and `Finished TypeScript`. The route table gains
`ƒ /orgs/[organizationId]/settings`. The command exits 0.

- [ ] **Step 11: Commit**

```text
git add apps/web
git commit -m "feat: enable the shell executor in organization settings"
```

## Task 18: Run workers in Compose and smoke-test executions end to end

**Files:**

- Modify: `crates/worker/src/lib.rs`
- Modify: `crates/worker/src/main.rs`
- Modify: `crates/worker/tests/execution.rs`
- Modify: `crates/executor-shell/src/lib.rs`
- Modify: `crates/executor-shell/tests/contract.rs`
- Modify: `docker/rust/Dockerfile`
- Modify: `docker-compose.yml`
- Modify: `.env.example`
- Modify: `.github/workflows/ci.yml`
- Modify: `README.md`
- Modify: `docs/superpowers/specs/2026-07-28-corqty-workflow-orchestrator-design.md`
- Test: `crates/worker/tests/health.rs`
- Test: `scripts/smoke-foundation.ps1`

The worker binary exists since Task 14, but Compose starts no worker, so a run started in the stack never leaves
`queued`. This task ships the worker as an image and a scalable Compose service, and extends the smoke test from
the API and the studio to real executions with three workers (spec §17, §18 "End-to-end and Compose smoke tests",
acceptance criteria 6, 8 and 10).

**Health.** A worker counts as healthy while it reaches the database. `WorkerConfig` gains `alive_file` (default
`std::env::temp_dir()` joined with `corqty-worker.alive`, so `/tmp/corqty-worker.alive` in the container), and `run`
touches it after every successful claim call. The worker claims on every poll tick, every 500 ms, even when all its
slots are busy (the limit is then 0), so a busy worker stays healthy however long its attempts run. A worker whose
claims fail stops touching the file. `corqty_worker::check_alive(path)` accepts a file modified at most 10 s ago;
a modification time in the future (the clock went back) counts as fresh. `corqty-worker healthcheck` runs
`check_alive` on the default path before it reads any environment variable or sets up tracing, so the Compose
healthcheck needs no database connection. It exits 0, or 1 with the reason on stderr.

**Shell log cap.** Task 14 left one gap: the shell executor cut each attempt's log at exactly 10,485,760 bytes, so
the database never saw the log pass its cap. A shell node that flooded its log ended with `log_truncated` false and
no `node.log_truncated` event. The shell executor now follows the worker's rule: it counts the bytes it has sent,
sends the chunk that takes the attempt past the cap whole, and drops everything after it while it keeps draining
the pipes. The database then cuts the text at a character boundary and writes the one `node.log_truncated` event.
The shell contract test pins the new rule.

**Image and service.** The builder stage of `docker/rust/Dockerfile` also compiles `corqty-worker`. The new `worker`
target starts from `runtime`, adds `bash` and `jq` (the tools a trusted shell command can call, including the
seeded default command), and runs `corqty-worker` as uid 10001. The Compose `worker` service:

- builds the `worker` target and runs Docker's init (`init: true`), which forwards SIGTERM and reaps the orphans of
  shell commands;
- has a read-only root filesystem with a tmpfs on `/tmp`, where the scratch directories and the alive file live;
- gets `stop_grace_period: 30s`: the 25 s drain plus the 5 s cancel grace (D11);
- reads `DATABASE_URL_WORKER` and `CORQTY_WORKER_CAPACITY` (4 by default);
- waits for `migrate` to complete;
- publishes no port and has no `container_name`, so `--scale worker=N` works. Every process registers a new
  `Uuid::now_v7()` worker id, so the replicas never share one;
- checks its health with `corqty-worker healthcheck` every 2 s.

`.env.example` documents the worker URL, the capacity and the fixed run, node and log limits. CI starts the stack
with `CORQTY_WORKER_CAPACITY=1` and `--scale worker=3`, and its timeout grows from 30 to 45 minutes for the release
build of three binaries and the longer smoke test.

**Smoke test.** `scripts/smoke-foundation.ps1` takes `-ExpectedWorkers` (default 1, for a plain
`docker compose up`) and then checks:

1. exactly that many `worker` containers run and are healthy; each runs as uid 10001 and `docker inspect` shows it
   unprivileged, with a read-only root filesystem and no bind or volume mounts;
2. the published simulated-infrastructure workflow, started with `{"input":{"region":"us-east-1"}}` and polled for
   up to 60 s, succeeds; `vpc` received the region; the `vpcId` input of `rds` and of `deploy` equals the `vpcId`
   output of `vpc`; and the `rds` and `deploy` intervals overlap. With one slot per worker, the overlap proves two
   different workers ran them;
3. a workflow of three `corqty.mock.task` nodes, where `fail` has a `failureMessage` and feeds `child`, and
   `unrelated` stands alone, ends `failed` with `child` skipped and `unrelated` succeeded;
4. through the Next.js rewrite (`$WebBase/api/v1/...`, with the editor's session cookie and `Accept-Encoding:
   gzip`, as a browser sends it), the event stream of a run whose only node waits 5 s answers 200
   `text/event-stream` with no `Content-Encoding` and a `Cache-Control` that contains `no-transform`. A `node.state
   running` frame arrives within 3 s, the frames carry seq 1..N, and the stream ends after the terminal
   `run.state succeeded`. `System.Net.Http.HttpClient` with `HttpCompletionOption.ResponseHeadersRead` reads the
   frames as they come, so a buffering or compressing proxy fails this check. The same URL with `Last-Event-ID:
   <lastEventSeq>` on the finished run answers 204;
5. after the admin enables shell, a `corqty.shell.command` node whose `input` is the literal `{"name":"corqty"}`
   and whose command is `jq '{result: {greeting: ("hello " + .input.name)}}' "$CORQTY_INPUT" > "$CORQTY_OUTPUT"`
   returns `{"result":{"greeting":"hello corqty"}}`, which proves the resolved inputs reach `CORQTY_INPUT`;
6. the outsider gets 404 for the infrastructure run;
7. the audit log also holds `organization.shell_enabled` and `run.started`.

The README gains a Workers section, and the spec records the Milestone 3 decisions under §19.

- [ ] **Step 1: Write the failing worker tests**

Create `crates/worker/tests/health.rs`:

```rust
use std::fs::File;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

use corqty_worker::check_alive;
use tempfile::TempDir;

/// Makes the file at `path` look `age` old.
fn make_old(path: &Path, age: Duration) {
    let file = File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - age).unwrap();
}

#[test]
fn the_alive_file_counts_for_10_seconds() {
    let directory = TempDir::new().unwrap();
    let alive_file = directory.path().join("corqty-worker.alive");
    assert!(check_alive(&alive_file).is_err());

    File::create(&alive_file).unwrap();
    assert_eq!(check_alive(&alive_file), Ok(()));
    make_old(&alive_file, Duration::from_secs(9));
    assert_eq!(check_alive(&alive_file), Ok(()));
    make_old(&alive_file, Duration::from_secs(11));
    let stale = format!("{} is 11 s old", alive_file.display());
    assert_eq!(check_alive(&alive_file), Err(stale));
}

#[test]
fn the_healthcheck_subcommand_checks_the_alive_file_in_the_temporary_directory() {
    let directory = TempDir::new().unwrap();
    let healthcheck = || -> Output {
        Command::new(env!("CARGO_BIN_EXE_corqty-worker"))
            .arg("healthcheck")
            // The healthcheck needs no database. std::env::temp_dir() reads TMPDIR on unix
            // and TMP on Windows.
            .env_remove("DATABASE_URL_WORKER")
            .env("TMPDIR", directory.path())
            .env("TMP", directory.path())
            .output()
            .unwrap()
    };
    let missing = healthcheck();
    assert!(!missing.status.success());

    File::create(directory.path().join("corqty-worker.alive")).unwrap();
    let fresh = healthcheck();
    let stderr = String::from_utf8_lossy(&fresh.stderr);
    assert!(fresh.status.success(), "{stderr}");
}
```

In `crates/worker/tests/execution.rs`:

Replace:

```rust
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
```

With:

```rust
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
```

Replace:

```rust
use corqty_worker::{WorkerConfig, run};
```

With:

```rust
use corqty_worker::{WorkerConfig, check_alive, run};
```

Replace:

```rust
/// `base` with the top-level fields of `changes` replaced.
fn merge(mut base: Value, changes: Value) -> Value {
```

With:

```rust
/// Sets the modification time of the file at `path`.
fn set_modified(path: &Path, time: SystemTime) {
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(time).unwrap();
}

/// `base` with the top-level fields of `changes` replaced.
fn merge(mut base: Value, changes: Value) -> Value {
```

Replace:

```rust
    assert_eq!(truncations, 1);
}
```

With:

```rust
    assert_eq!(truncations, 1);
}

#[tokio::test]
#[serial]
async fn every_successful_claim_touches_the_alive_file() {
    let fixture = Fixture::new().await;
    let directory = TempDir::new().unwrap();
    let config = WorkerConfig {
        alive_file: directory.path().join("corqty-worker.alive"),
        ..fixture.config(1)
    };
    let alive_file = config.alive_file.clone();
    let path = alive_file.as_path();
    let worker = fixture.start(config, mock());
    // A worker with nothing to run still claims every 50 ms, and each claim touches the file.
    eventually(|| async move { check_alive(path).ok() }).await;
    let stale = SystemTime::now() - Duration::from_secs(60);
    set_modified(path, stale);
    eventually(|| async move { check_alive(path).ok() }).await;

    // Without its database the worker's claims fail, so the file goes stale.
    fixture.database.worker.close().await;
    sleep(Duration::from_millis(200)).await;
    set_modified(path, stale);
    sleep(Duration::from_millis(500)).await;
    assert!(check_alive(path).is_err());
    worker.stop().await;
}
```

The new tests:

- `the_alive_file_counts_for_10_seconds`: a missing file is an error; a new file, and one 9 s old, pass; one 11 s
  old fails with `<path> is 11 s old`.
- `the_healthcheck_subcommand_checks_the_alive_file_in_the_temporary_directory`: the built binary
  (`CARGO_BIN_EXE_corqty-worker`) run as `corqty-worker healthcheck` without `DATABASE_URL_WORKER` fails without the
  file and succeeds once it exists. `std::env::temp_dir()` reads `TMPDIR` on unix and `TMP` on Windows, so the test
  sets both.
- `every_successful_claim_touches_the_alive_file`: a worker with nothing to run makes the file fresh, makes it
  fresh again after the test ages it by 60 s, and lets it go stale once its pool is closed and its claims fail.

- [ ] **Step 2: Run the tests and verify RED**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-worker --test health --test execution
```

Expected: neither test compiles:

```text
error[E0432]: unresolved import `corqty_worker::check_alive`
 --> crates\worker\tests\health.rs:6:5
  |
6 | use corqty_worker::check_alive;
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^ no `check_alive` in the root

error[E0432]: unresolved import `corqty_worker::check_alive`
  --> crates\worker\tests\execution.rs:23:35
   |
23 | use corqty_worker::{WorkerConfig, check_alive, run};
   |                                   ^^^^^^^^^^^ no `check_alive` in the root

error[E0560]: struct `WorkerConfig` has no field named `alive_file`
   --> crates\worker\tests\execution.rs:911:9
    |
911 |         alive_file: directory.path().join("corqty-worker.alive"),
    |         ^^^^^^^^^^ `WorkerConfig` does not have this field
```

The run also reports E0609 (`no field alive_file on type WorkerConfig`) at line 914, then
``error: could not compile `corqty-worker` (test "health") due to 1 previous error`` and
``error: could not compile `corqty-worker` (test "execution") due to 3 previous errors``.

- [ ] **Step 3: Touch the alive file after every claim and add the healthcheck subcommand**

In `crates/worker/src/lib.rs`:

Replace:

```rust
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
```

With:

```rust
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
```

Replace:

```rust
/// How many log chunks an executor may send before it waits for the worker.
const LOG_CHANNEL_CAPACITY: usize = 64;
```

With:

```rust
/// How many log chunks an executor may send before it waits for the worker.
const LOG_CHANNEL_CAPACITY: usize = 64;
/// A worker counts as alive while it has claimed this recently.
const ALIVE_MAX_AGE: Duration = Duration::from_secs(10);
```

Replace:

```rust
    /// Where each attempt gets a scratch directory of its own.
    pub scratch_root: PathBuf,
}
```

With:

```rust
    /// Where each attempt gets a scratch directory of its own.
    pub scratch_root: PathBuf,
    /// Touched after every successful claim, so that a healthcheck can tell the worker still
    /// reaches the database.
    pub alive_file: PathBuf,
}
```

Replace:

```rust
            scratch_root: std::env::temp_dir(),
```

With:

```rust
            scratch_root: std::env::temp_dir(),
            alive_file: std::env::temp_dir().join("corqty-worker.alive"),
```

Replace:

```rust
            Ok(claimed) => {
```

With:

```rust
            Ok(claimed) => {
                touch(&worker.config.alive_file);
```

Replace:

```rust
    Ok(())
}

struct Worker {
```

With:

```rust
    Ok(())
}

/// Checks that a worker touched `alive_file` within the last 10 s, as it does after every
/// successful claim. The `healthcheck` subcommand runs this.
pub fn check_alive(alive_file: &Path) -> Result<(), String> {
    let modified = std::fs::metadata(alive_file)
        .and_then(|metadata| metadata.modified())
        .map_err(|error| format!("{}: {error}", alive_file.display()))?;
    // A file from the future, after the clock went back, counts as fresh.
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default();
    if age > ALIVE_MAX_AGE {
        return Err(format!(
            "{} is {} s old",
            alive_file.display(),
            age.as_secs()
        ));
    }
    Ok(())
}

/// Creates the file, or sets its modification time to now.
fn touch(path: &Path) {
    let touched = std::fs::File::create(path).and_then(|file| file.set_modified(SystemTime::now()));
    if let Err(error) = touched {
        tracing::warn!(error = %error, "touching the alive file failed");
    }
}

struct Worker {
```

Replace the whole of `crates/worker/src/main.rs`:

```rust
#![forbid(unsafe_code)]

use std::sync::Arc;

use corqty_executor_mock::MockExecutor;
use corqty_executor_shell::ShellExecutor;
use corqty_plugin_host::Registry;
use corqty_plugin_sdk::Executor;
use corqty_worker::{WorkerConfig, check_alive, run};
use sqlx::postgres::PgPoolOptions;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The Compose healthcheck reads only the alive file that the running worker touches.
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        return Ok(check_alive(&WorkerConfig::default().alive_file)?);
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "corqty_worker=info".into()),
        )
        .init();
    let database_url =
        std::env::var("DATABASE_URL_WORKER").map_err(|_| "DATABASE_URL_WORKER must be set")?;
    let mut config = WorkerConfig::default();
    if let Ok(capacity) = std::env::var("CORQTY_WORKER_CAPACITY") {
        config.capacity = capacity
            .parse()
            .ok()
            .filter(|capacity| *capacity >= 1)
            .ok_or("CORQTY_WORKER_CAPACITY must be a whole number of at least 1")?;
    }
    let executors: Vec<Arc<dyn Executor>> = vec![Arc::new(MockExecutor), Arc::new(ShellExecutor)];
    let registry = Registry::new(executors)?;
    // One connection for each running attempt, and one to claim with.
    let pool = PgPoolOptions::new()
        .max_connections(config.capacity.unsigned_abs() + 1)
        .connect(&database_url)
        .await?;
    tracing::info!(
        worker_id = %config.worker_id,
        capacity = config.capacity,
        executors = ?registry.advertised(),
        "the worker is ready"
    );
    let shutdown = CancellationToken::new();
    let signal = shutdown.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        signal.cancel();
    });
    run(config, registry, pool, shutdown).await?;
    Ok(())
}

/// Resolves on ctrl-c, or on SIGTERM (what `docker compose stop` sends) on unix.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install the ctrl-c handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install the SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}
```

- [ ] **Step 4: Verify GREEN**

```text
cargo test -p corqty-worker
```

Expected: `execution` 13 passed in about 13 s and `health` 2 passed.

Two negative controls show that the alive-file test catches the mistakes it targets. Each changed one place in
`lib.rs`, ran `cargo test -p corqty-worker --test execution every_successful_claim`, and restored the file:

- No `touch` after a claim: the test fails after 10 s with `the condition holds within 10 s: Elapsed(())`.
- A `touch` after a failed claim as well: the test fails with `assertion failed: check_alive(path).is_err()`,
  because a worker that cannot reach its database would stay healthy.

- [ ] **Step 5: Make the shell log-cap test require the chunk that crosses the cap**

In `crates/executor-shell/tests/contract.rs`:

Replace:

```rust
    assert_eq!(
        chunks.iter().map(|chunk| chunk.text.len()).sum::<usize>(),
        10_485_760
    );
```

With:

```rust
    // Chunks stop after the one that takes the log past the cap, which goes out whole.
    let sent: usize = chunks.iter().map(|chunk| chunk.text.len()).sum();
    let crossing = chunks.last().expect("the command logged").text.len();
    assert!(
        sent > 10_485_760 && sent - crossing <= 10_485_760,
        "{sent} bytes"
    );
```

- [ ] **Step 6: Run the shell tests on Linux and verify RED**

The shell tests are `#![cfg(unix)]`, so they run in a Linux container, from the repo root:

```text
docker run --rm -v "${PWD}:/work" -w /work -e CARGO_TARGET_DIR=/tmp/target rust:1.96-bookworm cargo test -p corqty-executor-shell
```

Expected: 14 passed and 1 failed. The flood delivers exactly the cap and never passes it:

```text
thread 'twenty_mebibytes_of_output_stop_at_the_log_cap_without_blocking_the_command' (2503) panicked at crates/executor-shell/tests/contract.rs:260:5:
10485760 bytes
```

The thread number varies. The run ends with ``error: test failed, to rerun pass `-p corqty-executor-shell --test contract` ``.

- [ ] **Step 7: Send the chunk that crosses the cap whole**

In `crates/executor-shell/src/lib.rs`:

Replace:

```rust
        let budget = AtomicUsize::new(MAX_LOG_BYTES_PER_NODE_RUN);
```

With:

```rust
        let sent = AtomicUsize::new(0);
```

Replace:

```rust
                forward(stdout, LogStream::Stdout, &budget, &context.logs),
                forward(stderr, LogStream::Stderr, &budget, &context.logs),
```

With:

```rust
                forward(stdout, LogStream::Stdout, &sent, &context.logs),
                forward(stderr, LogStream::Stderr, &sent, &context.logs),
```

Replace:

```rust
    /// Sends a pipe to the log channel in chunks of at most 8 KiB while the attempt's log
    /// budget lasts. Then it keeps reading and drops the rest, so the command never blocks
    /// on a full pipe.
    async fn forward(
        mut pipe: impl AsyncRead + Unpin,
        stream: LogStream,
        budget: &AtomicUsize,
```

With:

```rust
    /// Sends a pipe to the log channel in chunks of at most 8 KiB until the attempt has sent
    /// more than the log cap. Then it keeps reading and drops the rest, so the command never
    /// blocks on a full pipe.
    async fn forward(
        mut pipe: impl AsyncRead + Unpin,
        stream: LogStream,
        sent: &AtomicUsize,
```

Replace:

```rust
            let mut text = String::from_utf8_lossy(&pending).into_owned();
            pending = rest;
            let (Ok(left) | Err(left)) =
                budget.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                    Some(left.saturating_sub(text.len()))
                });
            text.truncate(text.floor_char_boundary(left));
            if !text.is_empty() {
                let _ = logs.send(LogChunk { stream, text }).await;
            }
```

With:

```rust
            let text = String::from_utf8_lossy(&pending).into_owned();
            pending = rest;
            // The chunk that takes the log past the cap still goes out whole, so the database
            // sees the log pass the cap and records the truncation.
            let before = sent.fetch_add(text.len(), Ordering::Relaxed);
            if before <= MAX_LOG_BYTES_PER_NODE_RUN && !text.is_empty() {
                let _ = logs.send(LogChunk { stream, text }).await;
            }
```

- [ ] **Step 8: Verify GREEN on Linux and lint the unix code**

```text
docker run --rm -v "${PWD}:/work" -w /work -e CARGO_TARGET_DIR=/tmp/target rust:1.96-bookworm cargo test -p corqty-executor-shell
```

Expected: `contract` 15 passed in about 3 s, among them
`twenty_mebibytes_of_output_stop_at_the_log_cap_without_blocking_the_command`.

Windows never compiles the shell's `#[cfg(unix)] mod imp` or the worker's SIGTERM branch, and the healthcheck test
should also pass where `std::env::temp_dir()` reads `TMPDIR`. Lint the workspace and run the health tests on Linux.
The mount is read-only, and `--locked` keeps cargo from rewriting the lockfile:

```text
docker run --rm -v "${PWD}:/work:ro" -w /work -e CARGO_TARGET_DIR=/tmp/target rust:1.96-bookworm bash -c 'cargo clippy --locked --workspace --all-targets -- -D warnings && cargo test --locked -p corqty-worker --test health'
```

Expected: clippy prints `Finished` with no warnings, and `health` reports 2 passed.

- [ ] **Step 9: Write the failing smoke checks**

In `scripts/smoke-foundation.ps1`:

Replace:

```powershell
    [string]$WebBase = "http://localhost:3000"
```

With:

```powershell
    [string]$WebBase = "http://localhost:3000",
    [int]$ExpectedWorkers = 1
```

Replace:

```powershell
$audit = Invoke-RestMethod `
```

With:

```powershell
# Every worker replica runs healthy as uid 10001, unprivileged, on a read-only root
# filesystem and without bind or volume mounts.
$workers = @(
    docker compose --project-name $ProjectName ps --format json worker |
        ConvertFrom-Json
)
$healthyWorkers = @(
    $workers | Where-Object { $_.State -eq "running" -and $_.Health -eq "healthy" }
)
if ($workers.Count -ne $ExpectedWorkers -or $healthyWorkers.Count -ne $ExpectedWorkers) {
    throw "Expected $ExpectedWorkers healthy workers, found $($healthyWorkers.Count) of $($workers.Count)"
}
foreach ($worker in $workers) {
    $workerUser = docker exec $worker.ID id -u
    $isolation = docker inspect `
        --format "{{.HostConfig.Privileged}} {{.HostConfig.ReadonlyRootfs}} {{len .Mounts}}" `
        $worker.ID
    if ($workerUser -ne "10001" -or $isolation -ne "false true 0") {
        throw "Worker $($worker.Name) runs as user $workerUser with '$isolation' (privileged, read-only, mounts)"
    }
}

function New-SmokeTask {
    param(
        [string]$Id,
        [int]$DelayMs = 0,
        $FailureMessage = $null
    )

    @{
        id = $Id
        definition = @{ key = "corqty.mock.task"; version = 1 }
        displayName = $Id
        position = @{ x = 0; y = 0 }
        config = @{
            mode = "fixed"
            output = @{ result = $Id }
            delayMs = $DelayMs
            failureMessage = $FailureMessage
        }
        inputs = @{}
        retry = @{ maxAttempts = 1 }
        timeoutSeconds = 60
    }
}

# Creates a workflow as the editor and publishes the graph as version 1. Returns its path.
function Publish-SmokeWorkflow {
    param(
        [string]$Name,
        [object[]]$Nodes,
        [object[]]$Edges = @()
    )

    $created = Invoke-SmokeRequest `
        -User $editor `
        -Method Post `
        -Path "$organizationPath/workflows" `
        -Body @{ name = $Name }
    $path = "$organizationPath/workflows/$($created.Body.id)"
    $saved = Invoke-SmokeRequest `
        -User $editor `
        -Method Put `
        -Path "$path/draft" `
        -Body @{
            revision = 1
            graph = @{
                schemaVersion = 1
                workflowInputSchema = @{ type = "object" }
                nodes = $Nodes
                edges = $Edges
                viewport = @{ x = 0; y = 0; zoom = 1 }
            }
        }
    $published = Invoke-SmokeRequest `
        -User $editor `
        -Method Post `
        -Path "$path/publish" `
        -Body @{ revision = 2 }
    if ($published.Status -ne 201) {
        throw "Publishing '$Name' returned $($created.Status), $($saved.Status) and $($published.Status)"
    }
    $path
}

function Start-SmokeRun {
    param(
        [string]$WorkflowPath,
        [hashtable]$RunInput = @{}
    )

    $started = Invoke-SmokeRequest `
        -User $editor `
        -Method Post `
        -Path "$WorkflowPath/runs" `
        -Body @{ input = $RunInput }
    if ($started.Status -ne 201 -or $started.Body.status -ne "queued") {
        throw "Starting a run of $WorkflowPath returned $($started.Status)"
    }
    $started.Body
}

# Polls the run for up to 60 s and returns it once it has finished.
function Wait-SmokeRun {
    param([string]$RunId)

    $deadline = (Get-Date).AddSeconds(60)
    while ($true) {
        $run = (Invoke-SmokeRequest -User $editor -Method Get -Path "$organizationPath/runs/$RunId").Body
        if ($run.status -notin @("queued", "running")) {
            return $run
        }
        if ((Get-Date) -gt $deadline) {
            throw "Run $RunId is still $($run.status) after 60 s"
        }
        Start-Sleep -Milliseconds 250
    }
}

function Get-SmokeNode {
    param(
        $Run,
        [string]$NodeId
    )

    $Run.nodes | Where-Object { $_.nodeId -eq $NodeId }
}

# vpc's output reaches rds and deploy. With one slot per worker, rds and deploy overlap
# only when two workers run them.
$infrastructureRun = Wait-SmokeRun (Start-SmokeRun $infrastructurePath @{ region = "us-east-1" }).id
# Applying the blueprint gave the nodes fresh ids, so they are found by definition.
$infrastructureNodes = @{}
foreach ($node in $infrastructureRun.graph.nodes) {
    $infrastructureNodes[$node.definition.key] = Get-SmokeNode $infrastructureRun $node.id
}
$vpc = $infrastructureNodes["corqty.simulated.create-vpc"]
$rds = $infrastructureNodes["corqty.simulated.create-rds"]
$deploy = $infrastructureNodes["corqty.simulated.deploy-service"]
if (
    $infrastructureRun.status -ne "succeeded" -or
    $vpc.input.region -ne "us-east-1" -or
    $rds.input.vpcId -ne $vpc.output.vpcId -or
    $deploy.input.vpcId -ne $vpc.output.vpcId
) {
    throw "The infrastructure run ended $($infrastructureRun.status) without passing the VPC id on"
}
if (
    [DateTimeOffset]$rds.startedAt -ge [DateTimeOffset]$deploy.finishedAt -or
    [DateTimeOffset]$deploy.startedAt -ge [DateTimeOffset]$rds.finishedAt
) {
    throw "rds and deploy did not run at the same time"
}

# A failed node skips only its descendants.
$failurePath = Publish-SmokeWorkflow -Name "Smoke failure" -Nodes @(
    (New-SmokeTask -Id "fail" -FailureMessage "The smoke test fails this node."),
    (New-SmokeTask -Id "child"),
    (New-SmokeTask -Id "unrelated")
) -Edges @(
    @{
        id = "fail-child"
        source = @{ nodeId = "fail"; port = "result" }
        target = @{ nodeId = "child"; port = "input" }
    }
)
$failureRun = Wait-SmokeRun (Start-SmokeRun $failurePath).id
$states = @(
    $failureRun.nodes | Sort-Object nodeId | ForEach-Object { "$($_.nodeId) $($_.state)" }
) -join ", "
if (
    $failureRun.status -ne "failed" -or
    $states -ne "child skipped, fail failed, unrelated succeeded" -or
    (Get-SmokeNode $failureRun "fail").error.code -ne "mock_failure"
) {
    throw "The failure run ended $($failureRun.status) with $states"
}

# Events stream live through the Next.js rewrite: a frame arrives while the node still
# runs, so the rewrite neither buffers nor compresses the stream.
$streamPath = Publish-SmokeWorkflow -Name "Smoke stream" -Nodes @(
    New-SmokeTask -Id "slow" -DelayMs 5000
)
$streamRun = Start-SmokeRun $streamPath
Add-Type -AssemblyName System.Net.Http
$handler = New-Object System.Net.Http.HttpClientHandler
$handler.CookieContainer = $editor.Session.Cookies
$client = New-Object System.Net.Http.HttpClient($handler)
# Browsers accept gzip, and Next.js compresses (and so buffers) any text response that
# lacks Cache-Control: no-transform.
$client.DefaultRequestHeaders.Add("Accept-Encoding", "gzip")
$eventsUrl = "$WebBase/api/v1$organizationPath/runs/$($streamRun.id)/events?after=0"
$clock = [System.Diagnostics.Stopwatch]::StartNew()
$stream = $client.GetAsync(
    $eventsUrl,
    [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead
).GetAwaiter().GetResult()
if (
    [int]$stream.StatusCode -ne 200 -or
    $stream.Content.Headers.ContentType.MediaType -ne "text/event-stream" -or
    $stream.Content.Headers.ContentEncoding.Count -ne 0 -or
    -not $stream.Headers.CacheControl.NoTransform
) {
    throw "The event stream returned $([int]$stream.StatusCode) with Content-Encoding '$($stream.Content.Headers.ContentEncoding)' and Cache-Control '$($stream.Headers.CacheControl)'"
}
$reader = New-Object System.IO.StreamReader(
    $stream.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
)
$frames = @()
$runningAfter = $null
while ($true) {
    $line = $reader.ReadLineAsync()
    if (-not $line.Wait(10000) -or $clock.Elapsed.TotalSeconds -gt 30) {
        throw "The event stream did not end after the run finished"
    }
    if ($null -eq $line.Result) {
        break
    }
    if ($line.Result.StartsWith("data: ")) {
        $frame = $line.Result.Substring(6) | ConvertFrom-Json
        $frames += $frame
        if ($null -eq $runningAfter -and $frame.type -eq "node.state" -and $frame.state -eq "running") {
            $runningAfter = $clock.Elapsed
        }
    }
}
if ($null -eq $runningAfter -or $runningAfter.TotalSeconds -gt 3) {
    throw "The running node.state frame took $runningAfter to arrive through the web service"
}
$sequence = @($frames | ForEach-Object { $_.seq }) -join ","
$last = $frames[-1]
if (
    $sequence -ne ((1..$frames.Count) -join ",") -or
    $last.type -ne "run.state" -or
    $last.state -ne "succeeded"
) {
    throw "The event stream sent seq $sequence and ended with $($last | ConvertTo-Json -Compress)"
}
$streamRun = Wait-SmokeRun $streamRun.id
$caughtUp = New-Object System.Net.Http.HttpRequestMessage(
    [System.Net.Http.HttpMethod]::Get,
    $eventsUrl
)
$caughtUp.Headers.Add("Last-Event-ID", [string]$streamRun.lastEventSeq)
$replayed = $client.SendAsync($caughtUp).GetAwaiter().GetResult()
if ($streamRun.lastEventSeq -ne $last.seq -or [int]$replayed.StatusCode -ne 204) {
    throw "A caught-up stream of the finished run returned $([int]$replayed.StatusCode), not 204"
}
$client.Dispose()

# An admin opts into the trusted shell executor. The shell node reads its resolved inputs
# from CORQTY_INPUT and writes JSON to CORQTY_OUTPUT with jq from the worker image.
$shellEnabled = Invoke-SmokeRequest `
    -User $admin `
    -Method Put `
    -Path "$organizationPath/settings/shell" `
    -Body @{ enabled = $true }
if ($shellEnabled.Status -ne 200 -or $shellEnabled.Body.shellEnabled -ne $true) {
    throw "Enabling the shell executor returned $($shellEnabled.Status)"
}
$greet = New-SmokeTask -Id "greet"
$greet.definition.key = "corqty.shell.command"
$greet.config = @{
    command = 'jq ''{result: {greeting: ("hello " + .input.name)}}'' "$CORQTY_INPUT" > "$CORQTY_OUTPUT"'
    env = @{}
}
$greet.inputs = @{ input = @{ kind = "literal"; value = @{ name = "corqty" } } }
$shellPath = Publish-SmokeWorkflow -Name "Smoke shell" -Nodes @($greet)
$shellRun = Wait-SmokeRun (Start-SmokeRun $shellPath).id
$greeting = (Get-SmokeNode $shellRun "greet").output | ConvertTo-Json -Compress -Depth 8
if ($shellRun.status -ne "succeeded" -or $greeting -ne '{"result":{"greeting":"hello corqty"}}') {
    throw "The shell run ended $($shellRun.status) with output $greeting"
}

$foreignRun = Invoke-SmokeRequest `
    -User $outsider `
    -Method Get `
    -Path "/orgs/$outsiderOrganizationId/runs/$($infrastructureRun.id)"
if ($foreignRun.Status -ne 404) {
    throw "Cross-tenant run request returned $($foreignRun.Status), not 404"
}

$audit = Invoke-RestMethod `
```

Replace:

```powershell
    "workflow.published"
```

With:

```powershell
    "workflow.published",
    "organization.shell_enabled",
    "run.started"
```

- [ ] **Step 10: Run the smoke test against a stack without workers and verify RED**

```text
docker compose --project-name corqty-m3 up --build --wait
```

Expected: exit code 0 after `Container corqty-m3-migrate-1 Exited`, `Container corqty-m3-api-1 Healthy` and
`Container corqty-m3-web-1 Healthy`. There is no worker container.

```text
./scripts/smoke-foundation.ps1 -ProjectName corqty-m3 -ExpectedWorkers 3
```

In Windows PowerShell 5.1, run
`powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-foundation.ps1 -ProjectName corqty-m3 -ExpectedWorkers 3`.

Expected: every earlier check passes. The script then stops at line 297 with:

```text
Expected 3 healthy workers, found 0 of 0
```

```text
docker compose --project-name corqty-m3 down --volumes
```

Expected: `Volume corqty-m3_postgres-data Removed` and `Network corqty-m3_default Removed`.

- [ ] **Step 11: Build the worker image and add the worker service**

In `docker/rust/Dockerfile`:

Replace:

```dockerfile
    cargo build --release --locked -p corqty-api -p corqty-migrator \
    && install -D -m 0755 -t /out target/release/corqty-api target/release/corqty-migrator
```

With:

```dockerfile
    cargo build --release --locked -p corqty-api -p corqty-migrator -p corqty-worker \
    && install -D -m 0755 -t /out \
        target/release/corqty-api target/release/corqty-migrator target/release/corqty-worker
```

Replace:

```dockerfile
ENTRYPOINT ["corqty-api"]
```

With:

```dockerfile
ENTRYPOINT ["corqty-api"]

FROM runtime AS worker
# The tools that trusted shell commands can call. Other tools need a custom worker image.
RUN apt-get update \
    && apt-get install -y --no-install-recommends bash jq \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /out/corqty-worker /usr/local/bin/
USER 10001:10001
ENTRYPOINT ["corqty-worker"]
```

In `docker-compose.yml`:

Replace:

```yaml
  web:
```

With:

```yaml
  worker:
    build:
      context: .
      dockerfile: docker/rust/Dockerfile
      target: worker
    init: true
    # The shell executor writes only to its scratch directories in /tmp.
    read_only: true
    tmpfs:
      - /tmp
    # A worker drains its attempts for 25 s and then gives executors 5 s to stop.
    stop_grace_period: 30s
    environment:
      DATABASE_URL_WORKER: ${DATABASE_URL_WORKER:-postgres://corqty_worker:corqty_worker_dev@postgres:5432/corqty}
      CORQTY_WORKER_CAPACITY: ${CORQTY_WORKER_CAPACITY:-4}
    depends_on:
      migrate:
        condition: service_completed_successfully
    healthcheck:
      test: ["CMD", "/usr/local/bin/corqty-worker", "healthcheck"]
      interval: 2s
      timeout: 3s
      retries: 5

  web:
```

In `.env.example`:

Replace:

```text
DATABASE_URL_API=postgres://corqty_api:corqty_api_dev@postgres:5432/corqty
```

With:

```text
DATABASE_URL_API=postgres://corqty_api:corqty_api_dev@postgres:5432/corqty
DATABASE_URL_WORKER=postgres://corqty_worker:corqty_worker_dev@postgres:5432/corqty
# Node runs that each worker container executes at once.
CORQTY_WORKER_CAPACITY=4
# Run inputs, node inputs and node outputs are limited to 1 MiB each, and logs to 10 MiB
# per node run. These limits are fixed constants until Milestone 5.
```

- [ ] **Step 12: Run three single-slot workers in CI**

In `.github/workflows/ci.yml`:

Replace:

```yaml
    timeout-minutes: 30
```

With:

```yaml
    timeout-minutes: 45
```

Replace:

```yaml
        run: docker compose --project-name corqty-ci up --build --wait
      - name: Run Compose smoke test
        shell: pwsh
        run: ./scripts/smoke-foundation.ps1 -ProjectName corqty-ci
```

With:

```yaml
        env:
          CORQTY_WORKER_CAPACITY: "1"
        run: docker compose --project-name corqty-ci up --build --wait --scale worker=3
      - name: Run Compose smoke test
        shell: pwsh
        run: ./scripts/smoke-foundation.ps1 -ProjectName corqty-ci -ExpectedWorkers 3
```

- [ ] **Step 13: Document the workers and the Milestone 3 decisions**

In `README.md`:

Replace:

````markdown
automatic draft saves, and validated, immutable published versions.
````

With:

````markdown
automatic draft saves, and validated, immutable published versions.
Milestone 3 adds execution: manual runs scheduled in PostgreSQL, scalable
workers with the built-in mock and trusted shell executors, retries, leases
and cancellation, and live run views over server-sent events.
````

Replace:

````markdown
## Test
````

With:

````markdown
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
````

In `docs/superpowers/specs/2026-07-28-corqty-workflow-orchestrator-design.md`:

Replace:

```markdown
**Status:** Approved; Milestone 1 foundation and Milestone 2 catalog and studio implemented
```

With:

```markdown
**Status:** Approved; Milestone 1 foundation, Milestone 2 catalog and studio, and Milestone 3 execution core implemented
```

Replace:

```markdown
## 20. Acceptance criteria
```

With:

```markdown
### Milestone 3 decisions

- Executors are identified as `<key>@<version>`, for example `mock@1.0.0`, in the database and on the wire. In Milestone 3 `crates/plugin-host` holds only the static compiled registry. It rejects duplicate identities, malformed keys or versions, another host contract, and the `SecretRead` and `Network` capabilities, which arrive with Wasmtime in Milestone 4. Workers do not call an executor's config validation, because publishing already validated the config.
- The mock executor also takes an optional `failAttempts`: attempts up to that number fail with a retryable error. In `echo` mode it returns `{"result": <resolved inputs>}`.
- Node states move as §11.1 describes, plus `retry_wait -> canceled` and `retry_wait -> running`: a due retry is claimed directly, without a `queued` event. An attempt counts as soon as it is claimed. The retry backoff, `least(300, 2 * 2^(attempt - 1) * (1 + 0.2 * random()))` seconds, is computed in SQL.
- An expired lease counts as a retryable `lease_expired` failure of the same attempt. The node waits for a retry with the usual backoff, fails and skips its descendants when no attempts are left, or is canceled after a cancel request. Every API replica runs the recovery every 5 seconds, and `pg_try_advisory_xact_lock` lets one replica at a time do a pass.
- Cancelling a run cancels its blocked, queued and retry-waiting nodes at once and records the request. A running attempt learns of it from its heartbeat or its start, gets 5 seconds to stop and ends `canceled`. After a cancel request no failure is retried. A timeout fails the attempt with `timeout`, which is retryable only for executors that declare it (shell). Shutting a worker down is not a cancel: the worker stops claiming, lets running attempts finish for 25 seconds, then abandons the rest without reporting them, and lease recovery takes them over.
- Run events carry a gapless per-run `seq` and a `type`: `run.state`, `node.state` (with `attempt` and, where it applies, `error` or `output`), `node.log` (stream `stdout`, `stderr` or `system`) and `node.log_truncated`. Creating a run writes only `run.state queued`. `GET /orgs/{org_id}/runs/{run_id}/events` sends unnamed events with `id: <seq>`, replays after `Last-Event-ID` (or `?after=`), and ends after the terminal `run.state`, when the API shuts down, or after 10 minutes, after which the browser reconnects. A finished run whose events the client already has answers 204. The API sends `Cache-Control: no-cache, no-transform`, which keeps the Next.js rewrite from compressing the stream.
- The run routes are `POST /orgs/{org_id}/workflows/{workflow_id}/runs {input?, version?}` (the published version by default), `GET /orgs/{org_id}/workflows/{workflow_id}/runs` (the newest 50), `GET /orgs/{org_id}/runs/{run_id}`, `POST /orgs/{org_id}/runs/{run_id}/cancel`, and `POST /orgs/{org_id}/runs/{run_id}/rerun {version?}`, which starts a new run with the source run's input. They write the audit events `run.started`, `run.rerun` and `run.canceled`, which carry only identifiers and versions. A node run's API view includes its resolved `input`, and its `queueReason` is `waiting_for_compatible_worker` while no worker that advertises its executor has been seen in the last 60 seconds.
- The shell preset `corqty.shell.command` version 1 takes `{command, env}`. `command` runs under `/bin/sh -c` in a fresh scratch directory, which is also `HOME` and `TMPDIR`, with only `PATH`, `LANG`, `CORQTY_INPUT`, `CORQTY_OUTPUT` and the `env` entries whose names match `^[A-Z_][A-Z0-9_]*$` and do not start with `CORQTY_`. An admin enables shell per organization with `PUT /orgs/{org_id}/settings/shell`, which writes `organization.shell_enabled` or `organization.shell_disabled` only when the value changes. While shell is disabled, publishing and starting runs report each shell node as `executor_disabled`. Disabling it never touches existing runs. The studio palette lists shell definitions only while shell is enabled, and the settings page and the inspector warn that shell is for trusted users and is not a security boundary.
- The `corqty_worker` role has no table privileges. It may call only six security-definer functions: `register_worker`, `claim_node_runs`, `start_node_run`, `heartbeat`, `append_node_log` and `complete_node_run`. Every call after the claim is fenced by the worker id and a lease token. Each worker process registers a new `Uuid::now_v7()` identity, so scaled containers never share one.
- Runs are watched on `/orgs/{org_id}/runs/{run_id}`, which also works below 1024 px. Node-run states overlay the pinned version's graph, a details panel shows the selected node's state, attempts, queue reason, error, input and output, and a bottom drawer streams the run's logs, filtered to the selected node. The studio's bottom drawer lists recent runs and streams the newest run's logs while it is open on its Logs tab. The browser keeps at most 2,000 events per run.
- Run input, resolved node input and node output are limited to 1 MiB each, and logs to 10 MiB per node run, after which one `node.log_truncated` event marks the cut. Like the Milestone 2 limits, these are fixed constants until Milestone 5.

## 20. Acceptance criteria
```

- [ ] **Step 14: Verify GREEN with three single-slot workers**

```text
docker compose config --quiet
```

Expected: no output, exit code 0.

Start the stack with one slot per worker and three workers, as CI does. In PowerShell:

```text
$env:CORQTY_WORKER_CAPACITY = "1"
docker compose --project-name corqty-m3 up --build --wait --scale worker=3
Remove-Item Env:CORQTY_WORKER_CAPACITY
```

In bash, run `CORQTY_WORKER_CAPACITY=1 docker compose --project-name corqty-m3 up --build --wait --scale worker=3`.

Expected: one builder step compiles `corqty-api`, `corqty-migrator` and `corqty-worker` in release mode (it shows
`CACHED` when the Rust sources did not change since the last build), and the images include `corqty-m3-worker`
(about 135 MB). The command exits 0 after `Container corqty-m3-worker-1 Healthy`,
`Container corqty-m3-worker-2 Healthy`, `Container corqty-m3-worker-3 Healthy` and `Container corqty-m3-web-1
Healthy`.

```text
./scripts/smoke-foundation.ps1 -ProjectName corqty-m3 -ExpectedWorkers 3
```

Expected: the script prints nothing and exits 0, in about a minute.

```text
docker compose --project-name corqty-m3 logs worker
docker exec corqty-m3-worker-1 id
docker exec corqty-m3-worker-1 sh -c 'ls -la /tmp; cat /proc/1/comm'
```

Expected: each of the three workers logged `the worker is ready` with its own `worker_id`, `capacity=1` and
`executors=["mock@1.0.0", "shell@1.0.0"]`. `id` prints `uid=10001(corqty) gid=10001(corqty) groups=10001(corqty)`.
`/tmp` holds only `corqty-worker.alive`, owned by `corqty`, and PID 1 is `docker-init`.

Check that SIGTERM stops idle workers cleanly through the init:

```text
docker compose --project-name corqty-m3 stop worker
docker inspect --format "{{.State.ExitCode}}" corqty-m3-worker-1 corqty-m3-worker-2 corqty-m3-worker-3
```

Expected: `stop` returns in under a second, each exit code is `0`, and each worker's last log line is
`corqty_worker: shutting down: finishing running attempts running=0`.

```text
docker compose --project-name corqty-m3 down --volumes
```

Expected: `Volume corqty-m3_postgres-data Removed` and `Network corqty-m3_default Removed`.

- [ ] **Step 15: Run the full verification**

```text
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Expected: formatting, clippy and `git diff --check` exit 0 with no output beyond `Finished`. Every suite passes:

- corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 6, `catalog_http` 4,
  `events_http` 4, `loops` 1, `members_http` 3, `organizations_http` 1, `problems_http` 2, `publish_http` 2,
  `runs_http` 8, `settings_http` 2, `shutdown` 1, `workflows_http` 4.
- corqty-domain: `account_and_rbac` 6, `catalog_rules` 7, `graph_rules` 8, `publish_rules` 11, `run_rules` 8,
  `runtime_rules` 12.
- corqty-executor-mock: `contract` 11.
- corqty-executor-shell: `contract` 0 on Windows and macOS, 15 on Linux.
- corqty-persistence: `auth_store` 2, `catalog_guards` 4, `final_admin` 2, `migrations` 1, `run_store` 8,
  `tenant_isolation` 1, `test_reset` 1, `worker_queue` 15, `workflow_catalog` 6.
- corqty-plugin-host: `registry` 8.
- corqty-worker: `execution` 13, `health` 2.

That is 172 tests on Windows and 187 on Linux (CI).

```text
pnpm install --frozen-lockfile
pnpm web:lint
pnpm web:test
pnpm web:build
```

Expected: the install changes nothing. Lint prints `✓ Types generated successfully` with no tsc or oxlint findings.
`web:test` reports `Test Files  17 passed (17)` and `Tests  115 passed (115)`. The build ends with the route table
that lists `ƒ /orgs/[organizationId]/runs/[runId]` and `ƒ /orgs/[organizationId]/settings`.

- [ ] **Step 16: Commit**

```text
git add .env.example .github/workflows/ci.yml README.md crates/executor-shell crates/worker docker-compose.yml docker/rust/Dockerfile docs/superpowers/specs/2026-07-28-corqty-workflow-orchestrator-design.md scripts/smoke-foundation.ps1
git commit -m "feat: run workers in Compose and smoke-test executions end to end"
```

Expected: `git show --stat HEAD` lists exactly the 13 files above (525 insertions, 31 deletions).

## Task 19: Gate the shell executor at the operator level and bound live streams

**Files:**

- Create: `migrations/202610270001_live_event_details.sql`
- Modify: `crates/api/src/config.rs`
- Modify: `crates/api/src/error.rs`
- Modify: `crates/api/src/events.rs`
- Modify: `crates/api/src/organizations/settings.rs`
- Modify: `crates/api/src/runs.rs`
- Modify: `crates/api/src/state.rs`
- Modify: `crates/api/src/workflows.rs`
- Modify: `crates/worker/src/lib.rs`
- Modify: `crates/worker/src/main.rs`
- Modify: `crates/persistence/tests/worker_queue.rs`
- Modify: `crates/worker/tests/execution.rs`
- Modify: `docker-compose.yml`
- Modify: `.env.example`
- Modify: `.github/workflows/ci.yml`
- Modify: `README.md`
- Modify: `docs/superpowers/specs/2026-07-28-corqty-workflow-orchestrator-design.md`
- Test: `crates/api/tests/settings_http.rs`
- Test: `crates/api/tests/events_http.rs`
- Test: `crates/worker/tests/shell_switch.rs`
- Test: `scripts/smoke-foundation.ps1`

An adversarial review of Tasks 1-18 confirmed one serious security finding and three smaller defects in the
backend. This task fixes all four. Task 20 makes the matching changes in the web app.

**An operator switch for shell.** Anyone can register, and registering makes the new user the admin of a new
organization, who could then enable shell. The command would run in the shared worker containers as the worker's
uid. That uid can read the worker's database credential (`DATABASE_URL_WORKER`) from `/proc`. With it, the command
could claim, read and complete the node runs of every organization, not just its own. It could also read or change
the scratch directories of other organizations' attempts. Spec §3 limits shell to trusted, self-hosted
organizations, but nothing let an operator enforce that. Now the deployment decides first, with
`CORQTY_SHELL_EXECUTOR=enabled`. Any other value, or no value, means off. Both the API and the worker read it:

- The worker registers `ShellExecutor` only when the switch is on. Otherwise it logs `the shell executor is off`,
  and its `the worker is ready` line lists only `mock@1.0.0`. This is the part that protects other tenants: a worker
  without shell never claims a shell node.
- `ApiConfig` gains `shell_executor_available`, read from the same variable. `ApiConfig::for_tests()` sets it to
  true, so the existing tests keep their meaning.
- `GET /orgs/{org}/settings`, and `PUT .../settings/shell` too, answer `{shellAvailable, shellEnabled}`.
  `shellEnabled` is still the organization's own choice.
- While the deployment does not offer shell, `PUT .../settings/shell {"enabled": true}` answers 409
  `shell_unavailable`. Disabling always works.
- `organizations::settings::shell_allowed(state, access)` is the one rule for validate, publish, run creation and
  rerun: the deployment offers shell and the organization enabled it. Otherwise each shell node gets the existing
  `executor_disabled` diagnostic.
- Compose passes `CORQTY_SHELL_EXECUTOR: ${CORQTY_SHELL_EXECUTOR:-disabled}` to `api` and `worker`, and
  `.env.example` documents the variable with the warning. CI turns it on for the Compose stack.
- The smoke script checks `shellAvailable` before its shell step, and stops with a clear message without it.
- The README and the spec's Milestone 3 decisions state the trust boundary. They also state that a timeout or a
  cancel signals only the command's process group, so a process that calls `setsid` or daemonises is not stopped.

**A cap on open event streams.** An open SSE stream reads the database whenever its run changes, from the API's pool
of 20 connections. With no cap, one account could open thousands of streams on a busy run, and then requests and
lease recovery for every organization would wait for connections. `AppState` gains `streams: Arc<Semaphore>` with
256 permits (`events::MAX_STREAMS`) and `streams_per_user: Arc<Mutex<HashMap<Uuid, usize>>>`. After the
authorization and the 204 check, `stream_events` takes a `Slot`: a permit, plus one of the user's 8 places. Without a
slot it answers 429 `too_many_streams`, through the new `ApiError::TooManyRequests`. The slot lives in the stream's
`Follower`. Dropping the response body gives it back, so it returns when the stream ends, when the client goes away
or when the server shuts down.

**The scratch directory goes last.** The worker used to remove an attempt's scratch directory as soon as the executor
returned, and only then validated the output, flushed the logs and completed the attempt. No heartbeat renews the
lease once `supervise` returns. A slow recursive delete, of a large tree on a disk-backed scratch root, could outlast
the lease, and lease recovery would then throw away a successful attempt. Now `execute` leaves the `TempDir` in an
`Option` that `attempt` owns. `attempt` removes the directory with `spawn_blocking` after `complete` returns, or
after it decided not to report anything because the lease is gone or the worker is shutting down.

**The input in live events.** The live run view showed `Input: None` for every node that finished after the page
loaded, because no event carried the input that `finish_node_run` stores. Migration
`202610270001_live_event_details.sql` replaces `corqty.finish_node_run` with the same function, except that the
`node.state` event that ends an attempt also carries `input`. That event is `succeeded`, `failed`, `canceled` or
`retry_wait`. An input that was never resolved is NULL and is left out, like every top-level null.
`CREATE OR REPLACE` keeps the function's owner and privileges, so `catalog_guards.rs` passes unchanged.

- [ ] **Step 1: Write the failing tests**

In `crates/api/tests/settings_http.rs`:

Replace:

```rust
use axum::{
    body::to_bytes,
    http::{Method, StatusCode},
    response::Response,
};
use corqty_persistence::worker_queue::WorkerDb;
use serde_json::{Value, json};
use serial_test::serial;
use support::AuthFixture;
use uuid::Uuid;
```

With:

```rust
use axum::{
    body::to_bytes,
    http::{Method, StatusCode},
    response::Response,
};
use corqty_api::{config::ApiConfig, router::build_router, state::AppState};
use corqty_persistence::{store::PgStore, worker_queue::WorkerDb};
use corqty_test_support::API_URL;
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use support::AuthFixture;
use uuid::Uuid;
```

Replace:

```rust
    let (enabled, disabled) = (
        json!({ "shellEnabled": true }),
        json!({ "shellEnabled": false }),
    );
```

With:

```rust
    let (enabled, disabled) = (
        json!({ "shellAvailable": true, "shellEnabled": true }),
        json!({ "shellAvailable": true, "shellEnabled": false }),
    );
```

Replace:

```rust
    let shell_job = (run_id, "shell@1.0.0".to_owned());
    assert_eq!(claimed, [shell_job.clone(), shell_job]);
}
```

With:

```rust
    let shell_job = (run_id, "shell@1.0.0".to_owned());
    assert_eq!(claimed, [shell_job.clone(), shell_job]);
}

#[tokio::test]
#[serial]
async fn without_the_operator_switch_no_organization_can_use_shell() {
    let mut admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let org = format!("/api/v1/orgs/{}", admin.organization_id);

    // Published while the deployment offered shell and the organization had it enabled.
    set_shell(&admin, true).await;
    let preset = node(
        "preset",
        "corqty.shell.command",
        json!({ "command": "true", "env": {} }),
    );
    let graph = json!({
        "schemaVersion": 1,
        "workflowInputSchema": { "type": "object" },
        "nodes": [preset],
        "edges": [],
        "viewport": { "x": 0, "y": 0, "zoom": 1 },
    });
    let workflow = body(
        admin
            .post_json(&format!("{org}/workflows"), r#"{"name":"Script"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let workflow = format!("{org}/workflows/{}", workflow["id"].as_str().unwrap());
    let draft = format!("{workflow}/draft");
    let saved = json!({ "revision": 1, "graph": graph }).to_string();
    let saved = admin.request_json(Method::PUT, &draft, &saved).await;
    assert_eq!(saved.status(), StatusCode::OK);
    let publish = format!("{workflow}/publish");
    let published = admin.post_json(&publish, r#"{"revision":2}"#).await;
    assert_eq!(published.status(), StatusCode::CREATED);

    // The API restarts without CORQTY_SHELL_EXECUTOR=enabled.
    let config = ApiConfig {
        shell_executor_available: false,
        ..ApiConfig::for_tests()
    };
    let pool = PgPool::connect(API_URL).await.unwrap();
    admin.app = build_router(AppState::new(PgStore::new(pool), config));
    let settings = format!("{org}/settings");
    assert_eq!(
        body(admin.get(&settings).await, StatusCode::OK).await,
        json!({ "shellAvailable": false, "shellEnabled": true })
    );

    // Shell nodes count as disabled, so they neither publish nor start.
    let shell_node = json!([
        { "code": "executor_disabled", "pointer": "/nodes/0/definition", "message": SHELL_DISABLED },
    ]);
    let saved = json!({ "revision": 2, "graph": graph }).to_string();
    let saved = admin.request_json(Method::PUT, &draft, &saved).await;
    assert_eq!(saved.status(), StatusCode::OK);
    let refused = body(
        admin.post_json(&publish, r#"{"revision":3}"#).await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(
        (&refused["code"], &refused["diagnostics"]),
        (&json!("graph_invalid"), &shell_node)
    );
    let refused = body(
        admin.post_json(&format!("{workflow}/runs"), "{}").await,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(
        (&refused["code"], &refused["diagnostics"]),
        (&json!("executor_disabled"), &shell_node)
    );

    // No admin can turn shell on, but turning it off still works.
    let shell = format!("{settings}/shell");
    let refused = body(
        admin
            .request_json(Method::PUT, &shell, r#"{"enabled":true}"#)
            .await,
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(refused["code"], "shell_unavailable");
    assert_eq!(
        set_shell(&admin, false).await,
        json!({ "shellAvailable": false, "shellEnabled": false })
    );
}
```

The new test plays an operator who turns the switch off after an organization enabled shell and published a shell
node. It swaps the fixture's router for one built over a new `corqty_api` pool with
`shell_executor_available: false`. The session cookie lives in the database, so the same admin is still signed in.
That API reports the organization's choice next to `shellAvailable: false`, refuses to publish the node or to start
the published version, refuses to enable shell, and still lets the admin disable it.

In `crates/api/tests/events_http.rs`:

Replace:

```rust
#[tokio::test]
#[serial]
async fn the_server_stops_on_shutdown_although_a_stream_is_open() {
```

With:

```rust
#[tokio::test]
#[serial]
async fn each_user_keeps_at_most_8_streams_open() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let (path, _) = start_run(&admin).await;
    let events = format!("{path}/events");
    let mut streams = Vec::new();
    for _ in 0..8 {
        streams.push(Frames::new(open(&admin.app, &admin, &events, None).await));
    }
    let refused = body(
        open(&admin.app, &admin, &events, None).await,
        StatusCode::TOO_MANY_REQUESTS,
    )
    .await;
    assert_eq!(refused["code"], "too_many_streams");

    // A stream that ends gives its slot back.
    drop(streams.remove(0));
    let mut frames = Frames::new(open(&admin.app, &admin, &events, None).await);
    assert_eq!(frames.next().await.as_deref(), Some("retry: 2000"));
}

#[tokio::test]
#[serial]
async fn the_server_stops_on_shutdown_although_a_stream_is_open() {
```

The events of successful attempts now carry their input. The persistence tests' `succeeded` helper reports an empty
input object, so their expected events gain `"input": {}`.

In `crates/persistence/tests/worker_queue.rs`:

Replace:

```rust
/// `event` with one more field.
fn with_field(mut event: Value, key: &str, value: Value) -> Value {
    event[key] = value;
    event
}
```

With:

```rust
/// `event` with one more field.
fn with_field(mut event: Value, key: &str, value: Value) -> Value {
    event[key] = value;
    event
}

/// The event of a first attempt that succeeded with `output`. It carries the attempt's input,
/// the empty object that `succeeded` reports.
fn success(node_id: &str, seq: i64, output: Value) -> Value {
    let event = with_field(node_state(node_id, "succeeded", 1, seq), "input", json!({}));
    with_field(event, "output", output)
}
```

Replace:

```rust
            node_state("split", "running", 1, 3),
            with_field(
                node_state("split", "succeeded", 1, 4),
                "output",
                output("split")
            ),
            node_state("other", "running", 1, 5),
            with_field(
                node_state("other", "succeeded", 1, 6),
                "output",
                output("other")
            ),
            node_state("join", "queued", 0, 7),
            node_state("join", "running", 1, 8),
            with_field(
                node_state("join", "succeeded", 1, 9),
                "output",
                output("join")
            ),
```

With:

```rust
            node_state("split", "running", 1, 3),
            success("split", 4, output("split")),
            node_state("other", "running", 1, 5),
            success("other", 6, output("other")),
            node_state("join", "queued", 0, 7),
            node_state("join", "running", 1, 8),
            success("join", 9, output("join")),
```

Replace:

```rust
            node_state("c", "skipped", 0, 4),
            with_field(
                node_state("u", "succeeded", 1, 5),
                "output",
                json!({"result": "u"})
            ),
```

With:

```rust
            node_state("c", "skipped", 0, 4),
            success("u", 5, json!({"result": "u"})),
```

In `crates/worker/tests/execution.rs`:

Replace:

```rust
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep, timeout};
```

With:

```rust
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep, timeout};
```

Replace:

```rust
fn mock() -> Vec<Arc<dyn Executor>> {
    vec![Arc::new(MockExecutor)]
}
```

With:

```rust
/// An executor that keeps its scratch directory for the test, waits until the test opens its
/// gate, and returns `{"result": "done"}` without logging.
#[derive(Clone, Default)]
struct Gated {
    scratch_dir: Arc<Mutex<Option<PathBuf>>>,
    gate: Arc<Notify>,
}

impl Executor for Gated {
    fn metadata(&self) -> ExecutorMetadata {
        MOCK.metadata()
    }

    fn validate(&self, config: &Value) -> Vec<Diagnostic> {
        MOCK.validate(config)
    }

    fn execute<'a>(
        &'a self,
        _request: ExecuteRequest,
        context: &'a ExecutionContext,
    ) -> BoxFuture<'a, Result<Map<String, Value>, ExecutorError>> {
        *self.scratch_dir.lock().unwrap() = Some(context.scratch_dir.clone());
        Box::pin(async move {
            self.gate.notified().await;
            Ok(Map::from_iter([("result".to_owned(), json!("done"))]))
        })
    }
}

fn mock() -> Vec<Arc<dyn Executor>> {
    vec![Arc::new(MockExecutor)]
}
```

Replace:

```rust
    assert_eq!(
        events.first(),
        Some(&json!({"type": "run.state", "state": "queued", "seq": 1}))
    );
    assert_eq!(events.last().unwrap()["state"], "succeeded");
    assert_eq!(fixture.scratch_entries(), 0);
}
```

With:

```rust
    assert_eq!(
        events.first(),
        Some(&json!({"type": "run.state", "state": "queued", "seq": 1}))
    );
    assert_eq!(events.last().unwrap()["state"], "succeeded");
    // The event that ends an attempt carries the attempt's input.
    let rds = events
        .iter()
        .find(|event| event["nodeId"] == "rds" && event["state"] == "succeeded");
    assert_eq!(rds.unwrap()["input"], json!({"vpcId": "vpc-0simulated"}));
    assert_eq!(fixture.scratch_entries(), 0);
}
```

Replace:

```rust
#[tokio::test]
#[serial]
async fn echo_returns_the_resolved_inputs_as_the_result() {
```

With:

```rust
#[tokio::test]
#[serial]
async fn the_scratch_directory_stays_until_the_outcome_is_reported() {
    let fixture = Fixture::new().await;
    let run = fixture
        .run(&graph(&[task("gated", json!({}))], &[]), json!({}))
        .await;
    let gated = Gated::default();
    let worker = fixture.start(fixture.config(1), vec![Arc::new(gated.clone())]);
    fixture.started(run, "gated").await;

    // Completing locks the run first, so this lock holds the report back.
    let owner = &fixture.database.owner;
    let mut lock = owner.begin().await.unwrap();
    sqlx::query("SELECT id FROM runs WHERE id = $1 FOR UPDATE")
        .bind(run)
        .execute(&mut *lock)
        .await
        .unwrap();
    gated.gate.notify_one();
    eventually(|| async move {
        let waiting: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_locks WHERE NOT granted)")
                .fetch_one(owner)
                .await
                .unwrap();
        waiting.then_some(())
    })
    .await;
    let scratch_dir = gated.scratch_dir.lock().unwrap().clone().unwrap();
    assert!(scratch_dir.is_dir(), "removed before the report");

    lock.commit().await.unwrap();
    assert_eq!(fixture.finished(run).await, "succeeded");
    worker.stop().await;
    assert!(!scratch_dir.exists());
}

#[tokio::test]
#[serial]
async fn echo_returns_the_resolved_inputs_as_the_result() {
```

The scratch test makes the order observable without a test hook. `complete_node_run` locks the run's row first
(D9), so the test holds that lock through the owner pool and then lets the `Gated` executor return. The executor
sends no logs, so nothing but the completion needs that lock. Neither the claim nor the heartbeat locks a runs row.
A lock request that is not granted therefore means the worker has reached `complete`, and the scratch directory must
still exist at that moment. Once the test commits, the run succeeds. After `worker.stop()`, which waits for every
attempt, the directory is gone.

Create `crates/worker/tests/shell_switch.rs`:

```rust
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use corqty_test_support::{TestDatabase, WORKER_URL};
use serial_test::serial;
use tempfile::TempDir;
use tokio::time::{sleep, timeout};

/// A worker process, killed when dropped.
struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Starts the worker binary with `CORQTY_SHELL_EXECUTOR` set to `switch`, or unset, and
/// returns the executors it registers.
async fn registered_executors(switch: Option<&str>) -> Vec<String> {
    let database = TestDatabase::reset().await;
    // The worker's alive file goes to its temporary directory. std::env::temp_dir() reads
    // TMPDIR on unix and TMP on Windows.
    let directory = TempDir::new().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_corqty-worker"));
    command
        .env("DATABASE_URL_WORKER", WORKER_URL)
        .env_remove("CORQTY_SHELL_EXECUTOR")
        .env("TMPDIR", directory.path())
        .env("TMP", directory.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(switch) = switch {
        command.env("CORQTY_SHELL_EXECUTOR", switch);
    }
    let _worker = Process(command.spawn().unwrap());
    let registered = async {
        loop {
            let executors: Option<Vec<String>> =
                sqlx::query_scalar("SELECT executors FROM worker_instances")
                    .fetch_optional(&database.owner)
                    .await
                    .unwrap();
            if let Some(executors) = executors {
                return executors;
            }
            sleep(Duration::from_millis(50)).await;
        }
    };
    timeout(Duration::from_secs(10), registered)
        .await
        .expect("the worker registers within 10 s")
}

#[tokio::test]
#[serial]
async fn only_the_operator_switch_gives_workers_the_shell_executor() {
    assert_eq!(registered_executors(None).await, ["mock@1.0.0"]);
    assert_eq!(registered_executors(Some("true")).await, ["mock@1.0.0"]);
    assert_eq!(
        registered_executors(Some("enabled")).await,
        ["mock@1.0.0", "shell@1.0.0"]
    );
}
```

`shell_switch.rs` starts the real `corqty-worker` binary three times on the test database: with the variable unset,
set to `true`, and set to `enabled`. Each time it reads the executors the process registered in `worker_instances`.
The temporary directory points at a `TempDir`, so the worker's alive file stays out of the developer's temp folder.
`Process` kills the worker when the test ends or fails.

- [ ] **Step 2: Run the tests and verify RED**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-api --test settings_http
```

Expected: the settings test does not compile:

```text
error[E0560]: struct `ApiConfig` has no field named `shell_executor_available`
   --> crates\api\tests\settings_http.rs:301:9
    |
301 |         shell_executor_available: false,
    |         ^^^^^^^^^^^^^^^^^^^^^^^^ `ApiConfig` does not have this field
    |
    = note: available fields are: `bind_address`, `database_url`, `public_web_url`, `secure_cookies`, `session_days`
```

The run ends with ``error: could not compile `corqty-api` (test "settings_http") due to 1 previous error``.

```text
cargo test -p corqty-api --test events_http
```

Expected: 4 passed and 1 failed. The ninth stream is served:

```text
thread 'each_user_keeps_at_most_8_streams_open' (31396) panicked at crates\api\tests\events_http.rs:83:5:
assertion `left == right` failed
  left: 200
 right: 429
```

The thread number varies.

```text
cargo test -p corqty-persistence --test worker_queue
```

Expected: 13 passed and 2 failed, `a_child_waits_once_for_each_distinct_parent_and_the_run_succeeds` at
`crates\persistence\tests\worker_queue.rs:530:5` and `a_failure_skips_only_its_descendants_and_fails_the_run` at
`crates\persistence\tests\worker_queue.rs:618:5`, both with ``assertion `left == right` failed``. In the stored
events (`left`), every `succeeded` event lacks the `"input": Object {}` that the expected events (`right`) carry.

```text
cargo test -p corqty-worker --test execution
```

Expected: 12 passed and 2 failed, in about 13 s:

```text
thread 'the_scratch_directory_stays_until_the_outcome_is_reported' (46564) panicked at crates\worker\tests\execution.rs:669:5:
removed before the report
```

```text
thread 'both_starter_blueprints_run_to_completion' (47948) panicked at crates\worker\tests\execution.rs:498:5:
assertion `left == right` failed
  left: Null
 right: Object {"vpcId": String("vpc-0simulated")}
```

```text
cargo test -p corqty-worker --test shell_switch
```

Expected: 1 failed. A worker without the switch still registers shell:

```text
thread 'only_the_operator_switch_gives_workers_the_shell_executor' (30228) panicked at crates\worker\tests\shell_switch.rs:59:5:
assertion `left == right` failed
  left: ["mock@1.0.0", "shell@1.0.0"]
 right: ["mock@1.0.0"]
```

- [ ] **Step 3: Add the operator switch to the API and the worker**

Replace the whole of `crates/api/src/config.rs`:

```rust
#[derive(Clone)]
pub struct ApiConfig {
    pub bind_address: String,
    pub database_url: String,
    pub public_web_url: String,
    pub secure_cookies: bool,
    pub session_days: i64,
    /// Whether this deployment offers the trusted shell executor at all
    /// (`CORQTY_SHELL_EXECUTOR=enabled`). Without it no organization can enable shell.
    pub shell_executor_available: bool,
}

impl ApiConfig {
    pub fn from_env() -> Result<Self, std::env::VarError> {
        Ok(Self {
            bind_address: std::env::var("CORQTY_BIND_ADDRESS")
                .unwrap_or_else(|_| "0.0.0.0:8080".to_owned()),
            database_url: std::env::var("DATABASE_URL_API")?,
            public_web_url: std::env::var("CORQTY_PUBLIC_WEB_URL")
                .unwrap_or_else(|_| "http://localhost:3000".to_owned()),
            secure_cookies: std::env::var("CORQTY_SECURE_COOKIES")
                .map_or(true, |value| value != "false"),
            session_days: 30,
            shell_executor_available: std::env::var("CORQTY_SHELL_EXECUTOR")
                .is_ok_and(|value| value == "enabled"),
        })
    }

    #[must_use]
    pub fn for_tests() -> Self {
        Self {
            bind_address: "127.0.0.1:0".to_owned(),
            database_url: String::new(),
            public_web_url: "http://localhost:3000".to_owned(),
            secure_cookies: false,
            session_days: 30,
            shell_executor_available: true,
        }
    }
}
```

Replace the whole of `crates/api/src/organizations/settings.rs`:

```rust
use axum::{Extension, Json, extract::State};
use corqty_domain::rbac::Capability;
use corqty_persistence::{models::SessionRecord, organizations};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    authz::{OrgAccess, authorize},
    error::ApiError,
    extract::{ApiJson, ApiPath},
    state::AppState,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Whether the deployment offers the shell executor; see `ApiConfig`.
    shell_available: bool,
    shell_enabled: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShellSetting {
    enabled: bool,
}

pub async fn get_settings(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(organization_id): ApiPath<Uuid>,
) -> Result<Json<Settings>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let shell_enabled = organizations::shell_enabled(&mut access.tx, organization_id).await?;
    access.commit().await?;
    Ok(Json(Settings {
        shell_available: state.config.shell_executor_available,
        shell_enabled,
    }))
}

/// Allows or refuses shell nodes in new publishes and runs. Runs that already exist keep
/// their shell nodes either way. Only a deployment that offers the shell executor lets an
/// admin enable it; disabling always works.
pub async fn set_shell(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(organization_id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<ShellSetting>,
) -> Result<Json<Settings>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::EnableShellExecutor,
    )
    .await?;
    if request.enabled && !state.config.shell_executor_available {
        return Err(ApiError::Conflict("shell_unavailable"));
    }
    organizations::set_shell_enabled(
        &mut access.tx,
        organization_id,
        access.user_id,
        request.enabled,
    )
    .await?;
    access.commit().await?;
    Ok(Json(Settings {
        shell_available: state.config.shell_executor_available,
        shell_enabled: request.enabled,
    }))
}

/// Whether the organization may publish and run shell nodes: the deployment offers the shell
/// executor and an admin of the organization enabled it.
pub async fn shell_allowed(state: &AppState, access: &mut OrgAccess) -> Result<bool, sqlx::Error> {
    Ok(state.config.shell_executor_available
        && organizations::shell_enabled(&mut access.tx, access.organization_id).await?)
}
```

In `crates/api/src/workflows.rs`:

Replace:

```rust
use corqty_persistence::{
    catalog,
    models::SessionRecord,
    organizations,
    workflows::{self, VersionRecord, VersionSummaryRecord, WorkflowRecord, WorkflowSummaryRecord},
};
```

With:

```rust
use corqty_persistence::{
    catalog,
    models::SessionRecord,
    workflows::{self, VersionRecord, VersionSummaryRecord, WorkflowRecord, WorkflowSummaryRecord},
};
```

Replace:

```rust
    error::ApiError,
    extract::{ApiJson, ApiPath},
    state::AppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewWorkflow {
```

With:

```rust
    error::ApiError,
    extract::{ApiJson, ApiPath},
    organizations::settings::shell_allowed,
    state::AppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewWorkflow {
```

Replace:

```rust
    if !workflows::workflow_exists(&mut access.tx, workflow_id).await? {
        return Err(ApiError::NotFound);
    }
    let shell_enabled = organizations::shell_enabled(&mut access.tx, organization_id).await?;
```

With:

```rust
    if !workflows::workflow_exists(&mut access.tx, workflow_id).await? {
        return Err(ApiError::NotFound);
    }
    let shell_enabled = shell_allowed(&state, &mut access).await?;
```

Replace:

```rust
        return Err(ApiError::Conflict("draft_already_published"));
    }
    let shell_enabled = organizations::shell_enabled(&mut access.tx, organization_id).await?;
```

With:

```rust
        return Err(ApiError::Conflict("draft_already_published"));
    }
    let shell_enabled = shell_allowed(&state, &mut access).await?;
```

Replace:

```rust
/// Every reason the organization cannot publish `graph`: the publish rules, then each shell
/// node while the organization has not enabled the shell executor.
```

With:

```rust
/// Every reason the organization cannot publish `graph`: the publish rules, then each shell
/// node while the organization may not use shell (see `shell_allowed`).
```

In `crates/api/src/runs.rs`:

Replace:

```rust
use corqty_persistence::{
    catalog,
    models::SessionRecord,
    organizations,
    runs::{self, CancelError, NewRun, RunRecord, RunSummaryRecord},
    workflows,
};
```

With:

```rust
use corqty_persistence::{
    catalog,
    models::SessionRecord,
    runs::{self, CancelError, NewRun, RunRecord, RunSummaryRecord},
    workflows,
};
```

Replace:

```rust
    error::ApiError,
    extract::{ApiJson, ApiPath},
    state::AppState,
};
```

With:

```rust
    error::ApiError,
    extract::{ApiJson, ApiPath},
    organizations::settings::shell_allowed,
    state::AppState,
};
```

Replace:

```rust
    let run = create(&mut access, workflow_id, version, input, None).await?;
```

With:

```rust
    let run = create(&state, &mut access, workflow_id, version, input, None).await?;
```

Replace:

```rust
    let run = create(
        &mut access,
        source.summary.workflow_id,
```

With:

```rust
    let run = create(
        &state,
        &mut access,
        source.summary.workflow_id,
```

Replace:

```rust
/// Creates a run of the workflow's `version` from its plan and reads it back. `input` must
/// match that version's workflow input schema, and a version with shell nodes runs only while
/// the organization has the shell executor enabled.
async fn create(
    access: &mut OrgAccess,
```

With:

```rust
/// Creates a run of the workflow's `version` from its plan and reads it back. `input` must
/// match that version's workflow input schema, and a version with shell nodes runs only while
/// the organization may use shell (see `shell_allowed`).
async fn create(
    state: &AppState,
    access: &mut OrgAccess,
```

Replace:

```rust
    if !organizations::shell_enabled(&mut access.tx, access.organization_id).await? {
```

With:

```rust
    if !shell_allowed(state, access).await? {
```

Replace the whole of `crates/worker/src/main.rs`:

```rust
#![forbid(unsafe_code)]

use std::sync::Arc;

use corqty_executor_mock::MockExecutor;
use corqty_executor_shell::ShellExecutor;
use corqty_plugin_host::Registry;
use corqty_plugin_sdk::Executor;
use corqty_worker::{WorkerConfig, check_alive, run};
use sqlx::postgres::PgPoolOptions;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The Compose healthcheck reads only the alive file that the running worker touches.
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        return Ok(check_alive(&WorkerConfig::default().alive_file)?);
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "corqty_worker=info".into()),
        )
        .init();
    let database_url =
        std::env::var("DATABASE_URL_WORKER").map_err(|_| "DATABASE_URL_WORKER must be set")?;
    let mut config = WorkerConfig::default();
    if let Ok(capacity) = std::env::var("CORQTY_WORKER_CAPACITY") {
        config.capacity = capacity
            .parse()
            .ok()
            .filter(|capacity| *capacity >= 1)
            .ok_or("CORQTY_WORKER_CAPACITY must be a whole number of at least 1")?;
    }
    let mut executors: Vec<Arc<dyn Executor>> = vec![Arc::new(MockExecutor)];
    // Shell commands run as this process's user, which holds the worker's database credential,
    // so only the operator can offer them.
    if std::env::var("CORQTY_SHELL_EXECUTOR").is_ok_and(|value| value == "enabled") {
        executors.push(Arc::new(ShellExecutor));
    } else {
        tracing::info!("the shell executor is off: CORQTY_SHELL_EXECUTOR is not \"enabled\"");
    }
    let registry = Registry::new(executors)?;
    // One connection for each running attempt, and one to claim with.
    let pool = PgPoolOptions::new()
        .max_connections(config.capacity.unsigned_abs() + 1)
        .connect(&database_url)
        .await?;
    tracing::info!(
        worker_id = %config.worker_id,
        capacity = config.capacity,
        executors = ?registry.advertised(),
        "the worker is ready"
    );
    let shutdown = CancellationToken::new();
    let signal = shutdown.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        signal.cancel();
    });
    run(config, registry, pool, shutdown).await?;
    Ok(())
}

/// Resolves on ctrl-c, or on SIGTERM (what `docker compose stop` sends) on unix.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install the ctrl-c handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install the SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}
```

- [ ] **Step 4: Cap the open event streams**

In `crates/api/src/error.rs`:

Replace:

```rust
    #[error("unsupported media type")]
    UnsupportedMediaType,
```

With:

```rust
    #[error("unsupported media type")]
    UnsupportedMediaType,
    #[error("too many requests")]
    TooManyRequests(&'static str),
```

Replace:

```rust
            Self::UnsupportedMediaType => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Unsupported media type",
                "unsupported_media_type",
            ),
```

With:

```rust
            Self::UnsupportedMediaType => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Unsupported media type",
                "unsupported_media_type",
            ),
            Self::TooManyRequests(code) => {
                (StatusCode::TOO_MANY_REQUESTS, "Too many requests", code)
            }
```

In `crates/api/src/state.rs`:

Replace:

```rust
use std::sync::Arc;

use corqty_persistence::store::PgStore;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::{auth::crypto::PasswordEngine, config::ApiConfig, events::Wake};
```

With:

```rust
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use corqty_persistence::store::PgStore;
use tokio::sync::{Semaphore, broadcast};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    auth::crypto::PasswordEngine,
    config::ApiConfig,
    events::{MAX_STREAMS, Wake},
};
```

Replace:

```rust
    /// Wakes the open run event streams. `events::spawn_listener` feeds it from the database.
    pub wake: broadcast::Sender<Wake>,
}
```

With:

```rust
    /// Wakes the open run event streams. `events::spawn_listener` feeds it from the database.
    pub wake: broadcast::Sender<Wake>,
    /// One permit for each run event stream that may be open in this process.
    pub streams: Arc<Semaphore>,
    /// How many run event streams each user has open in this process.
    pub streams_per_user: Arc<Mutex<HashMap<Uuid, usize>>>,
}
```

Replace:

```rust
            // A stream that falls 1024 wake-ups behind reads again.
            wake: broadcast::channel(1024).0,
        }
```

With:

```rust
            // A stream that falls 1024 wake-ups behind reads again.
            wake: broadcast::channel(1024).0,
            streams: Arc::new(Semaphore::new(MAX_STREAMS)),
            streams_per_user: Arc::default(),
        }
```

In `crates/api/src/events.rs`:

Replace:

```rust
use std::{collections::VecDeque, convert::Infallible, time::Duration};
```

With:

```rust
use std::{
    collections::{HashMap, VecDeque},
    convert::Infallible,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
```

Replace:

```rust
use tokio::{
    sync::broadcast::{self, error::RecvError},
    task::JoinHandle,
    time::Instant,
};
```

With:

```rust
use tokio::{
    sync::{
        OwnedSemaphorePermit,
        broadcast::{self, error::RecvError},
    },
    task::JoinHandle,
    time::Instant,
};
```

Replace:

```rust
const MIN_BACKOFF: Duration = Duration::from_millis(250);
const MAX_BACKOFF: Duration = Duration::from_secs(10);
```

With:

```rust
const MIN_BACKOFF: Duration = Duration::from_millis(250);
const MAX_BACKOFF: Duration = Duration::from_secs(10);
/// How many streams one API process keeps open, and how many of them one user may have. Every
/// open stream reads the database when its run changes, so the streams must not be able to
/// take every pooled connection.
pub const MAX_STREAMS: usize = 256;
const MAX_STREAMS_PER_USER: usize = 8;
```

Replace:

```rust
/// the id and its payload as the data. A finished run whose events the client already has
/// answers 204, which stops the browser from reconnecting.
```

With:

```rust
/// the id and its payload as the data. A finished run whose events the client already has
/// answers 204, which stops the browser from reconnecting. A stream past the process's or the
/// user's limit answers 429.
```

Replace:

```rust
    if is_finished(&run.summary.status) && cursor >= run.last_event_seq {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
```

With:

```rust
    if is_finished(&run.summary.status) && cursor >= run.last_event_seq {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    let slot =
        Slot::take(&state, session.user_id).ok_or(ApiError::TooManyRequests("too_many_streams"))?;
```

Replace:

```rust
        read_again_at: Instant::now(),
        finished: false,
    };
```

With:

```rust
        read_again_at: Instant::now(),
        finished: false,
        _slot: slot,
    };
```

Replace:

```rust
/// One stream's place in its run's events.
```

With:

```rust
/// An open stream's place among the streams that the process and its user may have open,
/// given back when the stream ends.
struct Slot {
    _permit: OwnedSemaphorePermit,
    per_user: Arc<Mutex<HashMap<Uuid, usize>>>,
    user_id: Uuid,
}

impl Slot {
    /// A slot for another stream of the user, or None when the process or the user has as many
    /// open as it may.
    fn take(state: &AppState, user_id: Uuid) -> Option<Self> {
        let permit = Arc::clone(&state.streams).try_acquire_owned().ok()?;
        let mut per_user = state
            .streams_per_user
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let open = per_user.entry(user_id).or_default();
        if *open >= MAX_STREAMS_PER_USER {
            return None;
        }
        *open += 1;
        Some(Self {
            _permit: permit,
            per_user: Arc::clone(&state.streams_per_user),
            user_id,
        })
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut per_user = self.per_user.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(open) = per_user.get_mut(&self.user_id) {
            *open -= 1;
            if *open == 0 {
                per_user.remove(&self.user_id);
            }
        }
    }
}

/// One stream's place in its run's events.
```

Replace:

```rust
    /// Whether the run's terminal `run.state` has been read.
    finished: bool,
}
```

With:

```rust
    /// Whether the run's terminal `run.state` has been read.
    finished: bool,
    /// Held while the stream lives.
    _slot: Slot,
}
```

A refused slot drops its permit at once. The per-user count is a plain `std::sync::Mutex`: it is held only for a
map update and never across an `.await`.

- [ ] **Step 5: Remove the scratch directory after the outcome is reported**

In `crates/worker/src/lib.rs`:

Replace:

```rust
use serde_json::{Map, Value};
use sqlx::PgPool;
```

With:

```rust
use serde_json::{Map, Value};
use sqlx::PgPool;
use tempfile::TempDir;
```

Replace:

```rust
    /// Starts a claimed attempt, runs it and reports its logs and outcome, unless the lease
    /// is gone or the worker shuts down first.
```

With:

```rust
    /// Starts a claimed attempt, runs it and reports its logs and outcome, unless the lease
    /// is gone or the worker shuts down first. Its scratch directory is removed last.
```

Replace:

```rust
        let outcome = if job.cancel_requested {
            Some(Outcome::Canceled)
        } else {
            self.execute(&claimed.executor, &job, &mut logs).await
        };
        let Some(outcome) = outcome else { return };
        // The logs go first: completing ends the lease that appending needs.
        if !logs.flush().await {
            return;
        }
        if let Err(error) = retrying(|| self.db.complete(lease.0, lease.1, &outcome)).await {
            tracing::warn!(error = %error, "completing a node run failed");
        }
    }

    /// Resolves the inputs and runs the executor in a new scratch directory. None when the
    /// outcome must not be reported.
    async fn execute(&self, executor: &str, job: &Job, logs: &mut Logs<'_>) -> Option<Outcome> {
```

With:

```rust
        let mut scratch = None;
        let outcome = if job.cancel_requested {
            Some(Outcome::Canceled)
        } else {
            self.execute(&claimed.executor, &job, &mut logs, &mut scratch)
                .await
        };
        // The logs go first: completing ends the lease that appending needs.
        if let Some(outcome) = outcome
            && logs.flush().await
            && let Err(error) = retrying(|| self.db.complete(lease.0, lease.1, &outcome)).await
        {
            tracing::warn!(error = %error, "completing a node run failed");
        }
        // The outcome is reported, or never will be, so nothing needs the directory now.
        // Removing it before the report, while no heartbeat renews the lease, could outlast
        // the lease and lose the outcome.
        if let Some(scratch) = scratch
            && let Ok(Err(error)) = tokio::task::spawn_blocking(move || scratch.close()).await
        {
            tracing::warn!(error = %error, "removing a scratch directory failed");
        }
    }

    /// Resolves the inputs and runs the executor in a new scratch directory, which it leaves
    /// in `scratch` for the caller to remove. None when the outcome must not be reported.
    async fn execute(
        &self,
        executor: &str,
        job: &Job,
        logs: &mut Logs<'_>,
        scratch: &mut Option<TempDir>,
    ) -> Option<Outcome> {
```

Replace:

```rust
        let scratch = tempfile::Builder::new()
            .prefix("corqty-node-")
            .tempdir_in(&self.config.scratch_root);
        let scratch = match scratch {
            Ok(scratch) => scratch,
```

With:

```rust
        let created = tempfile::Builder::new()
            .prefix("corqty-node-")
            .tempdir_in(&self.config.scratch_root);
        let scratch_dir = match created {
            Ok(created) => scratch.insert(created).path().to_owned(),
```

Replace:

```rust
        let ended = self
            .supervise(
                found.as_ref(),
                request,
                scratch.path().to_owned(),
                limit,
                logs,
            )
            .await;
        // The executor has returned or been dropped, so nothing uses the directory now.
        if let Ok(Err(error)) = tokio::task::spawn_blocking(move || scratch.close()).await {
            tracing::warn!(error = %error, "removing a scratch directory failed");
        }

        Some(match ended {
```

With:

```rust
        let ended = self
            .supervise(found.as_ref(), request, scratch_dir, limit, logs)
            .await;
        Some(match ended {
```

The let chain (Rust 2024) keeps the old early returns: nothing is completed without an outcome or when the flush
finds the lease gone. Either way the function now reaches the removal.

- [ ] **Step 6: Put the input into the event that ends an attempt**

Create `migrations/202610270001_live_event_details.sql`:

```sql
-- The node.state event that finish_node_run writes when an attempt ends also carries the
-- node's input, so a live run view shows it without reading the run again. An input that was
-- never resolved is NULL and, like every top-level null, is left out of the event. The rest of
-- the function is the one that 202610260002 created. CREATE OR REPLACE keeps its owner and its
-- privileges: no role may call it directly.
CREATE OR REPLACE FUNCTION corqty.finish_node_run(p_node_run_id uuid, p_outcome jsonb)
RETURNS void
LANGUAGE plpgsql
SET search_path = public, pg_temp
AS $$
DECLARE
    v_run uuid;
    v_state text;
    v_children text[];
    v_event jsonb;
BEGIN
    SELECT n.run_id,
           CASE
               WHEN p_outcome ->> 'status' = 'succeeded' THEN 'succeeded'
               WHEN p_outcome ->> 'status' = 'canceled' OR r.cancel_requested_at IS NOT NULL
                   THEN 'canceled'
               WHEN (p_outcome #>> '{error,retryable}')::boolean AND n.attempt < n.max_attempts
                   THEN 'retry_wait'
               ELSE 'failed'
           END
    INTO v_run, v_state
    FROM node_runs AS n
    JOIN runs AS r ON r.id = n.run_id
    WHERE n.id = p_node_run_id;

    UPDATE node_runs
    SET state = v_state,
        input = coalesce(p_outcome -> 'input', input),
        output = CASE WHEN v_state = 'succeeded' THEN p_outcome -> 'output' END,
        error = p_outcome -> 'error',
        available_at = CASE
            WHEN v_state = 'retry_wait' THEN now() + make_interval(
                secs => least(300, 2 * power(2, attempt - 1) * (1 + 0.2 * random()))
            )
            ELSE available_at
        END,
        leased_by = NULL,
        lease_expires_at = NULL,
        finished_at = CASE WHEN v_state = 'retry_wait' THEN NULL ELSE now() END
    WHERE id = p_node_run_id
    RETURNING children,
              jsonb_build_object(
                  'type', 'node.state', 'nodeId', node_id, 'state', state,
                  'attempt', attempt, 'error', error, 'output', output, 'input', input
              )
    INTO v_children, v_event;
    PERFORM corqty.append_run_event(v_run, v_event);

    IF v_state = 'succeeded' THEN
        -- One statement locks the children in id order. Each parent decrements a child once,
        -- and the child is queued when no parent is left.
        FOR v_event IN
            WITH released AS (
                UPDATE node_runs AS child
                SET pending_parents = child.pending_parents - 1,
                    state = CASE WHEN child.pending_parents = 1 THEN 'queued' ELSE 'blocked' END,
                    available_at = now()
                FROM (
                    SELECT id
                    FROM node_runs
                    WHERE run_id = v_run AND node_id = ANY (v_children) AND state = 'blocked'
                    ORDER BY id
                    FOR UPDATE
                ) AS waiting
                WHERE child.id = waiting.id
                RETURNING child.id, child.node_id, child.state, child.attempt
            )
            SELECT jsonb_build_object(
                       'type', 'node.state', 'nodeId', node_id, 'state', state,
                       'attempt', attempt
                   )
            FROM released
            WHERE state = 'queued'
            ORDER BY id
        LOOP
            PERFORM corqty.append_run_event(v_run, v_event);
        END LOOP;
    ELSIF v_state IN ('failed', 'canceled') THEN
        FOR v_event IN
            WITH RECURSIVE below(node_id) AS (
                SELECT unnest(v_children)
                UNION
                SELECT child
                FROM below
                JOIN node_runs AS n ON n.run_id = v_run AND n.node_id = below.node_id
                CROSS JOIN unnest(n.children) AS child
            ),
            skipped AS (
                UPDATE node_runs AS descendant
                SET state = 'skipped', finished_at = now()
                FROM (
                    SELECT id
                    FROM node_runs
                    WHERE run_id = v_run
                      AND node_id IN (SELECT node_id FROM below)
                      AND state = 'blocked'
                    ORDER BY id
                    FOR UPDATE
                ) AS waiting
                WHERE descendant.id = waiting.id
                RETURNING descendant.id, descendant.node_id, descendant.attempt
            )
            SELECT jsonb_build_object(
                       'type', 'node.state', 'nodeId', node_id, 'state', 'skipped',
                       'attempt', attempt
                   )
            FROM skipped
            ORDER BY id
        LOOP
            PERFORM corqty.append_run_event(v_run, v_event);
        END LOOP;
    END IF;

    PERFORM corqty.settle_run(v_run);
END
$$;
```

Apart from `CREATE OR REPLACE` and the `'input', input` pair in the first `jsonb_build_object`, the function is
character for character the one in `202610260002_worker_functions.sql`. Lease recovery's outcome has no `input`, so
`coalesce` keeps the stored one, and the recovered node's event carries it when there is one.

- [ ] **Step 7: Verify GREEN**

```text
cargo test -p corqty-api --test settings_http --test events_http
cargo test -p corqty-persistence --test worker_queue
cargo test -p corqty-worker
```

Expected: `events_http` 5 passed and `settings_http` 3 passed; `worker_queue` 15 passed; `execution` 14 passed in
about 13 s, `health` 2 passed and `shell_switch` 1 passed in under a second.

A negative control shows that the stream test catches a slot that is never given back. Changing `*open -= 1;` to
`*open -= 0;` in `Slot::drop` and running `cargo test -p corqty-api --test events_http each_user` fails at
`crates\api\tests\events_http.rs:36:9` with `left: 429` and `right: 200`. Restore the line afterwards.

- [ ] **Step 8: Turn the switch on for the Compose stack in CI and require it in the smoke test**

In `docker-compose.yml`:

Replace:

```yaml
      CORQTY_SECURE_COOKIES: ${CORQTY_SECURE_COOKIES:-false}
      RUST_LOG: ${RUST_LOG:-corqty_api=info,tower_http=info}
```

With:

```yaml
      CORQTY_SECURE_COOKIES: ${CORQTY_SECURE_COOKIES:-false}
      CORQTY_SHELL_EXECUTOR: ${CORQTY_SHELL_EXECUTOR:-disabled}
      RUST_LOG: ${RUST_LOG:-corqty_api=info,tower_http=info}
```

Replace:

```yaml
      CORQTY_WORKER_CAPACITY: ${CORQTY_WORKER_CAPACITY:-4}
```

With:

```yaml
      CORQTY_WORKER_CAPACITY: ${CORQTY_WORKER_CAPACITY:-4}
      CORQTY_SHELL_EXECUTOR: ${CORQTY_SHELL_EXECUTOR:-disabled}
```

In `.env.example`:

Replace:

```text
# Node runs that each worker container executes at once.
CORQTY_WORKER_CAPACITY=4
```

With:

```text
# Node runs that each worker container executes at once.
CORQTY_WORKER_CAPACITY=4
# "enabled" lets organization admins turn on the trusted shell executor (api and worker).
# Shell commands run as the shared worker user and can reach the worker's database
# credential and other organizations' jobs: enable it only on single-tenant or fully
# trusted deployments.
CORQTY_SHELL_EXECUTOR=disabled
```

In `.github/workflows/ci.yml`:

Replace:

```yaml
        env:
          CORQTY_WORKER_CAPACITY: "1"
```

With:

```yaml
        env:
          CORQTY_WORKER_CAPACITY: "1"
          CORQTY_SHELL_EXECUTOR: enabled
```

In `scripts/smoke-foundation.ps1`:

Replace:

```powershell
# An admin opts into the trusted shell executor. The shell node reads its resolved inputs
# from CORQTY_INPUT and writes JSON to CORQTY_OUTPUT with jq from the worker image.
$shellEnabled = Invoke-SmokeRequest `
```

With:

```powershell
# An admin opts into the trusted shell executor, which the deployment must offer
# (CORQTY_SHELL_EXECUTOR=enabled). The shell node reads its resolved inputs from
# CORQTY_INPUT and writes JSON to CORQTY_OUTPUT with jq from the worker image.
$settings = Invoke-SmokeRequest -User $admin -Method Get -Path "$organizationPath/settings"
if ($settings.Status -ne 200 -or $settings.Body.shellAvailable -ne $true) {
    throw "The shell executor is unavailable: start the stack with CORQTY_SHELL_EXECUTOR=enabled"
}
$shellEnabled = Invoke-SmokeRequest `
```

- [ ] **Step 9: Document the switch, the trust boundary and the stream cap**

In `README.md`:

Replace:

````markdown
```powershell
Copy-Item .env.example .env
docker compose up --build --wait
./scripts/smoke-foundation.ps1
```

Open http://localhost:3000 and create the first account and organization.
````

With:

````markdown
```powershell
Copy-Item .env.example .env
$env:CORQTY_SHELL_EXECUTOR = "enabled"
docker compose up --build --wait
./scripts/smoke-foundation.ps1
```

The smoke test runs a shell node, so these commands turn the trusted shell
executor on for this local stack. Read Workers before you turn it on
anywhere else.

Open http://localhost:3000 and create the first account and organization.
````

Replace:

````markdown
```powershell
docker compose up --build --wait --scale worker=3
./scripts/smoke-foundation.ps1 -ExpectedWorkers 3
```
````

With:

````markdown
```powershell
$env:CORQTY_SHELL_EXECUTOR = "enabled"
docker compose up --build --wait --scale worker=3
./scripts/smoke-foundation.ps1 -ExpectedWorkers 3
```
````

Replace:

````markdown
The shell executor is for trusted users only and is not a security
boundary. An organization admin enables it under Settings. Its commands run
inside the worker container as the worker's unprivileged user, with `bash`
and `jq` available. Commands that need other tools need a custom worker
image built from the `worker` target of `docker/rust/Dockerfile`.
````

With:

````markdown
The shell executor is off unless the operator sets
`CORQTY_SHELL_EXECUTOR=enabled` for the `api` and `worker` services. Only
then can an organization admin enable it under Settings. Turning it on lets
every organization admin on the deployment run commands as the shared worker
user, which can read the worker's database credential and so reach other
organizations' jobs. Turn it on only for single-tenant or fully trusted
deployments: the shell executor is not a security boundary. Its commands run
inside the worker container as the worker's unprivileged user, with `bash`
and `jq` available. A timeout or a cancel signals the command's process
group, so processes that call `setsid` or daemonise keep running. Commands
that need other tools need a custom worker image built from the `worker`
target of `docker/rust/Dockerfile`.
````

In `docs/superpowers/specs/2026-07-28-corqty-workflow-orchestrator-design.md`:

Replace:

```markdown
- Run events carry a gapless per-run `seq` and a `type`: `run.state`, `node.state` (with `attempt` and, where it applies, `error` or `output`), `node.log` (stream `stdout`, `stderr` or `system`) and `node.log_truncated`. Creating a run writes only `run.state queued`. `GET /orgs/{org_id}/runs/{run_id}/events` sends unnamed events with `id: <seq>`, replays after `Last-Event-ID` (or `?after=`), and ends after the terminal `run.state`, when the API shuts down, or after 10 minutes, after which the browser reconnects. A finished run whose events the client already has answers 204. The API sends `Cache-Control: no-cache, no-transform`, which keeps the Next.js rewrite from compressing the stream.
```

With:

```markdown
- Run events carry a gapless per-run `seq` and a `type`: `run.state`, `node.state` (with `attempt` and, where it applies, `error`, `output` or, once an attempt ends, the node's `input`), `node.log` (stream `stdout`, `stderr` or `system`) and `node.log_truncated`. Creating a run writes only `run.state queued`. `GET /orgs/{org_id}/runs/{run_id}/events` sends unnamed events with `id: <seq>`, replays after `Last-Event-ID` (or `?after=`), and ends after the terminal `run.state`, when the API shuts down, or after 10 minutes, after which the browser reconnects. A finished run whose events the client already has answers 204. The API sends `Cache-Control: no-cache, no-transform`, which keeps the Next.js rewrite from compressing the stream. Each API process keeps at most 256 streams open, and at most 8 for one user; another stream answers 429 `too_many_streams`.
```

Replace:

```markdown
- The shell preset `corqty.shell.command` version 1 takes `{command, env}`. `command` runs under `/bin/sh -c` in a fresh scratch directory, which is also `HOME` and `TMPDIR`, with only `PATH`, `LANG`, `CORQTY_INPUT`, `CORQTY_OUTPUT` and the `env` entries whose names match `^[A-Z_][A-Z0-9_]*$` and do not start with `CORQTY_`. An admin enables shell per organization with `PUT /orgs/{org_id}/settings/shell`, which writes `organization.shell_enabled` or `organization.shell_disabled` only when the value changes. While shell is disabled, publishing and starting runs report each shell node as `executor_disabled`. Disabling it never touches existing runs. The studio palette lists shell definitions only while shell is enabled, and the settings page and the inspector warn that shell is for trusted users and is not a security boundary.
```

With:

```markdown
- The shell preset `corqty.shell.command` version 1 takes `{command, env}`. `command` runs under `/bin/sh -c` in a fresh scratch directory, which is also `HOME` and `TMPDIR`, with only `PATH`, `LANG`, `CORQTY_INPUT`, `CORQTY_OUTPUT` and the `env` entries whose names match `^[A-Z_][A-Z0-9_]*$` and do not start with `CORQTY_`. An admin enables shell per organization with `PUT /orgs/{org_id}/settings/shell`, which writes `organization.shell_enabled` or `organization.shell_disabled` only when the value changes. While shell is disabled, publishing and starting runs report each shell node as `executor_disabled`. Disabling it never touches existing runs. The studio palette lists shell definitions only while shell is enabled, and the settings page and the inspector warn that shell is for trusted users and is not a security boundary.
- The operator offers the shell executor to a whole deployment with `CORQTY_SHELL_EXECUTOR=enabled` on the API and the workers; it is off by default. Without it the workers do not register `shell@1.0.0`, `GET /orgs/{org_id}/settings` answers `{shellAvailable: false, shellEnabled}`, enabling answers 409 `shell_unavailable` (disabling still works), and publishing and starting runs report every shell node as `executor_disabled` whatever the organization chose. Turning it on lets every organization admin on the deployment run commands as the shared worker user, which can read the worker's database credential and so reach other organizations' jobs, so it is only for single-tenant or fully trusted deployments. A timeout or a cancel signals the command's process group: processes that call `setsid` or daemonise leave the group and are not stopped.
```

The events decision now names the input and the stream cap. The new decision after the shell preset records the
switch, the trust boundary, and the process-group limit of timeout and cancel.

- [ ] **Step 10: Run the stack with and without the switch**

```text
docker compose config --quiet
```

Expected: no output, exit code 0.

Start the stack as CI does, with the switch on, one slot per worker and three workers. In PowerShell:

```text
$env:CORQTY_SHELL_EXECUTOR = "enabled"
$env:CORQTY_WORKER_CAPACITY = "1"
docker compose --project-name corqty-m3 up --build --wait --scale worker=3
```

In bash, run `CORQTY_SHELL_EXECUTOR=enabled CORQTY_WORKER_CAPACITY=1 docker compose --project-name corqty-m3 up
--build --wait --scale worker=3`.

Expected: the builder compiles `corqty-api`, `corqty-migrator` and `corqty-worker` in release mode. The command
exits 0 after `Container corqty-m3-worker-1 Healthy`, `Container corqty-m3-worker-2 Healthy`,
`Container corqty-m3-worker-3 Healthy` and `Container corqty-m3-web-1 Healthy`.

```text
./scripts/smoke-foundation.ps1 -ProjectName corqty-m3 -ExpectedWorkers 3
docker compose --project-name corqty-m3 logs worker
docker compose --project-name corqty-m3 exec api printenv CORQTY_SHELL_EXECUTOR
```

In Windows PowerShell 5.1, run the script with
`powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-foundation.ps1 -ProjectName corqty-m3 -ExpectedWorkers 3`.

Expected: the script prints nothing and exits 0 in about a minute. Each worker logged `the worker is ready` with
`capacity=1` and `executors=["mock@1.0.0", "shell@1.0.0"]`, and the API prints `enabled`.

Now recreate the API and the workers without the switch. In the same PowerShell session:

```text
Remove-Item Env:CORQTY_SHELL_EXECUTOR
docker compose --project-name corqty-m3 up --wait --scale worker=3
./scripts/smoke-foundation.ps1 -ProjectName corqty-m3 -ExpectedWorkers 3
docker compose --project-name corqty-m3 logs worker
```

Expected: `up` recreates `api` and the three workers and exits 0. The smoke script passes its earlier checks and
then stops at line 542 with:

```text
The shell executor is unavailable: start the stack with CORQTY_SHELL_EXECUTOR=enabled
```

Each recreated worker logged `the shell executor is off: CORQTY_SHELL_EXECUTOR is not "enabled"` and then
`the worker is ready` with `executors=["mock@1.0.0"]`.

```text
Remove-Item Env:CORQTY_WORKER_CAPACITY
docker compose --project-name corqty-m3 down --volumes
```

Expected: `Volume corqty-m3_postgres-data Removed` and `Network corqty-m3_default Removed`.

- [ ] **Step 11: Verify on Linux**

Windows never compiles the shell's `#[cfg(unix)] mod imp` or the worker's SIGTERM branch. Lint the workspace and run
the shell contract and health tests in a Linux container. The mount is read-only, and `--locked` keeps cargo from
rewriting the lockfile:

```text
docker run --rm -v "${PWD}:/work:ro" -w /work -e CARGO_TARGET_DIR=/tmp/target -v corqty-t19-target:/tmp/target rust:1.96-bookworm bash -c 'cargo clippy --locked --workspace --all-targets -- -D warnings && cargo test --locked -p corqty-executor-shell && cargo test --locked -p corqty-worker --test health'
```

Expected: clippy prints `Finished` with no warnings, `contract` reports 15 passed and `health` 2 passed.

The database-backed tests touched here also run on Linux. The container shares the host's network, so the tests
reach the test database at `127.0.0.1:55432`. Docker Desktop needs host networking turned on for this:

```text
docker run --rm --network host -v "${PWD}:/work:ro" -w /work -e CARGO_TARGET_DIR=/tmp/target -v corqty-t19-target:/tmp/target rust:1.96-bookworm bash -c 'cargo test --locked -p corqty-worker -p corqty-api --test settings_http --test events_http --test execution --test health --test shell_switch'
```

Expected: `events_http` 5 passed, `settings_http` 3 passed, `execution` 14 passed, `health` 2 passed and
`shell_switch` 1 passed. The named volume keeps the build between the two runs. Remove it afterwards with
`docker volume rm corqty-t19-target`.

- [ ] **Step 12: Run the full verification**

```text
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Expected: formatting, clippy and `git diff --check` exit 0 with no output beyond `Finished`. Every suite passes:

- corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 6, `catalog_http` 4,
  `events_http` 5, `loops` 1, `members_http` 3, `organizations_http` 1, `problems_http` 2, `publish_http` 2,
  `runs_http` 8, `settings_http` 3, `shutdown` 1, `workflows_http` 4.
- corqty-domain: `account_and_rbac` 6, `catalog_rules` 7, `graph_rules` 8, `publish_rules` 11, `run_rules` 8,
  `runtime_rules` 12.
- corqty-executor-mock: `contract` 11.
- corqty-executor-shell: `contract` 0 on Windows and macOS, 15 on Linux.
- corqty-persistence: `auth_store` 2, `catalog_guards` 4, `final_admin` 2, `migrations` 1, `run_store` 8,
  `tenant_isolation` 1, `test_reset` 1, `worker_queue` 15, `workflow_catalog` 6.
- corqty-plugin-host: `registry` 8.
- corqty-worker: `execution` 14, `health` 2, `shell_switch` 1.

That is 176 tests on Windows and 191 on Linux (CI).

- [ ] **Step 13: Commit**

```text
git add .env.example .github/workflows/ci.yml README.md crates/api crates/persistence/tests/worker_queue.rs crates/worker docker-compose.yml docs/superpowers/specs/2026-07-28-corqty-workflow-orchestrator-design.md migrations/202610270001_live_event_details.sql scripts/smoke-foundation.ps1
git commit -m "fix: gate the shell executor at the operator level and bound live streams"
```

Expected: `git show --stat HEAD` lists exactly the 21 files above (585 insertions, 73 deletions).

## Task 20: Keep live run views accurate and reconnecting

**Files:**

- Modify: `apps/web/src/api/types.ts`
- Modify: `apps/web/src/api/queries.ts`
- Modify: `apps/web/src/runs/use-run-events.ts`
- Modify: `apps/web/src/runs/run-view.tsx`
- Modify: `apps/web/src/runs/runs-drawer.tsx`
- Modify: `apps/web/src/views/settings-view.tsx`
- Modify: `apps/web/src/studio/studio.tsx`
- Modify: `apps/web/src/test/handlers.ts`
- Test: `apps/web/src/runs/use-run-events.test.tsx`
- Test: `apps/web/src/runs/run-view.test.tsx`
- Test: `apps/web/src/views/settings-view.test.tsx`
- Test: `apps/web/src/studio/studio.test.tsx`

The review of Tasks 1-18 found that the live run views drift from the server. After an outage the stream stops
for good with nothing on screen to say so. A node that succeeds after a retry keeps the old attempt's error,
and its input stays `None`. A node that no longer waits keeps `Waiting for a worker`. A refetch that loses the
race to a streamed event throws away fields no event carries, such as `cancelRequested`. This task fixes the
web side of those findings, and it also follows the operator switch from Task 19. Task 19's API adds
`shellAvailable` to `GET /settings`, 409 `shell_unavailable`, an `input` on the finishing `node.state` event,
and 429 `too_many_streams` at the stream caps. The tests mock that contract with MSW.

- **The stream survives outages.** `canStream(run)` in `src/runs/use-run-events.ts` is true once the run has
  data, unless its last read failed with an `ApiProblem` whose status is 401, 403 or 404. `RunView` and the
  studio drawer's `RunLogs` pass it as `enabled` instead of `run.isSuccess`.
  - A failed read during an outage keeps the cached run, so the stream is no longer torn down. The CLOSED path
    from Task 16 keeps retrying until the API answers: it reads the run again, backs off (1 s, doubling to
    30 s), and reconnects with `?after=`.
  - A 429 `too_many_streams` takes the same path.
  - When a stream opens while the run query is in error, the hook reads the run again, which clears the error.
- **The run page says when it may be behind.** The header gains a `role="status"` span:
  - `Reconnecting…` while the run query is in error and the stream keeps retrying;
  - `Live updates stopped.` after a 401, 403 or 404;
  - empty otherwise.
- **Events follow the server's rules.** A `node.state` event carries only the keys it changes, so `applyEvents`
  fills in the rest the way `finish_node_run` stores them:
  - `error: event.state === "succeeded" ? null : (event.error ?? node.error)`. A success has no error, while a
    retry canceled by `POST .../cancel` keeps its last error, as on the server.
  - `input: event.input ?? node.input`. The `RunEvent` type gains `input?` on `node.state`.
  - `queueReason` is kept only while the new state is `queued` or `retry_wait`. The server computes it only for
    those states.
  - `NodeDetails` shows the queue reason only for `queued` and `retry_wait` nodes.
  - The terminal `run.state` also invalidates the run once, so the final read brings what no event carries,
    such as finish times.
- **A refetch keeps the server's fields.** `useRun` returns `applyEvents(fresh, <cached events>)`. The fresh
  response keeps its own fields, and the streamed events newer than its `lastEventSeq` are replayed on top.
  Every event the cached run has applied is also in the event log, because `flush` writes both from one batch.
  `src/api/queries.ts` now imports `applyEvents` from `src/runs/use-run-events.ts`, which imports `queryKeys`
  from it. The cycle is safe because neither module uses the other's exports while it loads.
- **The deployment's shell switch.** `OrganizationSettings` gains `shellAvailable`.
  - While it is false, the settings page shows the trusted-user warning and `The shell executor is turned off
    for this deployment (CORQTY_SHELL_EXECUTOR).` Nobody gets the toggle or the enabled/disabled line.
  - The palette offers shell definitions only when `shellAvailable && shellEnabled`.
  - A PUT that races the switch shows `The shell executor could not be changed (shell_unavailable).` through
    the existing error path.
  - The default test handler answers `{shellAvailable: true, shellEnabled: false}` (the API's `for_tests()`
    also makes shell available), and the existing settings mocks gain `shellAvailable: true`.

- [ ] **Step 1: Write the failing stream tests**

The probe passes `canStream(query)` like the app and shows `Cancel requested`. The CLOSED test now gets the
stream cap's 429 on the browser's own retry, and counts the read after the terminal event. The new test
refetches through TanStack Query's focus signal (`visibilitychange` on `window`), as the palette test does.

In `apps/web/src/runs/use-run-events.test.tsx`:

Replace:

```tsx
import {
  EVENT_LIMIT,
  mergeEvents,
```

With:

```tsx
import {
  EVENT_LIMIT,
  canStream,
  mergeEvents,
```

Replace:

```tsx
  useRunEvents("org-1", "run-2", query.isSuccess)
  const events = useRunEventLog("org-1", "run-2")
  return (
    <>
      <p>Run {query.data?.status}</p>
```

With:

```tsx
  useRunEvents("org-1", "run-2", canStream(query))
  const events = useRunEventLog("org-1", "run-2")
  return (
    <>
      <p>Run {query.data?.status}</p>
      {query.data?.cancelRequested && <p>Cancel requested</p>}
```

Replace:

```tsx
      connections += 1
      // The browser's own retry fails for good (CLOSED).
      if (connections === 2) {
        return HttpResponse.json({ code: "unavailable" }, { status: 503 })
      }
```

With:

```tsx
      connections += 1
      // The browser's own retry fails for good (CLOSED), here because the
      // API already holds as many streams as it allows.
      if (connections === 2) {
        return HttpResponse.json({ code: "too_many_streams" }, { status: 429 })
      }
```

Replace:

```tsx
  // The run is read again before the new connection.
  expect(runReads).toBe(2)
})
```

With:

```tsx
  // The run is read again before the new connection, and once it ends.
  await waitFor(() => expect(runReads).toBe(3))
})

it("a refetch keeps the server's fields and replays newer events onto them", async () => {
  let cancelRequested = false
  server.use(
    http.get(runUrl, () => HttpResponse.json({ ...run, cancelRequested })),
    sse<{ message: RunEvent }>(eventsUrl, ({ client }) => {
      client.send({ id: "4", data: log(4) })
      client.send({ id: "5", data: vpcSucceeded })
    }),
  )
  renderWithProviders(<RunProbe />)
  await waitFor(() => expect(shownSeqs()).toEqual(["4", "5"]))

  // A cancel lands. The next read sees it but not events 4 and 5 yet.
  cancelRequested = true
  window.dispatchEvent(new Event("visibilitychange"))
  expect(await screen.findByText("Cancel requested")).toBeInTheDocument()
  expect(screen.getByText("vpc succeeded")).toBeInTheDocument()
})
```

- [ ] **Step 2: Write the failing run page tests**

The first test plays the failAttempts sequence: queued while waiting for a worker, running, retry_wait with an
error, running again, then succeeded with its input and output. The outage test drops the stream and fails
every request until the test brings the API back. The browser's own retry fails, then the hook's read and its
reconnect fail until the API is back. The last test refuses the stream and the read that follows with 401, and
checks that nothing reconnects after the first backoff.

In `apps/web/src/runs/run-view.test.tsx`:

Replace:

```tsx
  await userEvent.click(toggle)
  expect(toggle).toHaveAttribute("aria-expanded", "false")
  expect(logs).not.toBeVisible()
})
```

With:

```tsx
  await userEvent.click(toggle)
  expect(toggle).toHaveAttribute("aria-expanded", "false")
  expect(logs).not.toBeVisible()
})

it("the selected node's details follow its events", async () => {
  withRun({
    nodes: [
      {
        ...run.nodes[0],
        state: "queued",
        attempt: 0,
        maxAttempts: 2,
        queueReason: "waiting_for_compatible_worker",
        input: null,
        startedAt: null,
      },
      ...run.nodes.slice(1),
    ],
  })
  const connections = stream()
  renderRun()
  fireEvent.click(
    await screen.findByRole("group", { name: "Create VPC (simulated)" }),
  )
  const details = screen.getByRole("complementary", { name: "Node details" })
  expect(details).toHaveTextContent(
    "Waiting for a worker that runs mock@1.0.0.",
  )
  await waitFor(() => expect(connections).toHaveLength(1))

  // A worker picks the node up: it no longer waits for one.
  connections[0]({
    seq: 4,
    type: "node.state",
    nodeId: "vpc",
    state: "running",
    attempt: 1,
  })
  await waitFor(() =>
    expect(within(details).getByText("Running")).toBeInTheDocument(),
  )
  expect(details).not.toHaveTextContent("Waiting for a worker")

  connections[0]({
    seq: 5,
    type: "node.state",
    nodeId: "vpc",
    state: "retry_wait",
    attempt: 1,
    error: {
      code: "mock_retryable_failure",
      message: "Attempt 1 fails because failAttempts is 1.",
      retryable: true,
    },
  })
  await waitFor(() => expect(details).toHaveTextContent("failAttempts is 1"))

  // The retry succeeds: the first attempt's error goes, the input arrives.
  connections[0]({
    seq: 6,
    type: "node.state",
    nodeId: "vpc",
    state: "running",
    attempt: 2,
  })
  connections[0]({
    seq: 7,
    type: "node.state",
    nodeId: "vpc",
    state: "succeeded",
    attempt: 2,
    input: { region: "eu-west-1" },
    output: { vpcId: "vpc-0simulated" },
  })
  await waitFor(() =>
    expect(within(details).getByText("Succeeded")).toBeInTheDocument(),
  )
  expect(within(details).getByText("2 of 2")).toBeInTheDocument()
  expect(details).not.toHaveTextContent("mock_retryable_failure")
  const figure = (caption: string) =>
    within(details).getByText(caption).closest("figure")
  expect(figure("Input")).toHaveTextContent('{ "region": "eu-west-1" }')
  expect(figure("Output")).toHaveTextContent('{ "vpcId": "vpc-0simulated" }')
})
```

Replace:

```tsx
  expect(
    await screen.findByText("Cancel failed (run_finished)."),
  ).toBeInTheDocument()
})
```

With:

```tsx
  expect(
    await screen.findByText("Cancel failed (run_finished)."),
  ).toBeInTheDocument()
})

it("keeps following the run through an outage and says it is reconnecting", async () => {
  let down = false
  const afters: (string | null)[] = []
  const connections: {
    send: (event: RunEvent) => void
    close: () => void
  }[] = []
  const unavailable = () =>
    HttpResponse.json({ code: "unavailable" }, { status: 503 })
  server.use(
    http.get(runUrl, () => (down ? unavailable() : HttpResponse.json(run))),
    http.get(`${runUrl}/events`, () => (down ? unavailable() : undefined)),
    sse<{ message: RunEvent }>(`${runUrl}/events`, ({ client, request }) => {
      afters.push(new URL(request.url).searchParams.get("after"))
      client.send({ retry: 10 })
      connections.push({
        send: (event) => client.send({ id: String(event.seq), data: event }),
        close: () => client.close(),
      })
    }),
  )
  renderRun()
  await waitFor(() => expect(connections).toHaveLength(1))
  connections[0].send(log(4, "vpc"))
  expect(await screen.findByText("line 4")).toBeInTheDocument()

  // The API goes away: the browser's retry fails, and so does the run read.
  down = true
  connections[0].close()
  expect(await screen.findByText("Reconnecting…")).toHaveAttribute(
    "role",
    "status",
  )

  // Back again: the next attempt (after 1 s) resumes after event 4, and the
  // run is read again.
  down = false
  await waitFor(
    () => expect(screen.queryByText("Reconnecting…")).not.toBeInTheDocument(),
    { timeout: 3000 },
  )
  expect(afters).toEqual(["0", "4"])
  connections[1].send({ seq: 9, type: "run.state", state: "succeeded" })
  const status = screen.getByRole("status", { name: "Run status" })
  await waitFor(() => expect(status).toHaveTextContent("Succeeded"))
})

it("stops following the run once a read says it is gone", async () => {
  let connections = 0
  let signedOut = false
  server.use(
    http.get(runUrl, () =>
      signedOut
        ? HttpResponse.json({ code: "unauthenticated" }, { status: 401 })
        : HttpResponse.json(run),
    ),
    http.get(`${runUrl}/events`, () => {
      connections += 1
      signedOut = true
      return HttpResponse.json({ code: "unauthenticated" }, { status: 401 })
    }),
  )
  renderRun()
  expect(await screen.findByText("Live updates stopped.")).toHaveAttribute(
    "role",
    "status",
  )
  // No new connection after the first backoff (1 s).
  await new Promise((resolve) => setTimeout(resolve, 1200))
  expect(connections).toBe(1)
})
```

- [ ] **Step 3: Write the failing settings and palette tests**

The test API answers like Task 19's: shell is available unless a test says otherwise.

In `apps/web/src/test/handlers.ts`:

Replace:

```ts
    HttpResponse.json({ shellEnabled: false }),
```

With:

```ts
    HttpResponse.json({ shellAvailable: true, shellEnabled: false }),
```

The existing settings tests answer with `shellAvailable: true`; the new one has the operator switch off while
the organization still has shell enabled.

Replace the whole of `apps/web/src/views/settings-view.test.tsx`:

```tsx
import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { http, HttpResponse } from "msw/http"
import { expect, it } from "vitest"

import { AuthGate } from "@/auth/auth-gate"
import { AppShell } from "@/layout/app-shell"
import { meAs } from "@/test/handlers"
import { renderWithProviders } from "@/test/render"
import { server } from "@/test/server"
import { SettingsView } from "@/views/settings-view"

const settingsUrl = "/api/v1/orgs/:organizationId/settings"

// Mirrors the (app) and orgs/[organizationId] layouts around the page.
function renderSettings() {
  return renderWithProviders(
    <AuthGate>
      <AppShell organizationId="org-1">
        <SettingsView organizationId="org-1" />
      </AppShell>
    </AuthGate>,
    { route: "/orgs/org-1/settings" },
  )
}

it("an admin turns the shell executor on and off below the trusted-user warning", async () => {
  let shellEnabled = false
  const changes: unknown[] = []
  server.use(
    http.get(settingsUrl, () =>
      HttpResponse.json({ shellAvailable: true, shellEnabled }),
    ),
    http.put(`${settingsUrl}/shell`, async ({ request }) => {
      const change = (await request.json()) as { enabled: boolean }
      changes.push(change)
      shellEnabled = change.enabled
      return HttpResponse.json({ shellAvailable: true, shellEnabled })
    }),
  )
  renderSettings()
  const toggle = await screen.findByRole("checkbox", {
    name: "Enable the shell executor",
  })
  expect(screen.getByRole("link", { name: "Settings" })).toHaveAttribute(
    "aria-current",
    "page",
  )
  expect(screen.getByRole("alert")).toHaveTextContent(
    "The shell executor is not a security boundary",
  )
  expect(toggle).not.toBeChecked()

  await userEvent.click(toggle)
  await waitFor(() => expect(toggle).toBeChecked())
  await waitFor(() => expect(toggle).toBeEnabled())
  await userEvent.click(toggle)
  await waitFor(() => expect(toggle).not.toBeChecked())
  expect(changes).toEqual([{ enabled: true }, { enabled: false }])
})

it("an editor sees whether shell is enabled but cannot change it", async () => {
  server.use(
    http.get("/api/v1/auth/me", () => HttpResponse.json(meAs("editor"))),
    http.get(settingsUrl, () =>
      HttpResponse.json({ shellAvailable: true, shellEnabled: true }),
    ),
  )
  renderSettings()
  expect(
    await screen.findByText("The shell executor is enabled."),
  ).toBeInTheDocument()
  expect(screen.getByRole("alert")).toHaveTextContent("Trusted users only")
  expect(screen.queryByRole("checkbox")).not.toBeInTheDocument()
})

it("explains that the deployment turned the shell executor off", async () => {
  server.use(
    http.get(settingsUrl, () =>
      HttpResponse.json({ shellAvailable: false, shellEnabled: true }),
    ),
  )
  renderSettings()
  expect(
    await screen.findByText(
      "The shell executor is turned off for this deployment (CORQTY_SHELL_EXECUTOR).",
    ),
  ).toBeInTheDocument()
  expect(screen.queryByRole("checkbox")).not.toBeInTheDocument()
  expect(
    screen.queryByText("The shell executor is enabled."),
  ).not.toBeInTheDocument()
})

it("says why the shell executor could not be changed", async () => {
  server.use(
    http.put(`${settingsUrl}/shell`, () =>
      HttpResponse.json({ code: "forbidden" }, { status: 403 }),
    ),
  )
  renderSettings()
  const toggle = await screen.findByRole("checkbox", {
    name: "Enable the shell executor",
  })
  await userEvent.click(toggle)
  expect(
    await screen.findByText(
      "The shell executor could not be changed (forbidden).",
    ),
  ).toBeInTheDocument()
  expect(toggle).not.toBeChecked()
})
```

The palette test goes on after the admin enables shell. The operator then turns shell off for the deployment
while the organization keeps it enabled.

In `apps/web/src/studio/studio.test.tsx`:

Replace:

```tsx
  let shellEnabled = false
  let reads = 0
  server.use(
    http.get("/api/v1/orgs/:organizationId/settings", () => {
      reads += 1
      return HttpResponse.json({ shellEnabled })
    }),
  )
```

With:

```tsx
  let settings = { shellAvailable: true, shellEnabled: false }
  let reads = 0
  server.use(
    http.get("/api/v1/orgs/:organizationId/settings", () => {
      reads += 1
      return HttpResponse.json(settings)
    }),
  )
```

Replace:

```tsx
  shellEnabled = true
  window.dispatchEvent(new Event("visibilitychange"))
  expect(await within(definitions).findByText("Shell command")).toBeVisible()
})
```

With:

```tsx
  settings = { shellAvailable: true, shellEnabled: true }
  window.dispatchEvent(new Event("visibilitychange"))
  expect(await within(definitions).findByText("Shell command")).toBeVisible()

  // The operator turns shell off for the whole deployment.
  settings = { shellAvailable: false, shellEnabled: true }
  window.dispatchEvent(new Event("visibilitychange"))
  await waitFor(() =>
    expect(
      within(definitions).queryByText("Shell command"),
    ).not.toBeInTheDocument(),
  )
})
```

- [ ] **Step 4: Run the tests and verify RED**

Run:

```text
pnpm web:test
```

Expected: `Test Files  4 failed | 13 passed (17)` and `Tests  10 failed | 110 passed (120)`:
- In `use-run-events.test.tsx`, the five tests that render the probe fail with
  `TypeError: canStream is not a function`. Only `keeps the newest events and drops ones it has` passes. With
  just `canStream` added, two tests would still fail against the old hook:
  - `reconnects with ?after= once the browser gives up` with `AssertionError: expected 2 to be 3`;
  - the refetch test with `Unable to find an element with the text: Cancel requested`.
- `the selected node's details follow its events` fails with `expect(element).not.toHaveTextContent()` /
  `Expected element not to have text content: Waiting for a worker`.
- `keeps following the run through an outage and says it is reconnecting` fails with
  `TestingLibraryElementError: Unable to find an element with the text: Reconnecting…`. The failed read turned
  `run.isSuccess` off, so the stream stopped for good.
- `stops following the run once a read says it is gone` fails with `Unable to find an element with the text:
  Live updates stopped.`.
- `explains that the deployment turned the shell executor off` fails with `Unable to find an element with the
  text: The shell executor is turned off for this deployment (CORQTY_SHELL_EXECUTOR).`.
- `the palette offers the shell preset only while shell is enabled` fails with
  `expect(element).not.toBeInTheDocument()` / `expected document not to contain element, found <span ...>
  Shell command`.

- [ ] **Step 5: Apply events the way the server stores them**

In `apps/web/src/api/types.ts`:

Replace:

```ts
export interface OrganizationSettings {
  shellEnabled: boolean
}
```

With:

```ts
export interface OrganizationSettings {
  // False while the deployment keeps shell off (CORQTY_SHELL_EXECUTOR).
  shellAvailable: boolean
  shellEnabled: boolean
}
```

Replace:

```ts
// Event payloads omit absent keys: a node.state carries `error` when the node
// failed or waits to retry, and `output` when it succeeded.
```

With:

```ts
// Event payloads omit absent keys: a node.state carries `error` when the node
// failed or waits to retry, `output` when it succeeded, and `input` once a
// finished attempt resolved it.
```

Replace:

```ts
      error?: NonNullable<NodeRun["error"]>
      output?: Record<string, unknown>
```

With:

```ts
      error?: NonNullable<NodeRun["error"]>
      input?: Record<string, unknown>
      output?: Record<string, unknown>
```

In `apps/web/src/runs/use-run-events.ts`:

Replace:

```ts
import { useEffect } from "react"

import { queryKeys } from "@/api/queries"
```

With:

```ts
import { useEffect } from "react"

import { ApiProblem } from "@/api/client"
import { queryKeys } from "@/api/queries"
```

Replace:

```ts
// Events the snapshot already reflects (seq <= lastEventSeq) change nothing.
// A node.state carries only the keys it changes.
export function applyEvents(run: Run, events: RunEvent[]): Run {
```

With:

```ts
// Events the snapshot already reflects (seq <= lastEventSeq) change nothing.
// A node.state carries only the keys it changes, so the rest follows the
// server's rules: a success has no error (a canceled retry keeps its last
// one), and a queue reason holds only while the node waits.
export function applyEvents(run: Run, events: RunEvent[]): Run {
```

Replace:

```ts
              state: event.state,
              attempt: event.attempt,
              error: event.error ?? node.error,
              output: event.output ?? node.output,
            }
```

With:

```ts
              state: event.state,
              attempt: event.attempt,
              error:
                event.state === "succeeded"
                  ? null
                  : (event.error ?? node.error),
              input: event.input ?? node.input,
              output: event.output ?? node.output,
              queueReason:
                event.state === "queued" || event.state === "retry_wait"
                  ? node.queueReason
                  : null,
            }
```

Replace:

```ts
// Streams a run's events into the query cache while `enabled`. Pass
// `run.isSuccess`, so a 401 or 404 on a run refetch closes the stream. It
// resumes after the newest event in the cache, or replays from the start.
```

With:

```ts
// Whether to stream a loaded run. A failed read (an outage) keeps the cached
// run and the stream retrying; a 401, 403 or 404 ends it for good.
export function canStream(run: { data?: Run; error: Error | null }) {
  return (
    run.data !== undefined &&
    !(
      run.error instanceof ApiProblem &&
      [401, 403, 404].includes(run.error.status)
    )
  )
}

// Streams a run's events into the query cache while `enabled`; pass
// `canStream(run)`. It resumes after the newest event in the cache, or
// replays from the start.
```

Replace:

```ts
      current.onopen = () => {
        failures = 0
      }
```

With:

```ts
      current.onopen = () => {
        failures = 0
        // A run read that failed while the stream was down is tried again.
        if (queryClient.getQueryState(runKey)?.status === "error") {
          void queryClient.invalidateQueries({ queryKey: runKey, exact: true })
        }
      }
```

Replace:

```ts
        if (event.type === "run.state" && isTerminal(event.state)) {
          current.close()
          flush()
        }
```

With:

```ts
        if (event.type === "run.state" && isTerminal(event.state)) {
          current.close()
          flush()
          // The final read brings what no event carries, like finish times.
          void queryClient.invalidateQueries({ queryKey: runKey, exact: true })
        }
```

Replace:

```ts
        // CLOSED: a non-200 (204 included) that the browser never retries.
        // The status is hidden, so read the run first: a 401 or 404 turns
        // `enabled` off, and a finished run ends in done().
```

With:

```ts
        // CLOSED: a non-200 (204 included) that the browser never retries.
        // The status is hidden, so read the run first: a 401, 403 or 404
        // turns `enabled` off, and a finished run ends in done().
```

In `apps/web/src/api/queries.ts`:

Replace:

```ts
  WorkflowVersion,
} from "@/api/types"
```

With:

```ts
  WorkflowVersion,
} from "@/api/types"
import { applyEvents, type RunEventLog } from "@/runs/use-run-events"
```

Replace:

```ts
// The run stream also writes this run (runs/use-run-events.ts). A refetch
// never moves it back behind events already applied.
export function useRun(organizationId: string, runId: string) {
  return useQuery({
    queryKey: queryKeys.run(organizationId, runId),
    queryFn: async ({ signal, client, queryKey }) => {
      const fresh = await api<Run>(`/orgs/${organizationId}/runs/${runId}`, {
        signal,
      })
      const cached = client.getQueryData<Run>(queryKey)
      return cached && cached.lastEventSeq > fresh.lastEventSeq ? cached : fresh
    },
  })
}
```

With:

```ts
// The run stream also writes this run (runs/use-run-events.ts). A refetch
// replays the streamed events newer than its response onto it, so it never
// moves back behind them and still brings what no event carries, like
// cancelRequested.
export function useRun(organizationId: string, runId: string) {
  return useQuery({
    queryKey: queryKeys.run(organizationId, runId),
    queryFn: async ({ signal, client }) => {
      const fresh = await api<Run>(`/orgs/${organizationId}/runs/${runId}`, {
        signal,
      })
      const log = client.getQueryData<RunEventLog>(
        queryKeys.runEvents(organizationId, runId),
      )
      return applyEvents(fresh, log?.events ?? [])
    },
  })
}
```

- [ ] **Step 6: Keep the run page and the studio drawer streaming**

In `apps/web/src/runs/run-view.tsx`:

Replace:

```tsx
import { isTerminal, useRunEventLog, useRunEvents } from "@/runs/use-run-events"
```

With:

```tsx
import {
  canStream,
  isTerminal,
  useRunEventLog,
  useRunEvents,
} from "@/runs/use-run-events"
```

Replace:

```tsx
function RunHeader({
  organizationId,
  run,
}: {
  organizationId: string
  run: Run
}) {
```

With:

```tsx
function RunHeader({
  organizationId,
  run,
  connection,
}: {
  organizationId: string
  run: Run
  connection: string
}) {
```

Replace:

```tsx
      <span aria-live="polite">{notice}</span>
```

With:

```tsx
      <span aria-live="polite">{notice}</span>
      <span role="status" className="text-muted-foreground">
        {connection}
      </span>
```

Replace:

```tsx
      {nodeRun.queueReason === "waiting_for_compatible_worker" && (
        <p>Waiting for a worker that runs {nodeRun.executor}.</p>
      )}
```

With:

```tsx
      {(nodeRun.state === "queued" || nodeRun.state === "retry_wait") &&
        nodeRun.queueReason === "waiting_for_compatible_worker" && (
          <p>Waiting for a worker that runs {nodeRun.executor}.</p>
        )}
```

Replace:

```tsx
  const definitions = useNodeDefinitions(organizationId)
  useRunEvents(organizationId, runId, run.isSuccess)
```

With:

```tsx
  const definitions = useNodeDefinitions(organizationId)
  const streaming = canStream(run)
  useRunEvents(organizationId, runId, streaming)
```

Replace:

```tsx
  return (
    <section>
      <RunHeader organizationId={organizationId} run={run.data} />
```

With:

```tsx
  // A failed read leaves the last known run on screen; say why it may be
  // behind.
  const connection = !run.isError
    ? ""
    : streaming
      ? "Reconnecting…"
      : "Live updates stopped."

  return (
    <section>
      <RunHeader
        organizationId={organizationId}
        run={run.data}
        connection={connection}
      />
```

In `apps/web/src/runs/runs-drawer.tsx`:

Replace:

```tsx
import { useRunEventLog, useRunEvents } from "@/runs/use-run-events"
```

With:

```tsx
import { canStream, useRunEventLog, useRunEvents } from "@/runs/use-run-events"
```

Replace:

```tsx
  useRunEvents(organizationId, runId, streaming && run.isSuccess)
```

With:

```tsx
  useRunEvents(organizationId, runId, streaming && canStream(run))
```

- [ ] **Step 7: Follow the deployment's shell switch in settings and the palette**

Replace the whole of `apps/web/src/views/settings-view.tsx`:

```tsx
"use client"

import { useId, useState } from "react"

import { ApiProblem } from "@/api/client"
import { useSetShellEnabled, useSettings } from "@/api/queries"
import { useCan } from "@/auth/use-can"
import { ShellWarning } from "@/components/shell-warning"
import { Label } from "@/components/ui/label"

export function SettingsView({ organizationId }: { organizationId: string }) {
  const canEnableShell = useCan(organizationId, "enable_shell_executor")
  const settings = useSettings(organizationId)
  const setShellEnabled = useSetShellEnabled(organizationId)
  const [error, setError] = useState("")
  const headingId = useId()
  const toggleId = useId()

  async function change(enabled: boolean) {
    setError("")
    try {
      await setShellEnabled.mutateAsync(enabled)
    } catch (problem) {
      const code =
        problem instanceof ApiProblem ? problem.code : "network_error"
      setError(`The shell executor could not be changed (${code}).`)
    }
  }

  return (
    <section className="flex flex-col gap-5 p-4 md:p-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Settings</h1>
        <p className="text-sm text-muted-foreground">
          Options for the whole organization.
        </p>
      </div>
      <section aria-labelledby={headingId} className="grid max-w-2xl gap-3">
        <h2 id={headingId} className="text-lg font-semibold">
          Shell executor
        </h2>
        <p className="text-sm text-muted-foreground">
          Lets workflow nodes run shell commands on the workers. Only an admin
          can change it; runs that already exist are not affected.
        </p>
        <ShellWarning />
        {settings.isPending && (
          <p className="text-sm text-muted-foreground">Loading settings…</p>
        )}
        {settings.isError && (
          <p role="alert" className="text-sm text-destructive">
            The settings could not be loaded.
          </p>
        )}
        {settings.data && !settings.data.shellAvailable && (
          <p className="text-sm">
            The shell executor is turned off for this deployment
            (CORQTY_SHELL_EXECUTOR).
          </p>
        )}
        {settings.data?.shellAvailable &&
          (canEnableShell ? (
            <div className="flex items-center gap-2">
              <input
                id={toggleId}
                type="checkbox"
                className="size-4 accent-primary"
                checked={settings.data.shellEnabled}
                disabled={setShellEnabled.isPending}
                onChange={(event) => void change(event.target.checked)}
              />
              <Label htmlFor={toggleId}>Enable the shell executor</Label>
            </div>
          ) : (
            <p className="text-sm">
              The shell executor is{" "}
              {settings.data.shellEnabled ? "enabled" : "disabled"}.
            </p>
          ))}
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
      </section>
    </section>
  )
}
```

In `apps/web/src/studio/studio.tsx`:

Replace:

```tsx
  // The shell preset (and any definition on the shell executor) is offered
  // only once an admin enables shell.
  const paletteDefinitions = settings.data?.shellEnabled
    ? definitions
    : definitions.filter(({ executor }) => executor.key !== "shell")
```

With:

```tsx
  // The shell preset (and any definition on the shell executor) is offered
  // only while the deployment allows shell and an admin enabled it.
  const paletteDefinitions =
    settings.data?.shellAvailable && settings.data.shellEnabled
      ? definitions
      : definitions.filter(({ executor }) => executor.key !== "shell")
```

- [ ] **Step 8: Verify GREEN**

Run:

```text
pnpm web:test
```

Expected: `Test Files  17 passed (17)` and `Tests  120 passed (120)` with no unhandled errors. That is
use-run-events 6, run-view 9, settings-view 4, studio 26, and the 75 tests of the other thirteen files. The
outage and 401 tests each wait about 1.2 s for the hook's first backoff. On Node 25 or newer, each worker may
print `ExperimentalWarning: localStorage is not available because --localstorage-file was not provided.`; this
is harmless.

- [ ] **Step 9: Run lint and build**

Run:

```text
pnpm web:lint
```

Expected: `✓ Types generated successfully`, then `tsc --noEmit` and `oxlint` report no diagnostics. The command
exits 0.

Run:

```text
pnpm web:build
```

Expected: `✓ Compiled successfully` and `Finished TypeScript`. The route table is unchanged. The command exits
0.

- [ ] **Step 10: Commit**

```text
git add apps/web
git commit -m "fix: keep live run views accurate and reconnecting"
```
