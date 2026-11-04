# Configuration reference

Run [the credential initializer](../README.md#start-locally) before starting
Compose. It creates an ignored `.env` from [`.env.example`](../.env.example),
generates database credentials and a master key, and preserves an existing file.
Never use the example passwords for a public deployment.

Compose reads `.env` for interpolation. Rust binaries do **not** automatically
load that file: direct `cargo run` sessions need environment variables supplied
by their shell or secret manager. Host database URLs use a reachable hostname
such as `127.0.0.1`; Compose URLs use the `postgres` service hostname.

## Database and network

| Variable | Default in local Compose/example | Consumer and meaning |
|---|---|---|
| `POSTGRES_DB` | `corqty` | Initial PostgreSQL database; keep all database URL paths aligned |
| `POSTGRES_SUPERUSER_PASSWORD` | Example-only `postgres_dev` | PostgreSQL bootstrap superuser; initializer replaces it |
| `CORQTY_OWNER_DB_PASSWORD` | Example-only `corqty_owner_dev` | Password created for migration owner; initializer replaces it |
| `CORQTY_API_DB_PASSWORD` | Example-only `corqty_api_dev` | Restricted API database role password |
| `CORQTY_WORKER_DB_PASSWORD` | Example-only `corqty_worker_dev` | Restricted worker database role password |
| `DATABASE_URL_OWNER` | Owner URL to `postgres:5432/corqty` | Required by migrator; Compose has local fallback |
| `DATABASE_URL_API` | API URL to `postgres:5432/corqty` | Required by API; never substitute owner/superuser credentials |
| `DATABASE_URL_WORKER` | Worker URL to `postgres:5432/corqty` | Required by worker; never substitute owner/superuser credentials |
| `CORQTY_POSTGRES_PORT` | `5432` | Host loopback port in local Compose |
| `CORQTY_API_PORT` | `8080` | Host loopback port in local Compose |
| `CORQTY_WEB_PORT` | `3000` | Host loopback port in local Compose |
| `CORQTY_BIND_ADDRESS` | `0.0.0.0:8080` | Direct API process listener; application Compose fixes the container listener to this address |
| `CORQTY_PUBLIC_WEB_URL` | `http://localhost:3000` | Public web origin used for generated invitation URLs; update when changing the web port/origin |
| `CORQTY_API_ORIGIN` | Compose: `http://api:8080`; direct web: `http://127.0.0.1:8080` | Web proxy destination, resolved during the web build |
| `CORQTY_DOMAIN` | No default | Required by production override; hostname without scheme/path, e.g. `workflows.example.com` |

Passwords are set by database initialization only on a fresh volume. Changing
`.env` does not rotate existing database role passwords. Changing `POSTGRES_DB`
does not rewrite already configured database URLs. Coordinate both with the
[upgrade and credential procedures](operations.md).

## Security and execution switches

| Variable | Behavior |
|---|---|
| `CORQTY_MASTER_KEY` | Required by API and worker; base64 encoding of exactly 32 random bytes. Both must use the same protected key. No insecure runtime default. |
| `CORQTY_SECURE_COOKIES` | Local Compose defaults to `false`; production override forces `true`. The standalone API defaults to secure cookies unless the value is exactly `false`. |
| `CORQTY_SHELL_EXECUTOR` | Only the exact value `enabled` offers trusted shell/host commands. Otherwise disabled. Production override forces disabled. Organization settings further control built-in shell use. |
| `CORQTY_WORKER_CAPACITY` | Default `4`; positive integer accepted by the worker. This is concurrency per process, not a global run quota. Size it against memory, CPU, subprocesses, and database connections. |
| `RUST_LOG` | API default `corqty_api=info,tower_http=info`; worker binary default `corqty_worker=info`. Local Compose forwards this setting to API; worker defaults apply unless explicitly configured. |

Do not replace the master key as a rotation shortcut. Read [secrets](secrets.md)
and [operations](operations.md) first. Trusted commands run as the shared worker
user and can access worker credentials; see [security](security.md).

## Workload ceilings

The shared [limits parser](../crates/domain/src/limits.rs) accepts positive whole
numbers. These ceilings may be **lowered**, not raised. Invalid values, zero, or
above-ceiling values stop API/worker startup. Keep common settings aligned on
both processes. A Compose variable must also be forwarded to the appropriate
service; defining an arbitrary variable in `.env` alone does not inject it.

| Variable | Default / hard ceiling | Meaning |
|---|---:|---|
| `CORQTY_MAX_NODES` | 500 | Nodes in a graph |
| `CORQTY_MAX_EDGES` | 1,000 | Graph edges |
| `CORQTY_MAX_RUN_INPUT_BYTES` | 1,048,576 | Serialized run input |
| `CORQTY_MAX_NODE_INPUT_BYTES` | 1,048,576 | Resolved node input |
| `CORQTY_MAX_NODE_OUTPUT_BYTES` | 1,048,576 | Serialized node output |
| `CORQTY_MAX_LOG_BYTES` | 10,485,760 | Logs per node run; database emits one truncation event at the limit |
| `CORQTY_MAX_WEBHOOK_BYTES` | 1,048,576 | Raw webhook request body |
| `CORQTY_MAX_COMPONENT_BYTES` | 26,214,400 | Decoded WASM component upload/runtime budget |
| `CORQTY_MAX_WASM_MEMORY_BYTES` | 268,435,456 | Guest linear memory budget |
| `CORQTY_MAX_TIMEOUT_SECONDS` | 14,400 | Maximum node timeout |
| `CORQTY_DEFAULT_TIMEOUT_SECONDS` | 900 by default | Default node timeout; must not exceed configured maximum |

For a direct Rust process, an **unset** default timeout becomes the smaller of
900 and the configured maximum. Compose explicitly forwards a default of 900
even when the variable is absent from `.env`. When lowering the maximum below
900 in Compose, set `CORQTY_DEFAULT_TIMEOUT_SECONDS` to a value at or below that
maximum too. Existing runs capture some limits at creation, including log and
timeout ceilings; changing configuration is not a retroactive rewrite of run
snapshots.

The plugin HTTP route allows a larger JSON envelope for base64 component bytes.
Ingress limits must accommodate this separately from the 1 MiB webhook limit.
See [API](api.md) and [plugins](plugins.md).

## Rate limiting

Rate knobs use the separate [bounded rate parser](../crates/api/src/limits.rs):
missing/malformed values fall back to the default; numeric values are clamped
to the supported range. They do not share workload-limit fail-fast behavior.

| Variable or fixed bucket | Default per minute | Supported range / scope |
|---|---:|---|
| `CORQTY_AUTH_RATE_PER_MINUTE` | 30 | 1–1,000; login/registration by socket peer |
| `CORQTY_TOKEN_RATE_PER_MINUTE` | 120 | 1–10,000; valid service-token actor |
| `CORQTY_WEBHOOK_RATE_PER_MINUTE` | 120 | 1–10,000; valid webhook actor |
| Invitation acceptance | 30 | Fixed peer bucket |
| External authorization/webhook requests | 300 | Fixed peer bucket in addition to actor limits |

Counters are shared through PostgreSQL. Forwarding headers are not trusted.
Behind a proxy, several users share the same socket-peer bucket, so configure
client-aware limits at the trusted edge. Details are in [automation](automation.md).

## Fixed runtime and deployment values

These are current implementation/deployment defaults, not extra environment
variables. Change code/configuration and validate behavior when changing them.

| Setting | Current value | Source |
|---|---|---|
| Session lifetime | 30 days | [API config](../crates/api/src/config.rs) |
| API connection pool | 20 plus separate event listener | [API main](../crates/api/src/main.rs) |
| Worker pool | Capacity + 1 | [Worker main](../crates/worker/src/main.rs) |
| Worker claim poll / lease / heartbeat | 500 ms / 30 s / 10 s | [Worker config](../crates/worker/src/lib.rs) |
| Worker shutdown drain / cancel grace | 25 s / 5 s | [Worker config](../crates/worker/src/lib.rs) |
| Recovery / cron / retention loops | 5 s / 15 s / 1 hour | [API loops](../crates/api/src/loops.rs) |
| SSE connections | 256 per API process, 8 per user per process | [Events](../crates/api/src/events.rs) |
| SSE stream lifetime / keepalive / safety read | 10 minutes / 15 s / 30 s | [Events](../crates/api/src/events.rs) |
| WASM fuel | 100 million units per call | [Runtime](../crates/plugin-host/src/runtime.rs) |
| Worker container | 2 GiB RAM, 256 processes, 256 MiB temporary filesystem | [Compose](../docker-compose.yml) |

The web image sets `NODE_ENV=production`, disables Next telemetry, binds
`HOSTNAME=0.0.0.0`, and serves `PORT=3000`. These image/container values differ
from the host port mapping. Rebuild web when changing `CORQTY_API_ORIGIN`.

## Validate a configuration

```sh
docker compose config --quiet
docker compose -f docker-compose.yml -f docker/compose.production.yml config --quiet
docker compose -f docker/compose.test.yml config --quiet
```

The production command requires `CORQTY_DOMAIN`. `--quiet` checks the structure
without printing resolved secret values. Parsing does not validate live DNS,
certificate issuance, database credentials, or the contents of an existing
volume; complete the checks in [operations](operations.md).
