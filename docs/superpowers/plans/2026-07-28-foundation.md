# Corqty Milestone 1 Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a runnable Rust/React/PostgreSQL foundation with email/password sessions, organizations, fixed RBAC, tenant isolation, member invitations, administrative audit events, and local Docker Compose.

**Architecture:** The Axum API and React SPA are separate applications in one monorepo. PostgreSQL is accessed through a focused persistence crate; organization-scoped access always starts a transaction that sets user and organization context for row-level security. Domain validation and permission rules remain independent of HTTP and SQL.

**Tech Stack:** Rust 1.96 / edition 2024, Axum 0.8, Tokio 1, SQLx 0.8, PostgreSQL 18, Argon2id, React 19, TypeScript, Vite, React Router, Tailwind CSS, shadcn/ui, Vitest, Testing Library, Docker Compose.

---

## Scope boundary

This plan implements only Milestone 1 from the approved design. It intentionally excludes workflow drafts, XYFlow, node definitions, executors, workers, secrets, triggers, run scheduling, and the deployment-wide rate limiter assigned to Milestone 5. The result must remain runnable and establish the tenant/security boundaries those later plans use.

## File map

### Root and local infrastructure

- `Cargo.toml`: Rust workspace members, shared versions, and lint policy.
- `rust-toolchain.toml`: Rust 1.96 toolchain pin.
- `package.json`, `pnpm-workspace.yaml`: JavaScript workspace commands.
- `.env.example`: documented local configuration.
- `.dockerignore`: Docker build exclusions.
- `docker-compose.yml`: local PostgreSQL, migrator, API, and web services.
- `docker-compose.test.yml`: isolated PostgreSQL used by integration tests.
- `docker/postgres/init/001-roles.sh`: creates non-owner API and worker database roles.
- `docker/rust/Dockerfile`: API/migrator development image.
- `migrations/202607280001_base.sql`: account, organization, invitation, session, and audit tables.
- `migrations/202607280002_rls.sql`: tenant context functions and RLS policies.
- `migrations/202607280003_user_organizations.sql`: narrow organization-switcher read function.
- `migrations/202607280004_invitations.sql`: invitation claim and final-admin database invariant.
- `migrations/202607280005_user_audit.sql`: organization copies of account security events.

### Rust crates

- `crates/domain`: role/capability rules and validated account/organization values.
- `crates/persistence`: migrations, tenant transactions, account/session, organization/member, invitation, and audit repositories.
- `crates/api`: Axum router, auth middleware, CSRF, handlers, services, and error responses.
- `crates/migrator`: one-shot migration binary used by Compose.
- `crates/test-support`: database reset and owner/API pools for serial integration tests.

### React application

- `apps/web/src/api`: typed fetch client and endpoint types.
- `apps/web/src/auth`: auth context and protected routing.
- `apps/web/src/layout`: Vercel-style organization shell.
- `apps/web/src/pages`: login, registration, organization home, members, invitation acceptance, and audit log.
- `apps/web/src/components/ui`: shadcn/ui generated components.
- `apps/web/src/styles/globals.css`: shadcn/ui tokens and application base styles.

## Task 1: Scaffold the monorepo without product behavior

**Files:**

- Modify: `.gitignore`
- Create: `.dockerignore`
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `rustfmt.toml`
- Create: `crates/domain/Cargo.toml`
- Create: `crates/domain/src/lib.rs`
- Create: `crates/persistence/Cargo.toml`
- Create: `crates/persistence/src/lib.rs`
- Create: `crates/api/Cargo.toml`
- Create: `crates/api/src/lib.rs`
- Create: `crates/migrator/Cargo.toml`
- Create: `crates/migrator/src/main.rs`
- Create: `crates/test-support/Cargo.toml`
- Create: `crates/test-support/src/lib.rs`
- Create: `package.json`
- Create: `pnpm-workspace.yaml`
- Create: `apps/web/package.json`
- Create: `apps/web/index.html`
- Create: `apps/web/tsconfig.json`
- Create: `apps/web/tsconfig.app.json`
- Create: `apps/web/vite.config.ts`

- [ ] **Step 1: Add workspace-only configuration**

Create `Cargo.toml`:

```toml
[workspace]
resolver = "3"
members = [
  "crates/api",
  "crates/domain",
  "crates/migrator",
  "crates/persistence",
  "crates/test-support",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "Apache-2.0"
rust-version = "1.96"

[workspace.dependencies]
argon2 = "0.5"
axum = { version = "0.8", features = ["macros"] }
base64 = "0.22"
chrono = { version = "0.4", features = ["serde"] }
getrandom = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
sqlx = { version = "0.8", features = ["chrono", "json", "migrate", "postgres", "runtime-tokio-rustls", "uuid"] }
thiserror = "2"
tokio = { version = "1", features = ["macros", "net", "rt-multi-thread", "signal"] }
tower = { version = "0.5", features = ["util"] }
tower-http = { version = "0.6", features = ["trace"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
uuid = { version = "1", features = ["serde", "v4"] }

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
all = "warn"
module_name_repetitions = "allow"
```

Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.96.0"
components = ["clippy", "rustfmt"]
profile = "minimal"
```

Create `rustfmt.toml`:

```toml
edition = "2024"
newline_style = "Unix"
use_field_init_shorthand = true
```

Append these entries to `.gitignore`:

```gitignore
.env
.env.local
target/
node_modules/
dist/
coverage/
```

Create `.dockerignore`:

```dockerignore
.git
.superpowers
**/node_modules
**/dist
target
.env
coverage
```

- [ ] **Step 2: Add empty crate targets and manifests**

Create `crates/domain/Cargo.toml`:

```toml
[package]
name = "corqty-domain"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
serde.workspace = true
thiserror.workspace = true

[lints]
workspace = true
```

Create `crates/persistence/Cargo.toml`:

```toml
[package]
name = "corqty-persistence"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
chrono.workspace = true
corqty-domain = { path = "../domain" }
serde.workspace = true
serde_json.workspace = true
sqlx.workspace = true
thiserror.workspace = true
uuid.workspace = true

[dev-dependencies]
corqty-test-support = { path = "../test-support" }
serial_test = "3"
tokio.workspace = true

[lints]
workspace = true
```

Create `crates/api/Cargo.toml`:

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

Create `crates/migrator/Cargo.toml`:

```toml
[package]
name = "corqty-migrator"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
corqty-persistence = { path = "../persistence" }
sqlx.workspace = true
tokio.workspace = true

[lints]
workspace = true
```

Create `crates/test-support/Cargo.toml`:

```toml
[package]
name = "corqty-test-support"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish = false

[dependencies]
sqlx.workspace = true

[lints]
workspace = true
```

Create each initial Rust target with only the crate safety attribute:

```rust
#![forbid(unsafe_code)]
```

Use that content in:

- `crates/domain/src/lib.rs`
- `crates/persistence/src/lib.rs`
- `crates/api/src/lib.rs`
- `crates/test-support/src/lib.rs`

Create `crates/migrator/src/main.rs`:

```rust
#![forbid(unsafe_code)]

fn main() {}
```

- [ ] **Step 3: Add the JavaScript workspace configuration**

Create root `package.json`:

```json
{
  "name": "corqty",
  "private": true,
  "packageManager": "pnpm@10.33.2",
  "scripts": {
    "web:build": "pnpm --filter @corqty/web build",
    "web:lint": "pnpm --filter @corqty/web lint",
    "web:test": "pnpm --filter @corqty/web test"
  }
}
```

Create `pnpm-workspace.yaml`:

```yaml
packages:
  - apps/*
```

Create `apps/web/package.json`:

```json
{
  "name": "@corqty/web",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "build": "tsc -b && vite build",
    "dev": "vite --host 0.0.0.0",
    "lint": "tsc -b --pretty false",
    "test": "vitest run"
  }
}
```

Create `apps/web/index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Corqty</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

Create `apps/web/tsconfig.json`:

```json
{
  "files": [],
  "references": [{ "path": "./tsconfig.app.json" }]
}
```

Create `apps/web/tsconfig.app.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "useDefineForClassFields": true,
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "allowJs": false,
    "skipLibCheck": true,
    "esModuleInterop": true,
    "allowSyntheticDefaultImports": true,
    "strict": true,
    "forceConsistentCasingInFileNames": true,
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx",
    "baseUrl": ".",
    "paths": { "@/*": ["./src/*"] },
    "types": ["node", "vitest/globals", "@testing-library/jest-dom"]
  },
  "include": ["src", "vite.config.ts"]
}
```

Create `apps/web/vite.config.ts`:

```ts
import { fileURLToPath } from "node:url"
import react from "@vitejs/plugin-react"
import tailwindcss from "@tailwindcss/vite"
import { defineConfig } from "vitest/config"

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  server: {
    proxy: {
      "/api": process.env.VITE_API_PROXY ?? "http://localhost:8080",
    },
  },
})
```

- [ ] **Step 4: Install and lock frontend tooling**

Run:

```text
pnpm --dir apps/web add react@latest react-dom@latest react-router-dom@latest
pnpm --dir apps/web add -D @tailwindcss/vite@latest @testing-library/jest-dom@latest @testing-library/react@latest @testing-library/user-event@latest @types/node@latest @types/react@latest @types/react-dom@latest @vitejs/plugin-react@latest jsdom@latest tailwindcss@latest typescript@latest vite@latest vitest@latest
pnpm install
```

Expected: `pnpm-lock.yaml` is created and all commands exit 0.

- [ ] **Step 5: Validate metadata**

Run:

```text
cargo metadata --no-deps
pnpm --filter @corqty/web exec vite --version
```

Expected: both commands exit 0 and list the workspace packages.

- [ ] **Step 6: Commit**

```text
git add .gitignore .dockerignore Cargo.toml rust-toolchain.toml rustfmt.toml crates package.json pnpm-workspace.yaml pnpm-lock.yaml apps/web
git commit -m "chore: scaffold Corqty monorepo"
```

## Task 2: Implement domain validation and the fixed RBAC matrix

**Files:**

- Create: `crates/domain/src/error.rs`
- Create: `crates/domain/src/account.rs`
- Create: `crates/domain/src/rbac.rs`
- Modify: `crates/domain/src/lib.rs`
- Test: `crates/domain/tests/account_and_rbac.rs`

- [ ] **Step 1: Write failing domain tests**

Create `crates/domain/tests/account_and_rbac.rs`:

```rust
use corqty_domain::{
    account::{NormalizedEmail, OrganizationName, validate_password},
    rbac::{Capability, Role},
};

#[test]
fn normalizes_email_and_organization_name() {
    let email = NormalizedEmail::parse("  Admin@Example.COM ").unwrap();
    let name = OrganizationName::parse("  Acme Cloud  ").unwrap();

    assert_eq!(email.as_str(), "admin@example.com");
    assert_eq!(name.as_str(), "Acme Cloud");
}

#[test]
fn rejects_invalid_account_values() {
    assert!(NormalizedEmail::parse("not-an-email").is_err());
    assert!(OrganizationName::parse(" ").is_err());
    assert!(validate_password("short").is_err());
    assert!(validate_password(&"x".repeat(129)).is_err());
}

#[test]
fn fixed_roles_match_the_approved_permission_matrix() {
    let roles = [Role::Admin, Role::Editor, Role::Runner, Role::Viewer];
    for (capability, expected) in [
        (Capability::ViewOrganization, [true, true, true, true]),
        (Capability::EditWorkflows, [true, true, false, false]),
        (Capability::PublishWorkflows, [true, true, false, false]),
        (Capability::RunWorkflows, [true, true, true, false]),
        (Capability::ManageMembers, [true, false, false, false]),
        (Capability::ManageServiceTokens, [true, false, false, false]),
        (Capability::ManageSecrets, [true, false, false, false]),
        (Capability::ManagePlugins, [true, false, false, false]),
        (Capability::EnableShellExecutor, [true, false, false, false]),
    ] {
        for (role, allowed) in roles.into_iter().zip(expected) {
            assert_eq!(role.allows(capability), allowed, "{role}: {capability:?}");
        }
    }
}

#[test]
fn roles_round_trip_as_snake_case() {
    for (text, role) in [
        ("admin", Role::Admin),
        ("editor", Role::Editor),
        ("runner", Role::Runner),
        ("viewer", Role::Viewer),
    ] {
        assert_eq!(text.parse::<Role>().unwrap(), role);
        assert_eq!(role.as_str(), text);
    }
    assert!("owner".parse::<Role>().is_err());
}
```

- [ ] **Step 2: Run the tests and verify RED**

Run:

```text
cargo test -p corqty-domain --test account_and_rbac
```

Expected: compilation fails because `account`, `rbac`, and their public types do not exist.

- [ ] **Step 3: Implement minimal validated values**

Create `crates/domain/src/error.rs`:

```rust
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("email is invalid")]
    InvalidEmail,
    #[error("organization name must contain 1 to 100 characters")]
    InvalidOrganizationName,
    #[error("password must contain 12 to 128 characters")]
    InvalidPassword,
    #[error("role is invalid")]
    InvalidRole,
}
```

Create `crates/domain/src/account.rs`:

```rust
use serde::{Deserialize, Serialize};

use crate::error::DomainError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NormalizedEmail(String);

impl NormalizedEmail {
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        let normalized = value.trim().to_lowercase();
        let mut parts = normalized.split('@');
        let local = parts.next().unwrap_or_default();
        let domain = parts.next().unwrap_or_default();
        let valid = !local.is_empty()
            && domain.contains('.')
            && !domain.starts_with('.')
            && !domain.ends_with('.')
            && parts.next().is_none()
            && normalized.len() <= 320;
        valid
            .then_some(Self(normalized))
            .ok_or(DomainError::InvalidEmail)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OrganizationName(String);

impl OrganizationName {
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        let trimmed = value.trim();
        (1..=100)
            .contains(&trimmed.chars().count())
            .then(|| Self(trimmed.to_owned()))
            .ok_or(DomainError::InvalidOrganizationName)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn validate_password(value: &str) -> Result<(), DomainError> {
    (12..=128)
        .contains(&value.chars().count())
        .then_some(())
        .ok_or(DomainError::InvalidPassword)
}
```

Create `crates/domain/src/rbac.rs`:

```rust
use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::error::DomainError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    Editor,
    Runner,
    Viewer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    ViewOrganization,
    EditWorkflows,
    PublishWorkflows,
    RunWorkflows,
    ManageMembers,
    ManageServiceTokens,
    ManageSecrets,
    ManagePlugins,
    EnableShellExecutor,
}

impl Role {
    #[must_use]
    pub const fn allows(self, capability: Capability) -> bool {
        match self {
            Self::Admin => true,
            Self::Editor => matches!(
                capability,
                Capability::ViewOrganization
                    | Capability::EditWorkflows
                    | Capability::PublishWorkflows
                    | Capability::RunWorkflows
            ),
            Self::Runner => matches!(
                capability,
                Capability::ViewOrganization | Capability::RunWorkflows
            ),
            Self::Viewer => matches!(capability, Capability::ViewOrganization),
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Editor => "editor",
            Self::Runner => "runner",
            Self::Viewer => "viewer",
        }
    }
}

impl FromStr for Role {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "admin" => Ok(Self::Admin),
            "editor" => Ok(Self::Editor),
            "runner" => Ok(Self::Runner),
            "viewer" => Ok(Self::Viewer),
            _ => Err(DomainError::InvalidRole),
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
```

Update `crates/domain/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod account;
pub mod error;
pub mod rbac;
```

- [ ] **Step 4: Verify GREEN**

Run:

```text
cargo test -p corqty-domain
```

Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```text
git add crates/domain
git commit -m "feat: define account validation and fixed RBAC"
```

## Task 3: Bootstrap PostgreSQL roles, migrations, and isolated test support

**Files:**

- Create: `docker-compose.test.yml`
- Create: `docker/postgres/init/001-roles.sh`
- Create: `migrations/202607280001_base.sql`
- Create: `crates/persistence/src/migrate.rs`
- Modify: `crates/persistence/src/lib.rs`
- Modify: `crates/migrator/src/main.rs`
- Modify: `crates/test-support/src/lib.rs`
- Test: `crates/persistence/tests/migrations.rs`

- [ ] **Step 1: Add test-only PostgreSQL configuration**

Create `docker-compose.test.yml`:

```yaml
services:
  postgres-test:
    image: postgres:18-alpine
    environment:
      POSTGRES_DB: corqty_test
      POSTGRES_USER: corqty_owner
      POSTGRES_PASSWORD: corqty_owner_dev
      CORQTY_API_DB_PASSWORD: corqty_api_dev
      CORQTY_WORKER_DB_PASSWORD: corqty_worker_dev
    ports:
      - "55432:5432"
    volumes:
      - ./docker/postgres/init:/docker-entrypoint-initdb.d:ro
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U corqty_owner -d corqty_test"]
      interval: 1s
      timeout: 3s
      retries: 30
```

Create `docker/postgres/init/001-roles.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

psql \
  --username "$POSTGRES_USER" \
  --dbname "$POSTGRES_DB" \
  --set=api_password="$CORQTY_API_DB_PASSWORD" \
  --set=worker_password="$CORQTY_WORKER_DB_PASSWORD" <<'SQL'
SELECT format(
  'CREATE ROLE corqty_api LOGIN PASSWORD %L NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS',
  :'api_password'
)
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'corqty_api') \gexec

SELECT format(
  'CREATE ROLE corqty_worker LOGIN PASSWORD %L NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS',
  :'worker_password'
)
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'corqty_worker') \gexec

SELECT format(
  'GRANT CONNECT ON DATABASE %I TO corqty_api, corqty_worker',
  current_database()
) \gexec
SQL
```

Run:

```text
git add --chmod=+x docker/postgres/init/001-roles.sh
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` becomes healthy.

- [ ] **Step 2: Write the failing migration smoke test**

Create `crates/persistence/tests/migrations.rs`:

```rust
use corqty_test_support::TestDatabase;
use serial_test::serial;
use sqlx::Row;

#[tokio::test]
#[serial]
async fn migrations_create_foundation_tables() {
    let database = TestDatabase::reset().await;
    let row = sqlx::query(
        "SELECT to_regclass('public.users') AS users,
                to_regclass('public.organizations') AS organizations,
                to_regclass('public.audit_events') AS audit_events",
    )
    .fetch_one(&database.owner)
    .await
    .unwrap();

    assert_eq!(row.get::<String, _>("users"), "users");
    assert_eq!(
        row.get::<String, _>("organizations"),
        "organizations"
    );
    assert_eq!(row.get::<String, _>("audit_events"), "audit_events");
}
```

- [ ] **Step 3: Run the test and verify RED**

Run:

```text
cargo test -p corqty-persistence --test migrations
```

Expected: compilation fails because `TestDatabase` and the migrator do not exist.

- [ ] **Step 4: Implement test database reset and migration runner**

Replace `crates/test-support/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

use sqlx::{PgPool, postgres::PgPoolOptions};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

pub struct TestDatabase {
    pub owner: PgPool,
    pub api: PgPool,
}

impl TestDatabase {
    pub async fn reset() -> Self {
        let owner = PgPoolOptions::new()
            .max_connections(5)
            .connect("postgres://corqty_owner:corqty_owner_dev@localhost:55432/corqty_test")
            .await
            .expect("connect owner test database");
        MIGRATOR.run(&owner).await.expect("run test migrations");

        sqlx::query(
            "TRUNCATE TABLE
                audit_events,
                invitations,
                memberships,
                organizations,
                sessions,
                password_credentials,
                users
             RESTART IDENTITY CASCADE",
        )
        .execute(&owner)
        .await
        .expect("reset test data");

        let api = PgPoolOptions::new()
            // A single API connection makes the context-leak test prove that
            // SET LOCAL values disappear before this connection is reused.
            .max_connections(1)
            .connect("postgres://corqty_api:corqty_api_dev@localhost:55432/corqty_test")
            .await
            .expect("connect api test database");

        Self { owner, api }
    }
}
```

Create `crates/persistence/src/migrate.rs`:

```rust
use sqlx::PgPool;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

pub async fn run(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    MIGRATOR.run(pool).await
}
```

Replace `crates/persistence/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod migrate;
```

Replace `crates/migrator/src/main.rs`:

```rust
#![forbid(unsafe_code)]

use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_URL_OWNER")
        .expect("DATABASE_URL_OWNER must be set");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await?;
    corqty_persistence::migrate::run(&pool).await?;
    Ok(())
}
```

- [ ] **Step 5: Add the complete base migration**

Create `migrations/202607280001_base.sql`:

```sql
CREATE TABLE users (
    id uuid PRIMARY KEY,
    email text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT users_email_normalized CHECK (email = lower(trim(email)))
);
CREATE UNIQUE INDEX users_email_unique ON users (email);

CREATE TABLE password_credentials (
    user_id uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    password_hash text NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE sessions (
    id uuid PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash bytea NOT NULL UNIQUE,
    csrf_hash bytea NOT NULL,
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    last_seen_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX sessions_user_id_idx ON sessions(user_id);
CREATE INDEX sessions_expires_at_idx ON sessions(expires_at);

CREATE TABLE organizations (
    id uuid PRIMARY KEY,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    slug text NOT NULL UNIQUE,
    created_by uuid NOT NULL REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE memberships (
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role text NOT NULL CHECK (role IN ('admin', 'editor', 'runner', 'viewer')),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (organization_id, user_id)
);
CREATE INDEX memberships_user_id_idx ON memberships(user_id);

CREATE TABLE invitations (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    email text NOT NULL,
    role text NOT NULL CHECK (role IN ('admin', 'editor', 'runner', 'viewer')),
    token_hash bytea NOT NULL UNIQUE,
    invited_by uuid NOT NULL REFERENCES users(id),
    expires_at timestamptz NOT NULL,
    accepted_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT invitations_email_normalized CHECK (email = lower(trim(email)))
);
CREATE INDEX invitations_organization_id_idx ON invitations(organization_id);

CREATE TABLE audit_events (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE RESTRICT,
    actor_user_id uuid REFERENCES users(id) ON DELETE SET NULL,
    action text NOT NULL,
    resource_type text NOT NULL,
    resource_id text NOT NULL,
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    occurred_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX audit_events_org_cursor_idx
    ON audit_events(organization_id, id DESC);

GRANT USAGE ON SCHEMA public TO corqty_api;
GRANT SELECT, INSERT, UPDATE ON users, password_credentials, sessions TO corqty_api;
GRANT DELETE ON sessions TO corqty_api;
GRANT SELECT, INSERT, UPDATE, DELETE
    ON organizations, memberships, invitations
    TO corqty_api;
GRANT SELECT, INSERT ON audit_events TO corqty_api;
GRANT USAGE, SELECT ON SEQUENCE audit_events_id_seq TO corqty_api;
```

- [ ] **Step 6: Verify GREEN and migration idempotency**

Run:

```text
cargo test -p corqty-persistence --test migrations
$env:DATABASE_URL_OWNER="postgres://corqty_owner:corqty_owner_dev@localhost:55432/corqty_test"
cargo run -p corqty-migrator
cargo run -p corqty-migrator
```

Expected: the test passes and both migrator runs exit 0.

- [ ] **Step 7: Commit**

```text
git add docker-compose.test.yml docker/postgres migrations crates/persistence crates/migrator crates/test-support
git commit -m "feat: add PostgreSQL foundation migrations"
```

## Task 4: Enforce tenant isolation with transaction-local RLS context

**Files:**

- Create: `migrations/202607280002_rls.sql`
- Create: `crates/persistence/src/tenant.rs`
- Modify: `crates/persistence/src/lib.rs`
- Test: `crates/persistence/tests/tenant_isolation.rs`

- [ ] **Step 1: Write the failing cross-tenant test**

Create `crates/persistence/tests/tenant_isolation.rs`:

```rust
use corqty_persistence::tenant::begin_tenant;
use corqty_test_support::TestDatabase;
use serial_test::serial;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn api_role_cannot_read_another_organization() {
    let database = TestDatabase::reset().await;
    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();
    let org_a = Uuid::new_v4();
    let org_b = Uuid::new_v4();

    for (user, email) in [(user_a, "a@example.com"), (user_b, "b@example.com")] {
        sqlx::query("INSERT INTO users(id, email) VALUES ($1, $2)")
            .bind(user)
            .bind(email)
            .execute(&database.owner)
            .await
            .unwrap();
    }
    for (org, user, slug) in [(org_a, user_a, "a"), (org_b, user_b, "b")] {
        sqlx::query(
            "INSERT INTO organizations(id, name, slug, created_by)
             VALUES ($1, 'Test', $2, $3)",
        )
        .bind(org)
        .bind(slug)
        .bind(user)
        .execute(&database.owner)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships(organization_id, user_id, role)
             VALUES ($1, $2, 'admin')",
        )
        .bind(org)
        .bind(user)
        .execute(&database.owner)
        .await
        .unwrap();
    }

    let mut tx = begin_tenant(&database.api, user_a, org_a).await.unwrap();
    let rows = sqlx::query("SELECT id FROM organizations ORDER BY id")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<Uuid, _>("id"), org_a);
}
```

- [ ] **Step 2: Run the test and verify RED**

Run:

```text
cargo test -p corqty-persistence --test tenant_isolation
```

Expected: compilation fails because `begin_tenant` does not exist.

- [ ] **Step 3: Implement tenant transaction setup**

Create `crates/persistence/src/tenant.rs`:

```rust
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

pub async fn begin_tenant(
    pool: &PgPool,
    user_id: Uuid,
    organization_id: Uuid,
) -> Result<Transaction<'_, Postgres>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "SELECT
            set_config('corqty.user_id', $1, true),
            set_config('corqty.organization_id', $2, true)",
    )
    .bind(user_id.to_string())
    .bind(organization_id.to_string())
    .execute(&mut *tx)
    .await?;
    Ok(tx)
}

pub async fn begin_user(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Transaction<'_, Postgres>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('corqty.user_id', $1, true)")
        .bind(user_id.to_string())
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}
```

Add to `crates/persistence/src/lib.rs`:

```rust
pub mod tenant;
```

- [ ] **Step 4: Add RLS functions and policies**

Create `migrations/202607280002_rls.sql`:

```sql
CREATE SCHEMA corqty;
REVOKE ALL ON SCHEMA corqty FROM PUBLIC;
GRANT USAGE ON SCHEMA corqty TO corqty_api;

CREATE FUNCTION corqty.current_user_id()
RETURNS uuid
LANGUAGE sql
STABLE
AS $$
    SELECT NULLIF(current_setting('corqty.user_id', true), '')::uuid
$$;

CREATE FUNCTION corqty.current_organization_id()
RETURNS uuid
LANGUAGE sql
STABLE
AS $$
    SELECT NULLIF(current_setting('corqty.organization_id', true), '')::uuid
$$;

CREATE FUNCTION corqty.is_member(target_organization_id uuid)
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
    )
$$;

CREATE FUNCTION corqty.is_admin(target_organization_id uuid)
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
          AND role = 'admin'
    )
$$;

REVOKE ALL ON FUNCTION corqty.is_member(uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION corqty.is_admin(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.is_member(uuid) TO corqty_api;
GRANT EXECUTE ON FUNCTION corqty.is_admin(uuid) TO corqty_api;

CREATE FUNCTION corqty.is_organization_creator(target_organization_id uuid)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    SELECT EXISTS (
        SELECT 1
        FROM organizations
        WHERE id = target_organization_id
          AND created_by = corqty.current_user_id()
    )
$$;

REVOKE ALL ON FUNCTION corqty.is_organization_creator(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.is_organization_creator(uuid) TO corqty_api;

ALTER TABLE organizations ENABLE ROW LEVEL SECURITY;
ALTER TABLE organizations FORCE ROW LEVEL SECURITY;
CREATE POLICY organizations_select ON organizations
    FOR SELECT TO corqty_api
    USING (
        id = corqty.current_organization_id()
        AND corqty.is_member(id)
    );
CREATE POLICY organizations_insert ON organizations
    FOR INSERT TO corqty_api
    WITH CHECK (
        id = corqty.current_organization_id()
        AND created_by = corqty.current_user_id()
    );
CREATE POLICY organizations_update ON organizations
    FOR UPDATE TO corqty_api
    USING (
        id = corqty.current_organization_id()
        AND corqty.is_admin(id)
    )
    WITH CHECK (
        id = corqty.current_organization_id()
        AND corqty.is_admin(id)
    );
CREATE POLICY organizations_delete ON organizations
    FOR DELETE TO corqty_api
    USING (
        id = corqty.current_organization_id()
        AND corqty.is_admin(id)
    );

ALTER TABLE memberships ENABLE ROW LEVEL SECURITY;
ALTER TABLE memberships FORCE ROW LEVEL SECURITY;
CREATE POLICY memberships_select ON memberships
    FOR SELECT TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND corqty.is_member(organization_id)
    );
CREATE POLICY memberships_insert ON memberships
    FOR INSERT TO corqty_api
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND (
            corqty.is_admin(organization_id)
            OR (
                user_id = corqty.current_user_id()
                AND role = 'admin'
                AND corqty.is_organization_creator(organization_id)
            )
        )
    );
CREATE POLICY memberships_update ON memberships
    FOR UPDATE TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND corqty.is_admin(organization_id)
    )
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND corqty.is_admin(organization_id)
    );
CREATE POLICY memberships_delete ON memberships
    FOR DELETE TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND corqty.is_admin(organization_id)
    );

ALTER TABLE invitations ENABLE ROW LEVEL SECURITY;
ALTER TABLE invitations FORCE ROW LEVEL SECURITY;
CREATE POLICY invitations_admin ON invitations
    FOR ALL TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND corqty.is_admin(organization_id)
    )
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND corqty.is_admin(organization_id)
    );

ALTER TABLE audit_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_events FORCE ROW LEVEL SECURITY;
CREATE POLICY audit_events_select ON audit_events
    FOR SELECT TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND corqty.is_member(organization_id)
    );
CREATE POLICY audit_events_insert ON audit_events
    FOR INSERT TO corqty_api
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND corqty.is_member(organization_id)
    );
```

- [ ] **Step 5: Verify GREEN, role posture, and context cleanup**

Add this assertion to the test after the existing assertion:

```rust
let rows_without_context = sqlx::query("SELECT id FROM organizations")
    .fetch_all(&database.api)
    .await
    .unwrap();
assert!(rows_without_context.is_empty());

let (current_user, bypass_rls, superuser): (String, bool, bool) =
    sqlx::query_as(
        "SELECT rolname, rolbypassrls, rolsuper
         FROM pg_roles
         WHERE rolname = current_user",
    )
    .fetch_one(&database.api)
    .await
    .unwrap();
assert_eq!(current_user, "corqty_api");
assert!(!bypass_rls);
assert!(!superuser);

let table_owner: String = sqlx::query_scalar(
    "SELECT pg_get_userbyid(relowner)
     FROM pg_class
     WHERE oid = 'public.organizations'::regclass",
)
.fetch_one(&database.api)
.await
.unwrap();
assert_ne!(table_owner, current_user);
```

Run:

```text
cargo test -p corqty-persistence --test tenant_isolation
```

Expected: the raw unfiltered query sees only `org_a`; after rollback the same
single pooled connection sees no rows without context; and the API role is
neither the table owner, superuser, nor `BYPASSRLS`.

- [ ] **Step 6: Commit**

```text
git add migrations/202607280002_rls.sql crates/persistence
git commit -m "feat: enforce PostgreSQL tenant isolation"
```

## Task 5: Implement password and opaque-session cryptography

**Files:**

- Create: `crates/api/src/auth/mod.rs`
- Create: `crates/api/src/auth/crypto.rs`
- Create: `crates/api/src/error.rs`
- Modify: `crates/api/src/lib.rs`
- Test: `crates/api/src/auth/crypto.rs`

- [ ] **Step 1: Write failing crypto tests**

Create `crates/api/src/auth/crypto.rs` with only these tests:

```rust
#[cfg(test)]
mod tests {
    use super::{PasswordEngine, SecretToken};

    #[test]
    fn password_hash_verifies_only_the_original_password() {
        let hash = PasswordEngine::hash("correct horse battery staple").unwrap();
        assert!(PasswordEngine::verify("correct horse battery staple", &hash));
        assert!(!PasswordEngine::verify("wrong password", &hash));
    }

    #[test]
    fn generated_tokens_are_opaque_and_hashable() {
        let first = SecretToken::generate().unwrap();
        let second = SecretToken::generate().unwrap();

        assert_ne!(first.plaintext, second.plaintext);
        assert_eq!(first.hash.len(), 32);
        assert_eq!(second.hash.len(), 32);
    }
}
```

- [ ] **Step 2: Run the tests and verify RED**

Run:

```text
cargo test -p corqty-api auth::crypto::tests
```

Expected: compilation fails because `PasswordEngine` and `SecretToken` do not exist.

- [ ] **Step 3: Implement Argon2id and opaque tokens**

Add above the tests in `crates/api/src/auth/crypto.rs`:

```rust
use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

use crate::error::ApiError;

pub struct PasswordEngine;

impl PasswordEngine {
    pub fn hash(password: &str) -> Result<String, ApiError> {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|_| ApiError::Internal)
    }

    #[must_use]
    pub fn verify(password: &str, encoded_hash: &str) -> bool {
        let Ok(parsed) = PasswordHash::new(encoded_hash) else {
            return false;
        };
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    }
}

#[derive(Debug)]
pub struct SecretToken {
    pub plaintext: String,
    pub hash: Vec<u8>,
}

impl SecretToken {
    pub fn generate() -> Result<Self, ApiError> {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| ApiError::Internal)?;
        let plaintext = URL_SAFE_NO_PAD.encode(bytes);
        Ok(Self {
            hash: Self::hash(&plaintext),
            plaintext,
        })
    }

    #[must_use]
    pub fn hash(plaintext: &str) -> Vec<u8> {
        Sha256::digest(plaintext.as_bytes()).to_vec()
    }
}
```

Create `crates/api/src/error.rs`:

```rust
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("conflict")]
    Conflict(&'static str),
    #[error("validation failed")]
    Validation(&'static str),
    #[error("internal error")]
    Internal,
}

#[derive(Serialize)]
struct Problem {
    #[serde(rename = "type")]
    kind: &'static str,
    title: &'static str,
    status: u16,
    code: &'static str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, title, code) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized", "unauthorized"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "Forbidden", "forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "Not found", "not_found"),
            Self::Conflict(code) => (StatusCode::CONFLICT, "Conflict", code),
            Self::Validation(code) => {
                (StatusCode::UNPROCESSABLE_ENTITY, "Validation failed", code)
            }
            Self::Internal => {
                tracing::error!("request failed with an internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal error", "internal")
            }
        };
        (
            status,
            [(
                axum::http::header::CONTENT_TYPE,
                "application/problem+json",
            )],
            Json(Problem {
                kind: "about:blank",
                title,
                status: status.as_u16(),
                code,
            }),
        )
            .into_response()
    }
}
```

Create `crates/api/src/auth/mod.rs`:

```rust
pub mod crypto;
```

Replace `crates/api/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod auth;
pub mod error;
```

- [ ] **Step 4: Verify GREEN**

Run:

```text
cargo test -p corqty-api auth::crypto::tests
```

Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```text
git add crates/api/src
git commit -m "feat: add password and session token cryptography"
```

## Task 6: Persist registration, login lookup, and sessions

**Files:**

- Create: `crates/persistence/src/models.rs`
- Create: `crates/persistence/src/store.rs`
- Create: `crates/persistence/src/auth.rs`
- Modify: `crates/persistence/src/lib.rs`
- Test: `crates/persistence/tests/auth_store.rs`

- [ ] **Step 1: Write failing persistence tests**

Create `crates/persistence/tests/auth_store.rs`:

```rust
use chrono::{Duration, Utc};
use corqty_persistence::{
    auth::{NewRegistration, NewSession},
    store::PgStore,
};
use corqty_test_support::TestDatabase;
use serial_test::serial;
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn registration_creates_the_first_admin_and_session() {
    let database = TestDatabase::reset().await;
    let store = PgStore::new(database.api.clone());
    let user_id = Uuid::new_v4();
    let organization_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();

    let result = store
        .register(
            NewRegistration {
                user_id,
                email: "admin@example.com".to_owned(),
                password_hash: "argon-hash".to_owned(),
                organization_id,
                organization_name: "Acme Cloud".to_owned(),
                organization_slug: "acme-cloud-a1b2c3".to_owned(),
            },
            NewSession {
                session_id,
                token_hash: vec![1; 32],
                csrf_hash: vec![2; 32],
                expires_at: Utc::now() + Duration::days(30),
            },
        )
        .await
        .unwrap();

    assert_eq!(result.user_id, user_id);
    assert_eq!(result.organization_id, organization_id);
    assert_eq!(result.role, "admin");

    let session = store
        .session_by_token_hash(&[1; 32])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(session.user_id, user_id);
    assert_eq!(session.email, "admin@example.com");
}

#[tokio::test]
#[serial]
async fn duplicate_registration_returns_conflict() {
    let database = TestDatabase::reset().await;
    let store = PgStore::new(database.api.clone());

    for suffix in [1_u8, 2_u8] {
        let result = store
            .register(
                NewRegistration {
                    user_id: Uuid::new_v4(),
                    email: "same@example.com".to_owned(),
                    password_hash: "argon-hash".to_owned(),
                    organization_id: Uuid::new_v4(),
                    organization_name: "Acme".to_owned(),
                    organization_slug: format!("acme-{suffix}"),
                },
                NewSession {
                    session_id: Uuid::new_v4(),
                    token_hash: vec![suffix; 32],
                    csrf_hash: vec![suffix + 10; 32],
                    expires_at: Utc::now() + Duration::days(30),
                },
            )
            .await;

        if suffix == 1 {
            assert!(result.is_ok());
        } else {
            assert!(result.is_err());
        }
    }
}
```

- [ ] **Step 2: Run the tests and verify RED**

Run:

```text
cargo test -p corqty-persistence --test auth_store
```

Expected: compilation fails because `PgStore`, `NewRegistration`, and `NewSession` do not exist.

- [ ] **Step 3: Implement shared persistence records**

Create `crates/persistence/src/models.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct SessionRecord {
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub email: String,
    pub csrf_hash: Vec<u8>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummaryRecord {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationMembershipRecord {
    pub organization_id: Uuid,
    pub name: String,
    pub slug: String,
    pub role: String,
}

#[derive(Debug)]
pub struct RegistrationRecord {
    pub user_id: Uuid,
    pub organization_id: Uuid,
    pub role: String,
}
```

Create `crates/persistence/src/store.rs`:

```rust
use sqlx::PgPool;

#[derive(Clone)]
pub struct PgStore {
    pub(crate) pool: PgPool,
}

impl PgStore {
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }
}
```

- [ ] **Step 4: Implement registration and session queries**

Create `crates/persistence/src/auth.rs`:

```rust
use chrono::{DateTime, Utc};
use sqlx::Error;
use uuid::Uuid;

use crate::{
    models::{RegistrationRecord, SessionRecord},
    store::PgStore,
    tenant::begin_tenant,
};

pub struct NewRegistration {
    pub user_id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub organization_id: Uuid,
    pub organization_name: String,
    pub organization_slug: String,
}

pub struct NewSession {
    pub session_id: Uuid,
    pub token_hash: Vec<u8>,
    pub csrf_hash: Vec<u8>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct LoginRecord {
    pub user_id: Uuid,
    pub email: String,
    pub password_hash: String,
}

impl PgStore {
    pub async fn register(
        &self,
        registration: NewRegistration,
        session: NewSession,
    ) -> Result<RegistrationRecord, Error> {
        let mut tx = begin_tenant(
            &self.pool,
            registration.user_id,
            registration.organization_id,
        )
        .await?;

        sqlx::query("INSERT INTO users(id, email) VALUES ($1, $2)")
            .bind(registration.user_id)
            .bind(&registration.email)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO password_credentials(user_id, password_hash)
             VALUES ($1, $2)",
        )
        .bind(registration.user_id)
        .bind(&registration.password_hash)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO organizations(id, name, slug, created_by)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(registration.organization_id)
        .bind(&registration.organization_name)
        .bind(&registration.organization_slug)
        .bind(registration.user_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO memberships(organization_id, user_id, role)
             VALUES ($1, $2, 'admin')",
        )
        .bind(registration.organization_id)
        .bind(registration.user_id)
        .execute(&mut *tx)
        .await?;
        insert_session(&mut tx, registration.user_id, session).await?;
        tx.commit().await?;

        Ok(RegistrationRecord {
            user_id: registration.user_id,
            organization_id: registration.organization_id,
            role: "admin".to_owned(),
        })
    }

    pub async fn login_record(&self, email: &str) -> Result<Option<LoginRecord>, Error> {
        sqlx::query_as(
            "SELECT users.id AS user_id, users.email, password_credentials.password_hash
             FROM users
             JOIN password_credentials ON password_credentials.user_id = users.id
             WHERE users.email = $1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn create_session(
        &self,
        user_id: Uuid,
        session: NewSession,
    ) -> Result<(), Error> {
        let mut tx = self.pool.begin().await?;
        insert_session(&mut tx, user_id, session).await?;
        tx.commit().await
    }

    pub async fn session_by_token_hash(
        &self,
        token_hash: &[u8],
    ) -> Result<Option<SessionRecord>, Error> {
        sqlx::query_as(
            "SELECT sessions.id AS session_id, sessions.user_id, users.email,
                    sessions.csrf_hash, sessions.expires_at
             FROM sessions
             JOIN users ON users.id = sessions.user_id
             WHERE sessions.token_hash = $1
               AND sessions.expires_at > now()",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn delete_session(&self, session_id: Uuid) -> Result<(), Error> {
        sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_sessions(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<crate::models::SessionSummaryRecord>, Error> {
        sqlx::query_as(
            "SELECT id, created_at, last_seen_at, expires_at
             FROM sessions
             WHERE user_id = $1 AND expires_at > now()
             ORDER BY created_at DESC, id",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn change_password_and_revoke_other_sessions(
        &self,
        user_id: Uuid,
        current_session_id: Uuid,
        password_hash: &str,
    ) -> Result<(), Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE password_credentials
             SET password_hash = $2, updated_at = now()
             WHERE user_id = $1",
        )
        .bind(user_id)
        .bind(password_hash)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM sessions
             WHERE user_id = $1 AND id <> $2",
        )
        .bind(user_id)
        .bind(current_session_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }

    pub async fn revoke_user_session(
        &self,
        user_id: Uuid,
        session_id: Uuid,
    ) -> Result<bool, Error> {
        let affected = sqlx::query(
            "DELETE FROM sessions WHERE id = $1 AND user_id = $2",
        )
        .bind(session_id)
        .bind(user_id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(affected == 1)
    }
}

async fn insert_session(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    session: NewSession,
) -> Result<(), Error> {
    sqlx::query(
        "INSERT INTO sessions(id, user_id, token_hash, csrf_hash, expires_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(session.session_id)
    .bind(user_id)
    .bind(session.token_hash)
    .bind(session.csrf_hash)
    .bind(session.expires_at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
```

Update `crates/persistence/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod auth;
pub mod migrate;
pub mod models;
pub mod store;
pub mod tenant;
```

- [ ] **Step 5: Verify GREEN**

Run:

```text
cargo test -p corqty-persistence --test auth_store
```

Expected: 2 tests pass.

- [ ] **Step 6: Commit**

```text
git add crates/persistence
git commit -m "feat: persist registration and sessions"
```

## Task 7: Expose registration, login, session auth, CSRF, and logout

**Files:**

- Create: `crates/api/src/config.rs`
- Create: `crates/api/src/state.rs`
- Create: `crates/api/src/router.rs`
- Create: `crates/api/src/auth/cookies.rs`
- Create: `crates/api/src/auth/middleware.rs`
- Create: `crates/api/src/auth/service.rs`
- Create: `crates/api/src/auth/handlers.rs`
- Create: `migrations/202607280003_user_organizations.sql`
- Modify: `crates/api/src/auth/mod.rs`
- Modify: `crates/api/src/lib.rs`
- Create: `crates/api/src/main.rs`
- Create: `crates/api/tests/support/mod.rs`
- Test: `crates/api/tests/auth_http.rs`

- [ ] **Step 1: Write a failing HTTP integration test**

Create `crates/api/tests/auth_http.rs`:

```rust
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use corqty_api::{config::ApiConfig, router::build_router, state::AppState};
use corqty_persistence::store::PgStore;
use corqty_test_support::TestDatabase;
use serial_test::serial;
use tower::ServiceExt;

#[tokio::test]
#[serial]
async fn register_sets_session_and_me_authenticates_it() {
    let database = TestDatabase::reset().await;
    let state = AppState::new(
        PgStore::new(database.api),
        ApiConfig::for_tests(),
    );
    let app = build_router(state);
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/auth/register")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"email":"admin@example.com","password":"correct horse battery staple","organizationName":"Acme"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let cookies: Vec<_> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap().to_owned())
        .collect();
    let session = cookies
        .iter()
        .find(|cookie| cookie.starts_with("corqty_session="))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();

    let me = app
        .oneshot(
            Request::get("/api/v1/auth/me")
                .header(header::COOKIE, session)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(me.status(), StatusCode::OK);
    let body = to_bytes(me.into_body(), 64 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["user"]["email"], "admin@example.com");
    assert_eq!(json["organizations"][0]["role"], "admin");
}

#[tokio::test]
#[serial]
async fn mutating_session_route_rejects_missing_csrf_header() {
    let database = TestDatabase::reset().await;
    let state = AppState::new(
        PgStore::new(database.api),
        ApiConfig::for_tests(),
    );
    let app = build_router(state);

    let registration = app
        .clone()
        .oneshot(
            Request::post("/api/v1/auth/register")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"email":"admin@example.com","password":"correct horse battery staple","organizationName":"Acme"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let cookies: Vec<_> = registration
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| {
            value
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .to_owned()
        })
        .collect();
    let cookie_header = cookies.join("; ");
    let csrf = cookies
        .iter()
        .find_map(|cookie| cookie.strip_prefix("corqty_csrf="))
        .unwrap()
        .to_owned();

    let rejected = app
        .clone()
        .oneshot(
            Request::post("/api/v1/auth/logout")
                .header(header::COOKIE, &cookie_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);

    let accepted = app
        .oneshot(
            Request::post("/api/v1/auth/logout")
                .header(header::COOKIE, cookie_header)
                .header("x-csrf-token", csrf)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::NO_CONTENT);
}
```

- [ ] **Step 2: Run the test and verify RED**

Run:

```text
cargo test -p corqty-api --test auth_http
```

Expected: compilation fails because `ApiConfig`, `AppState`, and `build_router` do not exist.

- [ ] **Step 3: Implement API config, state, and cookie helpers**

Create `crates/api/src/config.rs`:

```rust
#[derive(Clone)]
pub struct ApiConfig {
    pub bind_address: String,
    pub database_url: String,
    pub public_web_url: String,
    pub secure_cookies: bool,
    pub session_days: i64,
}

impl ApiConfig {
    pub fn from_env() -> Result<Self, std::env::VarError> {
        Ok(Self {
            bind_address: std::env::var("CORQTY_BIND_ADDRESS")
                .unwrap_or_else(|_| "0.0.0.0:8080".to_owned()),
            database_url: std::env::var("DATABASE_URL_API")?,
            public_web_url: std::env::var("CORQTY_PUBLIC_WEB_URL")
                .unwrap_or_else(|_| "http://localhost:5173".to_owned()),
            secure_cookies: std::env::var("CORQTY_SECURE_COOKIES")
                .map_or(true, |value| value != "false"),
            session_days: 30,
        })
    }

    #[must_use]
    pub fn for_tests() -> Self {
        Self {
            bind_address: "127.0.0.1:0".to_owned(),
            database_url: String::new(),
            public_web_url: "http://localhost:5173".to_owned(),
            secure_cookies: false,
            session_days: 30,
        }
    }
}
```

Create `crates/api/src/state.rs`:

```rust
use std::sync::Arc;

use corqty_persistence::store::PgStore;

use crate::{auth::crypto::PasswordEngine, config::ApiConfig};

#[derive(Clone)]
pub struct AppState {
    pub store: PgStore,
    pub config: ApiConfig,
    pub dummy_password_hash: Arc<str>,
}

impl AppState {
    #[must_use]
    pub fn new(store: PgStore, config: ApiConfig) -> Self {
        Self {
            store,
            config,
            dummy_password_hash: PasswordEngine::hash(
                "corqty dummy password value",
            )
            .expect("dummy password hash")
            .into(),
        }
    }
}
```

Create `crates/api/src/auth/cookies.rs`:

```rust
use axum::http::{HeaderMap, HeaderValue, header};

use crate::{config::ApiConfig, error::ApiError};

pub const SESSION_COOKIE: &str = "corqty_session";
pub const CSRF_COOKIE: &str = "corqty_csrf";

pub fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")).map(str::to_owned))
}

pub fn append_auth_cookies(
    headers: &mut HeaderMap,
    config: &ApiConfig,
    session: &str,
    csrf: &str,
) -> Result<(), ApiError> {
    let secure = if config.secure_cookies { "; Secure" } else { "" };
    for value in [
        format!(
            "{SESSION_COOKIE}={session}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{secure}",
            config.session_days * 86_400
        ),
        format!(
            "{CSRF_COOKIE}={csrf}; Path=/; SameSite=Lax; Max-Age={}{secure}",
            config.session_days * 86_400
        ),
    ] {
        headers.append(
            header::SET_COOKIE,
            HeaderValue::from_str(&value).map_err(|_| ApiError::Internal)?,
        );
    }
    Ok(())
}

pub fn clear_auth_cookies(headers: &mut HeaderMap, config: &ApiConfig) {
    let secure = if config.secure_cookies { "; Secure" } else { "" };
    for name in [SESSION_COOKIE, CSRF_COOKIE] {
        let value = format!(
            "{name}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}"
        );
        headers.append(
            header::SET_COOKIE,
            HeaderValue::from_str(&value).expect("valid clear-cookie header"),
        );
    }
}
```

- [ ] **Step 4: Implement auth service and request middleware**

Create `crates/api/src/auth/service.rs`:

```rust
use chrono::{Duration, Utc};
use corqty_domain::account::{
    NormalizedEmail, OrganizationName, validate_password,
};
use corqty_persistence::{
    auth::{NewRegistration, NewSession},
    models::{OrganizationMembershipRecord, SessionRecord},
};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    auth::crypto::{PasswordEngine, SecretToken},
    error::ApiError,
    state::AppState,
};

pub struct IssuedSession {
    pub session_token: String,
    pub csrf_token: String,
}

#[derive(Serialize)]
pub struct UserView {
    pub id: Uuid,
    pub email: String,
}

#[derive(Serialize)]
pub struct MeView {
    pub user: UserView,
    pub organizations: Vec<OrganizationMembershipRecord>,
}

pub async fn register(
    state: &AppState,
    email: &str,
    password: &str,
    organization_name: &str,
) -> Result<IssuedSession, ApiError> {
    let email = NormalizedEmail::parse(email)
        .map_err(|_| ApiError::Validation("invalid_email"))?;
    validate_password(password)
        .map_err(|_| ApiError::Validation("invalid_password"))?;
    let organization_name = OrganizationName::parse(organization_name)
        .map_err(|_| ApiError::Validation("invalid_organization_name"))?;
    let password_owned = password.to_owned();
    let password_hash = tokio::task::spawn_blocking(move || {
        PasswordEngine::hash(&password_owned)
    })
    .await
    .map_err(|_| ApiError::Internal)??;
    let session_token = SecretToken::generate()?;
    let csrf_token = SecretToken::generate()?;
    let user_id = Uuid::new_v4();
    let organization_id = Uuid::new_v4();
    let slug_prefix: String = organization_name
        .as_str()
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let organization_slug = format!(
        "{}-{}",
        if slug_prefix.is_empty() { "org" } else { &slug_prefix },
        &organization_id.simple().to_string()[..8]
    );

    state
        .store
        .register(
            NewRegistration {
                user_id,
                email: email.as_str().to_owned(),
                password_hash,
                organization_id,
                organization_name: organization_name.as_str().to_owned(),
                organization_slug,
            },
            NewSession {
                session_id: Uuid::new_v4(),
                token_hash: session_token.hash,
                csrf_hash: csrf_token.hash,
                expires_at: Utc::now() + Duration::days(state.config.session_days),
            },
        )
        .await
        .map_err(map_database_error)?;

    Ok(IssuedSession {
        session_token: session_token.plaintext,
        csrf_token: csrf_token.plaintext,
    })
}

pub async fn login(
    state: &AppState,
    email: &str,
    password: &str,
) -> Result<IssuedSession, ApiError> {
    let record = match NormalizedEmail::parse(email) {
        Ok(email) => state
            .store
            .login_record(email.as_str())
            .await
            .map_err(|_| ApiError::Internal)?,
        Err(_) => None,
    };
    let password_owned = password.to_owned();
    let password_hash = match &record {
        Some(record) => record.password_hash.clone(),
        None => state.dummy_password_hash.to_string(),
    };
    let valid = tokio::task::spawn_blocking(move || {
        PasswordEngine::verify(&password_owned, &password_hash)
    })
    .await
    .map_err(|_| ApiError::Internal)?;
    let record = record.filter(|_| valid).ok_or(ApiError::Unauthorized)?;

    let session_token = SecretToken::generate()?;
    let csrf_token = SecretToken::generate()?;
    state
        .store
        .create_session(
            record.user_id,
            NewSession {
                session_id: Uuid::new_v4(),
                token_hash: session_token.hash,
                csrf_hash: csrf_token.hash,
                expires_at: Utc::now() + Duration::days(state.config.session_days),
            },
        )
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(IssuedSession {
        session_token: session_token.plaintext,
        csrf_token: csrf_token.plaintext,
    })
}

pub async fn me(
    state: &AppState,
    session: &SessionRecord,
) -> Result<MeView, ApiError> {
    let organizations = state
        .store
        .organizations_for_user(session.user_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(MeView {
        user: UserView {
            id: session.user_id,
            email: session.email.clone(),
        },
        organizations,
    })
}

pub async fn change_password(
    state: &AppState,
    session: &SessionRecord,
    current_password: &str,
    new_password: &str,
) -> Result<(), ApiError> {
    validate_password(new_password)
        .map_err(|_| ApiError::Validation("invalid_password"))?;
    let login = state
        .store
        .login_record(&session.email)
        .await
        .map_err(|_| ApiError::Internal)?
        .ok_or(ApiError::Unauthorized)?;
    let current_owned = current_password.to_owned();
    let current_hash = login.password_hash;
    let valid = tokio::task::spawn_blocking(move || {
        PasswordEngine::verify(&current_owned, &current_hash)
    })
    .await
    .map_err(|_| ApiError::Internal)?;
    if !valid {
        return Err(ApiError::Unauthorized);
    }
    let new_owned = new_password.to_owned();
    let new_hash = tokio::task::spawn_blocking(move || {
        PasswordEngine::hash(&new_owned)
    })
    .await
    .map_err(|_| ApiError::Internal)??;
    state
        .store
        .change_password_and_revoke_other_sessions(
            session.user_id,
            session.session_id,
            &new_hash,
        )
        .await
        .map_err(|_| ApiError::Internal)
}

fn map_database_error(error: sqlx::Error) -> ApiError {
    if error
        .as_database_error()
        .is_some_and(|database| database.code().as_deref() == Some("23505"))
    {
        ApiError::Conflict("email_already_registered")
    } else {
        ApiError::Internal
    }
}
```

Create `crates/api/src/auth/middleware.rs`:

```rust
use axum::{
    extract::{Request, State},
    http::{Method, header},
    middleware::Next,
    response::Response,
};
use corqty_persistence::models::SessionRecord;

use crate::{
    auth::{
        cookies::{CSRF_COOKIE, SESSION_COOKIE, read_cookie},
        crypto::SecretToken,
    },
    error::ApiError,
    state::AppState,
};

pub async fn authenticate(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let token = read_cookie(request.headers(), SESSION_COOKIE)
        .ok_or(ApiError::Unauthorized)?;
    let session = state
        .store
        .session_by_token_hash(&SecretToken::hash(&token))
        .await
        .map_err(|_| ApiError::Internal)?
        .ok_or(ApiError::Unauthorized)?;
    request.extensions_mut().insert(session);
    Ok(next.run(request).await)
}

pub async fn require_csrf(
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    if matches!(
        *request.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) {
        let session = request
            .extensions()
            .get::<SessionRecord>()
            .ok_or(ApiError::Unauthorized)?;
        let cookie = read_cookie(request.headers(), CSRF_COOKIE)
            .ok_or(ApiError::Forbidden)?;
        let header_token = request
            .headers()
            .get("x-csrf-token")
            .and_then(|value| value.to_str().ok())
            .ok_or(ApiError::Forbidden)?;
        if cookie != header_token
            || SecretToken::hash(header_token) != session.csrf_hash
        {
            return Err(ApiError::Forbidden);
        }
    }
    Ok(next.run(request).await)
}
```

- [ ] **Step 5: Implement handlers and router**

Create `crates/api/src/auth/handlers.rs`:

```rust
use axum::{
    Extension, Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use corqty_persistence::models::SessionRecord;
use serde::Deserialize;

use crate::{
    auth::{
        cookies::{append_auth_cookies, clear_auth_cookies},
        service,
    },
    error::ApiError,
    state::AppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRequest {
    email: String,
    password: String,
    organization_name: String,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

pub async fn register(
    State(state): State<AppState>,
    Json(request): Json<RegisterRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let session = service::register(
        &state,
        &request.email,
        &request.password,
        &request.organization_name,
    )
    .await?;
    let mut headers = HeaderMap::new();
    append_auth_cookies(
        &mut headers,
        &state.config,
        &session.session_token,
        &session.csrf_token,
    )?;
    Ok((StatusCode::CREATED, headers, Json(serde_json::json!({"ok": true}))))
}

pub async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let session = service::login(&state, &request.email, &request.password).await?;
    let mut headers = HeaderMap::new();
    append_auth_cookies(
        &mut headers,
        &state.config,
        &session.session_token,
        &session.csrf_token,
    )?;
    Ok((headers, Json(serde_json::json!({"ok": true}))))
}

pub async fn me(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
) -> Result<Json<service::MeView>, ApiError> {
    Ok(Json(service::me(&state, &session).await?))
}

pub async fn logout(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .store
        .delete_session(session.session_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    let mut headers = HeaderMap::new();
    clear_auth_cookies(&mut headers, &state.config);
    Ok((StatusCode::NO_CONTENT, headers, ()))
}

pub async fn change_password(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    Json(request): Json<ChangePasswordRequest>,
) -> Result<StatusCode, ApiError> {
    service::change_password(
        &state,
        &session,
        &request.current_password,
        &request.new_password,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_sessions(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
) -> Result<Json<Vec<corqty_persistence::models::SessionSummaryRecord>>, ApiError> {
    let sessions = state
        .store
        .list_sessions(session.user_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(Json(sessions))
}

pub async fn revoke_session(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    axum::extract::Path(session_id): axum::extract::Path<uuid::Uuid>,
) -> Result<StatusCode, ApiError> {
    let removed = state
        .store
        .revoke_user_session(session.user_id, session_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    removed
        .then_some(StatusCode::NO_CONTENT)
        .ok_or(ApiError::NotFound)
}
```

Create `crates/api/src/router.rs`:

```rust
use axum::{
    Router,
    http::StatusCode,
    middleware,
    routing::{get, post},
};
use tower_http::trace::TraceLayer;

use crate::{
    auth::{handlers, middleware as auth_middleware},
    state::AppState,
};

pub fn build_router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/auth/me", get(handlers::me))
        .route("/auth/logout", post(handlers::logout))
        .route("/auth/password", axum::routing::patch(handlers::change_password))
        .route("/auth/sessions", get(handlers::list_sessions))
        .route(
            "/auth/sessions/{session_id}",
            axum::routing::delete(handlers::revoke_session),
        )
        .layer(middleware::from_fn(auth_middleware::require_csrf))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware::authenticate,
        ));
    let public = Router::new()
        .route("/auth/register", post(handlers::register))
        .route("/auth/login", post(handlers::login))
        .route("/health", get(|| async { StatusCode::NO_CONTENT }));

    Router::new()
        .nest("/api/v1", public.merge(protected))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
```

Replace `crates/api/src/auth/mod.rs`:

```rust
pub mod cookies;
pub mod crypto;
pub mod handlers;
pub mod middleware;
pub mod service;
```

Replace `crates/api/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod auth;
pub mod config;
pub mod error;
pub mod router;
pub mod state;
```

Create `crates/api/src/main.rs`:

```rust
#![forbid(unsafe_code)]

use corqty_api::{
    config::ApiConfig,
    router::build_router,
    state::AppState,
};
use corqty_persistence::store::PgStore;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "corqty_api=info,tower_http=info".into()),
        )
        .init();
    let config = ApiConfig::from_env()?;
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&config.database_url)
        .await?;
    let listener = tokio::net::TcpListener::bind(&config.bind_address).await?;
    axum::serve(
        listener,
        build_router(AppState::new(PgStore::new(pool), config)),
    )
    .await?;
    Ok(())
}
```

- [ ] **Step 6: Add organization listing required by `/auth/me`**

Add this method in `crates/persistence/src/auth.rs`:

```rust
use crate::{models::OrganizationMembershipRecord, tenant::begin_user};

impl PgStore {
    pub async fn organizations_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<OrganizationMembershipRecord>, Error> {
        let mut tx = begin_user(&self.pool, user_id).await?;
        let records = sqlx::query_as(
            "SELECT organization_id, name, slug, role
             FROM corqty.organizations_for_current_user()
             ORDER BY name, organization_id",
        )
        .fetch_all(&mut *tx)
        .await?;
        tx.rollback().await?;
        Ok(records)
    }
}
```

Create `migrations/202607280003_user_organizations.sql`:

```sql
CREATE FUNCTION corqty.organizations_for_current_user()
RETURNS TABLE(
    organization_id uuid,
    name text,
    slug text,
    role text
)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    SELECT organizations.id,
           organizations.name,
           organizations.slug,
           memberships.role
    FROM memberships
    JOIN organizations
      ON organizations.id = memberships.organization_id
    WHERE memberships.user_id = corqty.current_user_id()
$$;

REVOKE ALL ON FUNCTION corqty.organizations_for_current_user() FROM PUBLIC;
GRANT EXECUTE
    ON FUNCTION corqty.organizations_for_current_user()
    TO corqty_api;
```

This narrow function supports a user's organization switcher without adding a broad cross-organization RLS policy or accepting an arbitrary user ID.

- [ ] **Step 7: Add the browser-like HTTP fixture**

Create `crates/api/tests/support/mod.rs` so later integration tests register
through the public API and retain both cookies:

```rust
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use corqty_api::{config::ApiConfig, router::build_router, state::AppState};
use corqty_persistence::store::PgStore;
use corqty_test_support::TestDatabase;
use tower::ServiceExt;
use uuid::Uuid;

pub struct AuthFixture {
    pub app: Router,
    pub csrf: String,
    pub user_id: Uuid,
    pub organization_id: Uuid,
    cookies: String,
}

impl AuthFixture {
    pub async fn registered(email: &str, organization_name: &str) -> Self {
        let database = TestDatabase::reset().await;
        let app = build_router(AppState::new(
            PgStore::new(database.api),
            ApiConfig::for_tests(),
        ));
        Self::register_on(app, email, organization_name).await
    }

    pub async fn register_another(
        &self,
        email: &str,
        organization_name: &str,
    ) -> Self {
        Self::register_on(self.app.clone(), email, organization_name).await
    }

    pub async fn login(&self, email: &str, password: &str) -> Self {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::post("/api/v1/auth/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "email": email,
                            "password": password,
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        Self::from_auth_response(self.app.clone(), response).await
    }

    async fn register_on(
        app: Router,
        email: &str,
        organization_name: &str,
    ) -> Self {
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/v1/auth/register")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "email": email,
                            "password": "correct horse battery staple",
                            "organizationName": organization_name,
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        Self::from_auth_response(app, response).await
    }

    async fn from_auth_response(app: Router, response: Response) -> Self {
        let cookie_pairs: Vec<String> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| {
                value
                    .to_str()
                    .unwrap()
                    .split(';')
                    .next()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        let csrf = cookie_pairs
            .iter()
            .find_map(|cookie| cookie.strip_prefix("corqty_csrf="))
            .unwrap()
            .to_owned();
        let cookies = cookie_pairs.join("; ");

        let me = app
            .clone()
            .oneshot(
                Request::get("/api/v1/auth/me")
                    .header(header::COOKIE, &cookies)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(me.status(), StatusCode::OK);
        let body = to_bytes(me.into_body(), 64 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        Self {
            app,
            csrf,
            user_id: json["user"]["id"].as_str().unwrap().parse().unwrap(),
            organization_id: json["organizations"][0]["organizationId"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap(),
            cookies,
        }
    }

    pub fn cookie_header(&self) -> &str {
        &self.cookies
    }

    pub async fn get(&self, path: &str) -> Response {
        self.app
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::COOKIE, &self.cookies)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    pub async fn post_json(&self, path: &str, json: &str) -> Response {
        self.request_json(Method::POST, path, json).await
    }

    pub async fn patch_json(&self, path: &str, json: &str) -> Response {
        self.request_json(Method::PATCH, path, json).await
    }

    pub async fn delete(&self, path: &str) -> Response {
        self.app
            .clone()
            .oneshot(
                Request::delete(path)
                    .header(header::COOKIE, &self.cookies)
                    .header("x-csrf-token", &self.csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn request_json(
        &self,
        method: Method,
        path: &str,
        json: &str,
    ) -> Response {
        self.app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, &self.cookies)
                    .header("x-csrf-token", &self.csrf)
                    .body(Body::from(json.to_owned()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }
}
```

Each integration-test file that uses it starts with:

```rust
mod support;

use support::AuthFixture;
```

- [ ] **Step 8: Verify GREEN, login rejection, and CSRF success**

Extend `crates/api/tests/auth_http.rs` with:

```rust
#[tokio::test]
#[serial]
async fn invalid_login_is_unauthorized() {
    let database = TestDatabase::reset().await;
    let app = build_router(AppState::new(
        PgStore::new(database.api),
        ApiConfig::for_tests(),
    ));
    let response = app
        .oneshot(
            Request::post("/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"email":"missing@example.com","password":"wrong password value"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
```

Run:

```text
cargo test -p corqty-api --test auth_http
```

Expected: 3 tests pass.

- [ ] **Step 9: Commit**

```text
git add crates/api crates/persistence migrations/202607280003_user_organizations.sql
git commit -m "feat: add session authentication API"
```

## Task 8: Add organization creation and role-checked request context

**Files:**

- Create: `crates/persistence/src/organizations.rs`
- Modify: `crates/persistence/src/lib.rs`
- Create: `crates/api/src/organizations/mod.rs`
- Create: `crates/api/src/organizations/handlers.rs`
- Create: `crates/api/src/organizations/service.rs`
- Modify: `crates/api/src/router.rs`
- Test: `crates/api/tests/organizations_http.rs`

- [ ] **Step 1: Write failing organization HTTP tests**

Create `crates/api/tests/organizations_http.rs`, import the shared fixture, and
assert:

```rust
mod support;

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use serial_test::serial;
use support::AuthFixture;
use tower::ServiceExt;

#[tokio::test]
#[serial]
async fn authenticated_user_can_create_a_second_organization() {
    let fixture = AuthFixture::registered("admin@example.com", "Acme").await;
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::post("/api/v1/organizations")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, fixture.cookie_header())
                .header("x-csrf-token", &fixture.csrf)
                .body(Body::from(r#"{"name":"Second Org"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let me = fixture
        .app
        .oneshot(
            Request::get("/api/v1/auth/me")
                .header(header::COOKIE, fixture.cookie_header())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(me.into_body(), 64 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["organizations"].as_array().unwrap().len(), 2);
}
```

- [ ] **Step 2: Run the test and verify RED**

Run:

```text
cargo test -p corqty-api --test organizations_http
```

Expected: the request returns `404 Not Found` because the route does not exist.

- [ ] **Step 3: Implement organization persistence**

Create `crates/persistence/src/organizations.rs`:

```rust
use sqlx::Error;
use uuid::Uuid;

use crate::{
    models::OrganizationMembershipRecord,
    store::PgStore,
    tenant::begin_tenant,
};

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
        tx.commit().await?;
        Ok(OrganizationMembershipRecord {
            organization_id,
            name: name.to_owned(),
            slug: slug.to_owned(),
            role: "admin".to_owned(),
        })
    }

    pub async fn membership_role(
        &self,
        user_id: Uuid,
        organization_id: Uuid,
    ) -> Result<Option<String>, Error> {
        let mut tx = begin_tenant(&self.pool, user_id, organization_id).await?;
        let role = sqlx::query_scalar(
            "SELECT role
             FROM memberships
             WHERE organization_id = $1 AND user_id = $2",
        )
        .bind(organization_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;
        tx.rollback().await?;
        Ok(role)
    }
}
```

Add `pub mod organizations;` to `crates/persistence/src/lib.rs`.

- [ ] **Step 4: Implement service, handler, and route**

Create `crates/api/src/organizations/service.rs`:

```rust
use corqty_domain::account::OrganizationName;
use corqty_persistence::models::{
    OrganizationMembershipRecord, SessionRecord,
};
use uuid::Uuid;

use crate::{error::ApiError, state::AppState};

pub async fn create(
    state: &AppState,
    session: &SessionRecord,
    name: &str,
) -> Result<OrganizationMembershipRecord, ApiError> {
    let name = OrganizationName::parse(name)
        .map_err(|_| ApiError::Validation("invalid_organization_name"))?;
    let organization_id = Uuid::new_v4();
    let prefix = name
        .as_str()
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();
    let slug = format!(
        "{}-{}",
        if prefix.is_empty() { "org" } else { &prefix },
        &organization_id.simple().to_string()[..8]
    );
    state
        .store
        .create_organization(session.user_id, organization_id, name.as_str(), &slug)
        .await
        .map_err(|_| ApiError::Internal)
}
```

Create `crates/api/src/organizations/handlers.rs`:

```rust
use axum::{
    Extension, Json,
    extract::State,
    http::StatusCode,
};
use corqty_persistence::{
    models::{OrganizationMembershipRecord, SessionRecord},
};
use serde::Deserialize;

use crate::{
    error::ApiError,
    organizations::service,
    state::AppState,
};

#[derive(Deserialize)]
pub struct CreateOrganization {
    name: String,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    Json(request): Json<CreateOrganization>,
) -> Result<(StatusCode, Json<OrganizationMembershipRecord>), ApiError> {
    Ok((
        StatusCode::CREATED,
        Json(service::create(&state, &session, &request.name).await?),
    ))
}
```

Create `crates/api/src/organizations/mod.rs`:

```rust
pub mod handlers;
pub mod service;
```

Add `pub mod organizations;` to `crates/api/src/lib.rs`.

Replace the local crate import in `crates/api/src/router.rs` with:

```rust
use crate::{
    auth::{handlers, middleware as auth_middleware},
    organizations,
    state::AppState,
};
```

Add the route to the protected router:

```rust
.route("/organizations", post(organizations::handlers::create))
```

- [ ] **Step 5: Verify GREEN**

Run:

```text
cargo test -p corqty-api --test organizations_http
```

Expected: the test passes and `/auth/me` returns both admin memberships.

- [ ] **Step 6: Commit**

```text
git add crates/api crates/persistence
git commit -m "feat: add organization creation"
```

## Task 9: Implement member invitations and the final-admin invariant

**Files:**

- Create: `migrations/202607280004_invitations.sql`
- Create: `crates/persistence/src/members.rs`
- Modify: `crates/persistence/src/lib.rs`
- Create: `crates/api/src/organizations/members.rs`
- Modify: `crates/api/src/organizations/mod.rs`
- Modify: `crates/api/src/router.rs`
- Test: `crates/api/tests/members_http.rs`
- Test: `crates/persistence/tests/final_admin.rs`

- [ ] **Step 1: Write failing member-management tests**

Create `crates/api/tests/members_http.rs` with HTTP fixtures and these assertions:

```rust
mod support;

use axum::{body::to_bytes, http::StatusCode};
use chrono::{Duration, Utc};
use serial_test::serial;
use support::AuthFixture;

#[tokio::test]
#[serial]
async fn admin_can_invite_and_matching_user_can_accept() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let invite_response = admin
        .post_json(
            &format!(
                "/api/v1/orgs/{}/invitations",
                admin.organization_id
            ),
            r#"{"email":"editor@example.com","role":"editor"}"#,
        )
        .await;
    assert_eq!(invite_response.status(), StatusCode::CREATED);
    let body = to_bytes(invite_response.into_body(), 64 * 1024).await.unwrap();
    let invite: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let token = invite["token"].as_str().unwrap().to_owned();
    let expires_at = invite["expiresAt"]
        .as_str()
        .unwrap()
        .parse::<chrono::DateTime<Utc>>()
        .unwrap();
    assert!(expires_at > Utc::now() + Duration::days(6));
    assert!(expires_at < Utc::now() + Duration::days(8));

    let wrong_user = admin
        .register_another("wrong@example.com", "Wrong Personal")
        .await;
    let mismatch = wrong_user
        .post_json(
            "/api/v1/invitations/accept",
            &serde_json::json!({ "token": token.clone() }).to_string(),
        )
        .await;
    assert_eq!(mismatch.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let editor = admin
        .register_another("editor@example.com", "Personal")
        .await;
    let accepted = editor
        .post_json(
            "/api/v1/invitations/accept",
            &serde_json::json!({ "token": token.clone() }).to_string(),
        )
        .await;
    assert_eq!(accepted.status(), StatusCode::OK);
    let reused = editor
        .post_json(
            "/api/v1/invitations/accept",
            &serde_json::json!({ "token": token }).to_string(),
        )
        .await;
    assert_eq!(reused.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let me = editor.get("/api/v1/auth/me").await;
    let me_body = to_bytes(me.into_body(), 64 * 1024).await.unwrap();
    let me_json: serde_json::Value = serde_json::from_slice(&me_body).unwrap();
    let accepted_membership = me_json["organizations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|membership| {
            membership["organizationId"] == admin.organization_id.to_string()
        })
        .unwrap();
    assert_eq!(accepted_membership["role"], "editor");

    let denied = editor
        .post_json(
            &format!(
                "/api/v1/orgs/{}/invitations",
                admin.organization_id
            ),
            r#"{"email":"other@example.com","role":"viewer"}"#,
        )
        .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
#[serial]
async fn final_admin_cannot_be_demoted_or_removed() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let demote = admin
        .patch_json(
            &format!(
                "/api/v1/orgs/{}/members/{}",
                admin.organization_id, admin.user_id
            ),
            r#"{"role":"viewer"}"#,
        )
        .await;
    assert_eq!(demote.status(), StatusCode::CONFLICT);

    let remove = admin
        .delete(&format!(
            "/api/v1/orgs/{}/members/{}",
            admin.organization_id, admin.user_id
        ))
        .await;
    assert_eq!(remove.status(), StatusCode::CONFLICT);
}

#[tokio::test]
#[serial]
async fn cross_organization_identifier_returns_not_found() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let outsider = admin
        .register_another("outsider@example.com", "Outside")
        .await;
    let response = outsider
        .get(&format!(
            "/api/v1/orgs/{}/members",
            admin.organization_id
        ))
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
```

The shared fixture uses session and CSRF cookies exactly as the browser does.

- [ ] **Step 2: Run the tests and verify RED**

Run:

```text
cargo test -p corqty-api --test members_http
```

Expected: requests return `404 Not Found`.

- [ ] **Step 3: Add the atomic invitation-acceptance function**

Create `migrations/202607280004_invitations.sql`:

```sql
CREATE FUNCTION corqty.assert_organization_has_admin(
    affected_organization_id uuid
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
BEGIN
    PERFORM 1
    FROM organizations
    WHERE id = affected_organization_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RETURN;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM memberships
        WHERE organization_id = affected_organization_id
          AND role = 'admin'
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'final_admin_required';
    END IF;
END;
$$;

REVOKE ALL
    ON FUNCTION corqty.assert_organization_has_admin(uuid)
    FROM PUBLIC;

CREATE FUNCTION corqty.ensure_organization_has_admin()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        PERFORM corqty.assert_organization_has_admin(NEW.organization_id);
    ELSIF TG_OP = 'DELETE' THEN
        PERFORM corqty.assert_organization_has_admin(OLD.organization_id);
    ELSE
        PERFORM corqty.assert_organization_has_admin(OLD.organization_id);
        IF NEW.organization_id <> OLD.organization_id THEN
            PERFORM corqty.assert_organization_has_admin(NEW.organization_id);
        END IF;
    END IF;
    RETURN NULL;
END;
$$;

REVOKE ALL
    ON FUNCTION corqty.ensure_organization_has_admin()
    FROM PUBLIC;

CREATE CONSTRAINT TRIGGER memberships_require_admin
AFTER INSERT OR UPDATE OR DELETE ON memberships
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION corqty.ensure_organization_has_admin();

CREATE FUNCTION corqty.accept_invitation(
    supplied_token_hash bytea
)
RETURNS TABLE(organization_id uuid, accepted_role text)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
    invitation_record invitations%ROWTYPE;
    accepting_user_id uuid;
    accepting_email text;
BEGIN
    accepting_user_id := corqty.current_user_id();
    IF accepting_user_id IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'invalid_invitation';
    END IF;

    SELECT email INTO accepting_email
    FROM users
    WHERE id = accepting_user_id;

    SELECT * INTO invitation_record
    FROM invitations
    WHERE token_hash = supplied_token_hash
      AND accepted_at IS NULL
      AND expires_at > now()
    FOR UPDATE;

    IF NOT FOUND OR invitation_record.email <> accepting_email THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'invalid_invitation';
    END IF;

    INSERT INTO memberships(organization_id, user_id, role)
    VALUES (
        invitation_record.organization_id,
        accepting_user_id,
        invitation_record.role
    )
    ON CONFLICT (organization_id, user_id)
    DO UPDATE SET role = EXCLUDED.role;

    UPDATE invitations
    SET accepted_at = now()
    WHERE id = invitation_record.id;

    INSERT INTO audit_events(
        organization_id,
        actor_user_id,
        action,
        resource_type,
        resource_id,
        metadata
    )
    VALUES (
        invitation_record.organization_id,
        accepting_user_id,
        'invitation.accepted',
        'membership',
        accepting_user_id::text,
        jsonb_build_object('role', invitation_record.role)
    );

    RETURN QUERY
    SELECT invitation_record.organization_id, invitation_record.role;
END;
$$;

REVOKE ALL ON FUNCTION corqty.accept_invitation(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.accept_invitation(bytea) TO corqty_api;
```

- [ ] **Step 4: Implement member persistence with locked admin counts**

Create `crates/persistence/src/members.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    store::PgStore,
    tenant::{begin_tenant, begin_user},
};

#[derive(Debug, thiserror::Error)]
pub enum MemberMutationError {
    #[error("the final admin cannot be changed")]
    FinalAdmin,
    #[error("member was not found")]
    NotFound,
    #[error(transparent)]
    Database(#[source] sqlx::Error),
}

impl From<sqlx::Error> for MemberMutationError {
    fn from(error: sqlx::Error) -> Self {
        if error
            .as_database_error()
            .is_some_and(|database| database.message() == "final_admin_required")
        {
            Self::FinalAdmin
        } else {
            Self::Database(error)
        }
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MemberRecord {
    pub user_id: Uuid,
    pub email: String,
    pub role: String,
}

impl PgStore {
    pub async fn create_invitation(
        &self,
        actor_user_id: Uuid,
        organization_id: Uuid,
        invitation_id: Uuid,
        email: &str,
        role: &str,
        token_hash: &[u8],
        expires_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let mut tx =
            begin_tenant(&self.pool, actor_user_id, organization_id).await?;
        sqlx::query(
            "INSERT INTO invitations(
                id, organization_id, email, role,
                token_hash, invited_by, expires_at
             ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(invitation_id)
        .bind(organization_id)
        .bind(email)
        .bind(role)
        .bind(token_hash)
        .bind(actor_user_id)
        .bind(expires_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }

    pub async fn accept_invitation(
        &self,
        user_id: Uuid,
        token_hash: &[u8],
    ) -> Result<(Uuid, String), sqlx::Error> {
        let mut tx = begin_user(&self.pool, user_id).await?;
        let accepted = sqlx::query_as(
            "SELECT organization_id, accepted_role
             FROM corqty.accept_invitation($1)",
        )
        .bind(token_hash)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(accepted)
    }

    pub async fn list_members(
        &self,
        actor_user_id: Uuid,
        organization_id: Uuid,
    ) -> Result<Vec<MemberRecord>, sqlx::Error> {
        let mut tx =
            begin_tenant(&self.pool, actor_user_id, organization_id).await?;
        let members = sqlx::query_as(
            "SELECT users.id AS user_id, users.email, memberships.role
             FROM memberships
             JOIN users ON users.id = memberships.user_id
             WHERE memberships.organization_id = $1
             ORDER BY users.email, users.id",
        )
        .bind(organization_id)
        .fetch_all(&mut *tx)
        .await?;
        tx.rollback().await?;
        Ok(members)
    }

    pub async fn change_member_role(
        &self,
        actor_user_id: Uuid,
        organization_id: Uuid,
        target_user_id: Uuid,
        new_role: &str,
    ) -> Result<(), MemberMutationError> {
        let mut tx =
            begin_tenant(&self.pool, actor_user_id, organization_id).await?;
        lock_organization(&mut tx, organization_id).await?;
        let current_role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM memberships
             WHERE organization_id = $1 AND user_id = $2
             FOR UPDATE",
        )
        .bind(organization_id)
        .bind(target_user_id)
        .fetch_optional(&mut *tx)
        .await?;
        let current_role = current_role.ok_or(MemberMutationError::NotFound)?;
        if current_role == "admin"
            && new_role != "admin"
            && admin_count(&mut tx, organization_id).await? == 1
        {
            return Err(MemberMutationError::FinalAdmin);
        }
        sqlx::query(
            "UPDATE memberships SET role = $3
             WHERE organization_id = $1 AND user_id = $2",
        )
        .bind(organization_id)
        .bind(target_user_id)
        .bind(new_role)
        .execute(&mut *tx)
        .await?;
        tx.commit().await.map_err(MemberMutationError::from)
    }

    pub async fn remove_member(
        &self,
        actor_user_id: Uuid,
        organization_id: Uuid,
        target_user_id: Uuid,
    ) -> Result<(), MemberMutationError> {
        let mut tx =
            begin_tenant(&self.pool, actor_user_id, organization_id).await?;
        lock_organization(&mut tx, organization_id).await?;
        let current_role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM memberships
             WHERE organization_id = $1 AND user_id = $2
             FOR UPDATE",
        )
        .bind(organization_id)
        .bind(target_user_id)
        .fetch_optional(&mut *tx)
        .await?;
        let current_role = current_role.ok_or(MemberMutationError::NotFound)?;
        if current_role == "admin"
            && admin_count(&mut tx, organization_id).await? == 1
        {
            return Err(MemberMutationError::FinalAdmin);
        }
        sqlx::query(
            "DELETE FROM memberships
             WHERE organization_id = $1 AND user_id = $2",
        )
        .bind(organization_id)
        .bind(target_user_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await.map_err(MemberMutationError::from)
    }
}

async fn lock_organization(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    organization_id: Uuid,
) -> Result<(), sqlx::Error> {
    let _: Uuid =
        sqlx::query_scalar("SELECT id FROM organizations WHERE id = $1 FOR UPDATE")
        .bind(organization_id)
        .fetch_one(&mut **tx)
        .await?;
    Ok(())
}

async fn admin_count(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    organization_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM memberships
         WHERE organization_id = $1 AND role = 'admin'",
    )
    .bind(organization_id)
    .fetch_one(&mut **tx)
    .await
}
```

Add `pub mod members;` to `crates/persistence/src/lib.rs`.

- [ ] **Step 5: Implement capability checks and member HTTP routes**

Create `crates/api/src/organizations/members.rs`:

```rust
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::{DateTime, Duration, Utc};
use corqty_domain::{
    account::NormalizedEmail,
    rbac::{Capability, Role},
};
use corqty_persistence::{
    members::{MemberMutationError, MemberRecord},
    models::SessionRecord,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::crypto::SecretToken,
    error::ApiError,
    state::AppState,
};

#[derive(Deserialize)]
pub struct InvitationRequest {
    email: String,
    role: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvitationResponse {
    token: String,
    invite_url: String,
    expires_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct AcceptInvitationRequest {
    token: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptInvitationResponse {
    organization_id: Uuid,
    role: String,
}

#[derive(Deserialize)]
pub struct ChangeRoleRequest {
    role: String,
}

pub async fn create_invitation(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    Path(organization_id): Path<Uuid>,
    Json(request): Json<InvitationRequest>,
) -> Result<(StatusCode, Json<InvitationResponse>), ApiError> {
    require_admin(&state, &session, organization_id).await?;
    let email = NormalizedEmail::parse(&request.email)
        .map_err(|_| ApiError::Validation("invalid_email"))?;
    let role = request
        .role
        .parse::<Role>()
        .map_err(|_| ApiError::Validation("invalid_role"))?;
    let token = SecretToken::generate()?;
    let expires_at = Utc::now() + Duration::days(7);
    state
        .store
        .create_invitation(
            session.user_id,
            organization_id,
            Uuid::new_v4(),
            email.as_str(),
            role.as_str(),
            &token.hash,
            expires_at,
        )
        .await
        .map_err(|_| ApiError::Internal)?;
    let base = state.config.public_web_url.trim_end_matches('/');
    let invite_url = format!(
        "{base}/invitations/accept#token={}",
        token.plaintext
    );
    Ok((
        StatusCode::CREATED,
        Json(InvitationResponse {
            token: token.plaintext,
            invite_url,
            expires_at,
        }),
    ))
}

pub async fn accept_invitation(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    Json(request): Json<AcceptInvitationRequest>,
) -> Result<Json<AcceptInvitationResponse>, ApiError> {
    let result = state
        .store
        .accept_invitation(
            session.user_id,
            &SecretToken::hash(&request.token),
        )
        .await
        .map_err(map_invitation_error)?;
    Ok(Json(AcceptInvitationResponse {
        organization_id: result.0,
        role: result.1,
    }))
}

pub async fn list_members(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    Path(organization_id): Path<Uuid>,
) -> Result<Json<Vec<MemberRecord>>, ApiError> {
    require_membership(&state, &session, organization_id).await?;
    let members = state
        .store
        .list_members(session.user_id, organization_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(Json(members))
}

pub async fn change_role(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    Path((organization_id, target_user_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<ChangeRoleRequest>,
) -> Result<StatusCode, ApiError> {
    require_admin(&state, &session, organization_id).await?;
    let role = request
        .role
        .parse::<Role>()
        .map_err(|_| ApiError::Validation("invalid_role"))?;
    state
        .store
        .change_member_role(
            session.user_id,
            organization_id,
            target_user_id,
            role.as_str(),
        )
        .await
        .map_err(map_member_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_member(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    Path((organization_id, target_user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    require_admin(&state, &session, organization_id).await?;
    state
        .store
        .remove_member(
            session.user_id,
            organization_id,
            target_user_id,
        )
        .await
        .map_err(map_member_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn require_membership(
    state: &AppState,
    session: &SessionRecord,
    organization_id: Uuid,
) -> Result<Role, ApiError> {
    state
        .store
        .membership_role(session.user_id, organization_id)
        .await
        .map_err(|_| ApiError::Internal)?
        .ok_or(ApiError::NotFound)?
        .parse::<Role>()
        .map_err(|_| ApiError::Internal)
}

async fn require_admin(
    state: &AppState,
    session: &SessionRecord,
    organization_id: Uuid,
) -> Result<(), ApiError> {
    require_membership(state, session, organization_id)
        .await?
        .allows(Capability::ManageMembers)
        .then_some(())
        .ok_or(ApiError::Forbidden)
}

fn map_member_error(error: MemberMutationError) -> ApiError {
    match error {
        MemberMutationError::FinalAdmin => {
            ApiError::Conflict("final_admin_required")
        }
        MemberMutationError::NotFound => ApiError::NotFound,
        MemberMutationError::Database(_) => ApiError::Internal,
    }
}

fn map_invitation_error(error: sqlx::Error) -> ApiError {
    match error.as_database_error().map(|database| database.message()) {
        Some("invalid_invitation") => {
            ApiError::Validation("invalid_invitation")
        }
        Some("final_admin_required") => {
            ApiError::Conflict("final_admin_required")
        }
        _ => ApiError::Internal,
    }
}
```

Add `pub mod members;` to `crates/api/src/organizations/mod.rs`.

Register these exact protected routes in `router.rs`:

```rust
.route(
    "/orgs/{organization_id}/invitations",
    post(organizations::members::create_invitation),
)
.route(
    "/invitations/accept",
    post(organizations::members::accept_invitation),
)
.route(
    "/orgs/{organization_id}/members",
    get(organizations::members::list_members),
)
.route(
    "/orgs/{organization_id}/members/{target_user_id}",
    axum::routing::patch(organizations::members::change_role)
        .delete(organizations::members::remove_member),
)
```

The returned link places token plaintext only in the SPA fragment; browsers do not send fragments in HTTP requests or access logs.

- [ ] **Step 6: Verify GREEN, editor denial, and concurrent protection**

Create `crates/persistence/tests/final_admin.rs`:

```rust
use corqty_test_support::TestDatabase;
use serial_test::serial;
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn concurrent_demotions_cannot_remove_every_admin() {
    let database = TestDatabase::reset().await;
    let organization_id = Uuid::new_v4();
    let first_admin = Uuid::new_v4();
    let second_admin = Uuid::new_v4();
    for (user_id, email) in [
        (first_admin, "first@example.com"),
        (second_admin, "second@example.com"),
    ] {
        sqlx::query("INSERT INTO users(id, email) VALUES ($1, $2)")
            .bind(user_id)
            .bind(email)
            .execute(&database.owner)
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO organizations(id, name, slug, created_by)
         VALUES ($1, 'Acme', 'acme', $2)",
    )
    .bind(organization_id)
    .bind(first_admin)
    .execute(&database.owner)
    .await
    .unwrap();
    for user_id in [first_admin, second_admin] {
        sqlx::query(
            "INSERT INTO memberships(organization_id, user_id, role)
             VALUES ($1, $2, 'admin')",
        )
        .bind(organization_id)
        .bind(user_id)
        .execute(&database.owner)
        .await
        .unwrap();
    }

    let mut first = database.owner.begin().await.unwrap();
    let mut second = database.owner.begin().await.unwrap();
    sqlx::query(
        "UPDATE memberships SET role = 'viewer'
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(first_admin)
    .execute(&mut *first)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE memberships SET role = 'viewer'
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(second_admin)
    .execute(&mut *second)
    .await
    .unwrap();

    let (first_result, second_result) = tokio::join!(first.commit(), second.commit());
    assert_ne!(first_result.is_ok(), second_result.is_ok());
    let remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM memberships
         WHERE organization_id = $1 AND role = 'admin'",
    )
    .bind(organization_id)
    .fetch_one(&database.owner)
    .await
    .unwrap();
    assert_eq!(remaining, 1);
}
```

Run:

```text
cargo test -p corqty-api --test members_http
cargo test -p corqty-persistence --test final_admin
```

Expected: invitation acceptance, final-admin protection, editor denial, and the database-level concurrent-demotion test pass.

- [ ] **Step 7: Commit**

```text
git add migrations/202607280004_invitations.sql crates/persistence crates/api
git commit -m "feat: add organization member invitations"
```

## Task 10: Record and expose append-only administrative audit events

**Files:**

- Create: `migrations/202607280005_user_audit.sql`
- Create: `crates/persistence/src/audit.rs`
- Modify: `crates/persistence/src/auth.rs`
- Modify: `crates/persistence/src/organizations.rs`
- Modify: `crates/persistence/src/members.rs`
- Modify: `crates/persistence/src/lib.rs`
- Create: `crates/api/src/audit.rs`
- Modify: `crates/api/src/lib.rs`
- Modify: `crates/api/src/router.rs`
- Test: `crates/api/tests/audit_http.rs`
- Modify: `crates/persistence/tests/final_admin.rs`

- [ ] **Step 1: Write a failing audit test**

Create `crates/api/tests/audit_http.rs`:

```rust
mod support;

use axum::{body::to_bytes, http::StatusCode};
use serial_test::serial;
use support::AuthFixture;

#[tokio::test]
#[serial]
async fn authentication_and_invitation_write_safe_audit_events() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let second_session = admin
        .login(
            "admin@example.com",
            "correct horse battery staple",
        )
        .await;
    let changed = second_session
        .patch_json(
            "/api/v1/auth/password",
            r#"{
                "currentPassword":"correct horse battery staple",
                "newPassword":"a newer correct horse battery staple"
            }"#,
        )
        .await;
    assert_eq!(changed.status(), StatusCode::NO_CONTENT);

    let sessions_before = second_session
        .get("/api/v1/auth/sessions")
        .await;
    let sessions_before_body =
        to_bytes(sessions_before.into_body(), 64 * 1024).await.unwrap();
    let sessions_before_json: serde_json::Value =
        serde_json::from_slice(&sessions_before_body).unwrap();
    let current_session_id = sessions_before_json[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let third_session = second_session
        .login(
            "admin@example.com",
            "a newer correct horse battery staple",
        )
        .await;

    let sessions_after = second_session
        .get("/api/v1/auth/sessions")
        .await;
    let sessions_after_body =
        to_bytes(sessions_after.into_body(), 64 * 1024).await.unwrap();
    let sessions_after_json: serde_json::Value =
        serde_json::from_slice(&sessions_after_body).unwrap();
    let third_session_id = sessions_after_json
        .as_array()
        .unwrap()
        .iter()
        .find_map(|session| {
            let id = session["id"].as_str()?;
            (id != current_session_id).then(|| id.to_owned())
        })
        .unwrap();
    let revoked = second_session
        .delete(&format!(
            "/api/v1/auth/sessions/{third_session_id}"
        ))
        .await;
    assert_eq!(revoked.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        third_session.get("/api/v1/auth/me").await.status(),
        StatusCode::UNAUTHORIZED,
    );

    let logout_session = second_session
        .login(
            "admin@example.com",
            "a newer correct horse battery staple",
        )
        .await;
    let logged_out = logout_session
        .post_json("/api/v1/auth/logout", "{}")
        .await;
    assert_eq!(logged_out.status(), StatusCode::NO_CONTENT);

    let invite = second_session
        .post_json(
            &format!(
                "/api/v1/orgs/{}/invitations",
                admin.organization_id
            ),
            r#"{"email":"viewer@example.com","role":"viewer"}"#,
        )
        .await;
    assert_eq!(invite.status(), StatusCode::CREATED);
    let invite_body = to_bytes(invite.into_body(), 64 * 1024).await.unwrap();
    let invite_json: serde_json::Value =
        serde_json::from_slice(&invite_body).unwrap();
    let token = invite_json["token"].as_str().unwrap().to_owned();

    let response = second_session
        .get(&format!(
            "/api/v1/orgs/{}/audit-events",
            admin.organization_id
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let actions: Vec<_> = json["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["action"].as_str().unwrap())
        .collect();
    assert!(actions.contains(&"organization.created"));
    assert!(actions.contains(&"account.registered"));
    assert!(actions.contains(&"auth.login"));
    assert!(actions.contains(&"account.password_changed"));
    assert!(actions.contains(&"session.revoked"));
    assert!(actions.contains(&"auth.logout"));
    assert!(actions.contains(&"invitation.created"));
    assert!(!body
        .windows(token.len())
        .any(|window| window == token.as_bytes()));
}
```

Extend `crates/persistence/tests/final_admin.rs` with a direct transaction
test proving an audit row cannot survive a rejected final-admin mutation:

```rust
#[tokio::test]
#[serial]
async fn rejected_final_admin_change_rolls_back_its_audit_event() {
    let database = TestDatabase::reset().await;
    let user_id = Uuid::new_v4();
    let organization_id = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id, email) VALUES ($1, 'admin@example.com')")
        .bind(user_id)
        .execute(&database.owner)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO organizations(id, name, slug, created_by)
         VALUES ($1, 'Acme', 'acme', $2)",
    )
    .bind(organization_id)
    .bind(user_id)
    .execute(&database.owner)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO memberships(organization_id, user_id, role)
         VALUES ($1, $2, 'admin')",
    )
    .bind(organization_id)
    .bind(user_id)
    .execute(&database.owner)
    .await
    .unwrap();

    let mut tx = database.owner.begin().await.unwrap();
    sqlx::query(
        "UPDATE memberships SET role = 'viewer'
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO audit_events(
            organization_id, actor_user_id, action,
            resource_type, resource_id
         ) VALUES ($1, $2, 'membership.role_changed', 'membership', $3)",
    )
    .bind(organization_id)
    .bind(user_id)
    .bind(user_id.to_string())
    .execute(&mut *tx)
    .await
    .unwrap();
    assert!(tx.commit().await.is_err());

    let audit_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events
         WHERE organization_id = $1
           AND action = 'membership.role_changed'",
    )
    .bind(organization_id)
    .fetch_one(&database.owner)
    .await
    .unwrap();
    assert_eq!(audit_count, 0);
}
```

- [ ] **Step 2: Run the test and verify RED**

Run:

```text
cargo test -p corqty-api --test audit_http
cargo test -p corqty-persistence --test final_admin
```

Expected: the audit endpoint returns `404 Not Found`.

- [ ] **Step 3: Implement audit persistence**

Create `migrations/202607280005_user_audit.sql` so account-level security
events are copied into every organization audit stream the user currently
belongs to:

```sql
CREATE FUNCTION corqty.audit_current_user_memberships(
    audit_action text,
    audit_resource_type text,
    audit_resource_id text,
    audit_metadata jsonb
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
    actor_id uuid;
BEGIN
    actor_id := corqty.current_user_id();
    IF actor_id IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'missing_user_context';
    END IF;

    INSERT INTO audit_events(
        organization_id,
        actor_user_id,
        action,
        resource_type,
        resource_id,
        metadata
    )
    SELECT memberships.organization_id,
           actor_id,
           audit_action,
           audit_resource_type,
           audit_resource_id,
           audit_metadata
    FROM memberships
    WHERE memberships.user_id = actor_id;
END;
$$;

REVOKE ALL
    ON FUNCTION corqty.audit_current_user_memberships(
        text, text, text, jsonb
    )
    FROM PUBLIC;
GRANT EXECUTE
    ON FUNCTION corqty.audit_current_user_memberships(
        text, text, text, jsonb
    )
    TO corqty_api;
```

Create `crates/persistence/src/audit.rs`:

```rust
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::{store::PgStore, tenant::begin_tenant};

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AuditEventRecord {
    pub id: i64,
    pub actor_user_id: Option<Uuid>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub metadata: Value,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
}

pub async fn insert_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    organization_id: Uuid,
    actor_user_id: Uuid,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    metadata: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_events(
            organization_id, actor_user_id, action,
            resource_type, resource_id, metadata
         ) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(organization_id)
    .bind(actor_user_id)
    .bind(action)
    .bind(resource_type)
    .bind(resource_id)
    .bind(metadata)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn insert_user_event_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    metadata: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT corqty.audit_current_user_memberships($1, $2, $3, $4)",
    )
    .bind(action)
    .bind(resource_type)
    .bind(resource_id)
    .bind(metadata)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

impl PgStore {
    pub async fn list_audit_events(
        &self,
        user_id: Uuid,
        organization_id: Uuid,
        before_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<AuditEventRecord>, sqlx::Error> {
        let mut tx = begin_tenant(&self.pool, user_id, organization_id).await?;
        let items = sqlx::query_as(
            "SELECT id, actor_user_id, action, resource_type,
                    resource_id, metadata, occurred_at
             FROM audit_events
             WHERE organization_id = $1
               AND ($2::bigint IS NULL OR id < $2)
             ORDER BY id DESC
             LIMIT $3",
        )
        .bind(organization_id)
        .bind(before_id)
        .bind(limit.clamp(1, 100))
        .fetch_all(&mut *tx)
        .await?;
        tx.rollback().await?;
        Ok(items)
    }
}
```

Add `pub mod audit;` to `crates/persistence/src/lib.rs`.

- [ ] **Step 4: Write audit events in existing mutation transactions**

In `PgStore::register`, after the initial admin membership insert and before the session insert, add:

```rust
crate::audit::insert_in_transaction(
    &mut tx,
    registration.organization_id,
    registration.user_id,
    "organization.created",
    "organization",
    &registration.organization_id.to_string(),
    serde_json::json!({ "name": registration.organization_name }),
)
.await?;
crate::audit::insert_user_event_in_transaction(
    &mut tx,
    "account.registered",
    "user",
    &registration.user_id.to_string(),
    serde_json::json!({}),
)
.await?;
```

In `PgStore::create_organization`, after the admin membership insert and before commit, add:

```rust
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
```

In `PgStore::create_invitation`, after the invitation insert and before commit, add:

```rust
crate::audit::insert_in_transaction(
    &mut tx,
    organization_id,
    actor_user_id,
    "invitation.created",
    "invitation",
    &invitation_id.to_string(),
    serde_json::json!({ "email": email, "role": role }),
)
.await?;
```

In `PgStore::change_member_role`, after the membership update and before commit, add:

```rust
crate::audit::insert_in_transaction(
    &mut tx,
    organization_id,
    actor_user_id,
    "membership.role_changed",
    "membership",
    &target_user_id.to_string(),
    serde_json::json!({ "role": new_role }),
)
.await?;
```

In `PgStore::remove_member`, immediately before the delete, add the event while
the acting admin still satisfies the audit insert policy. The event remains in
the same transaction and rolls back if the delete or deferred constraint
fails:

```rust
crate::audit::insert_in_transaction(
    &mut tx,
    organization_id,
    actor_user_id,
    "membership.removed",
    "membership",
    &target_user_id.to_string(),
    serde_json::json!({}),
)
.await?;
```

Invitation acceptance is already audited inside `corqty.accept_invitation`, in the same transaction that claims the token. Never pass token plaintext, token hashes, password hashes, cookies, or full request bodies to audit metadata.

Replace these four methods in `crates/persistence/src/auth.rs` so login,
logout, password changes, and explicit session revocation are atomic with their
organization audit entries:

```rust
pub async fn create_session(
    &self,
    user_id: Uuid,
    session: NewSession,
) -> Result<(), Error> {
    let session_id = session.session_id;
    let mut tx = begin_user(&self.pool, user_id).await?;
    insert_session(&mut tx, user_id, session).await?;
    crate::audit::insert_user_event_in_transaction(
        &mut tx,
        "auth.login",
        "session",
        &session_id.to_string(),
        serde_json::json!({}),
    )
    .await?;
    tx.commit().await
}

pub async fn delete_session(
    &self,
    user_id: Uuid,
    session_id: Uuid,
) -> Result<(), Error> {
    let mut tx = begin_user(&self.pool, user_id).await?;
    sqlx::query("DELETE FROM sessions WHERE id = $1 AND user_id = $2")
        .bind(session_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    crate::audit::insert_user_event_in_transaction(
        &mut tx,
        "auth.logout",
        "session",
        &session_id.to_string(),
        serde_json::json!({}),
    )
    .await?;
    tx.commit().await
}

pub async fn change_password_and_revoke_other_sessions(
    &self,
    user_id: Uuid,
    current_session_id: Uuid,
    password_hash: &str,
) -> Result<(), Error> {
    let mut tx = begin_user(&self.pool, user_id).await?;
    sqlx::query(
        "UPDATE password_credentials
         SET password_hash = $2, updated_at = now()
         WHERE user_id = $1",
    )
    .bind(user_id)
    .bind(password_hash)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM sessions
         WHERE user_id = $1 AND id <> $2",
    )
    .bind(user_id)
    .bind(current_session_id)
    .execute(&mut *tx)
    .await?;
    crate::audit::insert_user_event_in_transaction(
        &mut tx,
        "account.password_changed",
        "user",
        &user_id.to_string(),
        serde_json::json!({}),
    )
    .await?;
    tx.commit().await
}

pub async fn revoke_user_session(
    &self,
    user_id: Uuid,
    session_id: Uuid,
) -> Result<bool, Error> {
    let mut tx = begin_user(&self.pool, user_id).await?;
    let affected = sqlx::query(
        "DELETE FROM sessions WHERE id = $1 AND user_id = $2",
    )
    .bind(session_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if affected == 1 {
        crate::audit::insert_user_event_in_transaction(
            &mut tx,
            "session.revoked",
            "session",
            &session_id.to_string(),
            serde_json::json!({}),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(affected == 1)
}
```

In `crates/api/src/auth/handlers.rs`, replace the logout store call with:

```rust
state
    .store
    .delete_session(session.user_id, session.session_id)
    .await
    .map_err(|_| ApiError::Internal)?;
```

- [ ] **Step 5: Implement the audit endpoint**

Create `crates/api/src/audit.rs`:

```rust
use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use corqty_persistence::models::SessionRecord;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error::ApiError, state::AppState};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditQuery {
    before_id: Option<i64>,
    limit: Option<i64>,
}

#[derive(Serialize)]
pub struct AuditResponse {
    items: Vec<corqty_persistence::audit::AuditEventRecord>,
}

pub async fn list(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    Path(organization_id): Path<Uuid>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<AuditResponse>, ApiError> {
    let _role = state
        .store
        .membership_role(session.user_id, organization_id)
        .await
        .map_err(|_| ApiError::Internal)?
        .ok_or(ApiError::NotFound)?;
    let items = state
        .store
        .list_audit_events(
            session.user_id,
            organization_id,
            query.before_id,
            query.limit.unwrap_or(50),
        )
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(Json(AuditResponse { items }))
}
```

Add `pub mod audit;` to `crates/api/src/lib.rs`. Replace the local crate import
in `crates/api/src/router.rs` with:

```rust
use crate::{
    audit,
    auth::{handlers, middleware as auth_middleware},
    organizations,
    state::AppState,
};
```

Register:

```rust
.route("/orgs/{organization_id}/audit-events", get(audit::list))
```

- [ ] **Step 6: Verify GREEN and append-only grants**

Run:

```text
cargo test -p corqty-api --test audit_http
```

Then verify the API role cannot mutate audit rows:

```text
docker compose -f docker-compose.test.yml exec -T postgres-test psql "postgres://corqty_api:corqty_api_dev@localhost/corqty_test" -c "UPDATE audit_events SET action='tampered'"
docker compose -f docker-compose.test.yml exec -T postgres-test psql "postgres://corqty_api:corqty_api_dev@localhost/corqty_test" -c "DELETE FROM audit_events"
```

Expected: the HTTP and rollback tests pass; both SQL commands fail with
permission denied for both mutations.

- [ ] **Step 7: Commit**

```text
git add migrations/202607280005_user_audit.sql crates/persistence crates/api
git commit -m "feat: add administrative audit events"
```

## Task 11: Build the Vercel-style auth application shell test-first

**Files:**

- Create: `apps/web/components.json`
- Create: `apps/web/src/styles/globals.css`
- Create: `apps/web/src/test/setup.ts`
- Create: `apps/web/src/api/client.ts`
- Create: `apps/web/src/api/types.ts`
- Create: `apps/web/src/auth/auth-context.tsx`
- Create: `apps/web/src/auth/protected-route.tsx`
- Create: `apps/web/src/pages/login-page.tsx`
- Create: `apps/web/src/pages/register-page.tsx`
- Create: `apps/web/src/layout/app-shell.tsx`
- Create: `apps/web/src/pages/organization-home-page.tsx`
- Create: `apps/web/src/app.tsx`
- Create: `apps/web/src/main.tsx`
- Modify: `apps/web/vite.config.ts`
- Test: `apps/web/src/app.test.tsx`

- [ ] **Step 1: Configure shadcn/ui and generate only registry components**

Create `apps/web/components.json`:

```json
{
  "$schema": "https://ui.shadcn.com/schema.json",
  "style": "base-nova",
  "rsc": false,
  "tsx": true,
  "tailwind": {
    "config": "",
    "css": "src/styles/globals.css",
    "baseColor": "neutral",
    "cssVariables": true,
    "prefix": ""
  },
  "aliases": {
    "components": "@/components",
    "utils": "@/lib/utils",
    "ui": "@/components/ui",
    "lib": "@/lib",
    "hooks": "@/hooks"
  },
  "iconLibrary": "lucide"
}
```

Run from `apps/web`:

```text
pnpm dlx shadcn@latest add button card field input table dialog badge separator
```

Expected: generated shadcn/ui files appear under `src/components/ui`, `src/lib/utils.ts` exists, and package dependencies are added. Treat these registry files as generated code; product compositions remain test-first.

- [ ] **Step 2: Write failing route/auth tests**

Configure Vitest in `vite.config.ts`:

```ts
test: {
  environment: "jsdom",
  setupFiles: ["./src/test/setup.ts"],
},
```

Create `apps/web/src/test/setup.ts`:

```ts
import "@testing-library/jest-dom/vitest"
```

Create `apps/web/src/app.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { MemoryRouter } from "react-router-dom"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { App } from "@/app"

describe("application authentication", () => {
  beforeEach(() => {
    vi.restoreAllMocks()
  })

  it("shows the login page for an anonymous protected route", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(null, { status: 401 }),
    )
    render(
      <MemoryRouter initialEntries={["/orgs/acme"]}>
        <App />
      </MemoryRouter>,
    )
    expect(await screen.findByRole("heading", { name: "Sign in to Corqty" }))
      .toBeInTheDocument()
  })

  it("submits registration and enters the organization shell", async () => {
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(new Response(null, { status: 401 }))
      .mockResolvedValueOnce(
        Response.json({ ok: true }, { status: 201 }),
      )
      .mockResolvedValueOnce(
        Response.json({
          user: { id: "user-1", email: "admin@example.com" },
          organizations: [{
            organizationId: "org-1",
            name: "Acme",
            slug: "acme",
            role: "admin",
          }],
        }),
      )
    render(
      <MemoryRouter initialEntries={["/register"]}>
        <App />
      </MemoryRouter>,
    )
    await userEvent.type(screen.getByLabelText("Email"), "admin@example.com")
    await userEvent.type(
      screen.getByLabelText("Password"),
      "correct horse battery staple",
    )
    await userEvent.type(screen.getByLabelText("Organization"), "Acme")
    await userEvent.click(screen.getByRole("button", { name: "Create account" }))

    expect(await screen.findByText("Acme")).toBeInTheDocument()
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/auth/register",
      expect.objectContaining({ method: "POST" }),
    )
  })
})
```

- [ ] **Step 3: Run the tests and verify RED**

Run:

```text
pnpm --filter @corqty/web test
```

Expected: module resolution fails because `@/app` does not exist.

- [ ] **Step 4: Implement typed API and auth context**

Create `apps/web/src/api/types.ts`:

```ts
export type Role = "admin" | "editor" | "runner" | "viewer"

export interface OrganizationMembership {
  organizationId: string
  name: string
  slug: string
  role: Role
}

export interface Me {
  user: { id: string; email: string }
  organizations: OrganizationMembership[]
}
```

Create `apps/web/src/api/client.ts`:

```ts
function csrfToken(): string | undefined {
  return document.cookie
    .split(";")
    .map((part) => part.trim())
    .find((part) => part.startsWith("corqty_csrf="))
    ?.split("=")
    .slice(1)
    .join("=")
}

export async function api<T>(
  path: string,
  init: RequestInit = {},
): Promise<T> {
  const headers = new Headers(init.headers)
  if (init.body) headers.set("content-type", "application/json")
  const csrf = csrfToken()
  if (csrf && init.method && init.method !== "GET") {
    headers.set("x-csrf-token", csrf)
  }
  const response = await fetch(`/api/v1${path}`, {
    ...init,
    headers,
    credentials: "include",
  })
  if (!response.ok) {
    const problem = await response
      .json()
      .catch(() => ({ code: "unknown" })) as { code?: string }
    throw new ApiProblem(response.status, problem.code ?? "unknown")
  }
  if (response.status === 204) return undefined as T
  return response.json() as Promise<T>
}

export class ApiProblem extends Error {
  constructor(
    public readonly status: number,
    public readonly code: string,
  ) {
    super(`api:${status}:${code}`)
  }
}
```

Create `apps/web/src/auth/auth-context.tsx`:

```tsx
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react"
import type { ReactNode } from "react"

import { api } from "@/api/client"
import type { Me } from "@/api/types"

interface AuthValue {
  me: Me | null
  loading: boolean
  refresh: () => Promise<void>
}

const AuthContext = createContext<AuthValue | null>(null)

export function AuthProvider({ children }: { children: ReactNode }) {
  const [me, setMe] = useState<Me | null>(null)
  const [loading, setLoading] = useState(true)
  const refresh = useCallback(async () => {
    try {
      setMe(await api<Me>("/auth/me"))
    } catch {
      setMe(null)
    } finally {
      setLoading(false)
    }
  }, [])
  useEffect(() => {
    void refresh()
  }, [refresh])
  const value = useMemo(() => ({ me, loading, refresh }), [me, loading, refresh])
  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth(): AuthValue {
  const value = useContext(AuthContext)
  if (!value) throw new Error("useAuth must be used inside AuthProvider")
  return value
}
```

Create `apps/web/src/auth/protected-route.tsx`:

```tsx
import { Navigate, Outlet, useLocation } from "react-router-dom"

import { useAuth } from "@/auth/auth-context"

export function ProtectedRoute() {
  const { me, loading } = useAuth()
  const location = useLocation()
  if (loading) return <div className="p-6 text-sm text-muted-foreground">Loading…</div>
  if (!me) return <Navigate to="/login" replace state={{ from: location }} />
  return <Outlet />
}
```

- [ ] **Step 5: Implement auth pages, shell, and routes**

Create `apps/web/src/pages/login-page.tsx`:

```tsx
import { FormEvent, useState } from "react"
import { Link, useLocation, useNavigate } from "react-router-dom"

import { api } from "@/api/client"
import { useAuth } from "@/auth/auth-context"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"

export function LoginPage() {
  const [email, setEmail] = useState("")
  const [password, setPassword] = useState("")
  const [error, setError] = useState("")
  const [submitting, setSubmitting] = useState(false)
  const { refresh } = useAuth()
  const location = useLocation()
  const navigate = useNavigate()

  async function submit(event: FormEvent) {
    event.preventDefault()
    setSubmitting(true)
    setError("")
    try {
      await api("/auth/login", {
        method: "POST",
        body: JSON.stringify({ email, password }),
      })
      await refresh()
      const from = (
        location.state as {
          from?: { pathname: string; search: string; hash: string }
        } | null
      )?.from
      navigate(
        from ? `${from.pathname}${from.search}${from.hash}` : "/",
        { replace: true },
      )
    } catch {
      setError("Email or password is incorrect.")
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <main className="grid min-h-screen place-items-center bg-muted/30 p-6">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle>Sign in to Corqty</CardTitle>
          <CardDescription>Continue to your workflow workspace.</CardDescription>
        </CardHeader>
        <CardContent>
          <form className="flex flex-col gap-5" onSubmit={submit}>
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="email">Email</FieldLabel>
                <Input
                  id="email"
                  type="email"
                  autoComplete="email"
                  required
                  value={email}
                  onChange={(event) => setEmail(event.target.value)}
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="password">Password</FieldLabel>
                <Input
                  id="password"
                  type="password"
                  autoComplete="current-password"
                  required
                  value={password}
                  onChange={(event) => setPassword(event.target.value)}
                />
              </Field>
            </FieldGroup>
            {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
            <Button type="submit" disabled={submitting}>
              {submitting ? "Signing in…" : "Sign in"}
            </Button>
            <p className="text-center text-sm text-muted-foreground">
              New to Corqty?{" "}
              <Link
                className="text-foreground underline"
                state={location.state}
                to="/register"
              >
                Create an account
              </Link>
            </p>
          </form>
        </CardContent>
      </Card>
    </main>
  )
}
```

Create `apps/web/src/pages/register-page.tsx`:

```tsx
import { FormEvent, useState } from "react"
import { Link, useLocation, useNavigate } from "react-router-dom"

import { api } from "@/api/client"
import { useAuth } from "@/auth/auth-context"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"

export function RegisterPage() {
  const [email, setEmail] = useState("")
  const [password, setPassword] = useState("")
  const [organizationName, setOrganizationName] = useState("")
  const [error, setError] = useState("")
  const [submitting, setSubmitting] = useState(false)
  const { refresh } = useAuth()
  const location = useLocation()
  const navigate = useNavigate()

  async function submit(event: FormEvent) {
    event.preventDefault()
    setSubmitting(true)
    setError("")
    try {
      await api("/auth/register", {
        method: "POST",
        body: JSON.stringify({ email, password, organizationName }),
      })
      await refresh()
      const from = (
        location.state as {
          from?: { pathname: string; search: string; hash: string }
        } | null
      )?.from
      navigate(
        from ? `${from.pathname}${from.search}${from.hash}` : "/",
        { replace: true },
      )
    } catch {
      setError("Check the form values and try again.")
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <main className="grid min-h-screen place-items-center bg-muted/30 p-6">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle>Create your workspace</CardTitle>
          <CardDescription>Your first membership will be an admin.</CardDescription>
        </CardHeader>
        <CardContent>
          <form className="flex flex-col gap-5" onSubmit={submit}>
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="organization">Organization</FieldLabel>
                <Input
                  id="organization"
                  required
                  value={organizationName}
                  onChange={(event) => setOrganizationName(event.target.value)}
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="email">Email</FieldLabel>
                <Input
                  id="email"
                  type="email"
                  autoComplete="email"
                  required
                  value={email}
                  onChange={(event) => setEmail(event.target.value)}
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="password">Password</FieldLabel>
                <Input
                  id="password"
                  type="password"
                  minLength={12}
                  maxLength={128}
                  autoComplete="new-password"
                  required
                  value={password}
                  onChange={(event) => setPassword(event.target.value)}
                />
              </Field>
            </FieldGroup>
            {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
            <Button type="submit" disabled={submitting}>
              {submitting ? "Creating…" : "Create account"}
            </Button>
            <p className="text-center text-sm text-muted-foreground">
              Already registered?{" "}
              <Link
                className="text-foreground underline"
                state={location.state}
                to="/login"
              >
                Sign in
              </Link>
            </p>
          </form>
        </CardContent>
      </Card>
    </main>
  )
}
```

Create `apps/web/src/layout/app-shell.tsx`:

```tsx
import { NavLink, Outlet, useNavigate, useParams } from "react-router-dom"

import { api } from "@/api/client"
import { useAuth } from "@/auth/auth-context"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Separator } from "@/components/ui/separator"

const links = [
  { label: "Overview", suffix: "" },
]

export function AppShell() {
  const { organizationId } = useParams()
  const { me, refresh } = useAuth()
  const navigate = useNavigate()
  const membership = me?.organizations.find(
    (organization) => organization.organizationId === organizationId,
  )
  if (!me || !membership) return <p className="p-6">Organization not found.</p>

  async function logout() {
    await api("/auth/logout", { method: "POST" })
    await refresh()
    navigate("/login")
  }

  return (
    <div className="min-h-screen bg-background md:grid md:grid-cols-[220px_1fr]">
      <a
        href="#main-content"
        className="sr-only z-50 rounded-md bg-background px-3 py-2 text-sm shadow focus:not-sr-only focus:fixed focus:left-3 focus:top-3"
      >
        Skip to content
      </a>
      <aside className="flex flex-wrap items-center border-b bg-sidebar p-3 text-sidebar-foreground md:min-h-screen md:flex-nowrap md:flex-col md:items-stretch md:border-b-0 md:border-r">
        <div className="flex items-center gap-2 px-2 py-3">
          <span className="grid size-7 place-items-center rounded-md bg-foreground text-xs font-bold text-background">C</span>
          <span className="font-semibold tracking-tight">Corqty</span>
        </div>
        <select
          aria-label="Organization"
          className="ml-auto h-9 max-w-48 rounded-md border bg-background px-2 text-sm md:ml-0 md:max-w-none"
          value={membership.organizationId}
          onChange={(event) => navigate(`/orgs/${event.target.value}`)}
        >
          {me.organizations.map((organization) => (
            <option key={organization.organizationId} value={organization.organizationId}>
              {organization.name}
            </option>
          ))}
        </select>
        <Separator className="mx-3 hidden md:my-3 md:block" />
        <nav
          aria-label="Organization"
          className="order-3 mt-3 flex basis-full gap-1 overflow-x-auto md:order-none md:mt-0 md:flex-col"
        >
          {links.map((link) => (
            <NavLink
              key={link.label}
              end={link.suffix === ""}
              to={`/orgs/${membership.organizationId}${link.suffix}`}
              className={({ isActive }) =>
                `rounded-md px-3 py-2 text-sm ${
                  isActive ? "bg-sidebar-accent font-medium" : "text-muted-foreground"
                }`
              }
            >
              {link.label}
            </NavLink>
          ))}
        </nav>
        <div className="ml-2 flex items-center gap-2 md:ml-0 md:mt-auto md:flex-col md:items-stretch md:gap-2 md:border-t md:pt-3">
          <span className="hidden truncate px-2 text-xs text-muted-foreground md:block">{me.user.email}</span>
          <div className="flex items-center justify-between gap-2 px-2">
            <Badge variant="secondary">{membership.role}</Badge>
            <Button variant="ghost" size="sm" onClick={() => void logout()}>Log out</Button>
          </div>
        </div>
      </aside>
      <main id="main-content" className="min-w-0"><Outlet /></main>
    </div>
  )
}
```

Create `apps/web/src/pages/organization-home-page.tsx`:

```tsx
import { useParams } from "react-router-dom"

export function OrganizationHomePage() {
  const { organizationId } = useParams()
  return (
    <section className="flex flex-col gap-2 p-4 md:p-6">
      <p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
        Foundation
      </p>
      <h1 className="text-2xl font-semibold tracking-tight">Organization ready</h1>
      <p className="text-sm text-muted-foreground">
        Tenant context: {organizationId}
      </p>
    </section>
  )
}
```

Create `apps/web/src/app.tsx`:

```tsx
import { Navigate, Route, Routes } from "react-router-dom"

import { AuthProvider, useAuth } from "@/auth/auth-context"
import { ProtectedRoute } from "@/auth/protected-route"
import { AppShell } from "@/layout/app-shell"
import { LoginPage } from "@/pages/login-page"
import { OrganizationHomePage } from "@/pages/organization-home-page"
import { RegisterPage } from "@/pages/register-page"

function NavigateToFirstOrganization() {
  const { me } = useAuth()
  const first = me?.organizations[0]
  if (!first) {
    return <p className="p-6 text-sm text-muted-foreground">No organization memberships.</p>
  }
  return <Navigate replace to={`/orgs/${first.organizationId}`} />
}

export function App() {
  return (
    <AuthProvider>
      <Routes>
        <Route path="/login" element={<LoginPage />} />
        <Route path="/register" element={<RegisterPage />} />
        <Route element={<ProtectedRoute />}>
          <Route path="/orgs/:organizationId" element={<AppShell />}>
            <Route index element={<OrganizationHomePage />} />
          </Route>
          <Route path="/" element={<NavigateToFirstOrganization />} />
        </Route>
      </Routes>
    </AuthProvider>
  )
}
```

Create `apps/web/src/main.tsx`:

```tsx
import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { BrowserRouter } from "react-router-dom"

import { App } from "@/app"
import "@/styles/globals.css"

const systemTheme = window.matchMedia("(prefers-color-scheme: dark)")
function applySystemTheme() {
  document.documentElement.classList.toggle("dark", systemTheme.matches)
}
applySystemTheme()
systemTheme.addEventListener("change", applySystemTheme)

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <BrowserRouter>
      <App />
    </BrowserRouter>
  </StrictMode>,
)
```

Create `apps/web/src/styles/globals.css`:

```css
@import "tailwindcss";
@import "tw-animate-css";
@import "shadcn/tailwind.css";

@custom-variant dark (&:is(.dark *));

@theme inline {
  --color-background: var(--background);
  --color-foreground: var(--foreground);
  --color-card: var(--card);
  --color-card-foreground: var(--card-foreground);
  --color-popover: var(--popover);
  --color-popover-foreground: var(--popover-foreground);
  --color-primary: var(--primary);
  --color-primary-foreground: var(--primary-foreground);
  --color-secondary: var(--secondary);
  --color-secondary-foreground: var(--secondary-foreground);
  --color-muted: var(--muted);
  --color-muted-foreground: var(--muted-foreground);
  --color-accent: var(--accent);
  --color-accent-foreground: var(--accent-foreground);
  --color-destructive: var(--destructive);
  --color-border: var(--border);
  --color-input: var(--input);
  --color-ring: var(--ring);
  --color-sidebar: var(--sidebar);
  --color-sidebar-foreground: var(--sidebar-foreground);
  --color-sidebar-accent: var(--sidebar-accent);
  --radius-sm: calc(var(--radius) * .6);
  --radius-md: calc(var(--radius) * .8);
  --radius-lg: var(--radius);
}

:root {
  --radius: .625rem;
  --background: oklch(1 0 0);
  --foreground: oklch(.145 0 0);
  --card: oklch(1 0 0);
  --card-foreground: oklch(.145 0 0);
  --popover: oklch(1 0 0);
  --popover-foreground: oklch(.145 0 0);
  --primary: oklch(.205 0 0);
  --primary-foreground: oklch(.985 0 0);
  --secondary: oklch(.97 0 0);
  --secondary-foreground: oklch(.205 0 0);
  --muted: oklch(.97 0 0);
  --muted-foreground: oklch(.556 0 0);
  --accent: oklch(.97 0 0);
  --accent-foreground: oklch(.205 0 0);
  --destructive: oklch(.577 .245 27.325);
  --border: oklch(.922 0 0);
  --input: oklch(.922 0 0);
  --ring: oklch(.708 0 0);
  --sidebar: oklch(.985 0 0);
  --sidebar-foreground: oklch(.145 0 0);
  --sidebar-accent: oklch(.97 0 0);
}

.dark {
  --background: oklch(.145 0 0);
  --foreground: oklch(.985 0 0);
  --card: oklch(.205 0 0);
  --card-foreground: oklch(.985 0 0);
  --popover: oklch(.205 0 0);
  --popover-foreground: oklch(.985 0 0);
  --primary: oklch(.922 0 0);
  --primary-foreground: oklch(.205 0 0);
  --secondary: oklch(.269 0 0);
  --secondary-foreground: oklch(.985 0 0);
  --muted: oklch(.269 0 0);
  --muted-foreground: oklch(.708 0 0);
  --accent: oklch(.269 0 0);
  --accent-foreground: oklch(.985 0 0);
  --destructive: oklch(.704 .191 22.216);
  --border: oklch(1 0 0 / 10%);
  --input: oklch(1 0 0 / 15%);
  --ring: oklch(.556 0 0);
  --sidebar: oklch(.205 0 0);
  --sidebar-foreground: oklch(.985 0 0);
  --sidebar-accent: oklch(.269 0 0);
}

@layer base {
  * { @apply border-border outline-ring/50; }
  body { @apply bg-background text-foreground antialiased; }
}

@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    scroll-behavior: auto !important;
    animation-duration: .01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: .01ms !important;
  }
}
```

- [ ] **Step 6: Verify GREEN**

Run:

```text
pnpm --filter @corqty/web test
pnpm --filter @corqty/web lint
```

Expected: 2 tests pass and TypeScript exits 0.

- [ ] **Step 7: Commit**

```text
git add apps/web pnpm-lock.yaml
git commit -m "feat: add authenticated application shell"
```

## Task 12: Add member and audit administration pages

**Files:**

- Create: `apps/web/src/pages/members-page.tsx`
- Create: `apps/web/src/pages/accept-invitation-page.tsx`
- Create: `apps/web/src/pages/audit-page.tsx`
- Modify: `apps/web/src/app.tsx`
- Modify: `apps/web/src/layout/app-shell.tsx`
- Test: `apps/web/src/pages/admin-pages.test.tsx`

- [ ] **Step 1: Write failing admin-page tests**

Create `apps/web/src/pages/admin-pages.test.tsx`:

```tsx
import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { MemoryRouter } from "react-router-dom"
import { beforeEach, expect, it, vi } from "vitest"

import { App } from "@/app"
import { AuditPage } from "@/pages/audit-page"
import { MembersPage } from "@/pages/members-page"

beforeEach(() => {
  vi.restoreAllMocks()
})

it("admin can create an invitation and sees the copyable token", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined)
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  })
  vi.spyOn(globalThis, "fetch")
    .mockResolvedValueOnce(
      Response.json([
        { userId: "u1", email: "admin@example.com", role: "admin" },
      ]),
    )
    .mockResolvedValueOnce(
      Response.json({ token: "invite-once", expiresAt: "2026-08-04T00:00:00Z" },
        { status: 201 }),
    )
  render(
    <MemoryRouter>
      <MembersPage
        organizationId="org-1"
        currentRole="admin"
        currentUserId="u1"
      />
    </MemoryRouter>,
  )
  await screen.findByText("admin@example.com")
  await userEvent.click(screen.getByRole("button", { name: "Invite member" }))
  await userEvent.type(screen.getByLabelText("Email"), "editor@example.com")
  await userEvent.click(screen.getByRole("button", { name: "Create invitation" }))
  expect(await screen.findByText("invite-once")).toBeInTheDocument()
  await userEvent.click(screen.getByRole("button", { name: "Copy link" }))
  expect(writeText).toHaveBeenCalledWith("invite-once")
})

it("viewer does not see member mutation actions", async () => {
  vi.spyOn(globalThis, "fetch").mockResolvedValue(
    Response.json([
      { userId: "u1", email: "viewer@example.com", role: "viewer" },
    ]),
  )
  render(
    <MemoryRouter>
      <MembersPage
        organizationId="org-1"
        currentRole="viewer"
        currentUserId="u1"
      />
    </MemoryRouter>,
  )
  await screen.findByText("viewer@example.com")
  expect(screen.queryByRole("button", { name: "Invite member" }))
    .not.toBeInTheDocument()
})

it("renders administrative audit events", async () => {
  vi.spyOn(globalThis, "fetch").mockResolvedValue(
    Response.json({
      items: [{
        id: 1,
        actorUserId: "u1",
        action: "invitation.created",
        resourceType: "invitation",
        resourceId: "i1",
        metadata: { role: "viewer" },
        occurredAt: "2026-07-28T00:00:00Z",
      }],
    }),
  )
  render(
    <MemoryRouter>
      <AuditPage organizationId="org-1" />
    </MemoryRouter>,
  )
  expect(await screen.findByText("invitation.created")).toBeInTheDocument()
})

it("accepts a fragment invitation and navigates to its organization", async () => {
  const me = {
    user: { id: "user-1", email: "editor@example.com" },
    organizations: [{
      organizationId: "org-1",
      name: "Acme",
      slug: "acme",
      role: "editor",
    }],
  }
  const fetchMock = vi
    .spyOn(globalThis, "fetch")
    .mockResolvedValueOnce(Response.json(me))
    .mockResolvedValueOnce(
      Response.json({ organizationId: "org-1", role: "editor" }),
    )
    .mockResolvedValueOnce(Response.json(me))
  render(
    <MemoryRouter
      initialEntries={["/invitations/accept#token=invite-once"]}
    >
      <App />
    </MemoryRouter>,
  )
  await waitFor(() => {
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/invitations/accept",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ token: "invite-once" }),
      }),
    )
  })
  expect(
    await screen.findByRole("heading", { name: "Organization ready" }),
  ).toBeInTheDocument()
})
```

- [ ] **Step 2: Run tests and verify RED**

Run:

```text
pnpm --filter @corqty/web test -- admin-pages.test.tsx
```

Expected: module resolution fails because `MembersPage` does not exist.

- [ ] **Step 3: Implement members and invitation acceptance**

Create `apps/web/src/pages/members-page.tsx`:

```tsx
import { FormEvent, useCallback, useEffect, useState } from "react"

import { ApiProblem, api } from "@/api/client"
import type { Role } from "@/api/types"
import { Button } from "@/components/ui/button"
import { Card } from "@/components/ui/card"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Field, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

interface Member {
  userId: string
  email: string
  role: Role
}

interface Invitation {
  token: string
  inviteUrl?: string
  expiresAt: string
}

export function MembersPage({
  organizationId,
  currentRole,
  currentUserId,
}: {
  organizationId: string
  currentRole: Role
  currentUserId: string
}) {
  const [members, setMembers] = useState<Member[]>([])
  const [showInvite, setShowInvite] = useState(false)
  const [email, setEmail] = useState("")
  const [role, setRole] = useState<Role>("viewer")
  const [invitation, setInvitation] = useState<Invitation | null>(null)
  const [error, setError] = useState("")
  const admin = currentRole === "admin"

  const load = useCallback(async () => {
    setMembers(await api<Member[]>(`/orgs/${organizationId}/members`))
  }, [organizationId])

  useEffect(() => {
    void load()
  }, [load])

  async function invite(event: FormEvent) {
    event.preventDefault()
    setError("")
    try {
      const created = await api<Invitation>(
        `/orgs/${organizationId}/invitations`,
        { method: "POST", body: JSON.stringify({ email, role }) },
      )
      setInvitation(created)
      setEmail("")
    } catch {
      setError("The invitation could not be created.")
    }
  }

  async function changeRole(userId: string, nextRole: Role) {
    setError("")
    try {
      await api(`/orgs/${organizationId}/members/${userId}`, {
        method: "PATCH",
        body: JSON.stringify({ role: nextRole }),
      })
      await load()
    } catch (problem) {
      setError(
        problem instanceof ApiProblem && problem.code === "final_admin_required"
          ? "Every organization must keep one admin."
          : "The member role could not be changed.",
      )
    }
  }

  async function remove(userId: string) {
    setError("")
    try {
      await api(`/orgs/${organizationId}/members/${userId}`, {
        method: "DELETE",
      })
      await load()
    } catch (problem) {
      setError(
        problem instanceof ApiProblem && problem.code === "final_admin_required"
          ? "Every organization must keep one admin."
          : "The member could not be removed.",
      )
    }
  }

  async function copyInvitation() {
    if (!invitation) return
    try {
      await navigator.clipboard.writeText(
        invitation.inviteUrl ?? invitation.token,
      )
    } catch {
      setError("The invitation link could not be copied.")
    }
  }

  return (
    <section className="flex flex-col gap-5 p-4 md:p-6">
      <header className="flex flex-col items-start justify-between gap-3 sm:flex-row sm:items-center">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Members</h1>
          <p className="text-sm text-muted-foreground">Fixed organization roles and invitations.</p>
        </div>
        {admin && <Button onClick={() => setShowInvite(true)}>Invite member</Button>}
      </header>
      {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
      {admin && (
        <Dialog
          open={showInvite}
          onOpenChange={(open) => {
            setShowInvite(open)
            if (!open) setInvitation(null)
          }}
        >
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Create invitation</DialogTitle>
              <DialogDescription>
                Choose an email and fixed organization role.
              </DialogDescription>
            </DialogHeader>
            <form className="grid gap-4" onSubmit={invite}>
              <Field>
                <FieldLabel htmlFor="invite-email">Email</FieldLabel>
                <Input
                  id="invite-email"
                  type="email"
                  required
                  value={email}
                  onChange={(event) => setEmail(event.target.value)}
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="invite-role">Role</FieldLabel>
                <select
                  id="invite-role"
                  className="h-9 rounded-md border bg-background px-3 text-sm"
                  value={role}
                  onChange={(event) => setRole(event.target.value as Role)}
                >
                  {(["admin", "editor", "runner", "viewer"] as Role[]).map((value) => (
                    <option key={value} value={value}>{value}</option>
                  ))}
                </select>
              </Field>
              <Button className="sm:justify-self-end" type="submit">
                Create invitation
              </Button>
            </form>
            {invitation && (
              <div aria-live="polite" className="rounded-md border bg-muted/40 p-3">
                <p className="text-xs text-muted-foreground">Shown once; expires {invitation.expiresAt}</p>
                <div className="mt-2 flex items-center gap-2">
                  <code className="min-w-0 flex-1 break-all text-xs">
                    {invitation.inviteUrl ?? invitation.token}
                  </code>
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    onClick={() => void copyInvitation()}
                  >
                    Copy link
                  </Button>
                </div>
              </div>
            )}
          </DialogContent>
        </Dialog>
      )}
      <Card className="overflow-x-auto">
        <Table>
          <TableHeader>
            <TableRow><TableHead>Email</TableHead><TableHead>Role</TableHead><TableHead /></TableRow>
          </TableHeader>
          <TableBody>
            {members.map((member) => (
              <TableRow key={member.userId}>
                <TableCell>{member.email}</TableCell>
                <TableCell>
                  {admin ? (
                    <select
                      aria-label={`Role for ${member.email}`}
                      className="h-8 rounded-md border bg-background px-2 text-sm"
                      value={member.role}
                      onChange={(event) => void changeRole(member.userId, event.target.value as Role)}
                    >
                      {(["admin", "editor", "runner", "viewer"] as Role[]).map((value) => (
                        <option key={value} value={value}>{value}</option>
                      ))}
                    </select>
                  ) : member.role}
                </TableCell>
                <TableCell className="text-right">
                  {admin && member.userId !== currentUserId && (
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => void remove(member.userId)}
                    >
                      Remove
                    </Button>
                  )}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </Card>
    </section>
  )
}
```

Create `apps/web/src/pages/accept-invitation-page.tsx`:

```tsx
import { useEffect, useRef, useState } from "react"
import { useLocation, useNavigate } from "react-router-dom"

import { api } from "@/api/client"
import { useAuth } from "@/auth/auth-context"

interface AcceptedInvitation {
  organizationId: string
  role: string
}

export function AcceptInvitationPage() {
  const location = useLocation()
  const navigate = useNavigate()
  const { refresh } = useAuth()
  const [error, setError] = useState("")
  const started = useRef(false)

  useEffect(() => {
    if (started.current) return
    started.current = true
    const token = new URLSearchParams(location.hash.slice(1)).get("token")
    if (!token) {
      setError("This invitation is invalid or expired.")
      return
    }
    void api<AcceptedInvitation>("/invitations/accept", {
      method: "POST",
      body: JSON.stringify({ token }),
    })
      .then(async (accepted) => {
        await refresh()
        navigate(`/orgs/${accepted.organizationId}`, { replace: true })
      })
      .catch(() => setError("This invitation is invalid or expired."))
  }, [location.hash, navigate, refresh])

  return (
    <main className="grid min-h-screen place-items-center p-6">
      <p className="text-sm text-muted-foreground">
        {error || "Accepting invitation…"}
      </p>
    </main>
  )
}
```

- [ ] **Step 4: Implement audit page and routes**

Replace the `links` constant in `apps/web/src/layout/app-shell.tsx` with:

```tsx
const links = [
  { label: "Overview", suffix: "" },
  { label: "Members", suffix: "/members" },
  { label: "Audit log", suffix: "/audit" },
]
```

Create `apps/web/src/pages/audit-page.tsx`:

```tsx
import { useEffect, useState } from "react"

import { api } from "@/api/client"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

interface AuditEvent {
  id: number
  actorUserId: string | null
  action: string
  resourceType: string
  resourceId: string
  metadata: Record<string, unknown>
  occurredAt: string
}

export function AuditPage({ organizationId }: { organizationId: string }) {
  const [items, setItems] = useState<AuditEvent[]>([])
  useEffect(() => {
    void api<{ items: AuditEvent[] }>(
      `/orgs/${organizationId}/audit-events?limit=50`,
    ).then((response) => setItems(response.items))
  }, [organizationId])

  return (
    <section className="flex flex-col gap-5 p-4 md:p-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Audit log</h1>
        <p className="text-sm text-muted-foreground">Append-only administrative history.</p>
      </div>
      <div className="overflow-x-auto rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Time</TableHead><TableHead>Action</TableHead>
              <TableHead>Resource</TableHead><TableHead>Actor</TableHead>
              <TableHead>Metadata</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {items.map((item) => (
              <TableRow key={item.id}>
                <TableCell>{new Date(item.occurredAt).toLocaleString()}</TableCell>
                <TableCell className="font-mono text-xs">{item.action}</TableCell>
                <TableCell>{item.resourceType}:{item.resourceId}</TableCell>
                <TableCell>{item.actorUserId ?? "system"}</TableCell>
                <TableCell className="font-mono text-xs">{JSON.stringify(item.metadata)}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </section>
  )
}
```

Replace `apps/web/src/app.tsx` with:

```tsx
import { Navigate, Route, Routes, useParams } from "react-router-dom"

import { AuthProvider, useAuth } from "@/auth/auth-context"
import { ProtectedRoute } from "@/auth/protected-route"
import { AppShell } from "@/layout/app-shell"
import { AcceptInvitationPage } from "@/pages/accept-invitation-page"
import { AuditPage } from "@/pages/audit-page"
import { LoginPage } from "@/pages/login-page"
import { MembersPage } from "@/pages/members-page"
import { OrganizationHomePage } from "@/pages/organization-home-page"
import { RegisterPage } from "@/pages/register-page"

function NavigateToFirstOrganization() {
  const { me } = useAuth()
  const first = me?.organizations[0]
  if (!first) {
    return (
      <p className="p-6 text-sm text-muted-foreground">
        No organization memberships.
      </p>
    )
  }
  return <Navigate replace to={`/orgs/${first.organizationId}`} />
}

function useCurrentMembership() {
  const { organizationId } = useParams()
  const { me } = useAuth()
  return me?.organizations.find(
    (organization) => organization.organizationId === organizationId,
  )
}

function MembersRoute() {
  const membership = useCurrentMembership()
  const { me } = useAuth()
  return membership && me ? (
    <MembersPage
      organizationId={membership.organizationId}
      currentRole={membership.role}
      currentUserId={me.user.id}
    />
  ) : (
    <Navigate replace to="/" />
  )
}

function AuditRoute() {
  const membership = useCurrentMembership()
  return membership ? (
    <AuditPage organizationId={membership.organizationId} />
  ) : (
    <Navigate replace to="/" />
  )
}

export function App() {
  return (
    <AuthProvider>
      <Routes>
        <Route path="/login" element={<LoginPage />} />
        <Route path="/register" element={<RegisterPage />} />
        <Route element={<ProtectedRoute />}>
          <Route
            path="/invitations/accept"
            element={<AcceptInvitationPage />}
          />
          <Route path="/orgs/:organizationId" element={<AppShell />}>
            <Route index element={<OrganizationHomePage />} />
            <Route path="members" element={<MembersRoute />} />
            <Route path="audit" element={<AuditRoute />} />
          </Route>
          <Route path="/" element={<NavigateToFirstOrganization />} />
        </Route>
      </Routes>
    </AuthProvider>
  )
}
```

- [ ] **Step 5: Verify GREEN**

Run:

```text
pnpm --filter @corqty/web test
pnpm --filter @corqty/web lint
pnpm --filter @corqty/web build
```

Expected: all tests pass and the production build exits 0.

- [ ] **Step 6: Commit**

```text
git add apps/web
git commit -m "feat: add member and audit administration UI"
```

## Task 13: Add the local Docker Compose stack and smoke test

**Files:**

- Create: `.env.example`
- Create: `docker-compose.yml`
- Create: `docker/rust/Dockerfile`
- Create: `apps/web/Dockerfile`
- Create: `scripts/smoke-foundation.ps1`
- Modify: `README.md`

- [ ] **Step 1: Write the smoke test before Compose implementation**

Create `scripts/smoke-foundation.ps1`:

```powershell
param(
    [string]$ProjectName = "corqty"
)

$ErrorActionPreference = "Stop"
$apiBase = "http://localhost:8080/api/v1"

$health = Invoke-WebRequest -UseBasicParsing "$apiBase/health"
if ($health.StatusCode -ne 204) {
    throw "API health check returned $($health.StatusCode)"
}

$web = Invoke-WebRequest -UseBasicParsing "http://localhost:5173"
if ($web.StatusCode -ne 200 -or $web.Content -notmatch "<title>Corqty</title>") {
    throw "Web application did not serve the Corqty shell"
}

function Register-SmokeUser {
    param(
        [string]$Email,
        [string]$OrganizationName
    )

    $webSession = New-Object Microsoft.PowerShell.Commands.WebRequestSession
    $body = @{
        email = $Email
        password = "correct horse battery staple"
        organizationName = $OrganizationName
    } | ConvertTo-Json
    $registration = Invoke-WebRequest `
        -UseBasicParsing `
        -WebSession $webSession `
        -Method Post `
        -ContentType "application/json" `
        -Body $body `
        "$apiBase/auth/register"
    if ($registration.StatusCode -ne 201) {
        throw "Registration returned $($registration.StatusCode)"
    }
    $csrf = $webSession.Cookies.GetCookies([uri]$apiBase) |
        Where-Object { $_.Name -eq "corqty_csrf" } |
        Select-Object -ExpandProperty Value
    if (-not $csrf) {
        throw "Registration did not set the CSRF cookie"
    }
    [pscustomobject]@{
        Session = $webSession
        Csrf = $csrf
    }
}

$suffix = [guid]::NewGuid().ToString("N")
$admin = Register-SmokeUser `
    -Email "admin-$suffix@example.com" `
    -OrganizationName "Smoke Organization"
$adminMe = Invoke-RestMethod `
    -WebSession $admin.Session `
    "$apiBase/auth/me"
if (
    $adminMe.organizations.Count -ne 1 -or
    $adminMe.organizations[0].role -ne "admin"
) {
    throw "Registration did not create the initial admin membership"
}
$organizationId = $adminMe.organizations[0].organizationId

$invitation = Invoke-RestMethod `
    -WebSession $admin.Session `
    -Method Post `
    -ContentType "application/json" `
    -Headers @{ "x-csrf-token" = $admin.Csrf } `
    -Body (@{
        email = "editor-$suffix@example.com"
        role = "editor"
    } | ConvertTo-Json) `
    "$apiBase/orgs/$organizationId/invitations"
if (-not $invitation.token -or $invitation.inviteUrl -notmatch "#token=") {
    throw "Invitation did not return its one-time fragment link"
}

$editor = Register-SmokeUser `
    -Email "editor-$suffix@example.com" `
    -OrganizationName "Editor Personal"
$accepted = Invoke-RestMethod `
    -WebSession $editor.Session `
    -Method Post `
    -ContentType "application/json" `
    -Headers @{ "x-csrf-token" = $editor.Csrf } `
    -Body (@{ token = $invitation.token } | ConvertTo-Json) `
    "$apiBase/invitations/accept"
if (
    $accepted.organizationId -ne $organizationId -or
    $accepted.role -ne "editor"
) {
    throw "Invitation acceptance returned the wrong membership"
}

$deniedStatus = 0
try {
    $null = Invoke-WebRequest `
        -UseBasicParsing `
        -WebSession $editor.Session `
        -Method Post `
        -ContentType "application/json" `
        -Headers @{ "x-csrf-token" = $editor.Csrf } `
        -Body (@{
            email = "denied-$suffix@example.com"
            role = "viewer"
        } | ConvertTo-Json) `
        "$apiBase/orgs/$organizationId/invitations"
} catch {
    $deniedStatus = [int]$_.Exception.Response.StatusCode
}
if ($deniedStatus -ne 403) {
    throw "Editor member-management request returned $deniedStatus, not 403"
}

$members = Invoke-RestMethod `
    -WebSession $admin.Session `
    "$apiBase/orgs/$organizationId/members"
if (
    @($members).Count -ne 2 -or
    @($members | Where-Object { $_.role -eq "editor" }).Count -ne 1
) {
    throw "Same-tenant member listing did not expose accepted membership"
}

$outsider = Register-SmokeUser `
    -Email "outsider-$suffix@example.com" `
    -OrganizationName "Outside"
$hiddenStatus = 0
try {
    $null = Invoke-WebRequest `
        -UseBasicParsing `
        -WebSession $outsider.Session `
        "$apiBase/orgs/$organizationId/members"
} catch {
    $hiddenStatus = [int]$_.Exception.Response.StatusCode
}
if ($hiddenStatus -ne 404) {
    throw "Cross-tenant member request returned $hiddenStatus, not 404"
}

$audit = Invoke-RestMethod `
    -WebSession $admin.Session `
    "$apiBase/orgs/$organizationId/audit-events?limit=50"
$actions = @($audit.items | ForEach-Object { $_.action })
foreach ($action in @(
    "organization.created",
    "invitation.created",
    "invitation.accepted"
)) {
    if ($actions -notcontains $action) {
        throw "Audit log is missing $action"
    }
}

$services = docker compose --project-name $ProjectName ps --format json |
    ConvertFrom-Json
$required = @("api", "postgres", "web")
foreach ($service in $required) {
    $entry = $services | Where-Object { $_.Service -eq $service }
    if ($null -eq $entry -or $entry.State -ne "running") {
        throw "Missing running service: $service"
    }
    if ($entry.Health -and $entry.Health -ne "healthy") {
        throw "Unhealthy service: $service"
    }
}
```

- [ ] **Step 2: Verify RED**

Run:

```text
docker compose config
```

Expected: command fails because `docker-compose.yml` does not exist.

- [ ] **Step 3: Add documented environment values**

Create `.env.example`:

```dotenv
POSTGRES_DB=corqty
POSTGRES_OWNER_USER=corqty_owner
POSTGRES_OWNER_PASSWORD=corqty_owner_dev
CORQTY_API_DB_PASSWORD=corqty_api_dev
CORQTY_WORKER_DB_PASSWORD=corqty_worker_dev
DATABASE_URL_OWNER=postgres://corqty_owner:corqty_owner_dev@postgres:5432/corqty
DATABASE_URL_API=postgres://corqty_api:corqty_api_dev@postgres:5432/corqty
CORQTY_BIND_ADDRESS=0.0.0.0:8080
CORQTY_PUBLIC_WEB_URL=http://localhost:5173
CORQTY_SECURE_COOKIES=false
VITE_API_PROXY=http://api:8080
RUST_LOG=corqty_api=info,tower_http=info
```

- [ ] **Step 4: Add Rust and web development images**

Create `docker/rust/Dockerfile`:

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

Create `apps/web/Dockerfile`:

```dockerfile
FROM node:24-alpine
RUN corepack enable
WORKDIR /workspace
COPY package.json pnpm-workspace.yaml pnpm-lock.yaml ./
COPY apps/web/package.json apps/web/package.json
RUN pnpm install --frozen-lockfile
COPY apps/web apps/web
CMD ["pnpm", "--filter", "@corqty/web", "dev"]
```

- [ ] **Step 5: Add Compose services**

Create `docker-compose.yml`:

```yaml
services:
  postgres:
    image: postgres:18-alpine
    environment:
      POSTGRES_DB: ${POSTGRES_DB:-corqty}
      POSTGRES_USER: ${POSTGRES_OWNER_USER:-corqty_owner}
      POSTGRES_PASSWORD: ${POSTGRES_OWNER_PASSWORD:-corqty_owner_dev}
      CORQTY_API_DB_PASSWORD: ${CORQTY_API_DB_PASSWORD:-corqty_api_dev}
      CORQTY_WORKER_DB_PASSWORD: ${CORQTY_WORKER_DB_PASSWORD:-corqty_worker_dev}
    ports:
      - "5432:5432"
    volumes:
      - postgres-data:/var/lib/postgresql/data
      - ./docker/postgres/init:/docker-entrypoint-initdb.d:ro
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U ${POSTGRES_OWNER_USER:-corqty_owner} -d ${POSTGRES_DB:-corqty}"]
      interval: 2s
      timeout: 3s
      retries: 30

  migrate:
    build:
      context: .
      dockerfile: docker/rust/Dockerfile
    command: ["cargo", "run", "--locked", "-p", "corqty-migrator"]
    environment:
      DATABASE_URL_OWNER: ${DATABASE_URL_OWNER:-postgres://corqty_owner:corqty_owner_dev@postgres:5432/corqty}
    depends_on:
      postgres:
        condition: service_healthy
    restart: "no"

  api:
    build:
      context: .
      dockerfile: docker/rust/Dockerfile
    environment:
      DATABASE_URL_API: ${DATABASE_URL_API:-postgres://corqty_api:corqty_api_dev@postgres:5432/corqty}
      CORQTY_BIND_ADDRESS: 0.0.0.0:8080
      CORQTY_PUBLIC_WEB_URL: ${CORQTY_PUBLIC_WEB_URL:-http://localhost:5173}
      CORQTY_SECURE_COOKIES: ${CORQTY_SECURE_COOKIES:-false}
      RUST_LOG: ${RUST_LOG:-corqty_api=info,tower_http=info}
    ports:
      - "8080:8080"
    depends_on:
      migrate:
        condition: service_completed_successfully
    healthcheck:
      test: ["CMD", "curl", "--fail", "http://localhost:8080/api/v1/health"]
      interval: 2s
      timeout: 3s
      retries: 30

  web:
    build:
      context: .
      dockerfile: apps/web/Dockerfile
    environment:
      VITE_API_PROXY: ${VITE_API_PROXY:-http://api:8080}
    ports:
      - "5173:5173"
    depends_on:
      api:
        condition: service_healthy

volumes:
  postgres-data:
```

- [ ] **Step 6: Add concise local-development documentation**

Create or replace `README.md` with:

````markdown
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
````

- [ ] **Step 7: Verify GREEN**

Run:

```text
docker compose config
$project = "corqty-smoke-$([guid]::NewGuid().ToString('N'))"
try {
    docker compose --project-name $project up --build --wait
    ./scripts/smoke-foundation.ps1 -ProjectName $project
    docker compose --project-name $project run --rm migrate
} finally {
    docker compose --project-name $project down --volumes
}
```

Expected: Compose validates; a uniquely named project starts with a fresh
PostgreSQL volume; registration creates an initial admin; the smoke script
exits 0; rerunning migrations exits 0; and the fresh stack is removed.

- [ ] **Step 8: Commit**

```text
git add .env.example docker-compose.yml docker scripts apps/web/Dockerfile README.md
git commit -m "chore: add local Docker Compose environment"
```

## Task 14: Add continuous verification and close Milestone 1

**Files:**

- Create: `.github/workflows/ci.yml`
- Modify: `docs/superpowers/specs/2026-07-28-corqty-workflow-orchestrator-design.md`

- [ ] **Step 1: Add CI with the same local commands**

Create `.github/workflows/ci.yml`:

```yaml
name: ci

on:
  push:
  pull_request:

jobs:
  verify:
    runs-on: ubuntu-latest
    timeout-minutes: 30
    services:
      postgres:
        image: postgres:18-alpine
        env:
          POSTGRES_DB: corqty_test
          POSTGRES_USER: corqty_owner
          POSTGRES_PASSWORD: corqty_owner_dev
        ports:
          - 55432:5432
        options: >-
          --health-cmd "pg_isready -U corqty_owner -d corqty_test"
          --health-interval 2s
          --health-timeout 3s
          --health-retries 30
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.96.0
        with:
          components: clippy,rustfmt
      - uses: pnpm/action-setup@v4
        with:
          version: 10.33.2
      - uses: actions/setup-node@v4
        with:
          node-version: 24
          cache: pnpm
      - name: Create database roles
        env:
          PGPASSWORD: corqty_owner_dev
        run: |
          psql -h localhost -p 55432 -U corqty_owner -d corqty_test \
            -c "CREATE ROLE corqty_api LOGIN PASSWORD 'corqty_api_dev' NOBYPASSRLS"
          psql -h localhost -p 55432 -U corqty_owner -d corqty_test \
            -c "CREATE ROLE corqty_worker LOGIN PASSWORD 'corqty_worker_dev' NOBYPASSRLS"
      - run: cargo fmt --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace
      - run: pnpm install --frozen-lockfile
      - run: pnpm web:lint
      - run: pnpm web:test
      - run: pnpm web:build
      - name: Start fresh Compose stack
        run: docker compose --project-name corqty-ci up --build --wait
      - name: Run Compose smoke test
        shell: pwsh
        run: ./scripts/smoke-foundation.ps1 -ProjectName corqty-ci
      - name: Remove Compose stack
        if: always()
        run: docker compose --project-name corqty-ci down --volumes
```

- [ ] **Step 2: Run the complete fresh verification**

Run:

```text
$testProject = "corqty-test-$([guid]::NewGuid().ToString('N'))"
$smokeProject = "corqty-smoke-$([guid]::NewGuid().ToString('N'))"
try {
    docker compose `
        --project-name $testProject `
        -f docker-compose.test.yml `
        up -d --wait
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    pnpm install --frozen-lockfile
    pnpm web:lint
    pnpm web:test
    pnpm web:build
    docker compose config
    docker compose --project-name $smokeProject up --build --wait
    ./scripts/smoke-foundation.ps1 -ProjectName $smokeProject
    git diff --check
} finally {
    docker compose --project-name $smokeProject down --volumes
    docker compose `
        --project-name $testProject `
        -f docker-compose.test.yml `
        down --volumes
}
```

Expected:

- Rust formatting and Clippy exit 0 with no warnings.
- All Rust tests pass.
- All frontend tests pass.
- TypeScript and Vite production build pass.
- Fresh test and development Compose projects start, pass, and are removed with
  their disposable volumes.
- `git diff --check` prints nothing.

- [ ] **Step 3: Update the master spec status**

Change only the master spec status line to:

```markdown
**Status:** Approved; Milestone 1 foundation implemented
```

Do not mark later milestone acceptance criteria complete.

- [ ] **Step 4: Commit**

```text
git add .github docs/superpowers/specs/2026-07-28-corqty-workflow-orchestrator-design.md
git commit -m "ci: verify foundation milestone"
```
