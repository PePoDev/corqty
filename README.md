# Corqty

Generic visual workflow orchestration. Milestone 1 provides the multi-tenant
account, RBAC, audit, API, React shell, PostgreSQL, and Docker foundations.

## Local start

```powershell
Copy-Item .env.example .env
docker compose up --build --wait
./scripts/smoke-foundation.ps1
```

Open http://localhost:5173 and create the first account and organization.
The host ports can be changed with `CORQTY_POSTGRES_PORT`,
`CORQTY_API_PORT`, and `CORQTY_WEB_PORT`.

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
