# Corqty Milestone 2 Catalog and Studio Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Harden the Milestone 1 foundation, move the web app to Next.js, and deliver Milestone 2 from the approved design: versioned node definitions and blueprints, draft persistence with optimistic revisions, server-authoritative publish validation into immutable checksummed versions, schema-driven widgets, and an XYFlow workflow studio.

**Architecture:** The Axum API stays the only backend. Each organization-scoped request is authorized once, inside the same tenant transaction that does the work, and repository functions take that connection. Graph, schema, catalog and publish rules live in `corqty-domain` with no HTTP or SQL. PostgreSQL stores the catalog behind row-level security; system definitions and blueprints are seeded by an idempotent SQL function. The Next.js App Router app is client-heavy: every API call goes through a same-origin `/api/v1` rewrite, server state lives in TanStack Query, and the studio canvas is a client-only XYFlow editor that applies the same connection rules as the backend through a shared fixture.

**Tech Stack:** Rust 1.96 / edition 2024, Axum 0.8, SQLx 0.8, PostgreSQL 18, jsonschema 0.60 (Draft 2020-12, offline), sha2, Next.js 16.4 App Router, React 19.3, TypeScript 7, TanStack Query 5, @xyflow/react 12, Tailwind CSS 4, shadcn/ui (Base UI), Vitest, Testing Library, MSW 3, oxlint, Docker Compose.

---

## Scope boundary

This plan implements Milestone 2 ("Catalog and studio") of the approved design. It also contains the five hardening changes from the post-Milestone 1 architecture review as its opening tasks (Tasks 1-5) and the move of the web app from Vite and React Router to Next.js (Tasks 6-7). It intentionally excludes runs, workers, executor execution, SSE, secrets storage, triggers, service tokens, runtime plugins, the shell executor preset, rate limiting and deployment-configurable limits; those belong to Milestones 3-5. Secret references are representable in drafts but cannot be published until secrets exist.

The repository must stay runnable after every task. Task 4 changes how the database roles are created, so local databases must be recreated once: `docker compose down -v` and `docker compose -f docker-compose.test.yml down -v`.

Tasks 20 and 21 close resource-exhaustion and draft-synchronization defects that an adversarial review found in the code from Tasks 9-18. They are separate tasks because each fix cuts across several earlier tasks. Do not deploy a build that stops before Task 21.

Every section was produced by implementing the task test-first in a scratch clone and then replayed from its parent commit; the replayed tree matched the verified commit byte for byte. The `Expected:` lines quote real output from those runs.

## Decisions settled for this milestone

- Node definitions are referenced from graphs by `{key, version}`. System keys start with `corqty.`; organization keys must not. Definitions and blueprints are immutable; definitions can be deprecated. There are no delete endpoints, and custom definitions and organization blueprints are created through the API only.
- Audit events are written in the same transaction as the change: `workflow.created`, `workflow.draft_saved` (`{revision}`), `workflow.published` (`{version, draftRevision, checksum}`), `node_definition.created`, `node_definition.deprecated` and `blueprint.created` (`{key, version}`). They never contain configuration or literal values.
- The only executor in Milestone 2 is the built-in `mock@1.0.0`. The shell preset is deferred to Milestone 3.
- Secret bindings and `secretReference` config values are saved in drafts, but publish reports `secret_not_found` for every reference until secrets exist (Milestone 4). Plaintext in a secret field is rejected.
- The version checksum is the SHA-256 of the graph serialized with sorted keys (server side), including the viewport.
- Draft saves only check that the graph deserializes and stays within 500 nodes and 1,000 edges. Validate checks a graph without saving. Publish validates the stored draft at an exact revision, and each draft revision can be published at most once.
- Applying a blueprint is `POST /workflows {name, blueprintId}`; the server copies the graph with fresh node and edge ids.
- Graph and timeout limits are fixed constants in Milestone 2; deployment configuration is deferred to Milestone 5.
- User JSON Schemas are self-contained (no `$ref`, `$dynamicRef` or `$recursiveRef`), Draft 2020-12 throughout and at most 16 KiB each. Definitions have at most 50 ports per side, validation reports at most 200 diagnostics, and validation runs on blocking threads.
- Only components that keep status (saved, saving, conflict, error, simulated, read-only) may use raw palette colors; everything else uses the monochrome semantic tokens.

## File map

### Database, infrastructure, and CI

- `docker/postgres/init/001-roles.sh`: creates the non-superuser, BYPASSRLS database owner `corqty_owner` plus the API and worker roles.
- `docker-compose.yml`, `docker-compose.test.yml`, `.env.example`: bootstrap superuser `postgres`, owner password, Next.js web service on port 3000 with a healthcheck.
- `.github/workflows/ci.yml`: runs the same role script instead of inline `CREATE ROLE`.
- `migrations/202610250001_workflow_catalog.sql`: catalog tables, RLS, grants, and `corqty.seed_system_catalog()` with the system definitions and blueprints.
- `scripts/smoke-foundation.ps1`: Compose smoke test extended to the catalog, draft save, publish and cross-tenant checks.
- `README.md`, `docs/superpowers/specs/2026-07-28-corqty-workflow-orchestrator-design.md`: Next.js, port 3000, and the Milestone 2 decisions.

### Rust crates

- `crates/domain`: capabilities per role, diagnostics, slugs, graph snapshot and connection rules, JSON Schema helpers, node-definition validation, publish validation and checksums; shared rules fixture under `tests/fixtures`.
- `crates/persistence`: `'static` tenant transactions, connection-based member/audit repositories, catalog and workflow repositories, catalog guard and reset tests.
- `crates/api`: `authz` (authorize inside the work transaction), typed problems with diagnostics, `ApiJson`/`ApiPath` extractors, catalog and workflow routes.
- `crates/test-support`: table-agnostic reset that reseeds system data.

### Web application (`apps/web`)

- `next.config.ts`, `postcss.config.mjs`, `vitest.config.ts`, `.oxlintrc.json`, `tsconfig.json`: Next.js, Tailwind, Vitest and lint configuration.
- `src/app`: App Router routes, layouts and providers.
- `src/api`: fetch client with problem diagnostics, types, TanStack Query hooks.
- `src/auth`: auth context, auth gate, return-to handling, `useCan`.
- `src/views`: screens (login, register, invitation, members, audit, workflows, catalog).
- `src/studio`: graph model, canvas, node, palette, inspector, widgets, draft autosave, diagnostics.
- `src/components/status`: the only place raw status colors are allowed.
- `src/test`: MSW server and handlers, render helper, raw-color scan.

## Task 1: Authorize organization requests once, inside the work transaction

**Files:**

- Create: `crates/api/src/authz.rs`
- Modify: `crates/domain/src/rbac.rs`
- Modify: `crates/persistence/src/tenant.rs`
- Modify: `crates/persistence/src/store.rs`
- Modify: `crates/persistence/src/organizations.rs`
- Modify: `crates/persistence/src/members.rs`
- Modify: `crates/persistence/src/audit.rs`
- Modify: `crates/api/src/lib.rs`
- Modify: `crates/api/src/audit.rs`
- Modify: `crates/api/src/organizations/members.rs`
- Modify: `crates/api/src/auth/service.rs`
- Modify: `crates/api/tests/support/mod.rs`
- Test: `crates/domain/tests/account_and_rbac.rs`
- Test: `crates/api/tests/auth_http.rs`
- Test: `crates/api/tests/authorization_matrix.rs`

Today every member, invitation and audit handler asks `PgStore::membership_role` for the caller's role in one
transaction and then does its work in a second one. This task reads the role inside the transaction that does the
work: `authorize` opens the tenant transaction, reads the role there and checks a `Capability`, and the handler
finishes with `OrgAccess::commit`. Repository functions take `conn: &mut PgConnection` and never open or commit
transactions themselves. `/auth/me` also starts returning each organization's `capabilities` so the web app can stop
comparing role strings.

- [ ] **Step 1: Write the failing tests**

The domain test pins `Capability::ALL` and `Role::capabilities`. The `/auth/me` test pins the snake_case wire
names. `authorization_matrix.rs` holds one fixture per test function: an admin's organization, an editor, a runner
and a viewer who joined it through invitations, and an outsider who only belongs to its own organization. Each row
asserts only outcomes that do not depend on the order rows run in: a read returns 200 for every role holding its
capability, a write returns 403 for every role lacking it (roles holding it are not called, so no row changes shared
state), and every route returns 404 for the outsider. Successful writes stay in the feature tests. Later tasks
append rows for their routes. The member and audit rows also hold on the current code, because they pin the
behaviour the refactor must keep. The second test in the file needs `corqty_api::authz`: it checks that
`OrgAccess::commit` turns the deferred `final_admin_required` trigger error into a 409 conflict.

Replace the whole of `crates/domain/tests/account_and_rbac.rs`:

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
fn roles_list_their_capabilities_in_matrix_order() {
    assert_eq!(Role::Admin.capabilities(), Capability::ALL);
    assert_eq!(
        Role::Editor.capabilities(),
        [
            Capability::ViewOrganization,
            Capability::EditWorkflows,
            Capability::PublishWorkflows,
            Capability::RunWorkflows,
        ]
    );
    assert_eq!(
        Role::Runner.capabilities(),
        [Capability::ViewOrganization, Capability::RunWorkflows]
    );
    assert_eq!(Role::Viewer.capabilities(), [Capability::ViewOrganization]);
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

In `crates/api/tests/auth_http.rs`:

Replace:

```rust
    assert_eq!(json["organizations"][0]["role"], "admin");
}
```

With:

```rust
    assert_eq!(json["organizations"][0]["role"], "admin");
    assert_eq!(
        json["organizations"][0]["capabilities"],
        serde_json::json!([
            "view_organization",
            "edit_workflows",
            "publish_workflows",
            "run_workflows",
            "manage_members",
            "manage_service_tokens",
            "manage_secrets",
            "manage_plugins",
            "enable_shell_executor"
        ])
    );
}
```

Rows send every request through one helper, so make it public. In `crates/api/tests/support/mod.rs`:

Replace:

```rust
    async fn request_json(&self, method: Method, path: &str, json: &str) -> Response {
```

With:

```rust
    pub async fn request_json(&self, method: Method, path: &str, json: &str) -> Response {
```

Create `crates/api/tests/authorization_matrix.rs`:

```rust
mod support;

use axum::{
    body::to_bytes,
    http::{Method, StatusCode},
};
use chrono::Utc;
use corqty_api::{authz::authorize, config::ApiConfig, error::ApiError, state::AppState};
use corqty_domain::rbac::{Capability, Role};
use corqty_persistence::{models::SessionRecord, store::PgStore};
use corqty_test_support::TestDatabase;
use serial_test::serial;
use support::AuthFixture;
use uuid::Uuid;

/// An admin's organization with an editor, a runner and a viewer who joined
/// through invitations, plus an outsider who only belongs to its own org.
struct Matrix {
    organization_id: Uuid,
    admin_user_id: Uuid,
    members: Vec<(Role, AuthFixture)>,
    outsider: AuthFixture,
}

impl Matrix {
    async fn new() -> Self {
        let admin = AuthFixture::registered("admin@example.com", "Acme").await;
        let mut members = Vec::new();
        for role in [Role::Editor, Role::Runner, Role::Viewer] {
            let email = format!("{role}@example.com");
            let invitation = admin
                .post_json(
                    &format!("/api/v1/orgs/{}/invitations", admin.organization_id),
                    &serde_json::json!({ "email": email, "role": role.as_str() }).to_string(),
                )
                .await;
            assert_eq!(invitation.status(), StatusCode::CREATED);
            let body = to_bytes(invitation.into_body(), 64 * 1024).await.unwrap();
            let invitation: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let member = admin.register_another(&email, "Personal").await;
            let accepted = member
                .post_json(
                    "/api/v1/invitations/accept",
                    &serde_json::json!({ "token": invitation["token"] }).to_string(),
                )
                .await;
            assert_eq!(accepted.status(), StatusCode::OK);
            members.push((role, member));
        }
        let outsider = admin
            .register_another("outsider@example.com", "Outside")
            .await;
        let organization_id = admin.organization_id;
        let admin_user_id = admin.user_id;
        members.insert(0, (Role::Admin, admin));
        Self {
            organization_id,
            admin_user_id,
            members,
            outsider,
        }
    }

    /// Reads must succeed for every role holding the capability; writes are
    /// only asserted for roles lacking it, so no row changes shared state.
    async fn assert_row(&self, method: Method, path: &str, body: &str, capability: Capability) {
        for (role, member) in &self.members {
            let expected = match (role.allows(capability), method == Method::GET) {
                (false, _) => StatusCode::FORBIDDEN,
                (true, true) => StatusCode::OK,
                (true, false) => continue,
            };
            let response = member.request_json(method.clone(), path, body).await;
            assert_eq!(response.status(), expected, "{role} {method} {path}");
        }
        let response = self.outsider.request_json(method.clone(), path, body).await;
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "outsider {method} {path}"
        );
    }
}

#[tokio::test]
#[serial]
async fn member_and_audit_routes_enforce_capabilities() {
    let matrix = Matrix::new().await;
    let org = format!("/api/v1/orgs/{}", matrix.organization_id);
    let member = format!("{org}/members/{}", matrix.admin_user_id);
    for (method, path, body, capability) in [
        (
            Method::GET,
            format!("{org}/members"),
            "",
            Capability::ViewOrganization,
        ),
        (
            Method::GET,
            format!("{org}/audit-events"),
            "",
            Capability::ViewOrganization,
        ),
        (
            Method::POST,
            format!("{org}/invitations"),
            r#"{"email":"new@example.com","role":"viewer"}"#,
            Capability::ManageMembers,
        ),
        (
            Method::PATCH,
            member.clone(),
            r#"{"role":"viewer"}"#,
            Capability::ManageMembers,
        ),
        (Method::DELETE, member, "", Capability::ManageMembers),
    ] {
        matrix.assert_row(method, &path, body, capability).await;
    }
}

#[tokio::test]
#[serial]
async fn org_access_commit_reports_the_deferred_final_admin_check_as_conflict() {
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
    let state = AppState::new(PgStore::new(database.api), ApiConfig::for_tests());
    let session = SessionRecord {
        session_id: Uuid::new_v4(),
        user_id,
        email: "admin@example.com".to_owned(),
        csrf_hash: Vec::new(),
        expires_at: Utc::now(),
    };

    let mut access = authorize(&state, &session, organization_id, Capability::ManageMembers)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE memberships SET role = 'viewer'
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(user_id)
    .execute(&mut *access.tx)
    .await
    .unwrap();

    assert!(matches!(
        access.commit().await,
        Err(ApiError::Conflict("final_admin_required"))
    ));
}
```

- [ ] **Step 2: Run the tests and verify RED**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-domain --test account_and_rbac
```

Expected: compilation fails with ``error[E0599]: no method named `capabilities` found for enum `Role` in the
current scope`` and ``error[E0599]: no variant, associated function, or constant named `ALL` found for enum
`Capability` in the current scope`` (5 errors).

```text
cargo test -p corqty-api --test authorization_matrix
```

Expected: compilation fails with ``error[E0432]: unresolved import `corqty_api::authz` ``.

```text
cargo test -p corqty-api --test auth_http
```

Expected: `register_sets_session_and_me_authenticates_it` fails at the new assertion:

```text
  left: Null
 right: Array [String("view_organization"), String("edit_workflows"), String("publish_workflows"), String("run_workflows"), String("manage_members"), String("manage_service_tokens"), String("manage_secrets"), String("manage_plugins"), String("enable_shell_executor")]
```

The result line is `test result: FAILED. 2 passed; 1 failed`.

- [ ] **Step 3: Give roles a serializable capability list**

`Capability` serializes as snake_case, `Capability::ALL` lists the nine capabilities in matrix order, and
`Role::capabilities` filters that list through `Role::allows`.

Replace the whole of `crates/domain/src/rbac.rs`:

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
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

impl Capability {
    pub const ALL: [Self; 9] = [
        Self::ViewOrganization,
        Self::EditWorkflows,
        Self::PublishWorkflows,
        Self::RunWorkflows,
        Self::ManageMembers,
        Self::ManageServiceTokens,
        Self::ManageSecrets,
        Self::ManagePlugins,
        Self::EnableShellExecutor,
    ];
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
    pub fn capabilities(self) -> Vec<Capability> {
        Capability::ALL
            .into_iter()
            .filter(|capability| self.allows(*capability))
            .collect()
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

- [ ] **Step 4: Run member, invitation and audit repositories on the caller's connection**

`begin_tenant` and `begin_user` return `Transaction<'static, Postgres>` (what `PgPool::begin` already yields), so
an `OrgAccess` can own one. `PgStore::begin_tenant` delegates to `tenant::begin_tenant`; the unused
`PgStore::pool()` goes away.

Replace the whole of `crates/persistence/src/tenant.rs`:

```rust
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

pub async fn begin_tenant(
    pool: &PgPool,
    user_id: Uuid,
    organization_id: Uuid,
) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
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
) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('corqty.user_id', $1, true)")
        .bind(user_id.to_string())
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}
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

    pub async fn begin_tenant(
        &self,
        user_id: Uuid,
        organization_id: Uuid,
    ) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
        crate::tenant::begin_tenant(&self.pool, user_id, organization_id).await
    }
}
```

`member_role` replaces `PgStore::membership_role`. It runs on the caller's connection and decodes the role, so a
role string the domain does not know is a decode error rather than a silent denial.

Replace the whole of `crates/persistence/src/organizations.rs`:

```rust
use corqty_domain::rbac::Role;
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
```

`insert_in_transaction` takes `conn: &mut PgConnection`; the registration and organization callers keep passing
`&mut tx`, which derefs to the connection. `list_audit_events` becomes a free function.
`insert_user_event_in_transaction` is unchanged.

Replace the whole of `crates/persistence/src/audit.rs`:

```rust
use serde::Serialize;
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

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
    conn: &mut PgConnection,
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
    .execute(conn)
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
    sqlx::query("SELECT corqty.audit_current_user_memberships($1, $2, $3, $4)")
        .bind(action)
        .bind(resource_type)
        .bind(resource_id)
        .bind(metadata)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub async fn list_audit_events(
    conn: &mut PgConnection,
    organization_id: Uuid,
    before_id: Option<i64>,
    limit: i64,
) -> Result<Vec<AuditEventRecord>, sqlx::Error> {
    sqlx::query_as(
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
    .fetch_all(conn)
    .await
}
```

Invitation and member mutations become free functions that take the connection and an explicit actor and no
longer commit. The deferred final-admin trigger therefore fires in `OrgAccess::commit`, so
`MemberMutationError::Database` uses a plain `#[from]`. `accept_invitation` keeps its own transaction because it
is not scoped to an organization the caller already belongs to.

In `crates/persistence/src/members.rs`:

Replace:

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
    Database(sqlx::Error),
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
```

With:

```rust
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{store::PgStore, tenant::begin_user};

#[derive(Debug, thiserror::Error)]
pub enum MemberMutationError {
    #[error("the final admin cannot be changed")]
    FinalAdmin,
    #[error("member was not found")]
    NotFound,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}
```

Replace:

```rust
impl PgStore {
    pub async fn create_invitation(
        &self,
        invitation: NewInvitation<'_>,
    ) -> Result<(), sqlx::Error> {
        let mut tx = begin_tenant(
            &self.pool,
            invitation.actor_user_id,
            invitation.organization_id,
        )
        .await?;
        sqlx::query(
            "INSERT INTO invitations(
                id, organization_id, email, role,
                token_hash, invited_by, expires_at
             ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(invitation.invitation_id)
        .bind(invitation.organization_id)
        .bind(invitation.email)
        .bind(invitation.role)
        .bind(invitation.token_hash)
        .bind(invitation.actor_user_id)
        .bind(invitation.expires_at)
        .execute(&mut *tx)
        .await?;
        crate::audit::insert_in_transaction(
            &mut tx,
            invitation.organization_id,
            invitation.actor_user_id,
            "invitation.created",
            "invitation",
            &invitation.invitation_id.to_string(),
            serde_json::json!({
                "email": invitation.email,
                "role": invitation.role,
            }),
        )
        .await?;
        tx.commit().await
    }

    pub async fn accept_invitation(
```

With:

```rust
pub async fn create_invitation(
    conn: &mut PgConnection,
    invitation: NewInvitation<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO invitations(
            id, organization_id, email, role,
            token_hash, invited_by, expires_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(invitation.invitation_id)
    .bind(invitation.organization_id)
    .bind(invitation.email)
    .bind(invitation.role)
    .bind(invitation.token_hash)
    .bind(invitation.actor_user_id)
    .bind(invitation.expires_at)
    .execute(&mut *conn)
    .await?;
    crate::audit::insert_in_transaction(
        conn,
        invitation.organization_id,
        invitation.actor_user_id,
        "invitation.created",
        "invitation",
        &invitation.invitation_id.to_string(),
        serde_json::json!({
            "email": invitation.email,
            "role": invitation.role,
        }),
    )
    .await
}

impl PgStore {
    pub async fn accept_invitation(
```

Replace:

```rust
        tx.commit().await?;
        Ok(accepted)
    }

    pub async fn list_members(
        &self,
        actor_user_id: Uuid,
        organization_id: Uuid,
    ) -> Result<Vec<MemberRecord>, sqlx::Error> {
        let mut tx = begin_tenant(&self.pool, actor_user_id, organization_id).await?;
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
```

With:

```rust
        tx.commit().await?;
        Ok(accepted)
    }
}

pub async fn list_members(
    conn: &mut PgConnection,
    organization_id: Uuid,
) -> Result<Vec<MemberRecord>, sqlx::Error> {
    sqlx::query_as(
        "SELECT users.id AS user_id, users.email, memberships.role
         FROM memberships
         JOIN users ON users.id = memberships.user_id
         WHERE memberships.organization_id = $1
         ORDER BY users.email, users.id",
    )
    .bind(organization_id)
    .fetch_all(conn)
    .await
}
```

Replace:

```rust
    pub async fn change_member_role(
        &self,
        actor_user_id: Uuid,
        organization_id: Uuid,
        target_user_id: Uuid,
        new_role: &str,
    ) -> Result<(), MemberMutationError> {
        let mut tx = begin_tenant(&self.pool, actor_user_id, organization_id).await?;
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
        tx.commit().await.map_err(MemberMutationError::from)
    }
```

With:

```rust
pub async fn change_member_role(
    conn: &mut PgConnection,
    actor_user_id: Uuid,
    organization_id: Uuid,
    target_user_id: Uuid,
    new_role: &str,
) -> Result<(), MemberMutationError> {
    lock_organization(conn, organization_id).await?;
    let current_role: Option<String> = sqlx::query_scalar(
        "SELECT role FROM memberships
         WHERE organization_id = $1 AND user_id = $2
         FOR UPDATE",
    )
    .bind(organization_id)
    .bind(target_user_id)
    .fetch_optional(&mut *conn)
    .await?;
    let current_role = current_role.ok_or(MemberMutationError::NotFound)?;
    if current_role == "admin"
        && new_role != "admin"
        && admin_count(conn, organization_id).await? == 1
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
    .execute(&mut *conn)
    .await?;
    crate::audit::insert_in_transaction(
        conn,
        organization_id,
        actor_user_id,
        "membership.role_changed",
        "membership",
        &target_user_id.to_string(),
        serde_json::json!({ "role": new_role }),
    )
    .await?;
    Ok(())
}
```

Replace:

```rust
    pub async fn remove_member(
        &self,
        actor_user_id: Uuid,
        organization_id: Uuid,
        target_user_id: Uuid,
    ) -> Result<(), MemberMutationError> {
        let mut tx = begin_tenant(&self.pool, actor_user_id, organization_id).await?;
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
        if current_role == "admin" && admin_count(&mut tx, organization_id).await? == 1 {
            return Err(MemberMutationError::FinalAdmin);
        }
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
```

With:

```rust
pub async fn remove_member(
    conn: &mut PgConnection,
    actor_user_id: Uuid,
    organization_id: Uuid,
    target_user_id: Uuid,
) -> Result<(), MemberMutationError> {
    lock_organization(conn, organization_id).await?;
    let current_role: Option<String> = sqlx::query_scalar(
        "SELECT role FROM memberships
         WHERE organization_id = $1 AND user_id = $2
         FOR UPDATE",
    )
    .bind(organization_id)
    .bind(target_user_id)
    .fetch_optional(&mut *conn)
    .await?;
    let current_role = current_role.ok_or(MemberMutationError::NotFound)?;
    if current_role == "admin" && admin_count(conn, organization_id).await? == 1 {
        return Err(MemberMutationError::FinalAdmin);
    }
    crate::audit::insert_in_transaction(
        conn,
        organization_id,
        actor_user_id,
        "membership.removed",
        "membership",
        &target_user_id.to_string(),
        serde_json::json!({}),
    )
    .await?;
    sqlx::query(
        "DELETE FROM memberships
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(target_user_id)
    .execute(conn)
    .await?;
    Ok(())
}
```

Replace:

```rust
async fn lock_organization(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    organization_id: Uuid,
) -> Result<(), sqlx::Error> {
    let _: Uuid = sqlx::query_scalar("SELECT id FROM organizations WHERE id = $1 FOR UPDATE")
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

With:

```rust
async fn lock_organization(
    conn: &mut PgConnection,
    organization_id: Uuid,
) -> Result<(), sqlx::Error> {
    let _: Uuid = sqlx::query_scalar("SELECT id FROM organizations WHERE id = $1 FOR UPDATE")
        .bind(organization_id)
        .fetch_one(conn)
        .await?;
    Ok(())
}

async fn admin_count(conn: &mut PgConnection, organization_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM memberships
         WHERE organization_id = $1 AND role = 'admin'",
    )
    .bind(organization_id)
    .fetch_one(conn)
    .await
}
```

- [ ] **Step 5: Authorize once per request in the API**

`authorize` opens the tenant transaction, reads the role inside it and checks the capability. A non-member, or a
request naming another organization, gets 404 `not_found`; a member lacking the capability gets 403 `forbidden`.
While an `OrgAccess` is alive the handler does all database work through `&mut access.tx` (never `&mut *access.tx`,
which trips `clippy::explicit_auto_deref`) and never touches the pool: the API test pool has one connection and a
second acquire would block for 30 seconds.

Create `crates/api/src/authz.rs`:

```rust
use corqty_domain::rbac::{Capability, Role};
use corqty_persistence::{models::SessionRecord, organizations::member_role};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{error::ApiError, state::AppState};

/// An open tenant transaction for a member whose role was read inside it.
/// Handlers do all database work through `&mut access.tx` and finish with
/// `access.commit()`; dropping it rolls back.
pub struct OrgAccess {
    pub tx: Transaction<'static, Postgres>,
    pub organization_id: Uuid,
    pub user_id: Uuid,
    pub role: Role,
}

/// Non-members (including requests naming another organization) get 404;
/// members lacking the capability get 403.
pub async fn authorize(
    state: &AppState,
    session: &SessionRecord,
    organization_id: Uuid,
    capability: Capability,
) -> Result<OrgAccess, ApiError> {
    let mut tx = state
        .store
        .begin_tenant(session.user_id, organization_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    let role = member_role(&mut tx, organization_id, session.user_id)
        .await
        .map_err(|_| ApiError::Internal)?
        .ok_or(ApiError::NotFound)?;
    if !role.allows(capability) {
        return Err(ApiError::Forbidden);
    }
    Ok(OrgAccess {
        tx,
        organization_id,
        user_id: session.user_id,
        role,
    })
}

impl OrgAccess {
    /// The final-admin trigger is deferred, so it can only fail here.
    pub async fn commit(self) -> Result<(), ApiError> {
        self.tx.commit().await.map_err(|error| {
            if error
                .as_database_error()
                .is_some_and(|database| database.message() == "final_admin_required")
            {
                ApiError::Conflict("final_admin_required")
            } else {
                ApiError::Internal
            }
        })
    }
}
```

Replace the whole of `crates/api/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod audit;
pub mod auth;
pub mod authz;
pub mod config;
pub mod error;
pub mod organizations;
pub mod router;
pub mod state;
```

Replace the whole of `crates/api/src/audit.rs`:

```rust
use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use corqty_domain::rbac::Capability;
use corqty_persistence::models::SessionRecord;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{authz::authorize, error::ApiError, state::AppState};

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
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let items = corqty_persistence::audit::list_audit_events(
        &mut access.tx,
        organization_id,
        query.before_id,
        query.limit.unwrap_or(50),
    )
    .await
    .map_err(|_| ApiError::Internal)?;
    access.commit().await?;
    Ok(Json(AuditResponse { items }))
}
```

The member and invitation handlers use `authorize`; `require_membership` and `require_admin` are deleted. Reads
commit too, so the connection goes back to the pool without a pending rollback.

In `crates/api/src/organizations/members.rs`:

Replace:

```rust
use corqty_persistence::{
    members::{MemberMutationError, MemberRecord, NewInvitation},
    models::SessionRecord,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{auth::crypto::SecretToken, error::ApiError, state::AppState};
```

With:

```rust
use corqty_persistence::{
    members::{self, MemberMutationError, MemberRecord, NewInvitation},
    models::SessionRecord,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{auth::crypto::SecretToken, authz::authorize, error::ApiError, state::AppState};
```

Replace:

```rust
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
        .create_invitation(NewInvitation {
            actor_user_id: session.user_id,
            organization_id,
            invitation_id: Uuid::new_v4(),
            email: email.as_str(),
            role: role.as_str(),
            token_hash: &token.hash,
            expires_at,
        })
        .await
        .map_err(|_| ApiError::Internal)?;
```

With:

```rust
) -> Result<(StatusCode, Json<InvitationResponse>), ApiError> {
    let mut access =
        authorize(&state, &session, organization_id, Capability::ManageMembers).await?;
    let email = NormalizedEmail::parse(&request.email)
        .map_err(|_| ApiError::Validation("invalid_email"))?;
    let role = request
        .role
        .parse::<Role>()
        .map_err(|_| ApiError::Validation("invalid_role"))?;
    let token = SecretToken::generate()?;
    let expires_at = Utc::now() + Duration::days(7);
    members::create_invitation(
        &mut access.tx,
        NewInvitation {
            actor_user_id: access.user_id,
            organization_id,
            invitation_id: Uuid::new_v4(),
            email: email.as_str(),
            role: role.as_str(),
            token_hash: &token.hash,
            expires_at,
        },
    )
    .await
    .map_err(|_| ApiError::Internal)?;
    access.commit().await?;
```

Replace:

```rust
) -> Result<Json<Vec<MemberRecord>>, ApiError> {
    require_membership(&state, &session, organization_id).await?;
    let members = state
        .store
        .list_members(session.user_id, organization_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(Json(members))
```

With:

```rust
) -> Result<Json<Vec<MemberRecord>>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let members = members::list_members(&mut access.tx, organization_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    access.commit().await?;
    Ok(Json(members))
```

Replace:

```rust
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
```

With:

```rust
    Json(request): Json<ChangeRoleRequest>,
) -> Result<StatusCode, ApiError> {
    let mut access =
        authorize(&state, &session, organization_id, Capability::ManageMembers).await?;
    let role = request
        .role
        .parse::<Role>()
        .map_err(|_| ApiError::Validation("invalid_role"))?;
    members::change_member_role(
        &mut access.tx,
        access.user_id,
        organization_id,
        target_user_id,
        role.as_str(),
    )
    .await
    .map_err(map_member_error)?;
    access.commit().await?;
    Ok(StatusCode::NO_CONTENT)
```

Replace:

```rust
    require_admin(&state, &session, organization_id).await?;
    state
        .store
        .remove_member(session.user_id, organization_id, target_user_id)
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
```

With:

```rust
    let mut access =
        authorize(&state, &session, organization_id, Capability::ManageMembers).await?;
    members::remove_member(
        &mut access.tx,
        access.user_id,
        organization_id,
        target_user_id,
    )
    .await
    .map_err(map_member_error)?;
    access.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
```

`/auth/me` wraps each membership record in `OrganizationView`, which flattens the record and adds
`capabilities`.

In `crates/api/src/auth/service.rs`:

Replace:

```rust
use corqty_domain::account::{NormalizedEmail, OrganizationName, validate_password};
```

With:

```rust
use corqty_domain::{
    account::{NormalizedEmail, OrganizationName, validate_password},
    rbac::{Capability, Role},
};
```

Replace:

```rust
#[derive(Serialize)]
pub struct MeView {
    pub user: UserView,
    pub organizations: Vec<OrganizationMembershipRecord>,
}
```

With:

```rust
#[derive(Serialize)]
pub struct OrganizationView {
    #[serde(flatten)]
    pub membership: OrganizationMembershipRecord,
    pub capabilities: Vec<Capability>,
}

#[derive(Serialize)]
pub struct MeView {
    pub user: UserView,
    pub organizations: Vec<OrganizationView>,
}
```

Replace:

```rust
pub async fn me(state: &AppState, session: &SessionRecord) -> Result<MeView, ApiError> {
    let organizations = state
        .store
        .organizations_for_user(session.user_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(MeView {
```

With:

```rust
pub async fn me(state: &AppState, session: &SessionRecord) -> Result<MeView, ApiError> {
    let organizations = state
        .store
        .organizations_for_user(session.user_id)
        .await
        .map_err(|_| ApiError::Internal)?
        .into_iter()
        .map(|membership| {
            let role = membership
                .role
                .parse::<Role>()
                .map_err(|_| ApiError::Internal)?;
            Ok(OrganizationView {
                membership,
                capabilities: role.capabilities(),
            })
        })
        .collect::<Result<_, ApiError>>()?;
    Ok(MeView {
```

- [ ] **Step 6: Verify GREEN**

```text
cargo test -p corqty-domain --test account_and_rbac
```

Expected: 5 tests pass.

```text
cargo test -p corqty-api --test authorization_matrix --test auth_http
```

Expected: `authorization_matrix` 2 tests pass (`member_and_audit_routes_enforce_capabilities`,
`org_access_commit_reports_the_deferred_final_admin_check_as_conflict`); `auth_http` 3 tests pass.

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

Expected: every suite passes. corqty-api: unit 2, `audit_http` 1, `auth_http` 3, `authorization_matrix` 2,
`members_http` 3, `organizations_http` 1. corqty-domain: `account_and_rbac` 5. corqty-persistence: `auth_store` 2,
`final_admin` 2, `migrations` 1, `tenant_isolation` 1. 23 tests in total.

- [ ] **Step 8: Commit**

```text
git add crates/domain crates/persistence/src crates/api/src crates/api/tests
git commit -m "feat: authorize organization requests inside the work transaction"
```

Expected: `git show --stat HEAD` lists exactly the 15 files above (575 insertions, 305 deletions).

## Task 2: Return typed problems with error causes and JSON Pointer diagnostics

**Files:**

- Create: `crates/domain/src/diagnostic.rs`
- Create: `crates/api/src/extract.rs`
- Modify: `crates/domain/src/lib.rs`
- Modify: `crates/api/src/error.rs`
- Modify: `crates/api/src/lib.rs`
- Modify: `crates/api/src/router.rs`
- Modify: `crates/api/src/audit.rs`
- Modify: `crates/api/src/authz.rs`
- Modify: `crates/api/src/auth/cookies.rs`
- Modify: `crates/api/src/auth/crypto.rs`
- Modify: `crates/api/src/auth/handlers.rs`
- Modify: `crates/api/src/auth/middleware.rs`
- Modify: `crates/api/src/auth/service.rs`
- Modify: `crates/api/src/organizations/handlers.rs`
- Modify: `crates/api/src/organizations/members.rs`
- Modify: `crates/api/src/organizations/service.rs`
- Test: `crates/api/tests/problems_http.rs`

Today `ApiError::Internal` throws its cause away, malformed JSON bodies and non-UUID path segments come back as
axum's `text/plain` rejections, unknown routes return an empty 404 and a wrong method an empty 405. This task makes
every error a problem+json body with a stable `code`: `ApiError::Internal` carries its source, which is logged at
ERROR and never serialized; `ApiError::Invalid` adds a `diagnostics` array of `corqty_domain::diagnostic::Diagnostic`
(`code`, RFC 6901 `pointer`, `message`) that the catalog and workflow tasks fill in; `ApiJson<T>` and `ApiPath<T>`
replace `Json<T>` and `Path<T>` as request extractors; and the router answers unknown routes and methods itself.
Every request body type also gets `#[serde(deny_unknown_fields)]`, so an unknown field is a 422 rather than being
ignored.

- [ ] **Step 1: Write the failing tests**

The HTTP tests send each kind of rejected request and compare the whole problem body. A JSON syntax error is 400
`invalid_json`; an unknown field or a wrong type is 422 `invalid_json`; a missing JSON content type is 415
`unsupported_media_type`; a body above axum's 2 MiB default limit is 413 `payload_too_large`. None of them carry
`diagnostics`. A non-UUID organization id is 404 `not_found` (the same as an unknown id), an unknown route is 404
and a wrong method is 405 `method_not_allowed`. They use the public login route, so only the 404 for the non-UUID id
needs the session that `AuthFixture` provides.

Create `crates/api/tests/problems_http.rs`:

```rust
mod support;

use axum::{
    body::{Body, to_bytes},
    http::{HeaderValue, Request, header},
    response::Response,
};
use serde_json::{Value, json};
use serial_test::serial;
use support::AuthFixture;
use tower::ServiceExt;

fn expected(status: u16, title: &str, code: &str) -> Value {
    json!({ "type": "about:blank", "title": title, "status": status, "code": code })
}

/// Asserts the response is a problem whose body carries the response status.
async fn problem(response: Response) -> Value {
    let status = response.status();
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).cloned(),
        Some(HeaderValue::from_static("application/problem+json")),
        "{status}"
    );
    let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let problem: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(problem["status"], status.as_u16());
    problem
}

#[tokio::test]
#[serial]
async fn rejected_json_bodies_return_problems_without_diagnostics() {
    let fixture = AuthFixture::registered("admin@example.com", "Acme").await;
    let credentials = r#"{"email":"admin@example.com","password":"correct horse battery staple"}"#;
    let oversized = format!(
        r#"{{"email":"admin@example.com","password":"{}"}}"#,
        "a".repeat(2 * 1024 * 1024)
    );
    for (content_type, body, problem_body) in [
        (
            "application/json",
            r#"{"email":"#.to_owned(),
            expected(400, "Bad request", "invalid_json"),
        ),
        (
            "application/json",
            r#"{"email":"admin@example.com","password":"correct horse battery staple","remember":true}"#.to_owned(),
            expected(422, "Validation failed", "invalid_json"),
        ),
        (
            "application/json",
            r#"{"email":"admin@example.com","password":7}"#.to_owned(),
            expected(422, "Validation failed", "invalid_json"),
        ),
        (
            "text/plain",
            credentials.to_owned(),
            expected(415, "Unsupported media type", "unsupported_media_type"),
        ),
        (
            "application/json",
            oversized,
            expected(413, "Payload too large", "payload_too_large"),
        ),
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(
                Request::post("/api/v1/auth/login")
                    .header(header::CONTENT_TYPE, content_type)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(problem(response).await, problem_body);
    }
}

#[tokio::test]
#[serial]
async fn unknown_routes_methods_and_identifiers_return_problems() {
    let fixture = AuthFixture::registered("admin@example.com", "Acme").await;
    for (path, problem_body) in [
        (
            "/api/v1/orgs/not-a-uuid/members",
            expected(404, "Not found", "not_found"),
        ),
        (
            "/api/v1/no-such-route",
            expected(404, "Not found", "not_found"),
        ),
        (
            "/api/v1/auth/login",
            expected(405, "Method not allowed", "method_not_allowed"),
        ),
    ] {
        assert_eq!(
            problem(fixture.get(path).await).await,
            problem_body,
            "{path}"
        );
    }
}
```

The unit tests render `ApiError` values directly: an `Invalid` problem lists its diagnostics, and an `Internal`
problem built from a `sqlx::Error` keeps the error's text out of the body.

In `crates/api/src/error.rs`:

Replace:

```rust
            .into_response()
    }
}
```

With:

```rust
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use axum::{body::to_bytes, http::StatusCode, response::IntoResponse};
    use corqty_domain::diagnostic::Diagnostic;
    use serde_json::{Value, json};

    use super::ApiError;

    async fn render(error: ApiError) -> (StatusCode, String) {
        let response = error.into_response();
        let status = response.status();
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn invalid_problems_list_their_diagnostics() {
        let (status, body) = render(ApiError::Invalid {
            code: "graph_invalid",
            diagnostics: vec![Diagnostic {
                code: "too_many_nodes".to_owned(),
                pointer: "/nodes".to_owned(),
                message: "a graph holds at most 500 nodes".to_owned(),
            }],
        })
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!({
                "type": "about:blank",
                "title": "Validation failed",
                "status": 422,
                "code": "graph_invalid",
                "diagnostics": [{
                    "code": "too_many_nodes",
                    "pointer": "/nodes",
                    "message": "a graph holds at most 500 nodes",
                }],
            })
        );
    }

    #[tokio::test]
    async fn internal_problems_never_include_their_source() {
        let source = sqlx::Error::Protocol("connection to db.internal:5432 refused".to_owned());
        let (status, body) = render(ApiError::from(source)).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!body.contains("db.internal"), "{body}");
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!({
                "type": "about:blank",
                "title": "Internal error",
                "status": 500,
                "code": "internal",
            })
        );
    }
}
```

- [ ] **Step 2: Run the tests and verify RED**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-api --test problems_http
```

Expected: both tests fail on the first content-type assertion, because axum's own rejection is plain text:

```text
assertion `left == right` failed: 400 Bad Request
  left: Some("text/plain; charset=utf-8")
 right: Some("application/problem+json")
```

The result line is `test result: FAILED. 0 passed; 2 failed`.

```text
cargo test -p corqty-api --lib
```

Expected: compilation fails with ``error[E0432]: unresolved import `corqty_domain::diagnostic` ``,
``error[E0599]: no variant named `Invalid` found for enum `ApiError` `` and ``error[E0308]: mismatched types``
(`ApiError::from(source)` expected `ApiError`, found `sqlx::Error`), 3 errors.

- [ ] **Step 3: Add the shared diagnostic type to the domain**

Validation in the domain (graph limits, node definitions, publish checks) reports `Diagnostic` values, and the API
serializes them unchanged.

Create `crates/domain/src/diagnostic.rs`:

```rust
use serde::{Deserialize, Serialize};

/// One problem in a submitted document, located by an RFC 6901 JSON Pointer
/// into that document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub pointer: String,
    pub message: String,
}
```

Replace the whole of `crates/domain/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod account;
pub mod diagnostic;
pub mod error;
pub mod rbac;
```

- [ ] **Step 4: Give API errors causes, diagnostics and typed rejections**

`ApiError::Internal` holds a boxed source. `From<sqlx::Error>` lets handlers use `?` on repository calls, and
`ApiError::internal` wraps any other error (join errors, header values, role parsing, and the Argon2 and getrandom
errors, which only implement `Display` here and so go in as their message). The source is logged as `error = %source`
at ERROR when the response is rendered and never serialized. `Invalid` is a 422 whose body lists `diagnostics`;
`Validation` stays a 422 without them. `BadRequest`, `MethodNotAllowed`, `PayloadTooLarge` and
`UnsupportedMediaType` cover the extractor and router rejections.

In `crates/api/src/error.rs`:

Replace:

```rust
use serde::Serialize;
```

With:

```rust
use corqty_domain::diagnostic::Diagnostic;
use serde::Serialize;
```

Replace:

```rust
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
```

With:

```rust
#[derive(Debug, Error)]
pub enum ApiError {
    #[error("bad request")]
    BadRequest(&'static str),
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("method not allowed")]
    MethodNotAllowed,
    #[error("conflict")]
    Conflict(&'static str),
    #[error("payload too large")]
    PayloadTooLarge,
    #[error("unsupported media type")]
    UnsupportedMediaType,
    #[error("validation failed")]
    Validation(&'static str),
    /// 422 whose problem body lists the diagnostics.
    #[error("validation failed")]
    Invalid {
        code: &'static str,
        diagnostics: Vec<Diagnostic>,
    },
    /// Logged when rendered; the response never contains the source.
    #[error("internal error")]
    Internal(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl ApiError {
    /// Wraps an error that has no `From` conversion (only `sqlx::Error` has one).
    pub fn internal(source: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        Self::Internal(source.into())
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        Self::internal(error)
    }
}
```

Replace:

```rust
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
            Self::Validation(code) => (StatusCode::UNPROCESSABLE_ENTITY, "Validation failed", code),
            Self::Internal => {
                tracing::error!("request failed with an internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal error",
                    "internal",
                )
            }
        };
```

With:

```rust
#[derive(Serialize)]
struct Problem {
    #[serde(rename = "type")]
    kind: &'static str,
    title: &'static str,
    status: u16,
    code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<Vec<Diagnostic>>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut diagnostics = None;
        let (status, title, code) = match self {
            Self::BadRequest(code) => (StatusCode::BAD_REQUEST, "Bad request", code),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized", "unauthorized"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "Forbidden", "forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "Not found", "not_found"),
            Self::MethodNotAllowed => (
                StatusCode::METHOD_NOT_ALLOWED,
                "Method not allowed",
                "method_not_allowed",
            ),
            Self::Conflict(code) => (StatusCode::CONFLICT, "Conflict", code),
            Self::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "Payload too large",
                "payload_too_large",
            ),
            Self::UnsupportedMediaType => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Unsupported media type",
                "unsupported_media_type",
            ),
            Self::Validation(code) => (StatusCode::UNPROCESSABLE_ENTITY, "Validation failed", code),
            Self::Invalid {
                code,
                diagnostics: found,
            } => {
                diagnostics = Some(found);
                (StatusCode::UNPROCESSABLE_ENTITY, "Validation failed", code)
            }
            Self::Internal(source) => {
                tracing::error!(error = %source, "request failed with an internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal error",
                    "internal",
                )
            }
        };
```

Replace:

```rust
            Json(Problem {
                kind: "about:blank",
                title,
                status: status.as_u16(),
                code,
            }),
```

With:

```rust
            Json(Problem {
                kind: "about:blank",
                title,
                status: status.as_u16(),
                code,
                diagnostics,
            }),
```

`ApiJson<T>` and `ApiPath<T>` are the axum extractors with an `ApiError` rejection. `JsonRejection` already knows
its status, so the mapping only turns each status into its problem code. Every path rejection is a 404, so a
malformed identifier looks exactly like one that does not exist.

Create `crates/api/src/extract.rs`:

```rust
use axum::{
    extract::{
        FromRequest, FromRequestParts,
        rejection::{JsonRejection, PathRejection},
    },
    http::StatusCode,
};

use crate::error::ApiError;

/// `axum::Json` with problem+json rejections: 400 `invalid_json` for a syntax
/// error, 422 `invalid_json` for a body that does not fit the type, 415 for a
/// missing JSON content type and 413 above axum's 2 MiB default body limit.
#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
pub struct ApiJson<T>(pub T);

/// `axum::extract::Path` whose rejection is a 404, so a malformed identifier
/// looks exactly like an unknown one.
#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(ApiError))]
pub struct ApiPath<T>(pub T);

impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        match rejection.status() {
            StatusCode::PAYLOAD_TOO_LARGE => Self::PayloadTooLarge,
            StatusCode::UNSUPPORTED_MEDIA_TYPE => Self::UnsupportedMediaType,
            StatusCode::UNPROCESSABLE_ENTITY => Self::Validation("invalid_json"),
            _ => Self::BadRequest("invalid_json"),
        }
    }
}

impl From<PathRejection> for ApiError {
    fn from(_: PathRejection) -> Self {
        Self::NotFound
    }
}
```

Replace the whole of `crates/api/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod audit;
pub mod auth;
pub mod authz;
pub mod config;
pub mod error;
pub mod extract;
pub mod organizations;
pub mod router;
pub mod state;
```

The router answers unmatched paths with a 404 problem and unsupported methods with a 405 problem. Nested routes
inherit the fallback, but `method_not_allowed_fallback` only reaches routes that already exist, so both go after
`nest`. Request spans and responses are traced at INFO, which the default `tower_http=info` filter shows. There is
no body-limit layer: axum's 2 MiB default applies to `ApiJson`.

Replace the whole of `crates/api/src/router.rs`:

```rust
use axum::{
    Router,
    http::StatusCode,
    middleware,
    routing::{get, post},
};
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::{
    audit,
    auth::{handlers, middleware as auth_middleware},
    error::ApiError,
    organizations,
    state::AppState,
};

pub fn build_router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/auth/me", get(handlers::me))
        .route("/auth/logout", post(handlers::logout))
        .route("/organizations", post(organizations::handlers::create))
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
        .route("/orgs/{organization_id}/audit-events", get(audit::list))
        .route(
            "/orgs/{organization_id}/members/{target_user_id}",
            axum::routing::patch(organizations::members::change_role)
                .delete(organizations::members::remove_member),
        )
        .route(
            "/auth/password",
            axum::routing::patch(handlers::change_password),
        )
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
        // `method_not_allowed_fallback` only reaches routes added before it.
        .fallback(|| async { ApiError::NotFound })
        .method_not_allowed_fallback(|| async { ApiError::MethodNotAllowed })
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
}
```

- [ ] **Step 5: Use the typed extractors and propagate causes in every handler**

Each `map_err(|_| ApiError::Internal)` on a `sqlx::Error` becomes `?`. The other sites use `ApiError::internal`. The
fallback arms of the database-error mappers keep the error with `error.into()`.

Replace the whole of `crates/api/src/audit.rs`:

```rust
use axum::{
    Extension, Json,
    extract::{Query, State},
};
use corqty_domain::rbac::Capability;
use corqty_persistence::models::SessionRecord;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{authz::authorize, error::ApiError, extract::ApiPath, state::AppState};

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
    ApiPath(organization_id): ApiPath<Uuid>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<AuditResponse>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let items = corqty_persistence::audit::list_audit_events(
        &mut access.tx,
        organization_id,
        query.before_id,
        query.limit.unwrap_or(50),
    )
    .await?;
    access.commit().await?;
    Ok(Json(AuditResponse { items }))
}
```

Replace the whole of `crates/api/src/authz.rs`:

```rust
use corqty_domain::rbac::{Capability, Role};
use corqty_persistence::{models::SessionRecord, organizations::member_role};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{error::ApiError, state::AppState};

/// An open tenant transaction for a member whose role was read inside it.
/// Handlers do all database work through `&mut access.tx` and finish with
/// `access.commit()`; dropping it rolls back.
pub struct OrgAccess {
    pub tx: Transaction<'static, Postgres>,
    pub organization_id: Uuid,
    pub user_id: Uuid,
    pub role: Role,
}

/// Non-members (including requests naming another organization) get 404;
/// members lacking the capability get 403.
pub async fn authorize(
    state: &AppState,
    session: &SessionRecord,
    organization_id: Uuid,
    capability: Capability,
) -> Result<OrgAccess, ApiError> {
    let mut tx = state
        .store
        .begin_tenant(session.user_id, organization_id)
        .await?;
    let role = member_role(&mut tx, organization_id, session.user_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if !role.allows(capability) {
        return Err(ApiError::Forbidden);
    }
    Ok(OrgAccess {
        tx,
        organization_id,
        user_id: session.user_id,
        role,
    })
}

impl OrgAccess {
    /// The final-admin trigger is deferred, so it can only fail here.
    pub async fn commit(self) -> Result<(), ApiError> {
        self.tx.commit().await.map_err(|error| {
            if error
                .as_database_error()
                .is_some_and(|database| database.message() == "final_admin_required")
            {
                ApiError::Conflict("final_admin_required")
            } else {
                error.into()
            }
        })
    }
}
```

Replace the whole of `crates/api/src/auth/cookies.rs`:

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
    let secure = if config.secure_cookies {
        "; Secure"
    } else {
        ""
    };
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
            HeaderValue::from_str(&value).map_err(ApiError::internal)?,
        );
    }
    Ok(())
}

pub fn clear_auth_cookies(headers: &mut HeaderMap, config: &ApiConfig) {
    let secure = if config.secure_cookies {
        "; Secure"
    } else {
        ""
    };
    for name in [SESSION_COOKIE, CSRF_COOKIE] {
        let value = format!("{name}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}");
        headers.append(
            header::SET_COOKIE,
            HeaderValue::from_str(&value).expect("valid clear-cookie header"),
        );
    }
}
```

Replace the whole of `crates/api/src/auth/crypto.rs`:

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
            .map_err(|error| ApiError::internal(error.to_string()))
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
        getrandom::fill(&mut bytes).map_err(|error| ApiError::internal(error.to_string()))?;
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

#[cfg(test)]
mod tests {
    use super::{PasswordEngine, SecretToken};

    #[test]
    fn password_hash_verifies_only_the_original_password() {
        let hash = PasswordEngine::hash("correct horse battery staple").unwrap();
        assert!(PasswordEngine::verify(
            "correct horse battery staple",
            &hash
        ));
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

Replace the whole of `crates/api/src/auth/middleware.rs`:

```rust
use axum::{
    extract::{Request, State},
    http::Method,
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
    let token = read_cookie(request.headers(), SESSION_COOKIE).ok_or(ApiError::Unauthorized)?;
    let session = state
        .store
        .session_by_token_hash(&SecretToken::hash(&token))
        .await?
        .ok_or(ApiError::Unauthorized)?;
    request.extensions_mut().insert(session);
    Ok(next.run(request).await)
}

pub async fn require_csrf(request: Request, next: Next) -> Result<Response, ApiError> {
    if matches!(
        *request.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) {
        let session = request
            .extensions()
            .get::<SessionRecord>()
            .ok_or(ApiError::Unauthorized)?;
        let cookie = read_cookie(request.headers(), CSRF_COOKIE).ok_or(ApiError::Forbidden)?;
        let header_token = request
            .headers()
            .get("x-csrf-token")
            .and_then(|value| value.to_str().ok())
            .ok_or(ApiError::Forbidden)?;
        if cookie != header_token || SecretToken::hash(header_token) != session.csrf_hash {
            return Err(ApiError::Forbidden);
        }
    }
    Ok(next.run(request).await)
}
```

Every request body type rejects unknown fields, and the session id in `DELETE /auth/sessions/{session_id}` goes
through `ApiPath`.

Replace the whole of `crates/api/src/auth/handlers.rs`:

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
    extract::{ApiJson, ApiPath},
    state::AppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisterRequest {
    email: String,
    password: String,
    organization_name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

pub async fn register(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<RegisterRequest>,
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
    Ok((
        StatusCode::CREATED,
        headers,
        Json(serde_json::json!({"ok": true})),
    ))
}

pub async fn login(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<LoginRequest>,
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
        .delete_session(session.user_id, session.session_id)
        .await?;
    let mut headers = HeaderMap::new();
    clear_auth_cookies(&mut headers, &state.config);
    Ok((StatusCode::NO_CONTENT, headers, ()))
}

pub async fn change_password(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiJson(request): ApiJson<ChangePasswordRequest>,
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
    let sessions = state.store.list_sessions(session.user_id).await?;
    Ok(Json(sessions))
}

pub async fn revoke_session(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(session_id): ApiPath<uuid::Uuid>,
) -> Result<StatusCode, ApiError> {
    let removed = state
        .store
        .revoke_user_session(session.user_id, session_id)
        .await?;
    removed
        .then_some(StatusCode::NO_CONTENT)
        .ok_or(ApiError::NotFound)
}
```

In `crates/api/src/auth/service.rs`:

Replace:

```rust
    let password_hash = tokio::task::spawn_blocking(move || PasswordEngine::hash(&password_owned))
        .await
        .map_err(|_| ApiError::Internal)??;
```

With:

```rust
    let password_hash = tokio::task::spawn_blocking(move || PasswordEngine::hash(&password_owned))
        .await
        .map_err(ApiError::internal)??;
```

Replace:

```rust
    let record = match NormalizedEmail::parse(email) {
        Ok(email) => state
            .store
            .login_record(email.as_str())
            .await
            .map_err(|_| ApiError::Internal)?,
```

With:

```rust
    let record = match NormalizedEmail::parse(email) {
        Ok(email) => state.store.login_record(email.as_str()).await?,
```

Replace:

```rust
        PasswordEngine::verify(&password_owned, &password_hash)
    })
    .await
    .map_err(|_| ApiError::Internal)?;
```

With:

```rust
        PasswordEngine::verify(&password_owned, &password_hash)
    })
    .await
    .map_err(ApiError::internal)?;
```

Replace:

```rust
                expires_at: Utc::now() + Duration::days(state.config.session_days),
            },
        )
        .await
        .map_err(|_| ApiError::Internal)?;
```

With:

```rust
                expires_at: Utc::now() + Duration::days(state.config.session_days),
            },
        )
        .await?;
```

Replace:

```rust
        .organizations_for_user(session.user_id)
        .await
        .map_err(|_| ApiError::Internal)?
        .into_iter()
        .map(|membership| {
            let role = membership
                .role
                .parse::<Role>()
                .map_err(|_| ApiError::Internal)?;
```

With:

```rust
        .organizations_for_user(session.user_id)
        .await?
        .into_iter()
        .map(|membership| {
            let role = membership
                .role
                .parse::<Role>()
                .map_err(ApiError::internal)?;
```

Replace:

```rust
        .login_record(&session.email)
        .await
        .map_err(|_| ApiError::Internal)?
        .ok_or(ApiError::Unauthorized)?;
```

With:

```rust
        .login_record(&session.email)
        .await?
        .ok_or(ApiError::Unauthorized)?;
```

Replace:

```rust
        tokio::task::spawn_blocking(move || PasswordEngine::verify(&current_owned, &current_hash))
            .await
            .map_err(|_| ApiError::Internal)?;
```

With:

```rust
        tokio::task::spawn_blocking(move || PasswordEngine::verify(&current_owned, &current_hash))
            .await
            .map_err(ApiError::internal)?;
```

Replace:

```rust
    let new_hash = tokio::task::spawn_blocking(move || PasswordEngine::hash(&new_owned))
        .await
        .map_err(|_| ApiError::Internal)??;
    state
        .store
        .change_password_and_revoke_other_sessions(session.user_id, session.session_id, &new_hash)
        .await
        .map_err(|_| ApiError::Internal)
}
```

With:

```rust
    let new_hash = tokio::task::spawn_blocking(move || PasswordEngine::hash(&new_owned))
        .await
        .map_err(ApiError::internal)??;
    Ok(state
        .store
        .change_password_and_revoke_other_sessions(session.user_id, session.session_id, &new_hash)
        .await?)
}
```

Replace:

```rust
        ApiError::Conflict("email_already_registered")
    } else {
        ApiError::Internal
    }
```

With:

```rust
        ApiError::Conflict("email_already_registered")
    } else {
        error.into()
    }
```

Replace the whole of `crates/api/src/organizations/handlers.rs`:

```rust
use axum::{Extension, Json, extract::State, http::StatusCode};
use corqty_persistence::models::{OrganizationMembershipRecord, SessionRecord};
use serde::Deserialize;

use crate::{error::ApiError, extract::ApiJson, organizations::service, state::AppState};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateOrganization {
    name: String,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiJson(request): ApiJson<CreateOrganization>,
) -> Result<(StatusCode, Json<OrganizationMembershipRecord>), ApiError> {
    Ok((
        StatusCode::CREATED,
        Json(service::create(&state, &session, &request.name).await?),
    ))
}
```

Replace the whole of `crates/api/src/organizations/service.rs`:

```rust
use corqty_domain::account::OrganizationName;
use corqty_persistence::models::{OrganizationMembershipRecord, SessionRecord};
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
    Ok(state
        .store
        .create_organization(session.user_id, organization_id, name.as_str(), &slug)
        .await?)
}
```

In `crates/api/src/organizations/members.rs`:

Replace:

```rust
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
```

With:

```rust
use axum::{Extension, Json, extract::State, http::StatusCode};
```

Replace:

```rust
use crate::{auth::crypto::SecretToken, authz::authorize, error::ApiError, state::AppState};

#[derive(Deserialize)]
pub struct InvitationRequest {
```

With:

```rust
use crate::{
    auth::crypto::SecretToken,
    authz::authorize,
    error::ApiError,
    extract::{ApiJson, ApiPath},
    state::AppState,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvitationRequest {
```

Replace:

```rust
#[derive(Deserialize)]
pub struct AcceptInvitationRequest {
```

With:

```rust
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptInvitationRequest {
```

Replace:

```rust
#[derive(Deserialize)]
pub struct ChangeRoleRequest {
```

With:

```rust
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeRoleRequest {
```

Replace:

```rust
    Path(organization_id): Path<Uuid>,
    Json(request): Json<InvitationRequest>,
```

With:

```rust
    ApiPath(organization_id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<InvitationRequest>,
```

Replace:

```rust
    )
    .await
    .map_err(|_| ApiError::Internal)?;
    access.commit().await?;
```

With:

```rust
    )
    .await?;
    access.commit().await?;
```

Replace:

```rust
    Json(request): Json<AcceptInvitationRequest>,
```

With:

```rust
    ApiJson(request): ApiJson<AcceptInvitationRequest>,
```

Replace:

```rust
    Path(organization_id): Path<Uuid>,
) -> Result<Json<Vec<MemberRecord>>, ApiError> {
```

With:

```rust
    ApiPath(organization_id): ApiPath<Uuid>,
) -> Result<Json<Vec<MemberRecord>>, ApiError> {
```

Replace:

```rust
    let members = members::list_members(&mut access.tx, organization_id)
        .await
        .map_err(|_| ApiError::Internal)?;
```

With:

```rust
    let members = members::list_members(&mut access.tx, organization_id).await?;
```

Replace:

```rust
    Path((organization_id, target_user_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<ChangeRoleRequest>,
```

With:

```rust
    ApiPath((organization_id, target_user_id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(request): ApiJson<ChangeRoleRequest>,
```

Replace:

```rust
    Path((organization_id, target_user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
```

With:

```rust
    ApiPath((organization_id, target_user_id)): ApiPath<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
```

Replace:

```rust
        MemberMutationError::Database(_) => ApiError::Internal,
```

With:

```rust
        MemberMutationError::Database(error) => error.into(),
```

Replace:

```rust
        _ => ApiError::Internal,
```

With:

```rust
        _ => error.into(),
```

- [ ] **Step 6: Verify GREEN**

```text
cargo test -p corqty-api --lib
```

Expected: 4 tests pass (`invalid_problems_list_their_diagnostics`, `internal_problems_never_include_their_source`
and the two `auth::crypto` tests).

```text
cargo test -p corqty-api --test problems_http
```

Expected: 2 tests pass (`rejected_json_bodies_return_problems_without_diagnostics`,
`unknown_routes_methods_and_identifiers_return_problems`).

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

Expected: every suite passes. corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 2,
`members_http` 3, `organizations_http` 1, `problems_http` 2. corqty-domain: `account_and_rbac` 5.
corqty-persistence: `auth_store` 2, `final_admin` 2, `migrations` 1, `tenant_isolation` 1. 27 tests in total.

- [ ] **Step 8: Commit**

```text
git add crates/domain/src crates/api/src crates/api/tests
git commit -m "feat: return typed problems with error causes and JSON Pointer diagnostics"
```

Expected: `git show --stat HEAD` lists exactly the 17 files above (350 insertions, 80 deletions).

## Task 3: Reset integration tests by truncating every table

**Files:**

- Modify: `crates/test-support/src/lib.rs`
- Test: `crates/persistence/tests/test_reset.rs`

`TestDatabase::reset()` names the seven foundation tables in its `TRUNCATE`. A table that a later migration adds,
such as the workflow catalog in Task 11, would keep its rows from one test to the next unless someone remembers to
extend that list. The reset now reads the table names from `pg_tables` (schema `public`, except `_sqlx_migrations`)
and truncates all of them with `RESTART IDENTITY CASCADE`. No migration is needed. The tests stay `#[serial]` and
the API pool keeps its single connection.

- [ ] **Step 1: Write the failing test**

As the owner, the test fills five foundation tables (users, organizations, memberships, invitations and
audit_events). It also creates `reset_probe`, a table that no migration creates, to stand in for the tables later
tasks add. After a second `reset()` it compares the row count of every `public` table except `_sqlx_migrations` with
`seeded_rows`, which returns 0 for every table. Task 11 makes it return 6 for `node_definitions` and 2 for
`blueprints`. The probe is dropped before the assertions, so a failing assertion cannot leave it behind, and
`CREATE TABLE IF NOT EXISTS` tolerates a probe left by an interrupted run.

Create `crates/persistence/tests/test_reset.rs`:

```rust
use corqty_test_support::TestDatabase;
use serial_test::serial;

/// Rows that `TestDatabase::reset()` puts back after truncating a table.
fn seeded_rows(_table: &str) -> i64 {
    0
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

- [ ] **Step 2: Run the test and verify RED**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-persistence --test test_reset
```

Expected: the test fails because the hard-coded list does not name `reset_probe`, so its row survives the reset:

```text
assertion `left == right` failed
  left: [("audit_events", 0), ("invitations", 0), ("memberships", 0), ("organizations", 0), ("password_credentials", 0), ("reset_probe", 1), ("sessions", 0), ("users", 0)]
 right: [("audit_events", 0), ("invitations", 0), ("memberships", 0), ("organizations", 0), ("password_credentials", 0), ("reset_probe", 0), ("sessions", 0), ("users", 0)]
```

The result line is `test result: FAILED. 0 passed; 1 failed`.

- [ ] **Step 3: Truncate every table found in the catalog**

`format('%I.%I', ...)` quotes each name, and one `TRUNCATE ... CASCADE` statement covers the foreign keys between
the tables.

Replace the whole of `crates/test-support/src/lib.rs`:

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
            .connect("postgres://corqty_owner:corqty_owner_dev@127.0.0.1:55432/corqty_test")
            .await
            .expect("connect owner test database");
        MIGRATOR.run(&owner).await.expect("run test migrations");

        // Every table the migrations created, so a new table needs no change here.
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT format('%I.%I', schemaname, tablename)
             FROM pg_tables
             WHERE schemaname = 'public' AND tablename <> '_sqlx_migrations'",
        )
        .fetch_all(&owner)
        .await
        .expect("list test tables");
        sqlx::query(&format!(
            "TRUNCATE TABLE {} RESTART IDENTITY CASCADE",
            tables.join(", ")
        ))
        .execute(&owner)
        .await
        .expect("reset test data");

        let api = PgPoolOptions::new()
            // A single API connection makes the context-leak test prove that
            // SET LOCAL values disappear before this connection is reused.
            .max_connections(1)
            .connect("postgres://corqty_api:corqty_api_dev@127.0.0.1:55432/corqty_test")
            .await
            .expect("connect api test database");

        Self { owner, api }
    }
}
```

- [ ] **Step 4: Verify GREEN**

```text
cargo test -p corqty-persistence --test test_reset
```

Expected: 1 test passes (`reset_truncates_every_table_in_public`).

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

Expected: every suite passes. corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 2,
`members_http` 3, `organizations_http` 1, `problems_http` 2. corqty-domain: `account_and_rbac` 5.
corqty-persistence: `auth_store` 2, `final_admin` 2, `migrations` 1, `tenant_isolation` 1, `test_reset` 1. 28 tests
in total.

- [ ] **Step 6: Commit**

```text
git add crates/test-support/src/lib.rs crates/persistence/tests/test_reset.rs
git commit -m "test: reset integration tests by truncating every table"
```

Expected: `git show --stat HEAD` lists exactly the 2 files above (81 insertions, 10 deletions).

## Task 4: Run migrations as a non-superuser database owner

**Files:**

- Modify: `docker/postgres/init/001-roles.sh`
- Modify: `docker-compose.yml`
- Modify: `docker-compose.test.yml`
- Modify: `.env.example`
- Modify: `.github/workflows/ci.yml`
- Modify: `README.md`
- Test: `crates/persistence/tests/catalog_guards.rs`

In Milestone 1, `POSTGRES_USER` is `corqty_owner`, so the PostgreSQL image creates that role as its bootstrap
superuser. Every migration, every `SECURITY DEFINER` helper in schema `corqty` and every test reset therefore runs
with superuser rights. After this task the bootstrap superuser is `postgres`, and its only job is the init script.
The script creates `corqty_owner` as `LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT BYPASSRLS` and makes it the
owner of the database. The migrator, the seeds and `TestDatabase::reset()` keep connecting as `corqty_owner` with
the same password, so `DATABASE_URL_OWNER` and the test connection strings do not change. `corqty_api` and
`corqty_worker` are created exactly as before.

Two of the owner's attributes are load-bearing:

- `INHERIT`: since PostgreSQL 15, schema `public` belongs to `pg_database_owner`, and only that role may create
  objects in it. The database owner is an implicit member of `pg_database_owner`. INHERIT lets `corqty_owner` use
  that membership to create tables in `public` and to run the migrations' `GRANT USAGE ON SCHEMA public`.
- `BYPASSRLS`: the tenant tables use `FORCE ROW LEVEL SECURITY`, and every policy names only `corqty_api`. Without
  BYPASSRLS the owner would get default-deny inside its own `SECURITY DEFINER` helpers, so `corqty.is_member` would
  always return false.

The healthchecks gain `-h 127.0.0.1`. The image runs the init scripts against a temporary server that listens only
on the Unix socket, so a TCP check passes only after the roles exist and the real server has started.

- [ ] **Step 1: Write the failing test**

The owner pool must connect as `corqty_owner`, which must not be a superuser and must bypass RLS. Every table in
`public` (`_sqlx_migrations` included) and every function in schema `corqty` must belong to `corqty_owner`. The
`UNION` reduces the owners to distinct names, so the comparison with `["corqty_owner"]` also fails when the query
finds no objects at all. Task 5 adds the row-level security checker to this file.

Create `crates/persistence/tests/catalog_guards.rs`:

```rust
use corqty_test_support::TestDatabase;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn migrations_run_as_a_non_superuser_owner() {
    let database = TestDatabase::reset().await;

    let role: (String, bool, bool) = sqlx::query_as(
        "SELECT rolname, rolsuper, rolbypassrls FROM pg_roles WHERE rolname = current_user",
    )
    .fetch_one(&database.owner)
    .await
    .unwrap();
    // BYPASSRLS keeps the SECURITY DEFINER helpers working: they read tables
    // with FORCE ROW LEVEL SECURITY whose policies only name corqty_api.
    assert_eq!(role, ("corqty_owner".to_owned(), false, true));

    let owners: Vec<String> = sqlx::query_scalar(
        "SELECT tableowner FROM pg_tables WHERE schemaname = 'public'
         UNION
         SELECT pg_get_userbyid(proowner) FROM pg_proc
         WHERE pronamespace = 'corqty'::regnamespace",
    )
    .fetch_all(&database.owner)
    .await
    .unwrap();
    assert_eq!(owners, ["corqty_owner"]);
}
```

- [ ] **Step 2: Run the test and verify RED**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-persistence --test catalog_guards
```

Expected: the test fails because the existing test database still uses `corqty_owner` as its bootstrap superuser:

```text
assertion `left == right` failed
  left: ("corqty_owner", true, true)
 right: ("corqty_owner", false, true)
```

The result line is `test result: FAILED. 0 passed; 1 failed`.

- [ ] **Step 3: Create the owner role from the bootstrap superuser**

`-v ON_ERROR_STOP=1` makes a failing statement fail the whole script, so neither the container init nor the CI step
can carry on with a half-created set of roles. The `NOT EXISTS` guards let the script run again against a database
that already has the roles, and `ALTER DATABASE ... OWNER TO` is idempotent. The file keeps its executable bit
(mode `100755`).

Replace the whole of `docker/postgres/init/001-roles.sh`:

```bash
#!/usr/bin/env bash
# PostgreSQL roles for local development, integration tests and CI. The
# bootstrap superuser runs this once; corqty_owner then owns the database and
# runs migrations, seeds and test resets without superuser rights.
set -euo pipefail

psql -v ON_ERROR_STOP=1 \
  --username "$POSTGRES_USER" \
  --dbname "$POSTGRES_DB" \
  --set=owner_password="$CORQTY_OWNER_DB_PASSWORD" \
  --set=api_password="$CORQTY_API_DB_PASSWORD" \
  --set=worker_password="$CORQTY_WORKER_DB_PASSWORD" <<'SQL'
-- INHERIT: CREATE on schema public reaches the owner through pg_database_owner.
-- BYPASSRLS: the SECURITY DEFINER helpers it owns read FORCE RLS tables.
SELECT format(
  'CREATE ROLE corqty_owner LOGIN PASSWORD %L NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT BYPASSRLS',
  :'owner_password'
)
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'corqty_owner') \gexec

SELECT format('ALTER DATABASE %I OWNER TO corqty_owner', current_database()) \gexec

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

- [ ] **Step 4: Make `postgres` the bootstrap superuser in Compose**

The superuser's password gets its own variable, and `CORQTY_OWNER_DB_PASSWORD` feeds the init script. The
`migrate` service still connects through `DATABASE_URL_OWNER` as `corqty_owner`.

In `docker-compose.yml`, apply these replacements in order:

Replace:

```yaml
      POSTGRES_USER: ${POSTGRES_OWNER_USER:-corqty_owner}
      POSTGRES_PASSWORD: ${POSTGRES_OWNER_PASSWORD:-corqty_owner_dev}
```

With:

```yaml
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: ${POSTGRES_SUPERUSER_PASSWORD:-postgres_dev}
      CORQTY_OWNER_DB_PASSWORD: ${CORQTY_OWNER_DB_PASSWORD:-corqty_owner_dev}
```

Replace:

```yaml
          "pg_isready -U ${POSTGRES_OWNER_USER:-corqty_owner} -d ${POSTGRES_DB:-corqty}",
```

With:

```yaml
          "pg_isready -h 127.0.0.1 -U postgres -d ${POSTGRES_DB:-corqty}",
```

In `docker-compose.test.yml`, apply these replacements in order:

Replace:

```yaml
      POSTGRES_USER: corqty_owner
      POSTGRES_PASSWORD: corqty_owner_dev
```

With:

```yaml
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: postgres_dev
      CORQTY_OWNER_DB_PASSWORD: corqty_owner_dev
```

Replace:

```yaml
      test: ["CMD-SHELL", "pg_isready -U corqty_owner -d corqty_test"]
```

With:

```yaml
      test: ["CMD-SHELL", "pg_isready -h 127.0.0.1 -U postgres -d corqty_test"]
```

In `.env.example`:

Replace:

```text
POSTGRES_OWNER_USER=corqty_owner
POSTGRES_OWNER_PASSWORD=corqty_owner_dev
```

With:

```text
POSTGRES_SUPERUSER_PASSWORD=postgres_dev
CORQTY_OWNER_DB_PASSWORD=corqty_owner_dev
```

- [ ] **Step 5: Run the same script in CI**

The CI service container has no init directory, so a step runs the script over TCP. psql reads `PGHOST`, `PGPORT`
and `PGPASSWORD`, and the script reads the other variables. This replaces the two inline `CREATE ROLE` commands,
so CI now gets the same owner role, attributes and database ownership as Compose.

In `.github/workflows/ci.yml`, apply these replacements in order:

Replace:

```yaml
          POSTGRES_USER: corqty_owner
          POSTGRES_PASSWORD: corqty_owner_dev
        ports:
          - 55432:5432
        options: >-
          --health-cmd "pg_isready -U corqty_owner -d corqty_test"
```

With:

```yaml
          POSTGRES_USER: postgres
          POSTGRES_PASSWORD: postgres_dev
        ports:
          - 55432:5432
        options: >-
          --health-cmd "pg_isready -h 127.0.0.1 -U postgres -d corqty_test"
```

Replace:

```yaml
      - name: Create database roles
        env:
          PGPASSWORD: corqty_owner_dev
        run: |
          psql -h localhost -p 55432 -U corqty_owner -d corqty_test \
            -c "CREATE ROLE corqty_api LOGIN PASSWORD 'corqty_api_dev' NOBYPASSRLS"
          psql -h localhost -p 55432 -U corqty_owner -d corqty_test \
            -c "CREATE ROLE corqty_worker LOGIN PASSWORD 'corqty_worker_dev' NOBYPASSRLS"
```

With:

```yaml
      - name: Create database roles
        env:
          PGHOST: localhost
          PGPORT: "55432"
          PGPASSWORD: postgres_dev
          POSTGRES_USER: postgres
          POSTGRES_DB: corqty_test
          CORQTY_OWNER_DB_PASSWORD: corqty_owner_dev
          CORQTY_API_DB_PASSWORD: corqty_api_dev
          CORQTY_WORKER_DB_PASSWORD: corqty_worker_dev
        run: bash docker/postgres/init/001-roles.sh
```

- [ ] **Step 6: Tell existing installations to recreate their databases**

The image runs init scripts only when the data directory is empty, so an existing volume keeps `corqty_owner` as its
superuser until it is recreated.

In `README.md`:

Replace:

````markdown
The test database is disposable:

```powershell
docker compose -f docker-compose.test.yml down -v
```
````

With:

````markdown
The test database is disposable:

```powershell
docker compose -f docker-compose.test.yml down -v
```

After upgrading to Milestone 2, recreate local databases once:
`docker compose down -v` and `docker compose -f docker-compose.test.yml down -v`.
````

- [ ] **Step 7: Recreate the test database and verify GREEN**

```text
docker compose -f docker-compose.test.yml down -v
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: the first command removes the container and its network, and the second reports `postgres-test` as
`Healthy`. `docker compose -f docker-compose.test.yml logs postgres-test` shows
`running /docker-entrypoint-initdb.d/001-roles.sh` followed by `CREATE ROLE`, `ALTER DATABASE`, `CREATE ROLE`,
`CREATE ROLE` and `GRANT`.

```text
cargo test -p corqty-persistence --test catalog_guards
```

Expected: 1 test passes (`migrations_run_as_a_non_superuser_owner`).

- [ ] **Step 8: Run the full backend verification**

```text
docker compose config --quiet
```

Expected: no output, exit code 0.

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

Expected: every suite passes. corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 2,
`members_http` 3, `organizations_http` 1, `problems_http` 2. corqty-domain: `account_and_rbac` 5.
corqty-persistence: `auth_store` 2, `catalog_guards` 1, `final_admin` 2, `migrations` 1, `tenant_isolation` 1,
`test_reset` 1. 29 tests in total. `test_reset` still creates and drops `public.reset_probe` as `corqty_owner`, which
shows that the non-superuser owner can create tables in `public`.

- [ ] **Step 9: Commit**

```text
git add docker/postgres/init/001-roles.sh docker-compose.yml docker-compose.test.yml .env.example .github/workflows/ci.yml README.md crates/persistence/tests/catalog_guards.rs
git commit -m "feat: run migrations as a non-superuser database owner"
```

Expected: `git show --stat HEAD` lists exactly the 7 files above (69 insertions, 19 deletions).

## Task 5: Guard row-level security in the database catalog

**Files:**

- Test: `crates/persistence/tests/catalog_guards.rs`

Tenant isolation rests on conventions that nothing checks yet. Every tenant table must enable and force row-level
security and have a policy. Every `SECURITY DEFINER` helper in schema `corqty` must pin its `search_path` and
revoke the default `EXECUTE` grant to PUBLIC. `corqty_worker` must have no privilege on any table until a later
milestone defines what it may touch. A migration that forgets one of these still passes every feature test, because
those tests exercise only the rules someone remembered to write. This task adds one SQL-backed checker,
`security_violations`, to `catalog_guards.rs`. It reads the system catalog and returns one message per violation:

- a table in `public` without `relrowsecurity AND relforcerowsecurity`, or without any policy. The exceptions are
  `_sqlx_migrations` and the identity tables `users`, `password_credentials` and `sessions`, which sign-in reads
  before any tenant context exists;
- a `SECURITY DEFINER` function in `corqty` without a `search_path=` entry in `proconfig`. Without one, the helper
  resolves table names through the caller's search path, so a caller could shadow `memberships` with a temporary
  table and have the helper trust its rows with the owner's rights;
- a `SECURITY DEFINER` function in `corqty` that PUBLIC can execute. PostgreSQL grants `EXECUTE` to PUBLIC by
  default and stores that default as a NULL `proacl`, so the checker expands NULL with `acldefault('f', proowner)`
  before it looks for grantee `0` (PUBLIC) in `aclexplode`;
- any privilege of `corqty_worker` on a table in `public`. `has_table_privilege` and `has_any_column_privilege`
  also count grants to PUBLIC and column-level grants, and the list includes `MAINTAIN` (PostgreSQL 17 and later).

The migrated database has no violations, so no migration changes. Task 11 adds five tenant tables and the
`corqty.can_edit` helper, and this guard checks them without any edit to this file.

- [ ] **Step 1: Write the failing tests**

The checker starts as a stub that reports nothing. `migrated_database_has_no_security_violations` holds for the
migrated database. `security_violations_report_unguarded_objects` creates one probe for each kind of violation in
an owner transaction: a table without row-level security, a `SELECT` grant on it to `corqty_worker`, and a
`SECURITY DEFINER` function that keeps PUBLIC's default `EXECUTE` and sets no `search_path`. It reads the violations
and rolls back before asserting, so the probes never outlive the test. The checker in Step 3 sorts its messages
(`ORDER BY 1`), so the expected list is fixed. The transaction is passed as `&mut tx`, which deref-coerces to
`&mut PgConnection`.

Replace the whole of `crates/persistence/tests/catalog_guards.rs`:

```rust
use corqty_test_support::TestDatabase;
use serial_test::serial;
use sqlx::PgConnection;

#[tokio::test]
#[serial]
async fn migrations_run_as_a_non_superuser_owner() {
    let database = TestDatabase::reset().await;

    let role: (String, bool, bool) = sqlx::query_as(
        "SELECT rolname, rolsuper, rolbypassrls FROM pg_roles WHERE rolname = current_user",
    )
    .fetch_one(&database.owner)
    .await
    .unwrap();
    // BYPASSRLS keeps the SECURITY DEFINER helpers working: they read tables
    // with FORCE ROW LEVEL SECURITY whose policies only name corqty_api.
    assert_eq!(role, ("corqty_owner".to_owned(), false, true));

    let owners: Vec<String> = sqlx::query_scalar(
        "SELECT tableowner FROM pg_tables WHERE schemaname = 'public'
         UNION
         SELECT pg_get_userbyid(proowner) FROM pg_proc
         WHERE pronamespace = 'corqty'::regnamespace",
    )
    .fetch_all(&database.owner)
    .await
    .unwrap();
    assert_eq!(owners, ["corqty_owner"]);
}

/// Every row-level security mistake the catalog shows, one message each.
async fn security_violations(_conn: &mut PgConnection) -> Vec<String> {
    Vec::new()
}

#[tokio::test]
#[serial]
async fn migrated_database_has_no_security_violations() {
    let database = TestDatabase::reset().await;
    let mut conn = database.owner.acquire().await.unwrap();

    assert_eq!(security_violations(&mut conn).await, Vec::<String>::new());
}

#[tokio::test]
#[serial]
async fn security_violations_report_unguarded_objects() {
    let database = TestDatabase::reset().await;
    let mut tx = database.owner.begin().await.unwrap();
    // One probe for each kind of violation; the rollback removes them all.
    sqlx::raw_sql(
        "CREATE TABLE public.guard_probe (id int);
         GRANT SELECT ON public.guard_probe TO corqty_worker;
         CREATE FUNCTION corqty.guard_probe() RETURNS int
         LANGUAGE sql SECURITY DEFINER AS 'SELECT 1';",
    )
    .execute(&mut *tx)
    .await
    .unwrap();

    let violations = security_violations(&mut tx).await;
    tx.rollback().await.unwrap();

    assert_eq!(
        violations,
        [
            "function corqty.guard_probe() is SECURITY DEFINER and executable by PUBLIC",
            "function corqty.guard_probe() is SECURITY DEFINER without search_path",
            "table guard_probe grants privileges to corqty_worker",
            "table guard_probe lacks forced row-level security or a policy",
        ]
    );
}
```

- [ ] **Step 2: Run the tests and verify RED**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-persistence --test catalog_guards
```

Expected: `migrations_run_as_a_non_superuser_owner` and `migrated_database_has_no_security_violations` pass, and
`security_violations_report_unguarded_objects` fails because the stub reports nothing:

```text
assertion `left == right` failed
  left: []
 right: ["function corqty.guard_probe() is SECURITY DEFINER and executable by PUBLIC", "function corqty.guard_probe() is SECURITY DEFINER without search_path", "table guard_probe grants privileges to corqty_worker", "table guard_probe lacks forced row-level security or a policy"]
```

The result line is `test result: FAILED. 2 passed; 1 failed`.

- [ ] **Step 3: Read the violations from the system catalog**

Each branch of the `UNION ALL` is one rule from the list above. The filter `relkind IN ('r', 'p')` covers plain and
partitioned tables.

In `crates/persistence/tests/catalog_guards.rs`:

Replace:

```rust
async fn security_violations(_conn: &mut PgConnection) -> Vec<String> {
    Vec::new()
}
```

With:

```rust
async fn security_violations(conn: &mut PgConnection) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT format('table %s lacks forced row-level security or a policy', c.relname)
         FROM pg_class c
         WHERE c.relnamespace = 'public'::regnamespace
           AND c.relkind IN ('r', 'p')
           -- Sign-in reads these before any tenant context exists.
           AND c.relname NOT IN ('_sqlx_migrations', 'users', 'password_credentials', 'sessions')
           AND NOT (
               c.relrowsecurity AND c.relforcerowsecurity
               AND EXISTS (SELECT FROM pg_policy pol WHERE pol.polrelid = c.oid)
           )
         UNION ALL
         SELECT format('function %s is SECURITY DEFINER without search_path', p.oid::regprocedure)
         FROM pg_proc p
         WHERE p.pronamespace = 'corqty'::regnamespace
           AND p.prosecdef
           AND NOT EXISTS (
               SELECT FROM unnest(p.proconfig) AS setting
               WHERE starts_with(setting, 'search_path=')
           )
         UNION ALL
         SELECT format('function %s is SECURITY DEFINER and executable by PUBLIC',
                       p.oid::regprocedure)
         FROM pg_proc p
         WHERE p.pronamespace = 'corqty'::regnamespace
           AND p.prosecdef
           -- A NULL proacl means the default ACL, which lets PUBLIC execute.
           AND EXISTS (
               SELECT FROM aclexplode(coalesce(p.proacl, acldefault('f', p.proowner))) AS acl
               WHERE acl.grantee = 0 AND acl.privilege_type = 'EXECUTE'
           )
         UNION ALL
         SELECT format('table %s grants privileges to corqty_worker', c.relname)
         FROM pg_class c
         WHERE c.relnamespace = 'public'::regnamespace
           AND c.relkind IN ('r', 'p')
           AND (
               has_table_privilege('corqty_worker', c.oid,
                   'SELECT, INSERT, UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER, MAINTAIN')
               OR has_any_column_privilege('corqty_worker', c.oid,
                   'SELECT, INSERT, UPDATE, REFERENCES')
           )
         ORDER BY 1",
    )
    .fetch_all(conn)
    .await
    .unwrap()
}
```

- [ ] **Step 4: Verify GREEN**

```text
cargo test -p corqty-persistence --test catalog_guards
```

Expected: 3 tests pass (`migrations_run_as_a_non_superuser_owner`, `migrated_database_has_no_security_violations`,
`security_violations_report_unguarded_objects`). `migrated_database_has_no_security_violations` now shows that the
Milestone 1 migrations already satisfy every rule.

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

Expected: every suite passes. corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 2,
`members_http` 3, `organizations_http` 1, `problems_http` 2. corqty-domain: `account_and_rbac` 5.
corqty-persistence: `auth_store` 2, `catalog_guards` 3, `final_admin` 2, `migrations` 1, `tenant_isolation` 1,
`test_reset` 1. 31 tests in total.

- [ ] **Step 6: Commit**

```text
git add crates/persistence/tests/catalog_guards.rs
git commit -m "test: guard row-level security in the database catalog"
```

Expected: `git show --stat HEAD` lists exactly the 1 file above (91 insertions).

## Task 6: Add cached server state, MSW tests, and capability-aware UI

**Files:**

- Create: `apps/web/src/api/queries.ts`
- Create: `apps/web/src/auth/use-can.ts`
- Create: `apps/web/src/test/handlers.ts`
- Create: `apps/web/src/test/server.ts`
- Create: `apps/web/src/test/render.tsx`
- Modify: `apps/web/package.json`
- Modify: `pnpm-lock.yaml`
- Modify: `apps/web/src/api/client.ts`
- Modify: `apps/web/src/api/types.ts`
- Modify: `apps/web/src/auth/auth-context.tsx`
- Modify: `apps/web/src/main.tsx`
- Modify: `apps/web/src/app.tsx`
- Modify: `apps/web/src/pages/members-page.tsx`
- Modify: `apps/web/src/pages/audit-page.tsx`
- Modify: `apps/web/src/test/setup.ts`
- Test: `apps/web/src/api/client.test.ts` (new)
- Test: `apps/web/src/auth/auth-context.test.tsx` (new)
- Test: `apps/web/src/app.test.tsx`
- Test: `apps/web/src/pages/admin-pages.test.tsx`

The web app stays on Vite and React Router in this task; Task 7 moves it to Next.js. Server state moves to
TanStack Query, every component test talks to Mock Service Worker instead of `fetch` spies, and the members page
asks `useCan(organizationId, "manage_members")` instead of comparing role strings. `me` stays owned by
`AuthProvider` (there is no `me` query), and `useCan` reads the `capabilities` that Task 1 adds to `/auth/me`.

- [ ] **Step 1: Add TanStack Query and MSW**

Run:

```text
pnpm --filter @corqty/web add @tanstack/react-query@5.104.5
pnpm --filter @corqty/web add -D msw@3.0.3
```

Expected: both commands end with `Done`, write the exact versions (no `^`) into `apps/web/package.json`, and
update `pnpm-lock.yaml`. The second command warns `✕ unmet peer msw@^2.4.9: found 3.0.3` for
`@vitest/mocker 4.1.10`; that optional peer only serves Vitest browser mode and is harmless here.

`apps/web/package.json` is now:

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
  },
  "dependencies": {
    "@base-ui/react": "^1.6.0",
    "@tanstack/react-query": "5.104.5",
    "class-variance-authority": "^0.7.1",
    "clsx": "^2.1.1",
    "lucide-react": "^1.27.0",
    "react": "^19.2.8",
    "react-dom": "^19.2.8",
    "react-router-dom": "^7.18.1",
    "tailwind-merge": "^3.6.0"
  },
  "devDependencies": {
    "@tailwindcss/vite": "^4.3.3",
    "@testing-library/jest-dom": "^7.0.0",
    "@testing-library/react": "^16.3.2",
    "@testing-library/user-event": "^14.6.1",
    "@types/node": "^26.1.2",
    "@types/react": "^19.2.17",
    "@types/react-dom": "^19.2.3",
    "@vitejs/plugin-react": "^6.0.4",
    "jsdom": "^30.0.0",
    "msw": "3.0.3",
    "shadcn": "^4.16.0",
    "tailwindcss": "^4.3.3",
    "tw-animate-css": "^1.4.0",
    "typescript": "^7.0.2",
    "vite": "^8.1.5",
    "vitest": "^4.1.10"
  }
}
```

- [ ] **Step 2: Add the MSW test harness**

`handlers.ts` holds the default handlers (a signed-in admin of `org-1` reading data) and fixtures that later
tasks reuse; tests override writes and failures with `server.use(...)`.

Create `apps/web/src/test/handlers.ts`:

```ts
import { http, HttpResponse } from "msw/http"

import type { AuditEvent, Capability, Me, Member, Role } from "@/api/types"

const capabilities: Record<Role, Capability[]> = {
  admin: [
    "view_organization",
    "edit_workflows",
    "publish_workflows",
    "run_workflows",
    "manage_members",
    "manage_service_tokens",
    "manage_secrets",
    "manage_plugins",
    "enable_shell_executor",
  ],
  editor: [
    "view_organization",
    "edit_workflows",
    "publish_workflows",
    "run_workflows",
  ],
  runner: ["view_organization", "run_workflows"],
  viewer: ["view_organization"],
}

export function meAs(role: Role): Me {
  return {
    user: { id: `${role}-user`, email: `${role}@example.com` },
    organizations: [
      {
        organizationId: "org-1",
        name: "Acme",
        slug: "acme",
        role,
        capabilities: capabilities[role],
      },
    ],
  }
}

export const members: Member[] = [
  { userId: "admin-user", email: "admin@example.com", role: "admin" },
  { userId: "viewer-user", email: "viewer@example.com", role: "viewer" },
]

export const auditEvents: AuditEvent[] = [
  {
    id: 1,
    actorUserId: "admin-user",
    action: "invitation.created",
    resourceType: "invitation",
    resourceId: "i1",
    metadata: { role: "viewer" },
    occurredAt: "2026-07-28T00:00:00Z",
  },
]

// A signed-in admin of org-1 reading data; tests override writes and failures.
export const handlers = [
  http.get("/api/v1/auth/me", () => HttpResponse.json(meAs("admin"))),
  http.get("/api/v1/orgs/:organizationId/members", () =>
    HttpResponse.json(members),
  ),
  http.get("/api/v1/orgs/:organizationId/audit-events", () =>
    HttpResponse.json({ items: auditEvents }),
  ),
]
```

Create `apps/web/src/test/server.ts`:

```ts
import { setupServer } from "msw/node"

import { handlers } from "@/test/handlers"

export const server = setupServer(...handlers)
```

`render.tsx` gives every render a fresh `QueryClient` with `retry: false`, so error tests do not wait through
retries.

Create `apps/web/src/test/render.tsx`:

```tsx
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { render } from "@testing-library/react"
import type { ReactElement } from "react"
import { MemoryRouter } from "react-router-dom"

export function renderWithProviders(ui: ReactElement, { route = "/" } = {}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[route]}>{ui}</MemoryRouter>
    </QueryClientProvider>,
  )
}
```

Replace the contents of `apps/web/src/test/setup.ts` with:

```ts
import "@testing-library/jest-dom/vitest"
import { cleanup } from "@testing-library/react"
import { afterAll, afterEach, beforeAll } from "vitest"

import { server } from "@/test/server"

beforeAll(() => server.listen({ onUnhandledFrame: "error" }))
afterEach(() => {
  cleanup()
  server.resetHandlers()
})
afterAll(() => server.close())
```

- [ ] **Step 3: Write the failing tests and port the existing ones to MSW**

Create `apps/web/src/api/client.test.ts`:

```ts
import { http, HttpResponse } from "msw/http"
import { expect, it } from "vitest"

import { api } from "@/api/client"
import { server } from "@/test/server"

it("sets the JSON content type only for string bodies without one", async () => {
  const contentTypes: (string | null)[] = []
  server.use(
    http.post("/api/v1/echo", ({ request }) => {
      contentTypes.push(request.headers.get("content-type"))
      return new HttpResponse(null, { status: 204 })
    }),
  )
  await api("/echo", { method: "POST", body: "{}" })
  await api("/echo", {
    method: "POST",
    body: "plain",
    headers: { "content-type": "text/plain" },
  })
  await api("/echo", { method: "POST", body: new Uint8Array([1]) })
  expect(contentTypes).toEqual(["application/json", "text/plain", null])
})

it("keeps problem diagnostics and defaults them to an empty list", async () => {
  const diagnostics = [
    { code: "cycle", pointer: "/edges/0", message: "Edge closes a cycle." },
  ]
  server.use(
    http.post("/api/v1/echo", () =>
      HttpResponse.json(
        { code: "graph_invalid", diagnostics },
        { status: 422 },
      ),
    ),
    http.get("/api/v1/echo", () => new HttpResponse(null, { status: 500 })),
  )
  await expect(
    api("/echo", { method: "POST", body: "{}" }),
  ).rejects.toMatchObject({ status: 422, code: "graph_invalid", diagnostics })
  await expect(api("/echo")).rejects.toMatchObject({
    status: 500,
    code: "unknown",
    diagnostics: [],
  })
})
```

Create `apps/web/src/auth/auth-context.test.tsx`:

```tsx
import { act, renderHook, waitFor } from "@testing-library/react"
import { http, HttpResponse } from "msw/http"
import { expect, it } from "vitest"

import { AuthProvider, useAuth } from "@/auth/auth-context"
import { server } from "@/test/server"

it("refresh keeps the session on a server error and clears it on 401", async () => {
  const { result } = renderHook(() => useAuth(), { wrapper: AuthProvider })
  await waitFor(() => {
    expect(result.current.me?.user.email).toBe("admin@example.com")
  })

  server.use(
    http.get("/api/v1/auth/me", () =>
      HttpResponse.json({ code: "internal" }, { status: 500 }),
    ),
  )
  await act(() => result.current.refresh())
  expect(result.current.me?.user.email).toBe("admin@example.com")

  server.use(
    http.get("/api/v1/auth/me", () => new HttpResponse(null, { status: 401 })),
  )
  await act(() => result.current.refresh())
  expect(result.current.me).toBeNull()
})
```

Replace the contents of `apps/web/src/app.test.tsx` with:

```tsx
import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { http, HttpResponse } from "msw/http"
import { describe, expect, it } from "vitest"

import { App } from "@/app"
import { renderWithProviders } from "@/test/render"
import { server } from "@/test/server"

describe("application authentication", () => {
  it("shows the login page for an anonymous protected route", async () => {
    server.use(
      http.get("/api/v1/auth/me", () => new HttpResponse(null, { status: 401 })),
    )
    renderWithProviders(<App />, { route: "/orgs/acme" })
    expect(
      await screen.findByRole("heading", { name: "Sign in to Corqty" }),
    ).toBeInTheDocument()
  })

  it("submits registration and enters the organization shell", async () => {
    let registered: unknown
    server.use(
      http.get(
        "/api/v1/auth/me",
        () => new HttpResponse(null, { status: 401 }),
        { once: true },
      ),
      http.post("/api/v1/auth/register", async ({ request }) => {
        registered = await request.json()
        return HttpResponse.json({ ok: true }, { status: 201 })
      }),
    )
    renderWithProviders(<App />, { route: "/register" })
    await userEvent.type(screen.getByLabelText("Email"), "admin@example.com")
    await userEvent.type(
      screen.getByLabelText("Password"),
      "correct horse battery staple",
    )
    await userEvent.type(screen.getByLabelText("Organization"), "Acme")
    await userEvent.click(screen.getByRole("button", { name: "Create account" }))

    expect(await screen.findByText("Acme")).toBeInTheDocument()
    expect(registered).toEqual({
      email: "admin@example.com",
      password: "correct horse battery staple",
      organizationName: "Acme",
    })
  })
})
```

Replace the contents of `apps/web/src/pages/admin-pages.test.tsx` with:

```tsx
import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { http, HttpResponse } from "msw/http"
import { expect, it, vi } from "vitest"

import { App } from "@/app"
import { meAs } from "@/test/handlers"
import { renderWithProviders } from "@/test/render"
import { server } from "@/test/server"

it("admin can create an invitation and sees the copyable token", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined)
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  })
  let invited: unknown
  server.use(
    http.post(
      "/api/v1/orgs/:organizationId/invitations",
      async ({ request }) => {
        invited = await request.json()
        return HttpResponse.json(
          { token: "invite-once", expiresAt: "2026-08-04T00:00:00Z" },
          { status: 201 },
        )
      },
    ),
  )
  renderWithProviders(<App />, { route: "/orgs/org-1/members" })
  await userEvent.click(
    await screen.findByRole("button", { name: "Invite member" }),
  )
  await userEvent.type(screen.getByLabelText("Email"), "editor@example.com")
  await userEvent.click(
    screen.getByRole("button", { name: "Create invitation" }),
  )
  expect(await screen.findByText("invite-once")).toBeInTheDocument()
  expect(invited).toEqual({ email: "editor@example.com", role: "viewer" })
  await userEvent.click(screen.getByRole("button", { name: "Copy link" }))
  expect(writeText).toHaveBeenCalledWith("invite-once")
})

it("a viewer sees no invite form or member actions", async () => {
  server.use(http.get("/api/v1/auth/me", () => HttpResponse.json(meAs("viewer"))))
  renderWithProviders(<App />, { route: "/orgs/org-1/members" })
  expect(
    await screen.findByRole("cell", { name: "admin@example.com" }),
  ).toBeInTheDocument()
  expect(
    screen.queryByRole("button", { name: "Invite member" }),
  ).not.toBeInTheDocument()
  expect(
    screen.queryByRole("button", { name: "Remove" }),
  ).not.toBeInTheDocument()
  expect(
    screen.queryByRole("combobox", { name: "Role for admin@example.com" }),
  ).not.toBeInTheDocument()
})

it("shows an error when the members cannot be loaded", async () => {
  server.use(
    http.get("/api/v1/orgs/:organizationId/members", () =>
      HttpResponse.json({ code: "internal" }, { status: 500 }),
    ),
  )
  renderWithProviders(<App />, { route: "/orgs/org-1/members" })
  expect(
    await screen.findByText("The members could not be loaded."),
  ).toBeInTheDocument()
})

it("renders administrative audit events", async () => {
  renderWithProviders(<App />, { route: "/orgs/org-1/audit" })
  expect(await screen.findByText("invitation.created")).toBeInTheDocument()
})

it("accepts a fragment invitation and navigates to its organization", async () => {
  let accepted: unknown
  server.use(
    http.post("/api/v1/invitations/accept", async ({ request }) => {
      accepted = await request.json()
      return HttpResponse.json({ organizationId: "org-1", role: "editor" })
    }),
  )
  renderWithProviders(<App />, {
    route: "/invitations/accept#token=invite-once",
  })
  expect(
    await screen.findByRole("heading", { name: "Organization ready" }),
  ).toBeInTheDocument()
  expect(accepted).toEqual({ token: "invite-once" })
})
```

- [ ] **Step 4: Run the tests and verify RED**

Run:

```text
pnpm web:test
```

Expected: `Tests  4 failed | 6 passed (10)` and `Errors  1 error`. The six ported tests already pass on MSW
against the old code. The failures are:

- `src/api/client.test.ts > sets the JSON content type only for string bodies without one`: received
  `["application/json", "application/json", "application/json"]` instead of
  `["application/json", "text/plain", null]`.
- `src/api/client.test.ts > keeps problem diagnostics and defaults them to an empty list`: the thrown
  `ApiProblem` has no `diagnostics`.
- `src/auth/auth-context.test.tsx > refresh keeps the session on a server error and clears it on 401`:
  `AssertionError: expected undefined to be 'admin@example.com'`.
- `src/pages/admin-pages.test.tsx > shows an error when the members cannot be loaded`: `Unable to find an
  element with the text: The members could not be loaded.`; the old page also leaks
  `Unhandled Rejection` `Error: api:500:internal` from `src/pages/members-page.tsx`.

- [ ] **Step 5: Extend the API types and client**

`types.ts` gains the `Capability` union, `OrganizationMembership.capabilities`, `Diagnostic`, and the member,
invitation and audit shapes that used to live inside the pages.

Replace the contents of `apps/web/src/api/types.ts` with:

```ts
export type Role = "admin" | "editor" | "runner" | "viewer"

export type Capability =
  | "view_organization"
  | "edit_workflows"
  | "publish_workflows"
  | "run_workflows"
  | "manage_members"
  | "manage_service_tokens"
  | "manage_secrets"
  | "manage_plugins"
  | "enable_shell_executor"

export interface OrganizationMembership {
  organizationId: string
  name: string
  slug: string
  role: Role
  capabilities: Capability[]
}

export interface Me {
  user: { id: string; email: string }
  organizations: OrganizationMembership[]
}

export interface Diagnostic {
  code: string
  pointer: string
  message: string
}

export interface Member {
  userId: string
  email: string
  role: Role
}

export interface Invitation {
  token: string
  inviteUrl?: string
  expiresAt: string
}

export interface AuditEvent {
  id: number
  actorUserId: string | null
  action: string
  resourceType: string
  resourceId: string
  metadata: Record<string, unknown>
  occurredAt: string
}
```

`api()` sets the JSON content type only for string bodies without an explicit one, and `ApiProblem` keeps the
problem's `diagnostics` (default `[]`). `signal` passes through with the rest of `init`.

Replace the contents of `apps/web/src/api/client.ts` with:

```ts
import type { Diagnostic } from "@/api/types"

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
  if (typeof init.body === "string" && !headers.has("content-type")) {
    headers.set("content-type", "application/json")
  }
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
    const problem = (await response.json().catch(() => ({}))) as {
      code?: string
      diagnostics?: Diagnostic[]
    }
    throw new ApiProblem(
      response.status,
      problem.code ?? "unknown",
      problem.diagnostics ?? [],
    )
  }
  if (response.status === 204) return undefined as T
  return response.json() as Promise<T>
}

export class ApiProblem extends Error {
  constructor(
    public readonly status: number,
    public readonly code: string,
    public readonly diagnostics: Diagnostic[] = [],
  ) {
    super(`api:${status}:${code}`)
  }
}
```

- [ ] **Step 6: Clear the session only on 401 and add useCan**

Replace the contents of `apps/web/src/auth/auth-context.tsx` with:

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

import { ApiProblem, api } from "@/api/client"
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
    } catch (problem) {
      if (problem instanceof ApiProblem && problem.status === 401) setMe(null)
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

Create `apps/web/src/auth/use-can.ts`:

```ts
import type { Capability } from "@/api/types"
import { useAuth } from "@/auth/auth-context"

export function useCan(organizationId: string, capability: Capability) {
  const { me } = useAuth()
  return (
    me?.organizations
      .find((organization) => organization.organizationId === organizationId)
      ?.capabilities.includes(capability) ?? false
  )
}
```

- [ ] **Step 7: Cache members and audit events with TanStack Query**

Query keys start with `["orgs", organizationId]`, so a member write invalidates everything cached for that
organization (members and the audit log both change).

Create `apps/web/src/api/queries.ts`:

```ts
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"

import { api } from "@/api/client"
import type { AuditEvent, Invitation, Member, Role } from "@/api/types"

export const queryKeys = {
  organization: (organizationId: string) => ["orgs", organizationId] as const,
  members: (organizationId: string) =>
    ["orgs", organizationId, "members"] as const,
  auditEvents: (organizationId: string) =>
    ["orgs", organizationId, "audit-events"] as const,
}

export function useMembers(organizationId: string) {
  return useQuery({
    queryKey: queryKeys.members(organizationId),
    queryFn: ({ signal }) =>
      api<Member[]>(`/orgs/${organizationId}/members`, { signal }),
  })
}

export function useAuditEvents(organizationId: string) {
  return useQuery({
    queryKey: queryKeys.auditEvents(organizationId),
    queryFn: ({ signal }) =>
      api<{ items: AuditEvent[] }>(
        `/orgs/${organizationId}/audit-events?limit=50`,
        { signal },
      ),
  })
}

// Member writes also append audit events: refresh the whole organization.
function useOrganizationMutation<Variables, Result>(
  organizationId: string,
  mutationFn: (variables: Variables) => Promise<Result>,
) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn,
    onSuccess: () =>
      queryClient.invalidateQueries({
        queryKey: queryKeys.organization(organizationId),
      }),
  })
}

export function useCreateInvitation(organizationId: string) {
  return useOrganizationMutation(
    organizationId,
    (body: { email: string; role: Role }) =>
      api<Invitation>(`/orgs/${organizationId}/invitations`, {
        method: "POST",
        body: JSON.stringify(body),
      }),
  )
}

export function useChangeMemberRole(organizationId: string) {
  return useOrganizationMutation(
    organizationId,
    ({ userId, role }: { userId: string; role: Role }) =>
      api<void>(`/orgs/${organizationId}/members/${userId}`, {
        method: "PATCH",
        body: JSON.stringify({ role }),
      }),
  )
}

export function useRemoveMember(organizationId: string) {
  return useOrganizationMutation(organizationId, (userId: string) =>
    api<void>(`/orgs/${organizationId}/members/${userId}`, {
      method: "DELETE",
    }),
  )
}
```

Replace the contents of `apps/web/src/main.tsx` with:

```tsx
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
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

const queryClient = new QueryClient()

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <App />
      </BrowserRouter>
    </QueryClientProvider>
  </StrictMode>,
)
```

Replace the contents of `apps/web/src/pages/audit-page.tsx` with:

```tsx
import { useAuditEvents } from "@/api/queries"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

export function AuditPage({ organizationId }: { organizationId: string }) {
  const auditEvents = useAuditEvents(organizationId)

  return (
    <section className="flex flex-col gap-5 p-4 md:p-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Audit log</h1>
        <p className="text-sm text-muted-foreground">
          Append-only administrative history.
        </p>
      </div>
      {auditEvents.isPending && (
        <p className="text-sm text-muted-foreground">Loading audit events…</p>
      )}
      {auditEvents.isError && (
        <p role="alert" className="text-sm text-destructive">
          The audit log could not be loaded.
        </p>
      )}
      <div className="overflow-x-auto rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Time</TableHead>
              <TableHead>Action</TableHead>
              <TableHead>Resource</TableHead>
              <TableHead>Actor</TableHead>
              <TableHead>Metadata</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {auditEvents.data?.items.map((item) => (
              <TableRow key={item.id}>
                <TableCell>
                  {new Date(item.occurredAt).toLocaleString()}
                </TableCell>
                <TableCell className="font-mono text-xs">
                  {item.action}
                </TableCell>
                <TableCell>
                  {item.resourceType}:{item.resourceId}
                </TableCell>
                <TableCell>{item.actorUserId ?? "system"}</TableCell>
                <TableCell className="font-mono text-xs">
                  {JSON.stringify(item.metadata)}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </section>
  )
}
```

In `apps/web/src/pages/members-page.tsx`, apply these replacements in order.

Replace:

```tsx
import { useCallback, useEffect, useState } from "react"
import type { FormEvent } from "react"

import { ApiProblem, api } from "@/api/client"
import type { Role } from "@/api/types"
```

With:

```tsx
import { useState } from "react"
import type { FormEvent } from "react"

import { ApiProblem } from "@/api/client"
import {
  useChangeMemberRole,
  useCreateInvitation,
  useMembers,
  useRemoveMember,
} from "@/api/queries"
import type { Invitation, Role } from "@/api/types"
import { useAuth } from "@/auth/auth-context"
import { useCan } from "@/auth/use-can"
```

Replace:

```tsx
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

const roles: Role[] = ["admin", "editor", "runner", "viewer"]

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
```

With:

```tsx
const roles: Role[] = ["admin", "editor", "runner", "viewer"]

function memberError(problem: unknown, fallback: string) {
  return problem instanceof ApiProblem && problem.code === "final_admin_required"
    ? "Every organization must keep one admin."
    : fallback
}

export function MembersPage({ organizationId }: { organizationId: string }) {
  const { me } = useAuth()
  const canManage = useCan(organizationId, "manage_members")
  const members = useMembers(organizationId)
  const createInvitation = useCreateInvitation(organizationId)
  const changeMemberRole = useChangeMemberRole(organizationId)
  const removeMember = useRemoveMember(organizationId)
  const [showInvite, setShowInvite] = useState(false)
  const [email, setEmail] = useState("")
  const [role, setRole] = useState<Role>("viewer")
  const [invitation, setInvitation] = useState<Invitation | null>(null)
  const [error, setError] = useState("")

  async function invite(event: FormEvent) {
    event.preventDefault()
    setError("")
    try {
      setInvitation(await createInvitation.mutateAsync({ email, role }))
      setEmail("")
```

Replace:

```tsx
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
```

With:

```tsx
    try {
      await changeMemberRole.mutateAsync({ userId, role: nextRole })
    } catch (problem) {
      setError(memberError(problem, "The member role could not be changed."))
    }
```

Replace:

```tsx
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
```

With:

```tsx
    try {
      await removeMember.mutateAsync(userId)
    } catch (problem) {
      setError(memberError(problem, "The member could not be removed."))
    }
```

Replace:

```tsx
        {admin && (
          <Button onClick={() => setShowInvite(true)}>Invite member</Button>
```

With:

```tsx
        {canManage && (
          <Button onClick={() => setShowInvite(true)}>Invite member</Button>
```

Replace:

```tsx
      {admin && (
        <Dialog
```

With:

```tsx
      {members.isPending && (
        <p className="text-sm text-muted-foreground">Loading members…</p>
      )}
      {members.isError && (
        <p role="alert" className="text-sm text-destructive">
          The members could not be loaded.
        </p>
      )}
      {canManage && (
        <Dialog
```

Replace:

```tsx
            {members.map((member) => (
```

With:

```tsx
            {members.data?.map((member) => (
```

Replace:

```tsx
                  {admin ? (
```

With:

```tsx
                  {canManage ? (
```

Replace:

```tsx
                  {admin && member.userId !== currentUserId && (
```

With:

```tsx
                  {canManage && member.userId !== me?.user.id && (
```

`MembersRoute` no longer passes a role or user id:

Replace the contents of `apps/web/src/app.tsx` with:

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
  return membership ? (
    <MembersPage organizationId={membership.organizationId} />
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

- [ ] **Step 8: Verify GREEN**

Run:

```text
pnpm web:test
```

Expected: `Test Files  4 passed (4)` and `Tests  10 passed (10)` with no unhandled errors. On Node 25 or newer
each worker may print `ExperimentalWarning: localStorage is not available because --localstorage-file was not
provided.`; it is harmless.

- [ ] **Step 9: Run lint and build**

Run:

```text
pnpm web:lint
```

Expected: `tsc -b --pretty false` exits 0 with no diagnostics.

Run:

```text
pnpm web:build
```

Expected: `tsc -b && vite build` exits 0 and prints `✓ built in`.

- [ ] **Step 10: Commit**

```text
git add apps/web pnpm-lock.yaml
git commit -m "feat: add cached server state and capability-aware UI"
```

## Task 7: Move the web app to Next.js App Router

**Files:**

- Create: `apps/web/next.config.ts`
- Create: `apps/web/postcss.config.mjs`
- Create: `apps/web/.oxlintrc.json`
- Create: `apps/web/vitest.config.ts`
- Create: `apps/web/src/app/layout.tsx`
- Create: `apps/web/src/app/providers.tsx`
- Create: `apps/web/src/app/(public)/login/page.tsx`
- Create: `apps/web/src/app/(public)/register/page.tsx`
- Create: `apps/web/src/app/(app)/layout.tsx`
- Create: `apps/web/src/app/(app)/page.tsx`
- Create: `apps/web/src/app/(app)/invitations/accept/page.tsx`
- Create: `apps/web/src/app/(app)/orgs/[organizationId]/layout.tsx`
- Create: `apps/web/src/app/(app)/orgs/[organizationId]/page.tsx`
- Create: `apps/web/src/app/(app)/orgs/[organizationId]/members/page.tsx`
- Create: `apps/web/src/app/(app)/orgs/[organizationId]/audit/page.tsx`
- Create: `apps/web/src/auth/auth-gate.tsx`
- Create: `apps/web/src/auth/return-to.ts`
- Create: `apps/web/src/views/first-organization-view.tsx`
- Modify: `apps/web/package.json`
- Modify: `pnpm-lock.yaml`
- Modify: `apps/web/tsconfig.json`
- Modify: `apps/web/components.json`
- Modify: `apps/web/src/app/globals.css` (moved unchanged from `apps/web/src/styles/globals.css`)
- Modify: `apps/web/src/views/accept-invitation-view.tsx` (moved from `apps/web/src/pages/accept-invitation-page.tsx`)
- Modify: `apps/web/src/views/audit-view.tsx` (moved from `apps/web/src/pages/audit-page.tsx`)
- Modify: `apps/web/src/views/login-view.tsx` (moved from `apps/web/src/pages/login-page.tsx`)
- Modify: `apps/web/src/views/members-view.tsx` (moved from `apps/web/src/pages/members-page.tsx`)
- Modify: `apps/web/src/views/organization-home-view.tsx` (moved from `apps/web/src/pages/organization-home-page.tsx`)
- Modify: `apps/web/src/views/register-view.tsx` (moved from `apps/web/src/pages/register-page.tsx`)
- Modify: `apps/web/src/layout/app-shell.tsx`
- Modify: `apps/web/src/test/render.tsx`
- Modify: `apps/web/src/test/setup.ts`
- Modify: `.gitignore`
- Modify: `.dockerignore`
- Modify: `.env.example`
- Modify: `README.md`
- Modify: `docker-compose.yml`
- Modify: `crates/api/src/config.rs`
- Modify: `scripts/smoke-foundation.ps1`
- Delete: `apps/web/vite.config.ts`
- Delete: `apps/web/index.html`
- Delete: `apps/web/tsconfig.app.json`
- Delete: `apps/web/src/main.tsx`
- Delete: `apps/web/src/app.tsx`
- Delete: `apps/web/src/auth/protected-route.tsx`
- Delete: `apps/web/src/app.test.tsx`
- Delete: `apps/web/src/pages/admin-pages.test.tsx`
- Test: `apps/web/src/views/auth-views.test.tsx` (new, replaces `apps/web/src/app.test.tsx`)
- Test: `apps/web/src/views/admin-views.test.tsx` (new, replaces `apps/web/src/pages/admin-pages.test.tsx`)

The web app moves from Vite and React Router to the Next.js 16 App Router and stays client-heavy. Route files
under `src/app` only unwrap route params and render a `"use client"` view from `src/views`. Every API call still
goes through `api()` to the same-origin `/api/v1`, which `next.config.ts` rewrites to the Rust API.
`src/pages` must not exist, because Next.js would treat it as the Pages Router. The tests keep their MSW
handlers. `next/navigation` is mocked once in `setup.ts`, and the tests render views inside the same `AuthGate`
and `AppShell` that the layouts use. They assert on the mocked router instead of following navigation.

- [ ] **Step 1: Swap the web dependencies**

Run:

```text
pnpm --filter @corqty/web add next@16.4.3 react@19.3.1 react-dom@19.3.1
pnpm --filter @corqty/web add -D @tailwindcss/postcss@4.3.3 postcss@8.5.31 oxlint@1.89.0 @types/react@19.3.0 @types/react-dom@19.3.0
pnpm --filter @corqty/web remove react-router-dom @tailwindcss/vite
```

Expected: each command ends with `Done` and updates `pnpm-lock.yaml`. Each one prints the warning
`✕ unmet peer msw@^2.4.9: found 3.0.3` for `@vitest/mocker 4.1.10`, which is harmless (Task 6). New packages are
saved with exact versions (`"next": "16.4.3"`). Packages that already had a caret keep it
(`"react": "^19.3.1"`).

The scripts switch to Next.js. `lint` runs `next typegen` first, so `tsc` sees the generated `PageProps` and
`LayoutProps` route types.

Replace the contents of `apps/web/package.json` with:

```json
{
  "name": "@corqty/web",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "build": "next build",
    "dev": "next dev -H 0.0.0.0 -p 3000",
    "lint": "next typegen && tsc --noEmit && oxlint",
    "test": "vitest run"
  },
  "dependencies": {
    "@base-ui/react": "^1.6.0",
    "@tanstack/react-query": "5.104.5",
    "class-variance-authority": "^0.7.1",
    "clsx": "^2.1.1",
    "lucide-react": "^1.27.0",
    "next": "16.4.3",
    "react": "^19.3.1",
    "react-dom": "^19.3.1",
    "tailwind-merge": "^3.6.0"
  },
  "devDependencies": {
    "@tailwindcss/postcss": "4.3.3",
    "@testing-library/jest-dom": "^7.0.0",
    "@testing-library/react": "^16.3.2",
    "@testing-library/user-event": "^14.6.1",
    "@types/node": "^26.1.2",
    "@types/react": "^19.3.0",
    "@types/react-dom": "^19.3.0",
    "@vitejs/plugin-react": "^6.0.4",
    "jsdom": "^30.0.0",
    "msw": "3.0.3",
    "oxlint": "1.89.0",
    "postcss": "8.5.31",
    "shadcn": "^4.16.0",
    "tailwindcss": "^4.3.3",
    "tw-animate-css": "^1.4.0",
    "typescript": "^7.0.2",
    "vite": "^8.1.5",
    "vitest": "^4.1.10"
  }
}
```

- [ ] **Step 2: Replace the Vite config with a Vitest config**

Next.js ignores `vitest.config.ts`. Only the React plugin and the `@` alias stay: Tailwind now runs through
PostCSS, and the `/api` dev proxy becomes a Next.js rewrite in Step 5.

Run:

```text
git rm apps/web/vite.config.ts
```

Expected: `rm 'apps/web/vite.config.ts'`.

Create `apps/web/vitest.config.ts`:

```ts
import { fileURLToPath } from "node:url"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vitest/config"

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
  },
})
```

- [ ] **Step 3: Write the failing tests on a mocked next/navigation**

`setup.ts` mocks `next/navigation` for every test file. `useRouter()` returns the `router` exported by
`render.tsx`, and `usePathname()` reads `window.location`. `renderWithProviders` moves jsdom's URL to the route
with `history.replaceState`, so `window.location.hash` and `usePathname()` match it. It also wraps the UI in
`AuthProvider`, which `<App />` used to supply. After each test, `setup.ts` also clears `sessionStorage` and the
router mocks.

Replace the contents of `apps/web/src/test/render.tsx` with:

```tsx
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { render } from "@testing-library/react"
import type { ReactElement } from "react"
import { vi } from "vitest"

import { AuthProvider } from "@/auth/auth-context"

// setup.ts mocks next/navigation: useRouter returns this router and
// usePathname reads window.location.
export const router = { push: vi.fn(), replace: vi.fn() }

export function renderWithProviders(ui: ReactElement, { route = "/" } = {}) {
  window.history.replaceState(null, "", route)
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  return render(
    <QueryClientProvider client={queryClient}>
      <AuthProvider>{ui}</AuthProvider>
    </QueryClientProvider>,
  )
}
```

Replace the contents of `apps/web/src/test/setup.ts` with:

```ts
import "@testing-library/jest-dom/vitest"
import { cleanup } from "@testing-library/react"
import { afterAll, afterEach, beforeAll, vi } from "vitest"

import { server } from "@/test/server"

vi.mock("next/navigation", async () => {
  const { router } = await import("@/test/render")
  return {
    usePathname: () => window.location.pathname,
    useRouter: () => router,
  }
})

beforeAll(() => server.listen({ onUnhandledFrame: "error" }))
afterEach(() => {
  cleanup()
  server.resetHandlers()
  sessionStorage.clear()
  vi.clearAllMocks()
})
afterAll(() => server.close())
```

The ported tests move next to the views:

```text
git rm apps/web/src/app.test.tsx apps/web/src/pages/admin-pages.test.tsx
```

Expected: `rm 'apps/web/src/app.test.tsx'` and `rm 'apps/web/src/pages/admin-pages.test.tsx'`.

Create `apps/web/src/views/auth-views.test.tsx`:

```tsx
import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { http, HttpResponse } from "msw/http"
import { expect, it } from "vitest"

import { AuthGate } from "@/auth/auth-gate"
import { renderWithProviders, router } from "@/test/render"
import { server } from "@/test/server"
import { AcceptInvitationView } from "@/views/accept-invitation-view"
import { FirstOrganizationView } from "@/views/first-organization-view"
import { LoginView } from "@/views/login-view"
import { RegisterView } from "@/views/register-view"

function signedOutOnce() {
  server.use(
    http.get(
      "/api/v1/auth/me",
      () => new HttpResponse(null, { status: 401 }),
      { once: true },
    ),
  )
}

it("sends an anonymous user to login and remembers the page", async () => {
  signedOutOnce()
  renderWithProviders(
    <AuthGate>
      <AcceptInvitationView />
    </AuthGate>,
    { route: "/invitations/accept#token=invite-once" },
  )
  await waitFor(() => expect(router.replace).toHaveBeenCalledWith("/login"))
  expect(sessionStorage.getItem("corqty.returnTo")).toBe(
    "/invitations/accept#token=invite-once",
  )
})

it("submits registration and returns to the remembered page", async () => {
  sessionStorage.setItem(
    "corqty.returnTo",
    "/invitations/accept#token=invite-once",
  )
  signedOutOnce()
  let registered: unknown
  server.use(
    http.post("/api/v1/auth/register", async ({ request }) => {
      registered = await request.json()
      return HttpResponse.json({ ok: true }, { status: 201 })
    }),
  )
  renderWithProviders(<RegisterView />, { route: "/register" })
  await userEvent.type(screen.getByLabelText("Email"), "admin@example.com")
  await userEvent.type(
    screen.getByLabelText("Password"),
    "correct horse battery staple",
  )
  await userEvent.type(screen.getByLabelText("Organization"), "Acme")
  await userEvent.click(screen.getByRole("button", { name: "Create account" }))

  await waitFor(() =>
    expect(router.replace).toHaveBeenCalledWith(
      "/invitations/accept#token=invite-once",
    ),
  )
  expect(sessionStorage.getItem("corqty.returnTo")).toBeNull()
  expect(registered).toEqual({
    email: "admin@example.com",
    password: "correct horse battery staple",
    organizationName: "Acme",
  })
})

it("signs in and ignores a return path to another origin", async () => {
  sessionStorage.setItem("corqty.returnTo", "//evil.example/orgs")
  signedOutOnce()
  server.use(
    http.post("/api/v1/auth/login", () => HttpResponse.json({ ok: true })),
  )
  renderWithProviders(<LoginView />, { route: "/login" })
  await userEvent.type(screen.getByLabelText("Email"), "admin@example.com")
  await userEvent.type(
    screen.getByLabelText("Password"),
    "correct horse battery staple",
  )
  await userEvent.click(screen.getByRole("button", { name: "Sign in" }))

  await waitFor(() => expect(router.replace).toHaveBeenCalledWith("/"))
  expect(sessionStorage.getItem("corqty.returnTo")).toBeNull()
})

it("opens the first organization from the home page", async () => {
  renderWithProviders(
    <AuthGate>
      <FirstOrganizationView />
    </AuthGate>,
  )
  await waitFor(() =>
    expect(router.replace).toHaveBeenCalledWith("/orgs/org-1"),
  )
})
```

`renderInShell` mirrors the `(app)` and `orgs/[organizationId]` layouts. The members view therefore renders only
after `me` has loaded, which keeps the viewer test deterministic.

Create `apps/web/src/views/admin-views.test.tsx`:

```tsx
import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { http, HttpResponse } from "msw/http"
import type { ReactElement } from "react"
import { expect, it, vi } from "vitest"

import { AuthGate } from "@/auth/auth-gate"
import { AppShell } from "@/layout/app-shell"
import { meAs } from "@/test/handlers"
import { renderWithProviders, router } from "@/test/render"
import { server } from "@/test/server"
import { AcceptInvitationView } from "@/views/accept-invitation-view"
import { AuditView } from "@/views/audit-view"
import { MembersView } from "@/views/members-view"

// Mirrors the (app) and orgs/[organizationId] layouts around a page.
function renderInShell(view: ReactElement, route: string) {
  return renderWithProviders(
    <AuthGate>
      <AppShell organizationId="org-1">{view}</AppShell>
    </AuthGate>,
    { route },
  )
}

it("admin can create an invitation and sees the copyable token", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined)
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  })
  let invited: unknown
  server.use(
    http.post(
      "/api/v1/orgs/:organizationId/invitations",
      async ({ request }) => {
        invited = await request.json()
        return HttpResponse.json(
          { token: "invite-once", expiresAt: "2026-08-04T00:00:00Z" },
          { status: 201 },
        )
      },
    ),
  )
  renderInShell(<MembersView organizationId="org-1" />, "/orgs/org-1/members")
  await userEvent.click(
    await screen.findByRole("button", { name: "Invite member" }),
  )
  await userEvent.type(screen.getByLabelText("Email"), "editor@example.com")
  await userEvent.click(
    screen.getByRole("button", { name: "Create invitation" }),
  )
  expect(await screen.findByText("invite-once")).toBeInTheDocument()
  expect(invited).toEqual({ email: "editor@example.com", role: "viewer" })
  await userEvent.click(screen.getByRole("button", { name: "Copy link" }))
  expect(writeText).toHaveBeenCalledWith("invite-once")
})

it("a viewer sees no invite form or member actions", async () => {
  server.use(http.get("/api/v1/auth/me", () => HttpResponse.json(meAs("viewer"))))
  renderInShell(<MembersView organizationId="org-1" />, "/orgs/org-1/members")
  expect(
    await screen.findByRole("cell", { name: "admin@example.com" }),
  ).toBeInTheDocument()
  expect(
    screen.queryByRole("button", { name: "Invite member" }),
  ).not.toBeInTheDocument()
  expect(
    screen.queryByRole("button", { name: "Remove" }),
  ).not.toBeInTheDocument()
  expect(
    screen.queryByRole("combobox", { name: "Role for admin@example.com" }),
  ).not.toBeInTheDocument()
})

it("shows an error when the members cannot be loaded", async () => {
  server.use(
    http.get("/api/v1/orgs/:organizationId/members", () =>
      HttpResponse.json({ code: "internal" }, { status: 500 }),
    ),
  )
  renderInShell(<MembersView organizationId="org-1" />, "/orgs/org-1/members")
  expect(
    await screen.findByText("The members could not be loaded."),
  ).toBeInTheDocument()
})

it("renders administrative audit events", async () => {
  renderInShell(<AuditView organizationId="org-1" />, "/orgs/org-1/audit")
  expect(await screen.findByText("invitation.created")).toBeInTheDocument()
  expect(screen.getByRole("link", { name: "Audit log" })).toHaveAttribute(
    "aria-current",
    "page",
  )
})

it("accepts a fragment invitation and opens its organization", async () => {
  let accepted: unknown
  server.use(
    http.post("/api/v1/invitations/accept", async ({ request }) => {
      accepted = await request.json()
      return HttpResponse.json({ organizationId: "org-1", role: "editor" })
    }),
  )
  renderWithProviders(
    <AuthGate>
      <AcceptInvitationView />
    </AuthGate>,
    { route: "/invitations/accept#token=invite-once" },
  )
  await waitFor(() =>
    expect(router.replace).toHaveBeenCalledWith("/orgs/org-1"),
  )
  expect(accepted).toEqual({ token: "invite-once" })
})
```

- [ ] **Step 4: Run the tests and verify RED**

Run:

```text
pnpm web:test
```

Expected: `Test Files  2 failed | 2 passed (4)` and `Tests  3 passed (3)`. Both new files fail to load with
`Error: Failed to resolve import "@/auth/auth-gate" from "src/views/admin-views.test.tsx". Does the file exist?`
(the same error for `src/views/auth-views.test.tsx`). Vite may name another missing `@/views/...` module first.

- [ ] **Step 5: Add the Next.js configuration**

The rewrite proxies `/api/v1` to `CORQTY_API_ORIGIN`. `next dev` reads it at startup, and `next build` freezes it
into the routes manifest. `turbopack.root` is the repository root, because pnpm keeps packages in the root
`node_modules/.pnpm`. `agentRules: false` stops `next dev` from writing into `AGENTS.md` and `CLAUDE.md`.

Create `apps/web/next.config.ts`:

```ts
import path from "node:path"
import type { NextConfig } from "next"

const apiOrigin = process.env.CORQTY_API_ORIGIN ?? "http://127.0.0.1:8080"

const nextConfig: NextConfig = {
  agentRules: false,
  turbopack: { root: path.resolve(import.meta.dirname, "../..") },
  async rewrites() {
    return [
      { source: "/api/v1/:path*", destination: `${apiOrigin}/api/v1/:path*` },
    ]
  },
}

export default nextConfig
```

Create `apps/web/postcss.config.mjs`:

```js
export default {
  plugins: { "@tailwindcss/postcss": {} },
}
```

Setting `plugins` replaces oxlint's defaults, so the four defaults are listed again next to `react`. The `react`
plugin also turns on `react/set-state-in-effect`, which is switched off here. It flags `void refresh()` in
`AuthProvider` even though that function only sets state after an `await`. It also flags the accept view's
missing-token message, which has to be set in an effect because the token is in `window.location.hash`.

Create `apps/web/.oxlintrc.json`:

```json
{
  "$schema": "./node_modules/oxlint/configuration_schema.json",
  "plugins": ["eslint", "typescript", "unicorn", "oxc", "react"],
  "rules": {
    "react/rules-of-hooks": "error",
    "react/exhaustive-deps": "error",
    "react/set-state-in-effect": "off"
  },
  "ignorePatterns": [".next/**", "next-env.d.ts"]
}
```

`tsconfig.json` becomes a single flat project that `next build` and `tsc --noEmit` both check. TypeScript 7
defaults `types` to `[]`, so they are listed. `allowJs: true` is the value Next.js adds on its first run;
with it already present, Next.js leaves the file untouched.

Replace the contents of `apps/web/tsconfig.json` with:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["DOM", "DOM.Iterable", "ESNext"],
    "allowJs": true,
    "skipLibCheck": true,
    "strict": true,
    "noEmit": true,
    "esModuleInterop": true,
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "resolveJsonModule": true,
    "isolatedModules": true,
    "jsx": "react-jsx",
    "incremental": true,
    "plugins": [{ "name": "next" }],
    "paths": { "@/*": ["./src/*"] },
    "types": ["node", "vitest/globals", "@testing-library/jest-dom"]
  },
  "include": [
    "next-env.d.ts",
    "src/**/*.ts",
    "src/**/*.tsx",
    ".next/types/**/*.ts",
    ".next/dev/types/**/*.ts",
    "*.ts",
    "*.mts"
  ],
  "exclude": ["node_modules"]
}
```

Replace the contents of `apps/web/components.json` with:

```json
{
  "$schema": "https://ui.shadcn.com/schema.json",
  "style": "base-nova",
  "rsc": true,
  "tsx": true,
  "tailwind": {
    "config": "",
    "css": "src/app/globals.css",
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

Remove the Vite entry point, the React Router app and its route guard:

```text
git rm apps/web/index.html apps/web/tsconfig.app.json apps/web/src/main.tsx apps/web/src/app.tsx apps/web/src/auth/protected-route.tsx
```

Expected: one `rm '...'` line per file.

- [ ] **Step 6: Add the root layout, providers and routes**

The inline script replaces the `matchMedia` toggle that lived in `main.tsx`. It runs before hydration, so
`<html>` carries `suppressHydrationWarning` for the `dark` class the script adds.

Create `apps/web/src/app/layout.tsx`:

```tsx
import type { Metadata } from "next"
import type { ReactNode } from "react"

import { Providers } from "@/app/providers"
import "./globals.css"

export const metadata: Metadata = { title: "Corqty" }

// Runs before hydration so the first paint already follows the system theme.
const themeScript = `(() => {
  const media = window.matchMedia("(prefers-color-scheme: dark)")
  const apply = () =>
    document.documentElement.classList.toggle("dark", media.matches)
  apply()
  media.addEventListener("change", apply)
})()`

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
      </head>
      <body>
        <Providers>{children}</Providers>
      </body>
    </html>
  )
}
```

`Providers` owns the browser `QueryClient` (default options, as in Task 6) and `AuthProvider`.

Create `apps/web/src/app/providers.tsx`:

```tsx
"use client"

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { useState } from "react"
import type { ReactNode } from "react"

import { AuthProvider } from "@/auth/auth-context"

export function Providers({ children }: { children: ReactNode }) {
  const [queryClient] = useState(() => new QueryClient())
  return (
    <QueryClientProvider client={queryClient}>
      <AuthProvider>{children}</AuthProvider>
    </QueryClientProvider>
  )
}
```

Move the stylesheet next to the layout without changing it:

```text
git mv apps/web/src/styles/globals.css apps/web/src/app/globals.css
```

Expected: no output.

Each route file renders one view. Organization routes await `params` (a Promise in Next.js 16) and pass the id
down. The organization layout wraps its pages in `AppShell`, which replaces React Router's `<Outlet />` with
`children`.

Create `apps/web/src/app/(public)/login/page.tsx`:

```tsx
import { LoginView } from "@/views/login-view"

export default function LoginPage() {
  return <LoginView />
}
```

Create `apps/web/src/app/(public)/register/page.tsx`:

```tsx
import { RegisterView } from "@/views/register-view"

export default function RegisterPage() {
  return <RegisterView />
}
```

Create `apps/web/src/app/(app)/layout.tsx`:

```tsx
import type { ReactNode } from "react"

import { AuthGate } from "@/auth/auth-gate"

export default function SignedInLayout({ children }: { children: ReactNode }) {
  return <AuthGate>{children}</AuthGate>
}
```

Create `apps/web/src/app/(app)/page.tsx`:

```tsx
import { FirstOrganizationView } from "@/views/first-organization-view"

export default function HomePage() {
  return <FirstOrganizationView />
}
```

Create `apps/web/src/app/(app)/invitations/accept/page.tsx`:

```tsx
import { AcceptInvitationView } from "@/views/accept-invitation-view"

export default function AcceptInvitationPage() {
  return <AcceptInvitationView />
}
```

Create `apps/web/src/app/(app)/orgs/[organizationId]/layout.tsx`:

```tsx
import { AppShell } from "@/layout/app-shell"

export default async function OrganizationLayout({
  children,
  params,
}: LayoutProps<"/orgs/[organizationId]">) {
  const { organizationId } = await params
  return <AppShell organizationId={organizationId}>{children}</AppShell>
}
```

Create `apps/web/src/app/(app)/orgs/[organizationId]/page.tsx`:

```tsx
import { OrganizationHomeView } from "@/views/organization-home-view"

export default async function OrganizationPage({
  params,
}: PageProps<"/orgs/[organizationId]">) {
  const { organizationId } = await params
  return <OrganizationHomeView organizationId={organizationId} />
}
```

Create `apps/web/src/app/(app)/orgs/[organizationId]/members/page.tsx`:

```tsx
import { MembersView } from "@/views/members-view"

export default async function MembersPage({
  params,
}: PageProps<"/orgs/[organizationId]/members">) {
  const { organizationId } = await params
  return <MembersView organizationId={organizationId} />
}
```

Create `apps/web/src/app/(app)/orgs/[organizationId]/audit/page.tsx`:

```tsx
import { AuditView } from "@/views/audit-view"

export default async function AuditPage({
  params,
}: PageProps<"/orgs/[organizationId]/audit">) {
  const { organizationId } = await params
  return <AuditView organizationId={organizationId} />
}
```

- [ ] **Step 7: Add the auth gate and the return path**

When `me` is null, the gate stores `pathname + search + hash` in `sessionStorage["corqty.returnTo"]` and replaces
the route with `/login` (no query string). After sign-in or registration, `takeReturnTo()` reads and removes
the value. It returns the value only if it starts with `/` and not `//`; otherwise it returns `/`.

Create `apps/web/src/auth/return-to.ts`:

```ts
const key = "corqty.returnTo"

// Storage can throw (disabled storage, private mode); returning is best effort.
export function rememberReturnTo(path: string) {
  try {
    sessionStorage.setItem(key, path)
  } catch {}
}

export function takeReturnTo(): string {
  let path: string | null = null
  try {
    path = sessionStorage.getItem(key)
    sessionStorage.removeItem(key)
  } catch {}
  return path?.startsWith("/") && !path.startsWith("//") ? path : "/"
}
```

Create `apps/web/src/auth/auth-gate.tsx`:

```tsx
"use client"

import { useRouter } from "next/navigation"
import { useEffect } from "react"
import type { ReactNode } from "react"

import { useAuth } from "@/auth/auth-context"
import { rememberReturnTo } from "@/auth/return-to"

export function AuthGate({ children }: { children: ReactNode }) {
  const { me, loading } = useAuth()
  const router = useRouter()

  useEffect(() => {
    if (loading || me) return
    const { pathname, search, hash } = window.location
    rememberReturnTo(`${pathname}${search}${hash}`)
    router.replace("/login")
  }, [loading, me, router])

  if (!me) {
    return <div className="p-6 text-sm text-muted-foreground">Loading…</div>
  }
  return children
}
```

- [ ] **Step 8: Move the screens into views**

Run:

```text
git mv apps/web/src/pages/accept-invitation-page.tsx apps/web/src/views/accept-invitation-view.tsx
git mv apps/web/src/pages/audit-page.tsx apps/web/src/views/audit-view.tsx
git mv apps/web/src/pages/login-page.tsx apps/web/src/views/login-view.tsx
git mv apps/web/src/pages/members-page.tsx apps/web/src/views/members-view.tsx
git mv apps/web/src/pages/organization-home-page.tsx apps/web/src/views/organization-home-view.tsx
git mv apps/web/src/pages/register-page.tsx apps/web/src/views/register-view.tsx
git clean -fd apps/web/src/pages apps/web/src/styles
```

Expected: the moves print nothing. Git does not track directories, so `git clean` prints
`Removing apps/web/src/pages/` and `Removing apps/web/src/styles/` if those empty directories were left behind;
otherwise it prints nothing. Afterwards neither directory exists.

The home route sends a signed-in user to the first organization:

Create `apps/web/src/views/first-organization-view.tsx`:

```tsx
"use client"

import { useRouter } from "next/navigation"
import { useEffect } from "react"

import { useAuth } from "@/auth/auth-context"

export function FirstOrganizationView() {
  const { me } = useAuth()
  const router = useRouter()
  const organizationId = me?.organizations[0]?.organizationId

  useEffect(() => {
    if (organizationId) router.replace(`/orgs/${organizationId}`)
  }, [organizationId, router])

  if (organizationId) return null
  return (
    <p className="p-6 text-sm text-muted-foreground">
      No organization memberships.
    </p>
  )
}
```

The accept view reads the token from `window.location.hash` inside its effect and replaces the route with the
organization:

Replace the contents of `apps/web/src/views/accept-invitation-view.tsx` with:

```tsx
"use client"

import { useRouter } from "next/navigation"
import { useEffect, useRef, useState } from "react"

import { api } from "@/api/client"
import { useAuth } from "@/auth/auth-context"

interface AcceptedInvitation {
  organizationId: string
  role: string
}

export function AcceptInvitationView() {
  const router = useRouter()
  const { refresh } = useAuth()
  const [error, setError] = useState("")
  const started = useRef(false)

  useEffect(() => {
    if (started.current) return
    started.current = true
    // The token travels in the fragment, so it never reaches a server log.
    const token = new URLSearchParams(window.location.hash.slice(1)).get(
      "token",
    )
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
        router.replace(`/orgs/${accepted.organizationId}`)
      })
      .catch(() => setError("This invitation is invalid or expired."))
  }, [refresh, router])

  return (
    <main className="grid min-h-screen place-items-center p-6">
      <p className="text-sm text-muted-foreground">
        {error || "Accepting invitation…"}
      </p>
    </main>
  )
}
```

Replace the contents of `apps/web/src/views/organization-home-view.tsx` with:

```tsx
"use client"

export function OrganizationHomeView({
  organizationId,
}: {
  organizationId: string
}) {
  return (
    <section className="flex flex-col gap-2 p-4 md:p-6">
      <p className="text-xs font-medium tracking-wider text-muted-foreground uppercase">
        Foundation
      </p>
      <h1 className="text-2xl font-semibold tracking-tight">
        Organization ready
      </h1>
      <p className="text-sm text-muted-foreground">
        Tenant context: {organizationId}
      </p>
    </section>
  )
}
```

Replace the contents of `apps/web/src/views/audit-view.tsx` with:

```tsx
"use client"

import { useAuditEvents } from "@/api/queries"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

export function AuditView({ organizationId }: { organizationId: string }) {
  const auditEvents = useAuditEvents(organizationId)

  return (
    <section className="flex flex-col gap-5 p-4 md:p-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Audit log</h1>
        <p className="text-sm text-muted-foreground">
          Append-only administrative history.
        </p>
      </div>
      {auditEvents.isPending && (
        <p className="text-sm text-muted-foreground">Loading audit events…</p>
      )}
      {auditEvents.isError && (
        <p role="alert" className="text-sm text-destructive">
          The audit log could not be loaded.
        </p>
      )}
      <div className="overflow-x-auto rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Time</TableHead>
              <TableHead>Action</TableHead>
              <TableHead>Resource</TableHead>
              <TableHead>Actor</TableHead>
              <TableHead>Metadata</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {auditEvents.data?.items.map((item) => (
              <TableRow key={item.id}>
                <TableCell>
                  {new Date(item.occurredAt).toLocaleString()}
                </TableCell>
                <TableCell className="font-mono text-xs">
                  {item.action}
                </TableCell>
                <TableCell>
                  {item.resourceType}:{item.resourceId}
                </TableCell>
                <TableCell>{item.actorUserId ?? "system"}</TableCell>
                <TableCell className="font-mono text-xs">
                  {JSON.stringify(item.metadata)}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </section>
  )
}
```

Login and registration return to `takeReturnTo()` once `me` is set:

Replace the contents of `apps/web/src/views/login-view.tsx` with:

```tsx
"use client"

import Link from "next/link"
import { useRouter } from "next/navigation"
import { useEffect, useState } from "react"
import type { FormEvent } from "react"

import { api } from "@/api/client"
import { useAuth } from "@/auth/auth-context"
import { takeReturnTo } from "@/auth/return-to"
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

export function LoginView() {
  const [email, setEmail] = useState("")
  const [password, setPassword] = useState("")
  const [error, setError] = useState("")
  const [submitting, setSubmitting] = useState(false)
  const { me, refresh } = useAuth()
  const router = useRouter()

  useEffect(() => {
    if (me) router.replace(takeReturnTo())
  }, [me, router])

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
          <CardTitle>
            <h1>Sign in to Corqty</h1>
          </CardTitle>
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
            {error && (
              <p role="alert" className="text-sm text-destructive">
                {error}
              </p>
            )}
            <Button type="submit" disabled={submitting}>
              {submitting ? "Signing in…" : "Sign in"}
            </Button>
            <p className="text-center text-sm text-muted-foreground">
              New to Corqty?{" "}
              <Link className="text-foreground underline" href="/register">
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

Replace the contents of `apps/web/src/views/register-view.tsx` with:

```tsx
"use client"

import Link from "next/link"
import { useRouter } from "next/navigation"
import { useEffect, useState } from "react"
import type { FormEvent } from "react"

import { api } from "@/api/client"
import { useAuth } from "@/auth/auth-context"
import { takeReturnTo } from "@/auth/return-to"
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

export function RegisterView() {
  const [email, setEmail] = useState("")
  const [password, setPassword] = useState("")
  const [organizationName, setOrganizationName] = useState("")
  const [error, setError] = useState("")
  const [submitting, setSubmitting] = useState(false)
  const { me, refresh } = useAuth()
  const router = useRouter()

  useEffect(() => {
    if (me) router.replace(takeReturnTo())
  }, [me, router])

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
          <CardTitle>
            <h1>Create your workspace</h1>
          </CardTitle>
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
            {error && (
              <p role="alert" className="text-sm text-destructive">
                {error}
              </p>
            )}
            <Button type="submit" disabled={submitting}>
              {submitting ? "Creating…" : "Create account"}
            </Button>
            <p className="text-center text-sm text-muted-foreground">
              Already registered?{" "}
              <Link className="text-foreground underline" href="/login">
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

In `apps/web/src/views/members-view.tsx`, apply these replacements in order.

Replace:

```tsx
import { useState } from "react"
```

With:

```tsx
"use client"

import { useState } from "react"
```

Replace:

```tsx
export function MembersPage({ organizationId }: { organizationId: string }) {
```

With:

```tsx
export function MembersView({ organizationId }: { organizationId: string }) {
```

`AppShell` takes the organization id and `children` from the layout, and uses `Link`, `useRouter` and
`usePathname` in place of React Router's `NavLink`, `useNavigate` and `useParams`. The active link keeps
`aria-current="page"`. Logging out only refreshes `me`: once it is null, the auth gate sends the browser to
`/login`.

Replace the contents of `apps/web/src/layout/app-shell.tsx` with:

```tsx
"use client"

import Link from "next/link"
import { usePathname, useRouter } from "next/navigation"
import type { ReactNode } from "react"

import { api } from "@/api/client"
import { useAuth } from "@/auth/auth-context"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Separator } from "@/components/ui/separator"

const links = [
  { label: "Overview", suffix: "" },
  { label: "Members", suffix: "/members" },
  { label: "Audit log", suffix: "/audit" },
]

export function AppShell({
  organizationId,
  children,
}: {
  organizationId: string
  children: ReactNode
}) {
  const { me, refresh } = useAuth()
  const router = useRouter()
  const pathname = usePathname()
  const membership = me?.organizations.find(
    (organization) => organization.organizationId === organizationId,
  )
  if (!me || !membership) {
    return <p className="p-6">Organization not found.</p>
  }

  async function logout() {
    await api("/auth/logout", { method: "POST" })
    // With `me` cleared, the auth gate sends the browser to /login.
    await refresh()
  }

  return (
    <div className="min-h-screen bg-background md:grid md:grid-cols-[220px_1fr]">
      <a
        href="#main-content"
        className="sr-only z-50 rounded-md bg-background px-3 py-2 text-sm shadow focus:not-sr-only focus:fixed focus:top-3 focus:left-3"
      >
        Skip to content
      </a>
      <aside className="flex flex-wrap items-center border-b bg-sidebar p-3 text-sidebar-foreground md:min-h-screen md:flex-nowrap md:flex-col md:items-stretch md:border-r md:border-b-0">
        <div className="flex items-center gap-2 px-2 py-3">
          <span className="grid size-7 place-items-center rounded-md bg-foreground text-xs font-bold text-background">
            C
          </span>
          <span className="font-semibold tracking-tight">Corqty</span>
        </div>
        <select
          aria-label="Organization"
          className="ml-auto h-9 max-w-48 rounded-md border bg-background px-2 text-sm md:ml-0 md:max-w-none"
          value={membership.organizationId}
          onChange={(event) => router.push(`/orgs/${event.target.value}`)}
        >
          {me.organizations.map((organization) => (
            <option
              key={organization.organizationId}
              value={organization.organizationId}
            >
              {organization.name}
            </option>
          ))}
        </select>
        <Separator className="mx-3 hidden md:my-3 md:block" />
        <nav
          aria-label="Organization"
          className="order-3 mt-3 flex basis-full gap-1 overflow-x-auto md:order-none md:mt-0 md:flex-col"
        >
          {links.map((link) => {
            const href = `/orgs/${membership.organizationId}${link.suffix}`
            const active = link.suffix
              ? pathname.startsWith(href)
              : pathname === href
            return (
              <Link
                key={link.label}
                href={href}
                aria-current={active ? "page" : undefined}
                className={`rounded-md px-3 py-2 text-sm ${
                  active
                    ? "bg-sidebar-accent font-medium"
                    : "text-muted-foreground"
                }`}
              >
                {link.label}
              </Link>
            )
          })}
        </nav>
        <div className="ml-2 flex items-center gap-2 md:mt-auto md:ml-0 md:flex-col md:items-stretch md:gap-2 md:border-t md:pt-3">
          <span className="hidden truncate px-2 text-xs text-muted-foreground md:block">
            {me.user.email}
          </span>
          <div className="flex items-center justify-between gap-2 px-2">
            <Badge variant="secondary">{membership.role}</Badge>
            <Button variant="ghost" size="sm" onClick={() => void logout()}>
              Log out
            </Button>
          </div>
        </div>
      </aside>
      <main id="main-content" className="min-w-0">
        {children}
      </main>
    </div>
  )
}
```

- [ ] **Step 9: Verify GREEN**

Run:

```text
pnpm web:test
```

Expected: `Test Files  4 passed (4)` and `Tests  12 passed (12)` with no unhandled errors. On Node 25 or newer,
each worker may print `ExperimentalWarning: localStorage is not available because --localstorage-file was not
provided.`; this is harmless.

- [ ] **Step 10: Move Compose, environment, docs and API defaults to port 3000**

In `.gitignore`:

Replace:

```text
node_modules/
dist/
```

With:

```text
node_modules/
dist/
.next/
next-env.d.ts
```

In `.dockerignore`:

Replace:

```text
**/node_modules
**/dist
```

With:

```text
**/node_modules
**/dist
**/.next
```

In `docker-compose.yml`, apply these replacements in order. The web service keeps running
`pnpm --filter @corqty/web dev` (now `next dev -H 0.0.0.0 -p 3000`) from the unchanged `apps/web/Dockerfile`.
Its healthcheck waits for the first `/login` compile.

Replace:

```yaml
      CORQTY_PUBLIC_WEB_URL: ${CORQTY_PUBLIC_WEB_URL:-http://localhost:5173}
```

With:

```yaml
      CORQTY_PUBLIC_WEB_URL: ${CORQTY_PUBLIC_WEB_URL:-http://localhost:3000}
```

Replace:

```yaml
    environment:
      VITE_API_PROXY: ${VITE_API_PROXY:-http://api:8080}
    ports:
      - "${CORQTY_WEB_PORT:-5173}:5173"
    depends_on:
      api:
        condition: service_healthy
```

With:

```yaml
    environment:
      CORQTY_API_ORIGIN: ${CORQTY_API_ORIGIN:-http://api:8080}
    ports:
      - "${CORQTY_WEB_PORT:-3000}:3000"
    depends_on:
      api:
        condition: service_healthy
    healthcheck:
      test: ["CMD", "wget", "-q", "-O", "/dev/null", "http://127.0.0.1:3000/login"]
      interval: 2s
      timeout: 5s
      retries: 60
```

In `.env.example`, apply these replacements in order:

Replace:

```text
CORQTY_WEB_PORT=5173
```

With:

```text
CORQTY_WEB_PORT=3000
```

Replace:

```text
CORQTY_PUBLIC_WEB_URL=http://localhost:5173
```

With:

```text
CORQTY_PUBLIC_WEB_URL=http://localhost:3000
```

Replace:

```text
VITE_API_PROXY=http://api:8080
```

With:

```text
CORQTY_API_ORIGIN=http://api:8080
```

In `crates/api/src/config.rs`, apply these replacements in order:

Replace:

```rust
            public_web_url: std::env::var("CORQTY_PUBLIC_WEB_URL")
                .unwrap_or_else(|_| "http://localhost:5173".to_owned()),
```

With:

```rust
            public_web_url: std::env::var("CORQTY_PUBLIC_WEB_URL")
                .unwrap_or_else(|_| "http://localhost:3000".to_owned()),
```

Replace:

```rust
            public_web_url: "http://localhost:5173".to_owned(),
```

With:

```rust
            public_web_url: "http://localhost:3000".to_owned(),
```

In `README.md`:

Replace:

```markdown
Open http://localhost:5173 and create the first account and organization.
```

With:

```markdown
Open http://localhost:3000 and create the first account and organization.
```

In `scripts/smoke-foundation.ps1`:

Replace:

```powershell
    [string]$WebBase = "http://localhost:5173"
```

With:

```powershell
    [string]$WebBase = "http://localhost:3000"
```

- [ ] **Step 11: Run lint, build and the Rust checks**

Run:

```text
pnpm web:lint
```

Expected: `next typegen` prints `Generating route types...` and `✓ Types generated successfully`. After that,
`tsc --noEmit` and `oxlint` report no diagnostics (in a terminal, oxlint prints `Found 0 warnings and 0 errors.`).
The command exits 0, and `apps/web/tsconfig.json` is unchanged.

Run:

```text
pnpm web:build
```

Expected: `▲ Next.js 16.4.3 (Turbopack)`, `✓ Compiled successfully`, `Finished TypeScript`, and a route table
with `○ /`, `○ /_not-found`, `○ /invitations/accept`, `○ /login`, `○ /register`, `ƒ /orgs/[organizationId]`,
`ƒ /orgs/[organizationId]/audit` and `ƒ /orgs/[organizationId]/members`. It exits 0.

Run:

```text
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: both exit 0.

- [ ] **Step 12: Commit**

```text
git add apps/web pnpm-lock.yaml .gitignore .dockerignore .env.example README.md docker-compose.yml crates/api/src/config.rs scripts/smoke-foundation.ps1
git commit -m "feat: move the web app to Next.js App Router"
```

## Task 8: Model the workflow graph and connection rules in the domain

**Files:**

- Create: `crates/domain/src/graph.rs`
- Create: `crates/domain/src/slug.rs`
- Create: `crates/domain/tests/fixtures/graph-rules.json`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/domain/Cargo.toml`
- Modify: `crates/domain/src/lib.rs`
- Modify: `crates/domain/src/diagnostic.rs`
- Modify: `crates/api/src/auth/service.rs`
- Modify: `crates/api/src/organizations/service.rs`
- Test: `crates/domain/tests/graph_rules.rs`
- Test: `crates/domain/tests/account_and_rbac.rs`
- Test: `crates/api/tests/organizations_http.rs`

Drafts, blueprints and published versions all store one graph shape, and the studio canvas has to refuse the same
connections the server refuses. This task puts that shape in `corqty_domain::graph`: the `GraphSnapshot` types
(camelCase JSON that rejects unknown fields), port data-type names, the compatibility rule, cycle detection, a
topological order (Kahn's algorithm, which Milestone 3 needs to find ready nodes) and the fixed size limits. The
compatibility and cycle cases live in `crates/domain/tests/fixtures/graph-rules.json`, which Task 16's Vitest suite
reads as well. `Diagnostic::new` keeps the validators of Tasks 9 and 10 short.

The task also moves slug generation into `corqty_domain::slug::slugify`. Registration already collapsed runs of
separators, but creating a second organization only trimmed them, so "Acme  Cloud" became `acme--cloud-` plus the
suffix. Both paths now call `slugify`, and Task 13 uses it for workflow slugs.

- [ ] **Step 1: Write the failing tests**

The domain tests parse JSON and build UUIDs, so `corqty-domain` first gains the `serde_json` and `uuid` workspace
dependencies. The workspace `uuid` also gains the `v7` feature (`Uuid::now_v7()`). That feature needs only `rng`,
which `v4` already enables, so no new crate is downloaded.

Replace the whole of `Cargo.toml`:

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
uuid = { version = "1", features = ["serde", "v4", "v7"] }

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
module_name_repetitions = "allow"
```

Replace the whole of `crates/domain/Cargo.toml`:

```toml
[package]
name = "corqty-domain"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
uuid.workspace = true

[lints]
workspace = true
```

`Cargo.lock` is regenerated by the next cargo command (Step 2): it adds `serde_json` and `uuid` to the
`corqty-domain` dependency list and changes no versions.

The fixture is shared with the web app, so its bytes matter.

Create `crates/domain/tests/fixtures/graph-rules.json`:

```json
{
  "compatibility": [
    { "source": "core.string", "target": "core.string", "compatible": true },
    { "source": "core.integer", "target": "core.number", "compatible": true },
    { "source": "core.number", "target": "core.integer", "compatible": false },
    { "source": "aws.vpc-id", "target": "core.any", "compatible": true },
    { "source": "core.object", "target": "core.any", "compatible": true },
    { "source": "core.any", "target": "core.string", "compatible": false },
    { "source": "aws.vpc-id", "target": "core.string", "compatible": false },
    { "source": "aws.vpc-id", "target": "aws.vpc-id", "compatible": true },
    { "source": "core.boolean", "target": "core.string", "compatible": false }
  ],
  "cycles": [
    { "name": "self edge", "edges": [["a", "a"]], "hasCycle": true },
    { "name": "two node loop", "edges": [["a", "b"], ["b", "a"]], "hasCycle": true },
    { "name": "three node loop", "edges": [["a", "b"], ["b", "c"], ["c", "a"]], "hasCycle": true },
    { "name": "diamond", "edges": [["a", "b"], ["a", "c"], ["b", "d"], ["c", "d"]], "hasCycle": false },
    { "name": "disconnected", "edges": [["a", "b"], ["c", "d"]], "hasCycle": false }
  ]
}
```

`graph_rules.rs` runs every fixture case through `compatible` and `has_cycle`, and also checks that every fixture
type is a valid data-type name. It pins the tie-break order of `topological_order` and checks that a cycle gives
`None`. It round-trips the wire example: positions and the viewport are compared as structs, because `f64` values
serialize as `0.0` where the example has `0`, and the rest of the JSON must come back unchanged. An unknown node field
is rejected. A secret binding serializes its id as `secretId` and `secret_id` is rejected. `GraphSnapshot::empty()`
has a pinned wire form, and the limits accept 500 nodes and 1000 edges and report one more of each.

Create `crates/domain/tests/graph_rules.rs`:

```rust
use corqty_domain::{
    diagnostic::Diagnostic,
    graph::{
        DefinitionRef, GraphEdge, GraphSnapshot, InputBinding, MAX_EDGES, MAX_NODES, PortRef,
        Position, Viewport, check_limits, compatible, has_cycle, is_valid_data_type,
        topological_order,
    },
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

/// Shared with the web app's tests, so both sides agree on these rules.
const RULES: &str = include_str!("fixtures/graph-rules.json");

/// A draft graph as the web app sends it.
const WIRE_EXAMPLE: &str = r#"{"schemaVersion":1,"workflowInputSchema":{"type":"object","properties":{"region":{"type":"string"}},"required":["region"]},
 "nodes":[{"id":"vpc","definition":{"key":"corqty.simulated.create-vpc","version":1},"displayName":"Create VPC (simulated)",
   "position":{"x":0,"y":0},"config":{"mode":"fixed","output":{"vpcId":"vpc-0simulated"},"delayMs":500,"failureMessage":null},
   "inputs":{"region":{"kind":"workflowInput","pointer":"/region"}},"retry":{"maxAttempts":1},"timeoutSeconds":900}],
 "edges":[{"id":"vpc-rds","source":{"nodeId":"vpc","port":"vpcId"},"target":{"nodeId":"rds","port":"vpcId"}}],
 "viewport":{"x":0,"y":0,"zoom":1}}"#;

#[derive(Deserialize)]
struct Rules {
    compatibility: Vec<CompatibilityCase>,
    cycles: Vec<CycleCase>,
}

#[derive(Deserialize)]
struct CompatibilityCase {
    source: String,
    target: String,
    compatible: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CycleCase {
    name: String,
    edges: Vec<(String, String)>,
    has_cycle: bool,
}

fn rules() -> Rules {
    serde_json::from_str(RULES).unwrap()
}

#[test]
fn compatibility_follows_the_shared_fixture() {
    for case in rules().compatibility {
        assert!(is_valid_data_type(&case.source), "{}", case.source);
        assert!(is_valid_data_type(&case.target), "{}", case.target);
        assert_eq!(
            compatible(&case.source, &case.target),
            case.compatible,
            "{} -> {}",
            case.source,
            case.target
        );
    }
}

#[test]
fn cycle_detection_follows_the_shared_fixture() {
    for case in rules().cycles {
        let edges: Vec<(&str, &str)> = case
            .edges
            .iter()
            .map(|(source, target)| (source.as_str(), target.as_str()))
            .collect();
        assert_eq!(has_cycle(&edges), case.has_cycle, "{}", case.name);
    }
}

#[test]
fn data_types_are_dotted_lowercase_names() {
    for valid in ["core.string", "aws.vpc-id", "acme.billing.invoice-2"] {
        assert!(is_valid_data_type(valid), "{valid}");
    }
    for invalid in [
        "string",
        "Core.string",
        "core.",
        ".core",
        "core..any",
        "core.1st",
        "core.any!",
    ] {
        assert!(!is_valid_data_type(invalid), "{invalid}");
    }
}

#[test]
fn topological_order_lists_sources_first_and_refuses_cycles() {
    let diamond = [("a", "b"), ("a", "c"), ("b", "d"), ("c", "d")];
    assert_eq!(
        topological_order(&["d", "c", "b", "a", "e"], &diamond),
        Some(["a", "e", "b", "c", "d"].map(str::to_owned).to_vec())
    );
    assert_eq!(
        topological_order(&["a", "b"], &[("a", "b"), ("b", "a")]),
        None
    );
}

#[test]
fn graph_snapshot_round_trips_the_wire_example() {
    let wire: serde_json::Value = serde_json::from_str(WIRE_EXAMPLE).unwrap();
    let graph: GraphSnapshot = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        graph.nodes[0].definition,
        DefinitionRef {
            key: "corqty.simulated.create-vpc".to_owned(),
            version: 1,
        }
    );
    assert_eq!(
        graph.nodes[0].inputs["region"],
        InputBinding::WorkflowInput {
            pointer: "/region".to_owned(),
        }
    );
    assert_eq!(
        graph.edges,
        [GraphEdge {
            id: "vpc-rds".to_owned(),
            source: PortRef {
                node_id: "vpc".to_owned(),
                port: "vpcId".to_owned(),
            },
            target: PortRef {
                node_id: "rds".to_owned(),
                port: "vpcId".to_owned(),
            },
        }]
    );
    assert_eq!(graph.nodes[0].position, Position { x: 0.0, y: 0.0 });
    assert_eq!(
        graph.viewport,
        Viewport {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
        }
    );

    // Floats serialize as 0.0 rather than 0, so those two fields were compared above.
    let mut written = serde_json::to_value(&graph).unwrap();
    let mut expected = wire.clone();
    for json in [&mut written, &mut expected] {
        json["nodes"][0]["position"].take();
        json["viewport"].take();
    }
    assert_eq!(written, expected);

    let mut unknown_field = wire;
    unknown_field["nodes"][0]["color"] = json!("red");
    assert!(serde_json::from_value::<GraphSnapshot>(unknown_field).is_err());
}

#[test]
fn secret_bindings_serialize_their_id_as_secret_id() {
    let binding = InputBinding::Secret {
        secret_id: Uuid::from_u128(7),
    };
    let wire = json!({"kind": "secret", "secretId": "00000000-0000-0000-0000-000000000007"});
    assert_eq!(serde_json::to_value(&binding).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<InputBinding>(wire).unwrap(),
        binding
    );
    assert!(
        serde_json::from_value::<InputBinding>(json!({
            "kind": "secret",
            "secret_id": "00000000-0000-0000-0000-000000000007",
        }))
        .is_err()
    );
}

#[test]
fn empty_graph_has_an_object_input_schema_and_a_unit_viewport() {
    assert_eq!(
        serde_json::to_value(GraphSnapshot::empty()).unwrap(),
        json!({
            "schemaVersion": 1,
            "workflowInputSchema": {"type": "object", "properties": {}},
            "nodes": [],
            "edges": [],
            "viewport": {"x": 0.0, "y": 0.0, "zoom": 1.0},
        })
    );
}

#[test]
fn limits_allow_500_nodes_and_1000_edges() {
    let example: GraphSnapshot = serde_json::from_str(WIRE_EXAMPLE).unwrap();
    let mut graph = GraphSnapshot::empty();
    graph.nodes = vec![example.nodes[0].clone(); MAX_NODES];
    graph.edges = vec![example.edges[0].clone(); MAX_EDGES];
    assert!(check_limits(&graph).is_empty());

    graph.nodes.push(example.nodes[0].clone());
    graph.edges.push(example.edges[0].clone());
    assert_eq!(
        check_limits(&graph),
        [
            Diagnostic::new(
                "too_many_nodes",
                "/nodes",
                "A graph can have at most 500 nodes."
            ),
            Diagnostic::new(
                "too_many_edges",
                "/edges",
                "A graph can have at most 1000 edges."
            ),
        ]
    );
}
```

The slug test covers the doubled-space case, non-ASCII letters (they separate words like punctuation does) and the
fallback when no letter or digit is left.

Replace the whole of `crates/domain/tests/account_and_rbac.rs`:

```rust
use corqty_domain::{
    account::{NormalizedEmail, OrganizationName, validate_password},
    rbac::{Capability, Role},
    slug::slugify,
};
use uuid::Uuid;

#[test]
fn normalizes_email_and_organization_name() {
    let email = NormalizedEmail::parse("  Admin@Example.COM ").unwrap();
    let name = OrganizationName::parse("  Acme Cloud  ").unwrap();

    assert_eq!(email.as_str(), "admin@example.com");
    assert_eq!(name.as_str(), "Acme Cloud");
}

#[test]
fn slugs_join_ascii_word_runs_and_end_with_a_suffix() {
    let suffix_source = Uuid::from_u128(0x0123_4567_89ab_cdef_0000_0000_0000_0000);

    assert_eq!(
        slugify("Acme  Cloud", "org", suffix_source),
        "acme-cloud-01234567"
    );
    assert_eq!(
        slugify("  Café -- Ops!  ", "org", suffix_source),
        "caf-ops-01234567"
    );
    assert_eq!(
        slugify("¿?", "workflow", suffix_source),
        "workflow-01234567"
    );
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
fn roles_list_their_capabilities_in_matrix_order() {
    assert_eq!(Role::Admin.capabilities(), Capability::ALL);
    assert_eq!(
        Role::Editor.capabilities(),
        [
            Capability::ViewOrganization,
            Capability::EditWorkflows,
            Capability::PublishWorkflows,
            Capability::RunWorkflows,
        ]
    );
    assert_eq!(
        Role::Runner.capabilities(),
        [Capability::ViewOrganization, Capability::RunWorkflows]
    );
    assert_eq!(Role::Viewer.capabilities(), [Capability::ViewOrganization]);
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

The organization test now creates "Acme  Cloud" and checks the slug it gets back.

Replace the whole of `crates/api/tests/organizations_http.rs`:

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
                .body(Body::from(r#"{"name":"Acme  Cloud"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let slug = created["slug"].as_str().unwrap();
    // One separator per gap between words, then an 8-character random suffix.
    assert!(slug.starts_with("acme-cloud-"), "{slug}");
    assert_eq!(slug.len(), "acme-cloud-".len() + 8, "{slug}");

    let me = fixture
        .app
        .clone()
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

- [ ] **Step 2: Run the tests and verify RED**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-domain --test graph_rules
```

Expected: compilation fails with ``error[E0432]: unresolved import `corqty_domain::graph` `` and, twice, with
``error[E0599]: no associated function or constant named `new` found for struct `Diagnostic` in the current scope``
(3 errors).

```text
cargo test -p corqty-domain --test account_and_rbac
```

Expected: compilation fails with ``error[E0432]: unresolved import `corqty_domain::slug` `` (1 error).

```text
cargo test -p corqty-api --test organizations_http
```

Expected: `authenticated_user_can_create_a_second_organization` panics at line 33 of `organizations_http.rs` and
prints the slug with a doubled hyphen, for example `acme--cloud-13cfec22` (the 8-character suffix is random). The
result line is `test result: FAILED. 0 passed; 1 failed`.

- [ ] **Step 3: Add the graph model and `slugify` to the domain**

`Diagnostic::new` takes the code as `&str` and the pointer and message as anything that converts into a `String`, so
callers can pass literals or `format!` output.

Replace the whole of `crates/domain/src/diagnostic.rs`:

```rust
use serde::{Deserialize, Serialize};

/// One problem in a submitted document, located by an RFC 6901 JSON Pointer
/// into that document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub pointer: String,
    pub message: String,
}

impl Diagnostic {
    pub fn new(code: &str, pointer: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            pointer: pointer.into(),
            message: message.into(),
        }
    }
}
```

Every graph struct derives `PartialEq` but not `Eq`, because positions and the viewport are floats. `InputBinding`
is tagged by `kind`, and `rename_all_fields` gives its variant fields camelCase names (`secretId`). `retry.maxAttempts`
and `timeoutSeconds` are plain `u32` so that out-of-range values such as 0 reach the publish validator in Task 10
instead of failing to parse. `is_valid_data_type` checks `^[a-z][a-z0-9-]*(\.[a-z][a-z0-9-]*)+$` without a regex crate.
`topological_order` counts every edge endpoint as a node, so `has_cycle` passes no node ids. Ready nodes are taken
first-in, first-out, which keeps the order deterministic and runs in O(nodes + edges).

Create `crates/domain/src/graph.rs`:

```rust
use std::collections::{BTreeMap, HashMap, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::diagnostic::Diagnostic;

pub const GRAPH_SCHEMA_VERSION: u32 = 1;
// ponytail: fixed limits in M2; deployment configuration arrives in Milestone 5.
pub const MAX_NODES: usize = 500;
pub const MAX_EDGES: usize = 1000;
pub const DEFAULT_TIMEOUT_SECONDS: u32 = 900;
pub const MAX_TIMEOUT_SECONDS: u32 = 14_400;
pub const MAX_ATTEMPTS: u32 = 5;

/// A workflow graph as drafts, blueprints and published versions store it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphSnapshot {
    pub schema_version: u32,
    pub workflow_input_schema: Value,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub viewport: Viewport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphNode {
    pub id: String,
    pub definition: DefinitionRef,
    pub display_name: String,
    pub position: Position,
    pub config: Value,
    pub inputs: BTreeMap<String, InputBinding>,
    pub retry: RetryPolicy,
    pub timeout_seconds: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefinitionRef {
    pub key: String,
    pub version: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Viewport {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetryPolicy {
    /// `u32` rather than a bounded type so out-of-range values reach the validator.
    pub max_attempts: u32,
}

/// Where a node input gets its value when no edge feeds it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum InputBinding {
    Literal { value: Value },
    WorkflowInput { pointer: String },
    Secret { secret_id: Uuid },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphEdge {
    pub id: String,
    pub source: PortRef,
    pub target: PortRef,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortRef {
    pub node_id: String,
    pub port: String,
}

impl GraphSnapshot {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            schema_version: GRAPH_SCHEMA_VERSION,
            workflow_input_schema: json!({"type": "object", "properties": {}}),
            nodes: Vec::new(),
            edges: Vec::new(),
            viewport: Viewport {
                x: 0.0,
                y: 0.0,
                zoom: 1.0,
            },
        }
    }
}

/// Port data types are dotted lowercase names such as `core.string` or `aws.vpc-id`
/// (`^[a-z][a-z0-9-]*(\.[a-z][a-z0-9-]*)+$`).
#[must_use]
pub fn is_valid_data_type(data_type: &str) -> bool {
    data_type.contains('.')
        && data_type.split('.').all(|segment| {
            segment.starts_with(|character: char| character.is_ascii_lowercase())
                && segment.chars().all(|character| {
                    character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
                })
        })
}

/// Whether an output of type `source` may feed an input of type `target`.
#[must_use]
pub fn compatible(source: &str, target: &str) -> bool {
    source == target
        || (source == "core.integer" && target == "core.number")
        || target == "core.any"
}

/// Whether the `(source, target)` node id pairs contain a cycle, self edges included.
#[must_use]
pub fn has_cycle(edges: &[(&str, &str)]) -> bool {
    topological_order(&[], edges).is_none()
}

/// Kahn's algorithm over `node_ids` and every edge endpoint. Ready nodes are taken in the
/// order they were first listed (`node_ids` first, then edge endpoints). `None` on a cycle.
#[must_use]
pub fn topological_order(node_ids: &[&str], edges: &[(&str, &str)]) -> Option<Vec<String>> {
    let mut nodes = Vec::new();
    let mut incoming: HashMap<&str, usize> = HashMap::new();
    let endpoints = edges.iter().flat_map(|&(source, target)| [source, target]);
    for id in node_ids.iter().copied().chain(endpoints) {
        if incoming.insert(id, 0).is_none() {
            nodes.push(id);
        }
    }
    let mut outgoing: HashMap<&str, Vec<&str>> = HashMap::new();
    for &(source, target) in edges {
        outgoing.entry(source).or_default().push(target);
        *incoming.entry(target).or_default() += 1;
    }

    let mut ready: VecDeque<&str> = nodes
        .iter()
        .copied()
        .filter(|id| incoming[id] == 0)
        .collect();
    let mut order = Vec::with_capacity(nodes.len());
    while let Some(id) = ready.pop_front() {
        order.push(id.to_owned());
        for &target in outgoing.get(id).into_iter().flatten() {
            let count = incoming.entry(target).or_default();
            *count -= 1;
            if *count == 0 {
                ready.push_back(target);
            }
        }
    }
    (order.len() == nodes.len()).then_some(order)
}

/// The size limits every stored graph must respect, checked before anything else.
#[must_use]
pub fn check_limits(graph: &GraphSnapshot) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if graph.nodes.len() > MAX_NODES {
        diagnostics.push(Diagnostic::new(
            "too_many_nodes",
            "/nodes",
            format!("A graph can have at most {MAX_NODES} nodes."),
        ));
    }
    if graph.edges.len() > MAX_EDGES {
        diagnostics.push(Diagnostic::new(
            "too_many_edges",
            "/edges",
            format!("A graph can have at most {MAX_EDGES} edges."),
        ));
    }
    diagnostics
}
```

`slugify` splits on every character that is not an ASCII letter or digit, so runs of separators collapse to one
hyphen and leading or trailing separators disappear.

Create `crates/domain/src/slug.rs`:

```rust
use uuid::Uuid;

/// Lowercase ASCII letter and digit runs of `name` joined by single hyphens (or
/// `fallback` when there are none), then `-` and the first 8 hex digits of `suffix_source`.
#[must_use]
pub fn slugify(name: &str, fallback: &str, suffix_source: Uuid) -> String {
    let base = name
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|run| !run.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .to_ascii_lowercase();
    let base = if base.is_empty() { fallback } else { &base };
    format!("{base}-{}", &suffix_source.simple().to_string()[..8])
}
```

Replace the whole of `crates/domain/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod account;
pub mod diagnostic;
pub mod error;
pub mod graph;
pub mod rbac;
pub mod slug;
```

- [ ] **Step 4: Generate organization slugs with `slugify`**

Both callers pass the fallback `org` and the new organization's id, which is a fresh `Uuid::new_v4()`, so the slug
suffix still matches the start of the id.

In `crates/api/src/auth/service.rs`, apply these replacements in order:

Replace:

```rust
    rbac::{Capability, Role},
};
```

With:

```rust
    rbac::{Capability, Role},
    slug::slugify,
};
```

Replace:

```rust
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
        if slug_prefix.is_empty() {
            "org"
        } else {
            &slug_prefix
        },
        &organization_id.simple().to_string()[..8]
    );
```

With:

```rust
    let organization_slug = slugify(organization_name.as_str(), "org", organization_id);
```

Replace the whole of `crates/api/src/organizations/service.rs`:

```rust
use corqty_domain::{account::OrganizationName, slug::slugify};
use corqty_persistence::models::{OrganizationMembershipRecord, SessionRecord};
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
    let slug = slugify(name.as_str(), "org", organization_id);
    Ok(state
        .store
        .create_organization(session.user_id, organization_id, name.as_str(), &slug)
        .await?)
}
```

- [ ] **Step 5: Verify GREEN**

```text
cargo test -p corqty-domain
```

Expected: `account_and_rbac` 6 tests and `graph_rules` 8 tests pass.

```text
cargo test -p corqty-api --test organizations_http --test auth_http
```

Expected: `auth_http` 3 tests and `organizations_http` 1 test pass.

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

Expected: every suite passes. corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 2,
`members_http` 3, `organizations_http` 1, `problems_http` 2. corqty-domain: `account_and_rbac` 6, `graph_rules` 8.
corqty-persistence: `auth_store` 2, `catalog_guards` 3, `final_admin` 2, `migrations` 1, `tenant_isolation` 1,
`test_reset` 1. 40 tests in total.

- [ ] **Step 7: Commit**

```text
git add Cargo.toml Cargo.lock crates/domain crates/api/src/auth/service.rs crates/api/src/organizations/service.rs crates/api/tests/organizations_http.rs
git commit -m "feat: model the workflow graph and connection rules in the domain"
```

Expected: `git show --stat HEAD` lists exactly the 13 files above (503 insertions, 47 deletions).

## Task 9: Validate node definitions, JSON Schemas, and widgets

**Files:**

- Create: `crates/domain/src/schema.rs`
- Create: `crates/domain/src/catalog.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/domain/Cargo.toml`
- Modify: `crates/domain/src/lib.rs`
- Modify: `crates/domain/src/diagnostic.rs`
- Test: `crates/domain/tests/catalog_rules.rs`

A node definition is data that an editor posts: typed input and output ports, each with a JSON Schema, a
`configSchema` with a `uiSchema` that picks one of ten fixed widgets per field, default config values and an
executor. This task adds the rules that decide whether such a definition is valid, as plain functions that return
`Diagnostic`s with JSON Pointers into the request body. Task 11 runs them over the system seeds, Task 12 over
`POST /node-definitions` and `POST /blueprints`, and Task 10 reuses `validate_config` and the schema helpers for
every node in a published graph.

`corqty_domain::schema` wraps the `jsonschema` crate. Schemas always compile as Draft 2020-12, a `$schema` naming
any other dialect is rejected, references to other documents are never fetched (`offline()`), and `pattern` uses
the linear-time `regex` engine so a user pattern cannot backtrack for ever. Instance errors are masked: the message
says "value" instead of echoing what was submitted, which matters for config values. `compile_at` turns a schema
that does not compile into `invalid_schema` diagnostics: one per meta-schema error at its exact pointer (for
example `/inputs/0/schema/type`), otherwise the compile error at the schema's own pointer.

`corqty_domain::catalog` holds the definition types and validators. Names, categories and descriptions must be
non-blank and fit their database columns, keys follow `^[a-z0-9]+([.-][a-z0-9]+)*$` and must not start with
`corqty.`, and the only executor is the built-in `mock` 1.0.0. In a config, a `fileText` value is capped at
262,144 bytes, and a `secretReference` value must be `{"kind":"secret","secretId":"<uuid>"}`. Milestone 2 has no
secrets, so every well-formed reference reports `secret_not_found`. `pointer_token` escapes `~` and `/` in field
names before they go into a pointer.

- [ ] **Step 1: Write the failing tests**

The tests need the `jsonschema` crate, so add it before the RED run. Default features stay off. They would add an
HTTP resolver (blocking reqwest plus rustls and aws-lc-rs, which needs cmake and NASM on Windows) and a resolver
that reads `file://` references from local disk. User schemas must never reach either one.

Replace the whole of `Cargo.toml`:

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
jsonschema = { version = "0.60.1", default-features = false }
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
uuid = { version = "1", features = ["serde", "v4", "v7"] }

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
module_name_repetitions = "allow"
```

Replace the whole of `crates/domain/Cargo.toml`:

```toml
[package]
name = "corqty-domain"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
jsonschema.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
uuid.workspace = true

[lints]
workspace = true
```

The next cargo command (Step 2) regenerates `Cargo.lock`. It adds 30 packages, among them `jsonschema` 0.60.1,
`referencing` 0.60.1, `jsonschema-value` 0.60.1, `fancy-regex` 0.19.2 and `regex` 1.13.1, and it adds `jsonschema`
to the `corqty-domain` dependency list. Because `foldhash` 0.2.0 now sits next to 0.1.5, the `hashbrown` 0.15.5
entry spells its dependency as `foldhash 0.1.5`. No existing version changes. `jsonschema` turns on serde_json's
`float_roundtrip` feature but not `preserve_order`, so `serde_json::Value` still serializes with sorted keys, which
Task 10's checksum relies on.

The table test starts from one valid definition and changes one value per row, and each row must produce exactly
one diagnostic. The config test checks that diagnostics never repeat submitted values.

Create `crates/domain/tests/catalog_rules.rs`:

```rust
use corqty_domain::{
    catalog::{
        FILE_TEXT_MAX_BYTES, NewNodeDefinition, validate_blueprint_metadata, validate_config,
        validate_new_definition,
    },
    diagnostic::Diagnostic,
    schema::{DIALECT, compile, instance_errors, schema_errors},
};
use serde_json::{Value, json};

/// An organization definition that passes every rule, with a config field for each widget
/// that has extra rules.
fn definition() -> Value {
    json!({
        "key": "acme.create-vpc",
        "displayName": "Create VPC",
        "category": "Infrastructure",
        "inputs": [{
            "name": "region",
            "dataType": "core.string",
            "schema": {"type": "string", "minLength": 1},
            "required": true
        }],
        "outputs": [{
            "name": "vpcId",
            "dataType": "aws.vpc-id",
            "schema": {"type": "string", "pattern": "^vpc-"}
        }],
        "configSchema": {
            "type": "object",
            "properties": {
                "mode": {"type": "string", "enum": ["fixed", "echo"]},
                "delayMs": {"type": "integer", "minimum": 0},
                "script": {"type": "string"},
                "token": {"type": ["object", "null"]}
            },
            "required": ["mode"],
            "additionalProperties": false
        },
        "uiSchema": {
            "mode": {"widget": "select", "label": "Mode"},
            "delayMs": {"widget": "number"},
            "script": {"widget": "fileText", "help": "Uploaded as text."},
            "token": {"widget": "secretReference"}
        },
        "defaults": {"mode": "fixed", "delayMs": 500, "token": null},
        "executor": {"key": "mock", "version": "1.0.0"}
    })
}

/// `definition()` with `value` set at `pointer` (inserted when the parent is an array).
fn with(pointer: &str, value: Value) -> NewNodeDefinition {
    let mut body = definition();
    let (parent, token) = pointer.rsplit_once('/').unwrap();
    match body.pointer_mut(parent).unwrap() {
        Value::Array(items) => items.insert(token.parse().unwrap(), value),
        object => object[token.replace("~1", "/")] = value,
    }
    serde_json::from_value(body).unwrap()
}

fn text(length: usize) -> Value {
    json!("x".repeat(length))
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<(&str, &str)> {
    diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.pointer.as_str()))
        .collect()
}

#[test]
fn a_well_formed_definition_has_no_diagnostics() {
    let definition: NewNodeDefinition = serde_json::from_value(definition()).unwrap();
    assert_eq!(definition.description, "");
    let diagnostics = validate_new_definition(&definition);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    for (pointer, value) in [
        ("/key", json!(format!("acme.{}", "x".repeat(95)))),
        ("/displayName", text(100)),
        ("/description", text(1000)),
        ("/category", text(60)),
    ] {
        let diagnostics = validate_new_definition(&with(pointer, value));
        assert!(diagnostics.is_empty(), "{pointer}: {diagnostics:?}");
    }
}

#[test]
fn each_definition_rule_reports_its_code_at_its_pointer() {
    for (pointer, value, code, at) in [
        ("/key", json!("Acme.Thing"), "invalid_key", "/key"),
        ("/key", json!("acme..thing"), "invalid_key", "/key"),
        (
            "/key",
            json!(format!("acme.{}", "x".repeat(96))),
            "invalid_key",
            "/key",
        ),
        ("/key", json!("corqty.custom"), "reserved_key", "/key"),
        (
            "/displayName",
            json!("   "),
            "invalid_display_name",
            "/displayName",
        ),
        (
            "/displayName",
            text(101),
            "invalid_display_name",
            "/displayName",
        ),
        (
            "/description",
            text(1001),
            "invalid_description",
            "/description",
        ),
        ("/category", json!(""), "invalid_category", "/category"),
        ("/category", text(61), "invalid_category", "/category"),
        (
            "/inputs/0/name",
            json!("1region"),
            "invalid_port_name",
            "/inputs/0/name",
        ),
        (
            "/outputs/0/name",
            json!("vpc-id"),
            "invalid_port_name",
            "/outputs/0/name",
        ),
        (
            "/inputs/1",
            json!({"name": "region", "dataType": "core.any", "schema": {}}),
            "duplicate_port",
            "/inputs/1/name",
        ),
        (
            "/outputs/0/dataType",
            json!("VpcId"),
            "invalid_data_type",
            "/outputs/0/dataType",
        ),
        (
            "/inputs/0/schema/type",
            json!("strin"),
            "invalid_schema",
            "/inputs/0/schema/type",
        ),
        (
            "/outputs/0/schema/pattern",
            json!("^(?=vpc-)"),
            "invalid_schema",
            "/outputs/0/schema",
        ),
        (
            "/outputs/0/schema/$ref",
            json!("https://example.com/vpc.json"),
            "invalid_schema",
            "/outputs/0/schema",
        ),
        (
            "/outputs/0/schema/$schema",
            json!("http://json-schema.org/draft-07/schema#"),
            "invalid_schema",
            "/outputs/0/schema",
        ),
        (
            "/configSchema/type",
            json!(["object", "null"]),
            "invalid_schema",
            "/configSchema",
        ),
        (
            "/uiSchema/colour",
            json!({"widget": "text"}),
            "unknown_config_field",
            "/uiSchema/colour",
        ),
        (
            "/uiSchema/a~1b",
            json!({"widget": "text"}),
            "unknown_config_field",
            "/uiSchema/a~1b",
        ),
        (
            "/uiSchema/mode/widget",
            json!("dropdown"),
            "unknown_widget",
            "/uiSchema/mode/widget",
        ),
        (
            "/uiSchema/script/widget",
            json!("secretReference"),
            "invalid_ui_schema",
            "/uiSchema/script",
        ),
        (
            "/defaults/delayMs",
            json!(-1),
            "invalid_defaults",
            "/defaults/delayMs",
        ),
        (
            "/executor/key",
            json!("shell"),
            "unknown_executor",
            "/executor",
        ),
        (
            "/executor/version",
            json!("2.0.0"),
            "unknown_executor",
            "/executor",
        ),
    ] {
        let diagnostics = validate_new_definition(&with(pointer, value));
        assert_eq!(codes(&diagnostics), [(code, at)], "{pointer}");
    }
}

#[test]
fn blueprint_metadata_follows_the_definition_rules() {
    assert!(validate_blueprint_metadata("acme.data-flow", "Data flow", "").is_empty());
    let diagnostics = validate_blueprint_metadata("corqty.flow", " ", &"x".repeat(1001));
    assert_eq!(
        codes(&diagnostics),
        [
            ("reserved_key", "/key"),
            ("invalid_display_name", "/displayName"),
            ("invalid_description", "/description"),
        ]
    );
}

#[test]
fn config_follows_the_schema_and_the_widget_value_contracts() {
    let spec = serde_json::from_value::<NewNodeDefinition>(definition())
        .unwrap()
        .spec;
    let check = |config: Value| validate_config(&spec, &config, "/nodes/0/config");

    let script = "x".repeat(FILE_TEXT_MAX_BYTES);
    let diagnostics = check(json!({"mode": "echo", "script": script, "token": null}));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    let diagnostics =
        check(json!({"mode": "loud", "delayMs": "soon", "token": "hunter2", "extra": 1}));
    let mut found = codes(&diagnostics);
    found.sort_unstable();
    assert_eq!(
        found,
        [
            ("invalid_config", "/nodes/0/config"),
            ("invalid_config", "/nodes/0/config/delayMs"),
            ("invalid_config", "/nodes/0/config/mode"),
            ("invalid_config", "/nodes/0/config/token"),
            ("invalid_secret_reference", "/nodes/0/config/token"),
        ]
    );
    for diagnostic in &diagnostics {
        for submitted in ["loud", "soon", "hunter2"] {
            assert!(!diagnostic.message.contains(submitted), "{diagnostic:?}");
        }
    }

    let script = "x".repeat(FILE_TEXT_MAX_BYTES + 1);
    assert_eq!(
        codes(&check(json!({"mode": "fixed", "script": script}))),
        [("file_text_too_large", "/nodes/0/config/script")]
    );
    let secret = json!({"kind": "secret", "secretId": "00000000-0000-0000-0000-000000000007"});
    assert_eq!(
        codes(&check(json!({"mode": "fixed", "token": secret}))),
        [("secret_not_found", "/nodes/0/config/token/secretId")]
    );
    let malformed = json!({"kind": "secret", "secretId": "not-a-uuid"});
    assert_eq!(
        codes(&check(json!({"mode": "fixed", "token": malformed}))),
        [("invalid_secret_reference", "/nodes/0/config/token")]
    );
}

#[test]
fn schemas_compile_offline_as_draft_2020_12_only() {
    let local_ref =
        json!({"$schema": DIALECT, "$defs": {"id": {"type": "string"}}, "$ref": "#/$defs/id"});
    assert!(compile(&local_ref).is_ok());
    assert!(compile(&json!({"$schema": "http://json-schema.org/draft-07/schema#"})).is_err());
    assert!(compile(&json!({"$ref": "file:///etc/passwd"})).is_err());

    assert_eq!(
        schema_errors(&json!(5)),
        [(
            String::new(),
            r#"5 is not of types "boolean", "object""#.to_owned()
        )]
    );
    let validator = compile(&json!({"type": "integer"})).unwrap();
    assert_eq!(
        instance_errors(&validator, &json!("hunter2")),
        [(
            String::new(),
            r#"value is not of type "integer""#.to_owned()
        )]
    );
}
```

- [ ] **Step 2: Run the tests and verify RED**

```text
cargo test -p corqty-domain --test catalog_rules
```

Expected: cargo downloads and compiles `jsonschema` and its dependencies, then compilation of the test fails with
``error[E0432]: unresolved imports `corqty_domain::catalog`, `corqty_domain::schema` `` (1 error).

- [ ] **Step 3: Add the schema helpers and pointer escaping**

Replace the whole of `crates/domain/src/diagnostic.rs`:

```rust
use serde::{Deserialize, Serialize};

/// One problem in a submitted document, located by an RFC 6901 JSON Pointer
/// into that document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub pointer: String,
    pub message: String,
}

impl Diagnostic {
    pub fn new(code: &str, pointer: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            pointer: pointer.into(),
            message: message.into(),
        }
    }
}

/// One RFC 6901 reference token: `~` becomes `~0` and `/` becomes `~1`.
#[must_use]
pub fn pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}
```

Create `crates/domain/src/schema.rs`:

```rust
use jsonschema::{PatternOptions, Validator};
use serde_json::Value;

use crate::diagnostic::Diagnostic;

/// The only JSON Schema dialect Corqty accepts.
pub const DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

/// Compiles a user-supplied schema as Draft 2020-12. References to other documents are
/// never fetched, and patterns use the linear-time `regex` engine.
pub fn compile(schema: &Value) -> Result<Validator, String> {
    match schema.get("$schema") {
        None => {}
        Some(dialect) if dialect == DIALECT => {}
        Some(_) => return Err(format!("\"$schema\" must be absent or \"{DIALECT}\".")),
    }
    jsonschema::draft202012::options()
        .offline()
        .with_pattern_options(PatternOptions::regex())
        .build(schema)
        .map_err(|error| error.to_string())
}

/// Every Draft 2020-12 meta-schema error in `schema`, as (pointer into the schema, message).
#[must_use]
pub fn schema_errors(schema: &Value) -> Vec<(String, String)> {
    let mut errors: Vec<(String, String)> = jsonschema::draft202012::meta::validator()
        .iter_errors(schema)
        .map(|error| (error.instance_path().to_string(), error.to_string()))
        .collect();
    // A schema that is neither an object nor a boolean fails every branch the same way.
    errors.dedup();
    errors
}

/// Every error of `instance`, as (pointer into the instance, message). Messages say "value"
/// instead of repeating submitted data.
#[must_use]
pub fn instance_errors(validator: &Validator, instance: &Value) -> Vec<(String, String)> {
    validator
        .iter_errors(instance)
        .map(|error| {
            (
                error.instance_path().to_string(),
                error.masked().to_string(),
            )
        })
        .collect()
}

/// Compiles `schema`, or reports why it does not compile as `invalid_schema` diagnostics:
/// one per meta-schema error under `pointer`, otherwise the compile error at `pointer`.
pub fn compile_at(schema: &Value, pointer: &str) -> Result<Validator, Vec<Diagnostic>> {
    compile(schema).map_err(|message| {
        let errors = schema_errors(schema);
        if errors.is_empty() {
            return vec![Diagnostic::new("invalid_schema", pointer, message)];
        }
        errors
            .into_iter()
            .map(|(path, message)| {
                Diagnostic::new("invalid_schema", format!("{pointer}{path}"), message)
            })
            .collect()
    })
}
```

- [ ] **Step 4: Add the node definition types and validators**

`NewNodeDefinition` flattens `NodeDefinitionSpec` into the request body. serde cannot combine
`deny_unknown_fields` with a flattened field, so unknown top-level fields are ignored, while every nested struct
still rejects unknown fields. Task 11 stores `spec` as JSON with exactly the keys `inputs`, `outputs`,
`configSchema`, `uiSchema`, `defaults` and `executor`.

Create `crates/domain/src/catalog.rs`:

```rust
use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    diagnostic::{Diagnostic, pointer_token},
    graph::{InputBinding, is_valid_data_type},
    schema::{compile, compile_at, instance_errors},
};

/// The form widgets a `uiSchema` field can name. The web app renders each one.
pub const WIDGETS: [&str; 10] = [
    "text",
    "textarea",
    "number",
    "boolean",
    "select",
    "multiSelect",
    "secretReference",
    "code",
    "keyValue",
    "fileText",
];
// ponytail: a static compiled registry; Milestone 3 adds executors that workers advertise.
pub const BUILTIN_EXECUTORS: &[(&str, &str)] = &[("mock", "1.0.0")];
pub const FILE_TEXT_MAX_BYTES: usize = 262_144;

/// What a node definition version does: its typed ports, its configuration form and the
/// executor that runs it. Stored as the `spec` column.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeDefinitionSpec {
    pub inputs: Vec<PortSpec>,
    pub outputs: Vec<PortSpec>,
    pub config_schema: Value,
    pub ui_schema: BTreeMap<String, UiField>,
    pub defaults: Value,
    pub executor: ExecutorRef,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortSpec {
    pub name: String,
    pub data_type: String,
    pub schema: Value,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiField {
    pub widget: String,
    pub label: Option<String>,
    pub help: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutorRef {
    pub key: String,
    pub version: String,
}

/// The body of `POST /node-definitions`. serde cannot deny unknown fields next to a
/// flattened one, so unknown top-level fields are ignored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewNodeDefinition {
    pub key: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    pub category: String,
    #[serde(flatten)]
    pub spec: NodeDefinitionSpec,
}

/// Every problem in a new organization definition, with pointers into the request body.
#[must_use]
pub fn validate_new_definition(definition: &NewNodeDefinition) -> Vec<Diagnostic> {
    let mut diagnostics = validate_blueprint_metadata(
        &definition.key,
        &definition.display_name,
        &definition.description,
    );
    if !fits(&definition.category, 60) {
        diagnostics.push(Diagnostic::new(
            "invalid_category",
            "/category",
            "A category must have 1 to 60 characters.",
        ));
    }
    diagnostics.extend(validate_spec(&definition.spec));
    diagnostics
}

/// The key, display name and description rules that organization definitions and blueprints share.
#[must_use]
pub fn validate_blueprint_metadata(
    key: &str,
    display_name: &str,
    description: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    // `^[a-z0-9]+([.-][a-z0-9]+)*$`, at most 100 characters.
    let valid_key = key.len() <= 100
        && key.split(['.', '-']).all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        });
    if !valid_key {
        diagnostics.push(Diagnostic::new(
            "invalid_key",
            "/key",
            "A key is lowercase letters and digits joined by single dots or hyphens, at most 100 characters.",
        ));
    }
    if key.starts_with("corqty.") {
        diagnostics.push(Diagnostic::new(
            "reserved_key",
            "/key",
            "Keys starting with \"corqty.\" belong to the system catalog.",
        ));
    }
    if !fits(display_name, 100) {
        diagnostics.push(Diagnostic::new(
            "invalid_display_name",
            "/displayName",
            "A display name must have 1 to 100 characters.",
        ));
    }
    if description.chars().count() > 1000 {
        diagnostics.push(Diagnostic::new(
            "invalid_description",
            "/description",
            "A description can have at most 1000 characters.",
        ));
    }
    diagnostics
}

/// Every problem in a definition's ports, schemas, form and executor. Pointers are relative
/// to the definition, whose top level holds the spec fields.
#[must_use]
pub fn validate_spec(spec: &NodeDefinitionSpec) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (side, ports) in [("inputs", &spec.inputs), ("outputs", &spec.outputs)] {
        let mut names = HashSet::new();
        for (index, port) in ports.iter().enumerate() {
            let pointer = format!("/{side}/{index}");
            let valid_name = port
                .name
                .starts_with(|character: char| character.is_ascii_alphabetic())
                && port
                    .name
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_');
            if !valid_name {
                diagnostics.push(Diagnostic::new(
                    "invalid_port_name",
                    format!("{pointer}/name"),
                    "A port name starts with a letter and has only letters, digits and underscores.",
                ));
            }
            if !names.insert(port.name.as_str()) {
                diagnostics.push(Diagnostic::new(
                    "duplicate_port",
                    format!("{pointer}/name"),
                    format!("Port names must be unique among the {side}."),
                ));
            }
            if !is_valid_data_type(&port.data_type) {
                diagnostics.push(Diagnostic::new(
                    "invalid_data_type",
                    format!("{pointer}/dataType"),
                    "A data type is a dotted lowercase name such as core.string.",
                ));
            }
            if let Err(errors) = compile_at(&port.schema, &format!("{pointer}/schema")) {
                diagnostics.extend(errors);
            }
        }
    }

    match compile_at(&spec.config_schema, "/configSchema") {
        Ok(validator) => {
            for (path, message) in instance_errors(&validator, &spec.defaults) {
                let pointer = format!("/defaults{path}");
                diagnostics.push(Diagnostic::new("invalid_defaults", pointer, message));
            }
        }
        Err(errors) => diagnostics.extend(errors),
    }
    if spec.config_schema.get("type") != Some(&json!("object")) {
        diagnostics.push(Diagnostic::new(
            "invalid_schema",
            "/configSchema",
            "The config schema must have \"type\": \"object\".",
        ));
    }

    let properties = spec.config_schema.get("properties");
    for (field, ui) in &spec.ui_schema {
        let pointer = format!("/uiSchema/{}", pointer_token(field));
        if !WIDGETS.contains(&ui.widget.as_str()) {
            diagnostics.push(Diagnostic::new(
                "unknown_widget",
                format!("{pointer}/widget"),
                format!("The widget must be one of: {}.", WIDGETS.join(", ")),
            ));
        }
        let property = properties.and_then(|properties| properties.get(field));
        let kind = property.and_then(|property| property.get("type"));
        if property.is_none() {
            diagnostics.push(Diagnostic::new(
                "unknown_config_field",
                pointer,
                "Every uiSchema field must be a property of the config schema.",
            ));
        } else if ui.widget == "secretReference"
            && kind != Some(&json!("object"))
            && kind != Some(&json!(["object", "null"]))
        {
            diagnostics.push(Diagnostic::new(
                "invalid_ui_schema",
                pointer,
                "A secretReference field must have type \"object\" or [\"object\", \"null\"].",
            ));
        }
    }

    let executor = (spec.executor.key.as_str(), spec.executor.version.as_str());
    if !BUILTIN_EXECUTORS.contains(&executor) {
        diagnostics.push(Diagnostic::new(
            "unknown_executor",
            "/executor",
            "The executor must be mock 1.0.0.",
        ));
    }
    diagnostics
}

/// Every problem in a node's `config`, with pointers under `pointer_prefix`.
#[must_use]
pub fn validate_config(
    spec: &NodeDefinitionSpec,
    config: &Value,
    pointer_prefix: &str,
) -> Vec<Diagnostic> {
    let Ok(validator) = compile(&spec.config_schema) else {
        return vec![Diagnostic::new(
            "invalid_config",
            pointer_prefix,
            "The node definition's config schema does not compile.",
        )];
    };
    let mut diagnostics: Vec<Diagnostic> = instance_errors(&validator, config)
        .into_iter()
        .map(|(path, message)| {
            Diagnostic::new("invalid_config", format!("{pointer_prefix}{path}"), message)
        })
        .collect();
    for (field, ui) in &spec.ui_schema {
        let Some(value) = config.get(field) else {
            continue;
        };
        let pointer = format!("{pointer_prefix}/{}", pointer_token(field));
        match ui.widget.as_str() {
            "fileText"
                if value
                    .as_str()
                    .is_some_and(|text| text.len() > FILE_TEXT_MAX_BYTES) =>
            {
                diagnostics.push(Diagnostic::new(
                    "file_text_too_large",
                    pointer,
                    format!("A file can have at most {FILE_TEXT_MAX_BYTES} bytes."),
                ));
            }
            "secretReference" if !value.is_null() => {
                diagnostics.push(match InputBinding::deserialize(value) {
                    // ponytail: no secrets until M4; replace with an org-scoped lookup then.
                    Ok(InputBinding::Secret { .. }) => Diagnostic::new(
                        "secret_not_found",
                        format!("{pointer}/secretId"),
                        "The secret does not exist.",
                    ),
                    _ => Diagnostic::new(
                        "invalid_secret_reference",
                        pointer,
                        "A secret reference is {\"kind\": \"secret\", \"secretId\": \"<uuid>\"}.",
                    ),
                });
            }
            _ => {}
        }
    }
    diagnostics
}

/// Not blank and at most `max` characters, so the value always fits its database column.
fn fits(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.chars().count() <= max
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
pub mod rbac;
pub mod schema;
pub mod slug;
```

- [ ] **Step 5: Verify GREEN**

```text
cargo test -p corqty-domain
```

Expected: `account_and_rbac` 6 tests, `catalog_rules` 5 tests and `graph_rules` 8 tests pass.

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

Expected: every suite passes. corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 2,
`members_http` 3, `organizations_http` 1, `problems_http` 2. corqty-domain: `account_and_rbac` 6, `catalog_rules` 5,
`graph_rules` 8. corqty-persistence: `auth_store` 2, `catalog_guards` 3, `final_admin` 2, `migrations` 1,
`tenant_isolation` 1, `test_reset` 1. 45 tests in total.

- [ ] **Step 7: Commit**

```text
git add Cargo.toml Cargo.lock crates/domain
git commit -m "feat: validate node definitions, JSON Schemas, and widgets"
```

Expected: `git show --stat HEAD` lists exactly the 8 files above (1008 insertions, 1 deletion).

## Task 10: Validate graphs for publishing and compute canonical checksums

**Files:**

- Create: `crates/domain/src/publish.rs`
- Modify: `Cargo.lock`
- Modify: `crates/domain/Cargo.toml`
- Modify: `crates/domain/src/catalog.rs`
- Modify: `crates/domain/src/lib.rs`
- Test: `crates/domain/tests/publish_rules.rs`

Draft saves accept any graph that deserializes and fits the size limits. Validate and Publish run the full rules,
and this task adds them as one pure function, `corqty_domain::publish::validate_graph`. It takes the graph and a
`Definitions` map, which holds every node definition the organization can see, keyed by `(key, version)`. Task 11
builds that map from the database and Task 12 runs the function over blueprint graphs. Tasks 13 and 14 run it on
Validate and Publish. The result is a list of `Diagnostic`s with RFC 6901 pointers into the graph JSON, sorted by
`(pointer, code)`, and an empty list means the graph can be published.

The rules, with the pointer each one reports at:

- `too_many_nodes` (`/nodes`) and `too_many_edges` (`/edges`) come from `check_limits`. A graph over a limit gets
  only these diagnostics, which keeps the work for the rest of the rules bounded. The `/validate` body can be up to
  2 MiB.
- `unsupported_schema_version` (`/schemaVersion`). `invalid_schema` (`/workflowInputSchema` or a meta-schema
  path below it): the schema must compile through `compile_at` and have `"type": "object"`.
- `duplicate_edge_id` (`/edges/i/id`) and `duplicate_node_id` (`/nodes/i/id`), reported on the second use of an
  id. `invalid_display_name` (`/nodes/i/displayName`) uses the definition rule `fits(_, 100)`, which this task
  makes `pub(crate)` in `catalog.rs`.
- `unknown_definition` (`/nodes/i/definition`): the `(key, version)` is not in the map. A definition from
  another organization is never in it. `unknown_executor` (same pointer): the executor is not in
  `BUILTIN_EXECUTORS`.
- `unknown_node` (`/edges/i/source` or `/edges/i/target`). `unknown_port` (`/edges/i/source` when the port is
  not an output of the source definition, `/edges/i/target` when it is not an input, and `/nodes/i/inputs/<name>`
  for a binding on an input that does not exist).
- `self_edge` (`/edges/i`). `cycle` (`/edges/i`): a single diagnostic, for the first edge in edge order that
  closes a cycle. That edge ends the shortest prefix of the edges that has a cycle, so a binary search over
  `has_cycle` finds it in O((V + E) log E). Self edges are left out of the cycle check because they already have
  their own code.
- `incompatible_ports` (`/edges/i`, judged by `compatible`). `multiply_bound_input`: at `/edges/i` for the second
  edge into a port, and at `/nodes/i/inputs/<name>` when an edge and a binding feed the same port.
- `missing_required_input` (`/nodes/i/inputs/<name>`): a required input with neither an edge nor a binding. An
  optional input can stay unbound.
- `invalid_literal` (`/nodes/i/inputs/<name>/value` plus the instance path): the literal is checked against the
  port schema, so `null` passes only when that schema allows it. The messages are masked.
- `invalid_pointer` (`/nodes/i/inputs/<name>/pointer`): a workflow input pointer must be valid RFC 6901, where
  `~` is only allowed in `~0` and `~1`. Each token must name a property, walking `properties` at every level, so
  `/a/b` means `properties.a.properties.b`. The empty pointer names the whole workflow input.
- `invalid_config`, `file_text_too_large`, `invalid_secret_reference` and `secret_not_found` come from
  `validate_config` with the prefix `/nodes/i/config`. A secret binding reports `secret_not_found` at
  `/nodes/i/inputs/<name>/secretId`, because Milestone 2 has no secrets.
- `invalid_retry` (`/nodes/i/retry/maxAttempts`, 1 to 5) and `invalid_timeout` (`/nodes/i/timeoutSeconds`, 1 to
  14,400).

Nothing cascades. An unknown definition skips that node's config, binding and required-input rules, and its
ports are not checked on edges. An unknown port skips the compatibility and multiple-binding checks that depend
on it. An edge whose other end is unknown still counts as feeding the input it targets, so that input does not
also show up as missing.

`canonical_checksum` returns the lowercase hex SHA-256 of the graph serialized as JSON with sorted keys. Going
through `serde_json::to_value` matters: a `Value` object is a `BTreeMap`, so its keys serialize sorted, while a
struct serializes its fields in declaration order. That holds only while serde_json's `preserve_order` feature
stays off, and a test guards it. The viewport is part of the graph, so it is part of the checksum too. Floats
print the way serde_json prints them (`0.0`, `1.0`), which is why the checksum is computed only on the server.
The empty graph's checksum is pinned to a literal: the SHA-256 of
`{"edges":[],"nodes":[],"schemaVersion":1,"viewport":{"x":0.0,"y":0.0,"zoom":1.0},"workflowInputSchema":{"properties":{},"type":"object"}}`.

- [ ] **Step 1: Write the failing tests**

The tests build a small catalog of two organization definitions and a graph that passes every rule. That graph
reads a nested workflow input, feeds an integer output into a number input, binds a nullable input to `null` and
leaves an optional input unbound. The table test changes one value per row, and each row must produce exactly
one diagnostic at the expected pointer.

Create `crates/domain/tests/publish_rules.rs`:

```rust
use corqty_domain::{
    catalog::FILE_TEXT_MAX_BYTES,
    diagnostic::Diagnostic,
    graph::GraphSnapshot,
    publish::{Definitions, canonical_checksum, validate_graph},
};
use serde_json::{Value, json};

/// A VPC that also reports its subnet count, and a database that needs the VPC id, a size
/// and a nullable label.
fn definitions() -> Definitions {
    let vpc = json!({
        "inputs": [
            {
                "name": "region",
                "dataType": "core.string",
                "schema": {"type": "string", "minLength": 1},
                "required": true
            },
            {"name": "after", "dataType": "core.any", "schema": {}}
        ],
        "outputs": [
            {"name": "vpcId", "dataType": "aws.vpc-id", "schema": {"type": "string", "pattern": "^vpc-"}},
            {"name": "subnets", "dataType": "core.integer", "schema": {"type": "integer"}}
        ],
        "configSchema": {
            "type": "object",
            "properties": {"script": {"type": "string"}, "token": {"type": ["object", "null"]}},
            "additionalProperties": false
        },
        "uiSchema": {"script": {"widget": "fileText"}, "token": {"widget": "secretReference"}},
        "defaults": {},
        "executor": {"key": "mock", "version": "1.0.0"}
    });
    let database = json!({
        "inputs": [
            {
                "name": "vpcId",
                "dataType": "aws.vpc-id",
                "schema": {"type": "string", "pattern": "^vpc-"},
                "required": true
            },
            {"name": "size", "dataType": "core.number", "schema": {"type": "number"}, "required": true},
            {
                "name": "label",
                "dataType": "core.string",
                "schema": {"type": ["string", "null"]},
                "required": true
            },
            {"name": "notes", "dataType": "core.any", "schema": {}}
        ],
        "outputs": [{"name": "endpoint", "dataType": "core.string", "schema": {"type": "string"}}],
        "configSchema": {"type": "object"},
        "uiSchema": {},
        "defaults": {},
        "executor": {"key": "mock", "version": "1.0.0"}
    });
    Definitions::from([
        (
            ("acme.vpc".to_owned(), 1),
            serde_json::from_value(vpc).unwrap(),
        ),
        (
            ("acme.db".to_owned(), 1),
            serde_json::from_value(database).unwrap(),
        ),
    ])
}

/// A publishable graph. It reads a nested workflow input, feeds an integer output into a
/// number input, binds a nullable input to `null` and leaves the optional `notes` unbound.
fn graph() -> Value {
    json!({
        "schemaVersion": 1,
        "workflowInputSchema": {
            "type": "object",
            "properties": {"network": {"type": "object", "properties": {"region": {"type": "string"}}}}
        },
        "nodes": [
            {
                "id": "vpc",
                "definition": {"key": "acme.vpc", "version": 1},
                "displayName": "VPC",
                "position": {"x": 0, "y": 0},
                "config": {"script": "echo", "token": null},
                "inputs": {"region": {"kind": "workflowInput", "pointer": "/network/region"}},
                "retry": {"maxAttempts": 1},
                "timeoutSeconds": 900
            },
            {
                "id": "db",
                "definition": {"key": "acme.db", "version": 1},
                "displayName": "Database",
                "position": {"x": 300, "y": 0},
                "config": {},
                "inputs": {"label": {"kind": "literal", "value": null}},
                "retry": {"maxAttempts": 5},
                "timeoutSeconds": 14400
            }
        ],
        "edges": [edge("vpc-db", "vpc.vpcId", "db.vpcId"), edge("subnets-size", "vpc.subnets", "db.size")],
        "viewport": {"x": 0, "y": 0, "zoom": 1}
    })
}

/// A valid VPC node that takes its region from a literal.
fn node(id: &str) -> Value {
    json!({
        "id": id,
        "definition": {"key": "acme.vpc", "version": 1},
        "displayName": "VPC",
        "position": {"x": 0, "y": 0},
        "config": {},
        "inputs": {"region": {"kind": "literal", "value": "eu-west-1"}},
        "retry": {"maxAttempts": 1},
        "timeoutSeconds": 900
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

/// `graph()` with each value set at its pointer (inserted when the parent is an array).
fn with(changes: &[(&str, Value)]) -> GraphSnapshot {
    let mut body = graph();
    for (pointer, value) in changes {
        let (parent, token) = pointer.rsplit_once('/').unwrap();
        match body.pointer_mut(parent).unwrap() {
            Value::Array(items) => items.insert(token.parse().unwrap(), value.clone()),
            object => object[token.replace("~1", "/")] = value.clone(),
        }
    }
    serde_json::from_value(body).unwrap()
}

fn text(length: usize) -> Value {
    json!("x".repeat(length))
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<(&str, &str)> {
    diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.pointer.as_str()))
        .collect()
}

#[test]
fn a_publishable_graph_has_no_diagnostics() {
    let diagnostics = validate_graph(&with(&[]), &definitions());
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    for (pointer, value) in [
        ("/nodes/0/displayName", text(100)),
        ("/nodes/0/timeoutSeconds", json!(1)),
        (
            "/nodes/1/inputs/notes",
            json!({"kind": "literal", "value": null}),
        ),
    ] {
        let diagnostics = validate_graph(&with(&[(pointer, value)]), &definitions());
        assert!(diagnostics.is_empty(), "{pointer}: {diagnostics:?}");
    }
}

#[test]
fn each_publish_rule_reports_its_code_at_its_pointer() {
    let secret = json!({"kind": "secret", "secretId": "00000000-0000-0000-0000-000000000007"});
    for (pointer, value, code, at) in [
        (
            "/schemaVersion",
            json!(2),
            "unsupported_schema_version",
            "/schemaVersion",
        ),
        (
            "/workflowInputSchema/type",
            json!("array"),
            "invalid_schema",
            "/workflowInputSchema",
        ),
        (
            "/workflowInputSchema/properties/network/type",
            json!("objekt"),
            "invalid_schema",
            "/workflowInputSchema/properties/network/type",
        ),
        ("/nodes/2", node("vpc"), "duplicate_node_id", "/nodes/2/id"),
        (
            "/edges/2",
            edge("vpc-db", "vpc.vpcId", "db.notes"),
            "duplicate_edge_id",
            "/edges/2/id",
        ),
        (
            "/nodes/0/displayName",
            json!(" "),
            "invalid_display_name",
            "/nodes/0/displayName",
        ),
        (
            "/nodes/0/displayName",
            text(101),
            "invalid_display_name",
            "/nodes/0/displayName",
        ),
        (
            "/nodes/1/definition/version",
            json!(2),
            "unknown_definition",
            "/nodes/1/definition",
        ),
        (
            "/edges/0/source/nodeId",
            json!("nat"),
            "unknown_node",
            "/edges/0/source",
        ),
        (
            "/edges/2",
            edge("gateway", "vpc.vpcId", "gateway.vpcId"),
            "unknown_node",
            "/edges/2/target",
        ),
        (
            "/edges/0/source/port",
            json!("region"),
            "unknown_port",
            "/edges/0/source",
        ),
        (
            "/edges/2",
            edge("endpoint", "vpc.vpcId", "db.endpoint"),
            "unknown_port",
            "/edges/2/target",
        ),
        (
            "/nodes/1/inputs/a~1b",
            json!({"kind": "literal", "value": 1}),
            "unknown_port",
            "/nodes/1/inputs/a~1b",
        ),
        (
            "/edges/2",
            edge("loop", "vpc.vpcId", "vpc.after"),
            "self_edge",
            "/edges/2",
        ),
        (
            "/edges/2",
            edge("back", "db.endpoint", "vpc.after"),
            "cycle",
            "/edges/2",
        ),
        (
            "/edges/1/source/port",
            json!("vpcId"),
            "incompatible_ports",
            "/edges/1",
        ),
        (
            "/edges/2",
            edge("again", "vpc.vpcId", "db.vpcId"),
            "multiply_bound_input",
            "/edges/2",
        ),
        (
            "/nodes/1/inputs/vpcId",
            json!({"kind": "literal", "value": "vpc-1"}),
            "multiply_bound_input",
            "/nodes/1/inputs/vpcId",
        ),
        (
            "/nodes/0/inputs",
            json!({}),
            "missing_required_input",
            "/nodes/0/inputs/region",
        ),
        (
            "/nodes/0/inputs/region",
            json!({"kind": "literal", "value": null}),
            "invalid_literal",
            "/nodes/0/inputs/region/value",
        ),
        (
            "/nodes/1/inputs/label/value",
            json!(5),
            "invalid_literal",
            "/nodes/1/inputs/label/value",
        ),
        (
            "/nodes/0/inputs/region/pointer",
            json!("/network/zone"),
            "invalid_pointer",
            "/nodes/0/inputs/region/pointer",
        ),
        (
            "/nodes/0/inputs/region/pointer",
            json!("/region"),
            "invalid_pointer",
            "/nodes/0/inputs/region/pointer",
        ),
        (
            "/nodes/0/inputs/region/pointer",
            json!("network/region"),
            "invalid_pointer",
            "/nodes/0/inputs/region/pointer",
        ),
        (
            "/nodes/0/inputs/region/pointer",
            json!("/network/~2region"),
            "invalid_pointer",
            "/nodes/0/inputs/region/pointer",
        ),
        (
            "/nodes/0/config/extra",
            json!(1),
            "invalid_config",
            "/nodes/0/config",
        ),
        (
            "/nodes/0/config/script",
            text(FILE_TEXT_MAX_BYTES + 1),
            "file_text_too_large",
            "/nodes/0/config/script",
        ),
        (
            "/nodes/0/config/token",
            json!({"kind": "literal", "value": "hunter2"}),
            "invalid_secret_reference",
            "/nodes/0/config/token",
        ),
        (
            "/nodes/0/config/token",
            secret.clone(),
            "secret_not_found",
            "/nodes/0/config/token/secretId",
        ),
        (
            "/nodes/0/inputs/region",
            secret,
            "secret_not_found",
            "/nodes/0/inputs/region/secretId",
        ),
        (
            "/nodes/0/retry/maxAttempts",
            json!(0),
            "invalid_retry",
            "/nodes/0/retry/maxAttempts",
        ),
        (
            "/nodes/0/retry/maxAttempts",
            json!(6),
            "invalid_retry",
            "/nodes/0/retry/maxAttempts",
        ),
        (
            "/nodes/0/timeoutSeconds",
            json!(0),
            "invalid_timeout",
            "/nodes/0/timeoutSeconds",
        ),
        (
            "/nodes/0/timeoutSeconds",
            json!(14_401),
            "invalid_timeout",
            "/nodes/0/timeoutSeconds",
        ),
    ] {
        let diagnostics = validate_graph(&with(&[(pointer, value)]), &definitions());
        assert_eq!(codes(&diagnostics), [(code, at)], "{pointer}");
    }
}

#[test]
fn an_executor_that_is_not_built_in_is_reported_on_the_node() {
    let mut definitions = definitions();
    let database = definitions.get_mut(&("acme.db".to_owned(), 1)).unwrap();
    database.executor.version = "2.0.0".to_owned();
    assert_eq!(
        codes(&validate_graph(&with(&[]), &definitions)),
        [("unknown_executor", "/nodes/1/definition")]
    );
}

#[test]
fn a_graph_over_the_limits_gets_only_the_limit_diagnostics() {
    let nodes: Vec<Value> = (0..501).map(|index| node(&format!("n{index}"))).collect();
    let graph = with(&[("/nodes", json!(nodes)), ("/schemaVersion", json!(2))]);
    assert_eq!(
        codes(&validate_graph(&graph, &definitions())),
        [("too_many_nodes", "/nodes")]
    );

    let edges: Vec<Value> = (0..1001)
        .map(|index| edge(&format!("e{index}"), "vpc.vpcId", "db.notes"))
        .collect();
    let graph = with(&[("/edges", json!(edges))]);
    assert_eq!(
        codes(&validate_graph(&graph, &definitions())),
        [("too_many_edges", "/edges")]
    );
}

#[test]
fn only_the_first_edge_that_closes_a_cycle_is_reported() {
    // Edge 3 lies on the cycle too, but edge 2 already closed it.
    let graph = with(&[
        ("/edges/2", edge("back", "db.endpoint", "vpc.after")),
        ("/edges/3", edge("notes", "vpc.vpcId", "db.notes")),
    ]);
    assert_eq!(
        codes(&validate_graph(&graph, &definitions())),
        [("cycle", "/edges/2")]
    );
}

#[test]
fn diagnostics_are_sorted_by_pointer_then_code() {
    let graph = with(&[
        ("/schemaVersion", json!(2)),
        ("/nodes/0/retry/maxAttempts", json!(0)),
        ("/edges/2", edge("loop", "vpc.subnets", "vpc.region")),
    ]);
    assert_eq!(
        codes(&validate_graph(&graph, &definitions())),
        [
            ("incompatible_ports", "/edges/2"),
            ("self_edge", "/edges/2"),
            ("multiply_bound_input", "/nodes/0/inputs/region"),
            ("invalid_retry", "/nodes/0/retry/maxAttempts"),
            ("unsupported_schema_version", "/schemaVersion"),
        ]
    );
}

#[test]
fn checksums_ignore_key_order_but_cover_the_viewport() {
    let ordered: GraphSnapshot = serde_json::from_str(
        r#"{"schemaVersion":1,"workflowInputSchema":{"type":"object","properties":{"a":{"type":"string"},"b":{}}},
 "nodes":[{"id":"n","definition":{"key":"acme.vpc","version":1},"displayName":"VPC","position":{"x":1.5,"y":-2},
   "config":{"script":"echo","token":null},
   "inputs":{"after":{"kind":"literal","value":{"z":1,"a":2}},"region":{"kind":"workflowInput","pointer":"/a"}},
   "retry":{"maxAttempts":1},"timeoutSeconds":900}],
 "edges":[],"viewport":{"x":0,"y":0,"zoom":1}}"#,
    )
    .unwrap();
    let reordered: GraphSnapshot = serde_json::from_str(
        r#"{"viewport":{"zoom":1,"y":0,"x":0},"edges":[],
 "nodes":[{"timeoutSeconds":900,"retry":{"maxAttempts":1},
   "inputs":{"region":{"pointer":"/a","kind":"workflowInput"},"after":{"value":{"a":2,"z":1},"kind":"literal"}},
   "config":{"token":null,"script":"echo"},
   "position":{"y":-2,"x":1.5},"displayName":"VPC","definition":{"version":1,"key":"acme.vpc"},"id":"n"}],
 "workflowInputSchema":{"properties":{"b":{},"a":{"type":"string"}},"type":"object"},"schemaVersion":1}"#,
    )
    .unwrap();

    let checksum = canonical_checksum(&ordered);
    assert_eq!(canonical_checksum(&reordered), checksum);
    assert_eq!(checksum.len(), 64);
    assert!(
        checksum
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    );

    let mut moved = ordered;
    moved.viewport.zoom = 1.25;
    assert_ne!(canonical_checksum(&moved), checksum);
}

#[test]
fn the_empty_graph_has_a_pinned_checksum() {
    // SHA-256 of {"edges":[],"nodes":[],"schemaVersion":1,"viewport":{"x":0.0,"y":0.0,"zoom":1.0},
    // "workflowInputSchema":{"properties":{},"type":"object"}} without the line break.
    assert_eq!(
        canonical_checksum(&GraphSnapshot::empty()),
        "0a4920bf2f3421535f9d6cadfab4be1b98d494db660d00049d0ac36224d67406"
    );
}

#[test]
fn serde_json_serializes_object_keys_in_sorted_order() {
    // The checksum relies on this: serde_json's `preserve_order` feature must stay off.
    assert_eq!(
        serde_json::to_string(&json!({"z": 1, "a": 2})).unwrap(),
        r#"{"a":2,"z":1}"#
    );
}
```

- [ ] **Step 2: Run the tests and verify RED**

```text
cargo test -p corqty-domain --test publish_rules
```

Expected: compilation of the test fails with ``error[E0432]: unresolved import `corqty_domain::publish` ``
(``could not find `publish` in `corqty_domain` ``, 1 error).

- [ ] **Step 3: Add the publish validator and the checksum**

The domain crate gains the `sha2` dependency, which the workspace already declares and `corqty-api` already
uses.

Replace the whole of `crates/domain/Cargo.toml`:

```toml
[package]
name = "corqty-domain"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
jsonschema.workspace = true
serde.workspace = true
serde_json.workspace = true
sha2.workspace = true
thiserror.workspace = true
uuid.workspace = true

[lints]
workspace = true
```

The next cargo command (Step 4) updates `Cargo.lock`. It adds `"sha2"` to the `corqty-domain` dependency list,
and nothing else changes, because `sha2` 0.10.9 is already locked.

In `crates/domain/src/catalog.rs`:

Replace:

```rust
/// Not blank and at most `max` characters, so the value always fits its database column.
fn fits(value: &str, max: usize) -> bool {
```

With:

```rust
/// Not blank and at most `max` characters, so the value always fits its database column.
pub(crate) fn fits(value: &str, max: usize) -> bool {
```

Create `crates/domain/src/publish.rs`:

```rust
use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    catalog::{BUILTIN_EXECUTORS, NodeDefinitionSpec, PortSpec, fits, validate_config},
    diagnostic::{Diagnostic, pointer_token},
    graph::{
        GRAPH_SCHEMA_VERSION, GraphSnapshot, InputBinding, MAX_ATTEMPTS, MAX_TIMEOUT_SECONDS,
        check_limits, compatible, has_cycle,
    },
    schema::{compile, compile_at, instance_errors},
};

/// The node definitions a graph may use, by (key, version): the system catalog plus the
/// organization's own.
pub type Definitions = HashMap<(String, i32), NodeDefinitionSpec>;

/// Every reason `graph` cannot be published, sorted by (pointer, code). Empty means publishable.
/// An unknown definition or port skips the rules that depend on it, so nothing cascades.
#[must_use]
pub fn validate_graph(graph: &GraphSnapshot, definitions: &Definitions) -> Vec<Diagnostic> {
    let mut diagnostics = check_limits(graph);
    if !diagnostics.is_empty() {
        // Nothing else is checked, so the work below stays bounded by the limits.
        return diagnostics;
    }
    if graph.schema_version != GRAPH_SCHEMA_VERSION {
        diagnostics.push(Diagnostic::new(
            "unsupported_schema_version",
            "/schemaVersion",
            format!("The graph schema version must be {GRAPH_SCHEMA_VERSION}."),
        ));
    }
    if let Err(errors) = compile_at(&graph.workflow_input_schema, "/workflowInputSchema") {
        diagnostics.extend(errors);
    }
    if graph.workflow_input_schema.get("type") != Some(&json!("object")) {
        diagnostics.push(Diagnostic::new(
            "invalid_schema",
            "/workflowInputSchema",
            "The workflow input schema must have \"type\": \"object\".",
        ));
    }

    // Each node id's definition, None when it is unknown. The first node with an id wins.
    let mut specs: HashMap<&str, Option<&NodeDefinitionSpec>> = HashMap::new();
    for node in &graph.nodes {
        let reference = (node.definition.key.clone(), node.definition.version);
        specs
            .entry(node.id.as_str())
            .or_insert_with(|| definitions.get(&reference));
    }

    let mut edge_ids = HashSet::new();
    // (node id, input port) pairs that an edge feeds.
    let mut fed = HashSet::new();
    // Edges between two different known nodes, for the cycle check.
    let (mut links, mut link_edges) = (Vec::new(), Vec::new());
    for (index, edge) in graph.edges.iter().enumerate() {
        let pointer = format!("/edges/{index}");
        if !edge_ids.insert(edge.id.as_str()) {
            diagnostics.push(Diagnostic::new(
                "duplicate_edge_id",
                format!("{pointer}/id"),
                "Edge ids must be unique.",
            ));
        }
        let source = specs.get(edge.source.node_id.as_str()).copied();
        let target = specs.get(edge.target.node_id.as_str()).copied();
        for (end, known) in [("source", source.is_some()), ("target", target.is_some())] {
            if !known {
                diagnostics.push(Diagnostic::new(
                    "unknown_node",
                    format!("{pointer}/{end}"),
                    "The edge must connect nodes of this graph.",
                ));
            }
        }
        if source.is_some() && target.is_some() {
            if edge.source.node_id == edge.target.node_id {
                diagnostics.push(Diagnostic::new(
                    "self_edge",
                    pointer.clone(),
                    "An edge cannot connect a node to itself.",
                ));
            } else {
                links.push((edge.source.node_id.as_str(), edge.target.node_id.as_str()));
                link_edges.push(index);
            }
        }

        let output = source.flatten().and_then(|spec| {
            let pointer = format!("{pointer}/source");
            find_port(
                &spec.outputs,
                "output",
                &edge.source.port,
                pointer,
                &mut diagnostics,
            )
        });
        let input = target.flatten().and_then(|spec| {
            let pointer = format!("{pointer}/target");
            find_port(
                &spec.inputs,
                "input",
                &edge.target.port,
                pointer,
                &mut diagnostics,
            )
        });
        if let Some(input) = input
            && !fed.insert((edge.target.node_id.as_str(), input.name.as_str()))
        {
            diagnostics.push(Diagnostic::new(
                "multiply_bound_input",
                pointer.clone(),
                "Another edge already feeds this input.",
            ));
        }
        if let (Some(output), Some(input)) = (output, input)
            && !compatible(&output.data_type, &input.data_type)
        {
            diagnostics.push(Diagnostic::new(
                "incompatible_ports",
                pointer,
                format!("{} cannot feed {}.", output.data_type, input.data_type),
            ));
        }
    }
    if has_cycle(&links) {
        // The first edge that closes a cycle ends the shortest cyclic prefix of `links`:
        // links[..low] has no cycle and links[..high] has one.
        let (mut low, mut high) = (0, links.len());
        while high - low > 1 {
            let middle = (low + high) / 2;
            if has_cycle(&links[..middle]) {
                high = middle;
            } else {
                low = middle;
            }
        }
        diagnostics.push(Diagnostic::new(
            "cycle",
            format!("/edges/{}", link_edges[high - 1]),
            "This edge closes a cycle.",
        ));
    }

    let mut node_ids = HashSet::new();
    for (index, node) in graph.nodes.iter().enumerate() {
        let pointer = format!("/nodes/{index}");
        if !node_ids.insert(node.id.as_str()) {
            diagnostics.push(Diagnostic::new(
                "duplicate_node_id",
                format!("{pointer}/id"),
                "Node ids must be unique.",
            ));
        }
        if !fits(&node.display_name, 100) {
            diagnostics.push(Diagnostic::new(
                "invalid_display_name",
                format!("{pointer}/displayName"),
                "A display name must have 1 to 100 characters.",
            ));
        }
        if !(1..=MAX_ATTEMPTS).contains(&node.retry.max_attempts) {
            diagnostics.push(Diagnostic::new(
                "invalid_retry",
                format!("{pointer}/retry/maxAttempts"),
                format!("A node can run 1 to {MAX_ATTEMPTS} times."),
            ));
        }
        if !(1..=MAX_TIMEOUT_SECONDS).contains(&node.timeout_seconds) {
            diagnostics.push(Diagnostic::new(
                "invalid_timeout",
                format!("{pointer}/timeoutSeconds"),
                format!("A timeout must be 1 to {MAX_TIMEOUT_SECONDS} seconds."),
            ));
        }
        let reference = (node.definition.key.clone(), node.definition.version);
        let Some(spec) = definitions.get(&reference) else {
            diagnostics.push(Diagnostic::new(
                "unknown_definition",
                format!("{pointer}/definition"),
                "The node definition does not exist.",
            ));
            continue;
        };
        let executor = (spec.executor.key.as_str(), spec.executor.version.as_str());
        if !BUILTIN_EXECUTORS.contains(&executor) {
            diagnostics.push(Diagnostic::new(
                "unknown_executor",
                format!("{pointer}/definition"),
                "The node definition's executor is not available.",
            ));
        }
        diagnostics.extend(validate_config(
            spec,
            &node.config,
            &format!("{pointer}/config"),
        ));

        for (name, binding) in &node.inputs {
            let at = format!("{pointer}/inputs/{}", pointer_token(name));
            let Some(port) = find_port(&spec.inputs, "input", name, at.clone(), &mut diagnostics)
            else {
                continue;
            };
            if fed.contains(&(node.id.as_str(), name.as_str())) {
                diagnostics.push(Diagnostic::new(
                    "multiply_bound_input",
                    at.clone(),
                    "An edge already feeds this input.",
                ));
            }
            match binding {
                InputBinding::Literal { value } => match compile(&port.schema) {
                    Ok(validator) => {
                        for (path, message) in instance_errors(&validator, value) {
                            let pointer = format!("{at}/value{path}");
                            diagnostics.push(Diagnostic::new("invalid_literal", pointer, message));
                        }
                    }
                    Err(_) => diagnostics.push(Diagnostic::new(
                        "invalid_literal",
                        format!("{at}/value"),
                        "The input port's schema does not compile.",
                    )),
                },
                InputBinding::WorkflowInput { pointer } => {
                    if !resolves(&graph.workflow_input_schema, pointer) {
                        diagnostics.push(Diagnostic::new(
                            "invalid_pointer",
                            format!("{at}/pointer"),
                            "The pointer must name a property of the workflow input schema.",
                        ));
                    }
                }
                // ponytail: no secrets until M4; replace with an org-scoped lookup then.
                InputBinding::Secret { .. } => diagnostics.push(Diagnostic::new(
                    "secret_not_found",
                    format!("{at}/secretId"),
                    "The secret does not exist.",
                )),
            }
        }
        for port in spec.inputs.iter().filter(|port| port.required) {
            let bound = node.inputs.contains_key(&port.name)
                || fed.contains(&(node.id.as_str(), port.name.as_str()));
            if !bound {
                diagnostics.push(Diagnostic::new(
                    "missing_required_input",
                    format!("{pointer}/inputs/{}", pointer_token(&port.name)),
                    "This input is required: connect an edge or add a binding.",
                ));
            }
        }
    }

    diagnostics.sort_by(|a, b| (&a.pointer, &a.code).cmp(&(&b.pointer, &b.code)));
    diagnostics
}

/// The lowercase hex SHA-256 of the graph as JSON with sorted object keys. The same graph
/// always gets the same checksum, whatever key order it was submitted with.
#[must_use]
pub fn canonical_checksum(graph: &GraphSnapshot) -> String {
    // Going through `Value` sorts every object's keys; serializing the struct would not.
    let value = serde_json::to_value(graph).expect("a graph always serializes");
    format!("{:x}", Sha256::digest(value.to_string()))
}

/// The port called `name` among `ports`, or an `unknown_port` diagnostic at `pointer`.
fn find_port<'a>(
    ports: &'a [PortSpec],
    side: &str,
    name: &str,
    pointer: String,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<&'a PortSpec> {
    let port = ports.iter().find(|port| port.name == name);
    if port.is_none() {
        diagnostics.push(Diagnostic::new(
            "unknown_port",
            pointer,
            format!("The node's definition has no {side} port with this name."),
        ));
    }
    port
}

/// Whether `pointer` is an RFC 6901 pointer whose tokens walk `properties` down `schema`
/// (`/a/b` names `properties.a.properties.b`).
fn resolves(schema: &Value, pointer: &str) -> bool {
    let Some(tokens) = pointer.strip_prefix('/') else {
        return pointer.is_empty();
    };
    tokens
        .split('/')
        .try_fold(schema, |schema, token| {
            // `~` only starts the escapes `~0` and `~1`.
            if token
                .split('~')
                .skip(1)
                .any(|rest| !rest.starts_with(['0', '1']))
            {
                return None;
            }
            let token = token.replace("~1", "/").replace("~0", "~");
            schema.get("properties")?.get(token.as_str())
        })
        .is_some()
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
pub mod schema;
pub mod slug;
```

- [ ] **Step 4: Verify GREEN**

```text
cargo test -p corqty-domain
```

Expected: `account_and_rbac` 6 tests, `catalog_rules` 5 tests, `graph_rules` 8 tests and `publish_rules` 9 tests
pass.

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

Expected: every suite passes. corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 2,
`members_http` 3, `organizations_http` 1, `problems_http` 2. corqty-domain: `account_and_rbac` 6, `catalog_rules` 5,
`graph_rules` 8, `publish_rules` 9. corqty-persistence: `auth_store` 2, `catalog_guards` 3, `final_admin` 2,
`migrations` 1, `tenant_isolation` 1, `test_reset` 1. 54 tests in total.

- [ ] **Step 6: Commit**

```text
git add Cargo.lock crates/domain
git commit -m "feat: validate graphs for publishing and compute canonical checksums"
```

Expected: `git show --stat HEAD` lists exactly the 6 files above (814 insertions, 1 deletion).

## Task 11: Store the workflow catalog with tenant isolation and system seeds

**Files:**

- Create: `migrations/202610250001_workflow_catalog.sql`
- Create: `crates/persistence/src/catalog.rs`
- Create: `crates/persistence/src/workflows.rs`
- Modify: `crates/persistence/src/lib.rs`
- Modify: `crates/test-support/src/lib.rs`
- Modify: `crates/persistence/tests/test_reset.rs`
- Test: `crates/persistence/tests/workflow_catalog.rs`

One migration adds five tables:

- `node_definitions` and `blueprints` hold the catalog. System rows have `scope = 'system'`, a NULL
  `organization_id` and a key that starts with `corqty.`. Every other row is an organization row. Two CHECK
  constraints tie the three columns together, so an organization row can never use the reserved prefix (the
  domain rejects it first, with `reserved_key`). Versions are integers from 1, and each `(organization_id, key)`
  gets `max + 1`. `UNIQUE NULLS NOT DISTINCT` makes the system rows unique too.
- `workflows` holds each workflow's name, slug and `published_version`.
- `workflow_drafts` holds its one editable graph and a revision counter.
- `workflow_versions` holds the immutable published graphs with their checksum and the draft revision each one
  came from. At most one version exists per draft revision.

Both child tables reference `workflows (organization_id, id)`, so a draft or version can never point at a
workflow in another organization.

Every table has forced row-level security with policies for `corqty_api` only:

- Reading needs membership (`corqty.is_member`). System catalog rows are readable without any context.
- Writing needs the new `corqty.can_edit` helper, which is true for admins and editors. It is `SECURITY
  DEFINER` with a fixed `search_path` and `REVOKE ALL ... FROM PUBLIC`, like the M1 helpers.
- Each helper call is wrapped in `(SELECT ...)`, so PostgreSQL runs it once per statement instead of once per
  row.

The grants make the catalog append-only. Node definitions, blueprints and versions get `SELECT, INSERT`. Node
definitions also get `UPDATE (deprecated_at)`, so a definition can be deprecated but never renamed. Workflows
and drafts get `SELECT, INSERT, UPDATE`. No table grants `DELETE`, and `corqty_worker` gets nothing.

PostgreSQL refuses a write in two different ways:

- A write that fails a `WITH CHECK` or lacks a grant fails with SQLSTATE 42501.
- An `UPDATE` whose `USING` clause hides the rows touches 0 rows without an error. That is what a runner, a
  viewer or another organization gets.

Task 5's `catalog_guards` check the new tables and the new helper without any change to that file.

`corqty.seed_system_catalog()` inserts the six definitions and two blueprints of design §8. Each row has a fixed
id, and the inserts use `ON CONFLICT (id) DO NOTHING`, so a second run changes nothing. The function is not
`SECURITY DEFINER`: it runs as `corqty_owner`, whose `BYPASSRLS` lets it write the forced-RLS tables. The
migration ends by calling it. `TestDatabase::reset()` truncates every table, so it calls the function again, and
the `seeded_rows` helper in `test_reset` now expects 6 definitions and 2 blueprints.

The repositories `corqty_persistence::catalog` and `corqty_persistence::workflows` are free functions that take
`conn: &mut PgConnection`. They never open a transaction. The handler's `OrgAccess` transaction supplies the
tenant context, and row-level security does the filtering, so the read functions take no organization id. Each
write appends its D9 audit row in the same transaction. The JSON columns are decoded with `#[sqlx(json)]`.
`NodeDefinitionRecord` flattens its spec into its JSON with `#[serde(flatten)]`, and `VersionRecord` flattens
its `VersionSummaryRecord`, so both serialize to the shapes in §9. `insert_version` computes the version number
in SQL as `COALESCE(MAX(version), 0) + 1` instead of taking it as an argument. That keeps it within clippy's
limit of seven arguments, and the caller already holds `lock_workflow`'s row locks, so the number cannot race.

- [ ] **Step 1: Write the failing tests**

`crates/persistence/tests/workflow_catalog.rs` has four tests:

- **Definitions and blueprints.** An editor of organization A stores, deprecates and lists definitions and
  blueprints through the repository. Deprecating a system definition gives `None`. Organization B's admin then
  sees only the system catalog and cannot deprecate A's definition. The audit log holds exactly the D9 rows.
- **Workflows.** The same flow for workflows: create from the data-processing blueprint, create an empty one,
  save a draft (a stale revision gives `None`), lock, publish, list and read. The list is ordered by
  `updated_at`, so the later save and publish move the first workflow ahead of the second. Organization B sees
  none of it, and its save attempt writes nothing.
- **Write rules.** As the owner the test writes one organization A row into every table. It then runs raw SQL as
  each role inside a rolled-back tenant transaction, checking reads, every write, the system catalog,
  published versions and deletes. The `attempt` helper returns the rows touched, or `None` after asserting that
  PostgreSQL refused the statement with 42501.
- **Seeds.** The migration ends with `SELECT corqty.seed_system_catalog();`, and two more calls leave 6 and 2
  rows. Every seeded definition passes `validate_new_definition` except for `reserved_key`. Each default output
  value is present and matches its output port's schema. Both blueprints pass `validate_graph` against
  `definitions_map`, and each node copies its definition's display name and defaults.

Create `crates/persistence/tests/workflow_catalog.rs`:

```rust
use corqty_domain::{
    catalog::{NewNodeDefinition, validate_new_definition},
    graph::GraphSnapshot,
    publish::{canonical_checksum, validate_graph},
    schema::{compile, instance_errors},
};
use corqty_persistence::{catalog, tenant::begin_tenant, workflows};
use corqty_test_support::TestDatabase;
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

/// Organization A with one member per role, and organization B with its own admin.
struct Fixture {
    database: TestDatabase,
    org_a: Uuid,
    org_b: Uuid,
    admin: Uuid,
    editor: Uuid,
    runner: Uuid,
    viewer: Uuid,
    admin_b: Uuid,
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
        Self {
            database,
            org_a,
            org_b,
            admin,
            editor,
            runner,
            viewer,
            admin_b,
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
                let code = error.as_database_error().and_then(|error| error.code());
                assert_eq!(code.as_deref(), Some("42501"), "{sql}: {error}");
                None
            }
        }
    }
}

/// Every audit row in order: (action, resource type, resource id, metadata).
async fn audit_rows(database: &TestDatabase) -> Vec<(String, String, String, Value)> {
    sqlx::query_as(
        "SELECT action, resource_type, resource_id, metadata FROM audit_events ORDER BY id",
    )
    .fetch_all(&database.owner)
    .await
    .unwrap()
}

/// An expected audit row. The resource type is the action's first segment.
fn audit(action: &str, resource_id: Uuid, metadata: Value) -> (String, String, String, Value) {
    let resource_type = action.split('.').next().unwrap();
    (
        action.to_owned(),
        resource_type.to_owned(),
        resource_id.to_string(),
        metadata,
    )
}

#[tokio::test]
#[serial]
async fn editors_store_definitions_and_blueprints_that_other_organizations_cannot_see() {
    let fixture = Fixture::new().await;
    let (a, editor) = (fixture.org_a, fixture.editor);
    let new: NewNodeDefinition = serde_json::from_value(json!({
        "key": "acme.thing", "displayName": "Thing", "category": "Testing",
        "inputs": [], "outputs": [], "configSchema": {"type": "object"}, "uiSchema": {},
        "defaults": {}, "executor": {"key": "mock", "version": "1.0.0"}
    }))
    .unwrap();
    let system_definition = "00000000-0000-7000-8000-000000000001".parse().unwrap();

    let mut tx = fixture.tx(editor, a).await;
    let first = catalog::insert_node_definition(&mut tx, a, editor, &new)
        .await
        .unwrap();
    let second = catalog::insert_node_definition(&mut tx, a, editor, &new)
        .await
        .unwrap();
    assert_eq!(
        (first.scope.as_str(), first.version, second.version),
        ("organization", 1, 2)
    );
    let deprecated = catalog::deprecate_node_definition(&mut tx, a, editor, first.id)
        .await
        .unwrap()
        .unwrap();
    assert!(deprecated.deprecated);
    let system = catalog::deprecate_node_definition(&mut tx, a, editor, system_definition);
    assert!(system.await.unwrap().is_none());
    let empty = GraphSnapshot::empty();
    let blueprint = catalog::insert_blueprint(&mut tx, a, editor, "acme.flow", "Flow", "", &empty)
        .await
        .unwrap();
    let stored = catalog::get_blueprint(&mut tx, blueprint.id).await.unwrap();
    assert_eq!((blueprint.version, stored.unwrap().graph), (1, empty));
    let definitions: Vec<_> = catalog::list_node_definitions(&mut tx)
        .await
        .unwrap()
        .into_iter()
        .map(|definition| (definition.key, definition.version, definition.deprecated))
        .collect();
    assert_eq!(definitions.len(), 8);
    assert_eq!(
        definitions[..2],
        [
            ("acme.thing".to_owned(), 1, true),
            ("acme.thing".to_owned(), 2, false)
        ]
    );
    assert_eq!(catalog::definitions_map(&mut tx).await.unwrap().len(), 8);
    assert_eq!(catalog::list_blueprints(&mut tx).await.unwrap().len(), 3);
    tx.commit().await.unwrap();

    // Organization B sees the system catalog only.
    let (b, admin_b) = (fixture.org_b, fixture.admin_b);
    let mut tx = fixture.tx(admin_b, b).await;
    let definitions = catalog::list_node_definitions(&mut tx).await.unwrap();
    assert!(
        definitions
            .iter()
            .all(|definition| definition.scope == "system")
    );
    assert_eq!(definitions.len(), 6);
    assert_eq!(catalog::definitions_map(&mut tx).await.unwrap().len(), 6);
    assert_eq!(catalog::list_blueprints(&mut tx).await.unwrap().len(), 2);
    let foreign = catalog::get_blueprint(&mut tx, blueprint.id).await.unwrap();
    assert!(foreign.is_none());
    let foreign = catalog::deprecate_node_definition(&mut tx, b, admin_b, second.id);
    assert!(foreign.await.unwrap().is_none());
    tx.commit().await.unwrap();

    let key = |version| json!({"key": "acme.thing", "version": version});
    assert_eq!(
        audit_rows(&fixture.database).await,
        [
            audit("node_definition.created", first.id, key(1)),
            audit("node_definition.created", second.id, key(2)),
            audit("node_definition.deprecated", first.id, key(1)),
            audit(
                "blueprint.created",
                blueprint.id,
                json!({"key": "acme.flow", "version": 1})
            ),
        ]
    );
}

#[tokio::test]
#[serial]
async fn editors_save_and_publish_workflows_that_other_organizations_cannot_see() {
    let fixture = Fixture::new().await;
    let (a, editor) = (fixture.org_a, fixture.editor);
    let blueprint_id = "00000000-0000-7000-8000-000000000102".parse().unwrap();

    let mut tx = fixture.tx(editor, a).await;
    let blueprint = catalog::get_blueprint(&mut tx, blueprint_id)
        .await
        .unwrap()
        .unwrap();
    let graph = blueprint.graph;
    let first = workflows::insert_workflow(
        &mut tx,
        a,
        editor,
        "First",
        "first",
        &graph,
        Some(blueprint_id),
    )
    .await
    .unwrap();
    assert_eq!(
        (
            first.draft.revision,
            &first.draft.graph,
            first.published_version
        ),
        (1, &graph, None)
    );
    tx.commit().await.unwrap();

    let mut tx = fixture.tx(editor, a).await;
    let empty = GraphSnapshot::empty();
    let second = workflows::insert_workflow(&mut tx, a, editor, "Second", "second", &empty, None)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let mut tx = fixture.tx(editor, a).await;
    let mut edited = graph.clone();
    edited.viewport.zoom = 2.0;
    let saved = workflows::save_draft(&mut tx, a, editor, first.id, 1, &edited).await;
    assert_eq!(saved.unwrap(), Some(2));
    let stale = workflows::save_draft(&mut tx, a, editor, first.id, 1, &edited).await;
    assert_eq!(stale.unwrap(), None);
    let locked = workflows::lock_workflow(&mut tx, first.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (locked.revision, &locked.graph, locked.published_version),
        (2, &edited, None)
    );
    let published = workflows::version_exists_for_revision(&mut tx, first.id, 2);
    assert!(!published.await.unwrap());
    let checksum = canonical_checksum(&edited);
    let version = workflows::insert_version(&mut tx, a, editor, first.id, 2, &edited, &checksum)
        .await
        .unwrap();
    assert_eq!((version.version, version.draft_revision), (1, 2));
    let published = workflows::version_exists_for_revision(&mut tx, first.id, 2);
    assert!(published.await.unwrap());
    tx.commit().await.unwrap();

    // The later save and publish moved First ahead of Second.
    let mut tx = fixture.tx(fixture.viewer, a).await;
    let listed: Vec<_> = workflows::list_workflows(&mut tx)
        .await
        .unwrap()
        .into_iter()
        .map(|workflow| {
            let published = (
                workflow.published_version,
                workflow.published_draft_revision,
            );
            (workflow.name, workflow.draft_revision, published)
        })
        .collect();
    assert_eq!(
        listed,
        [
            ("First".to_owned(), 2, (Some(1), Some(2))),
            ("Second".to_owned(), 1, (None, None))
        ]
    );
    assert!(workflows::workflow_exists(&mut tx, first.id).await.unwrap());
    let stored = workflows::get_workflow(&mut tx, first.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((stored.draft.revision, &stored.draft.graph), (2, &edited));
    assert_eq!(
        (stored.published_version, stored.published_draft_revision),
        (Some(1), Some(2))
    );
    let versions = workflows::list_versions(&mut tx, first.id).await.unwrap();
    assert_eq!(versions.len(), 1);
    let stored = workflows::get_version(&mut tx, first.id, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            stored.summary.id,
            stored.summary.checksum.as_str(),
            &stored.graph
        ),
        (version.id, checksum.as_str(), &edited)
    );
    tx.commit().await.unwrap();

    // Organization B sees none of it.
    let (b, admin_b) = (fixture.org_b, fixture.admin_b);
    let mut tx = fixture.tx(admin_b, b).await;
    assert!(workflows::list_workflows(&mut tx).await.unwrap().is_empty());
    assert!(!workflows::workflow_exists(&mut tx, first.id).await.unwrap());
    let foreign = workflows::get_workflow(&mut tx, first.id).await.unwrap();
    assert!(foreign.is_none());
    let foreign = workflows::lock_workflow(&mut tx, first.id).await.unwrap();
    assert!(foreign.is_none());
    let foreign = workflows::list_versions(&mut tx, first.id).await.unwrap();
    assert!(foreign.is_empty());
    let foreign = workflows::get_version(&mut tx, first.id, 1).await.unwrap();
    assert!(foreign.is_none());
    let foreign = workflows::save_draft(&mut tx, b, admin_b, first.id, 2, &empty).await;
    assert_eq!(foreign.unwrap(), None);
    tx.commit().await.unwrap();

    assert_eq!(
        audit_rows(&fixture.database).await,
        [
            audit(
                "workflow.created",
                first.id,
                json!({"blueprintId": blueprint_id})
            ),
            audit("workflow.created", second.id, json!({})),
            audit("workflow.draft_saved", first.id, json!({"revision": 2})),
            audit(
                "workflow.published",
                first.id,
                json!({"version": 1, "draftRevision": 2, "checksum": checksum})
            ),
        ]
    );
}

#[tokio::test]
#[serial]
async fn only_editors_write_and_only_their_own_organization() {
    let fixture = Fixture::new().await;
    let a = fixture.org_a;
    let (first, second) = (Uuid::new_v4(), Uuid::new_v4());
    // As the owner: organization A rows in every table, and a workflow without a draft.
    sqlx::raw_sql(&format!(
        "INSERT INTO node_definitions(
             organization_id, scope, key, version, display_name, category, spec
         ) VALUES ('{a}', 'organization', 'acme.thing', 1, 'Thing', 'Testing', '{{}}');
         INSERT INTO blueprints(organization_id, scope, key, version, display_name, graph)
         VALUES ('{a}', 'organization', 'acme.flow', 1, 'Flow', '{{}}');
         INSERT INTO workflows(id, organization_id, name, slug)
         VALUES ('{first}', '{a}', 'First', 'first'), ('{second}', '{a}', 'Second', 'second');
         INSERT INTO workflow_drafts(workflow_id, organization_id, graph, revision)
         VALUES ('{first}', '{a}', '{{}}', 1);
         INSERT INTO workflow_versions(
             organization_id, workflow_id, version, graph, checksum, draft_revision
         ) VALUES ('{a}', '{first}', 1, '{{}}', repeat('0', 64), 1);"
    ))
    .execute(&fixture.database.owner)
    .await
    .unwrap();

    let tables = [
        ("node_definitions", 1),
        ("blueprints", 1),
        ("workflows", 2),
        ("workflow_drafts", 1),
        ("workflow_versions", 1),
    ];
    for (table, rows) in tables {
        let sql = format!("SELECT FROM {table} WHERE organization_id = '{a}'");
        let viewer = fixture.attempt(fixture.viewer, a, &sql).await;
        assert_eq!(viewer, Some(rows), "{sql}");
        let other = fixture.attempt(fixture.admin_b, fixture.org_b, &sql).await;
        assert_eq!(other, Some(0), "{sql}");
    }

    // (statement, rows for A's admin and editor, rows for A's runner and viewer and B's admin)
    let writes = [
        (
            format!(
                "INSERT INTO node_definitions(
                     organization_id, scope, key, version, display_name, category, spec
                 ) VALUES ('{a}', 'organization', 'acme.thing', 2, 'Thing', 'Testing', '{{}}')"
            ),
            Some(1),
            None,
        ),
        (
            format!(
                "INSERT INTO blueprints(organization_id, scope, key, version, display_name, graph)
                 VALUES ('{a}', 'organization', 'acme.flow', 2, 'Flow', '{{}}')"
            ),
            Some(1),
            None,
        ),
        (
            format!(
                "INSERT INTO workflows(organization_id, name, slug)
                 VALUES ('{a}', 'Third', 'third')"
            ),
            Some(1),
            None,
        ),
        (
            format!(
                "INSERT INTO workflow_drafts(workflow_id, organization_id, graph, revision)
                 VALUES ('{second}', '{a}', '{{}}', 1)"
            ),
            Some(1),
            None,
        ),
        (
            format!(
                "INSERT INTO workflow_versions(
                     organization_id, workflow_id, version, graph, checksum, draft_revision
                 ) VALUES ('{a}', '{first}', 2, '{{}}', repeat('0', 64), 2)"
            ),
            Some(1),
            None,
        ),
        (
            "UPDATE node_definitions SET deprecated_at = now() WHERE scope = 'organization'"
                .to_owned(),
            Some(1),
            Some(0),
        ),
        (
            "UPDATE workflows SET name = 'Renamed'".to_owned(),
            Some(2),
            Some(0),
        ),
        (
            "UPDATE workflow_drafts SET revision = revision + 1".to_owned(),
            Some(1),
            Some(0),
        ),
    ];
    for (sql, editors, others) in &writes {
        for (user, organization, expected) in [
            (fixture.admin, a, editors),
            (fixture.editor, a, editors),
            (fixture.runner, a, others),
            (fixture.viewer, a, others),
            (fixture.admin_b, fixture.org_b, others),
        ] {
            let rows = fixture.attempt(user, organization, sql).await;
            assert_eq!(rows, *expected, "{sql}");
        }
    }

    // Nobody writes the system catalog, renames a definition or blueprint, or changes a
    // published version, and nothing is ever deleted.
    for sql in [
        "INSERT INTO node_definitions(scope, key, version, display_name, category, spec)
         VALUES ('system', 'corqty.probe', 1, 'Probe', 'Testing', '{}')",
        "INSERT INTO blueprints(scope, key, version, display_name, graph)
         VALUES ('system', 'corqty.probe', 1, 'Probe', '{}')",
        "UPDATE node_definitions SET display_name = 'Renamed'",
        "UPDATE blueprints SET display_name = 'Renamed'",
        "UPDATE workflow_versions SET checksum = repeat('1', 64)",
        "DELETE FROM node_definitions",
        "DELETE FROM blueprints",
        "DELETE FROM workflows",
        "DELETE FROM workflow_drafts",
        "DELETE FROM workflow_versions",
    ] {
        assert_eq!(fixture.attempt(fixture.admin, a, sql).await, None, "{sql}");
    }
    let system = "UPDATE node_definitions SET deprecated_at = now() WHERE scope = 'system'";
    assert_eq!(fixture.attempt(fixture.admin, a, system).await, Some(0));
}

#[tokio::test]
#[serial]
async fn system_seeds_are_idempotent_and_valid() {
    let migration = include_str!("../../../migrations/202610250001_workflow_catalog.sql");
    assert!(
        migration
            .trim_end()
            .ends_with("SELECT corqty.seed_system_catalog();")
    );

    let database = TestDatabase::reset().await;
    for _ in 0..2 {
        sqlx::query("SELECT corqty.seed_system_catalog()")
            .execute(&database.owner)
            .await
            .unwrap();
    }
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM node_definitions), (SELECT count(*) FROM blueprints)",
    )
    .fetch_one(&database.owner)
    .await
    .unwrap();
    assert_eq!(counts, (6, 2));

    // The system catalog needs no organization context.
    let mut conn = database.api.acquire().await.unwrap();
    let definitions = catalog::list_node_definitions(&mut conn).await.unwrap();
    let keys: Vec<_> = definitions.iter().map(|d| d.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "corqty.mock.normalize-payload",
            "corqty.mock.produce-summary",
            "corqty.mock.task",
            "corqty.simulated.create-rds",
            "corqty.simulated.create-vpc",
            "corqty.simulated.deploy-service",
        ]
    );
    for definition in &definitions {
        let new = NewNodeDefinition {
            key: definition.key.clone(),
            display_name: definition.display_name.clone(),
            description: definition.description.clone(),
            category: definition.category.clone(),
            spec: definition.spec.clone(),
        };
        // The organization rules apply, except that system keys use the reserved prefix.
        let diagnostics = validate_new_definition(&new);
        let codes: Vec<_> = diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.pointer.as_str()))
            .collect();
        assert_eq!(codes, [("reserved_key", "/key")], "{}", definition.key);
        for port in &definition.spec.outputs {
            let value = &definition.spec.defaults["output"][port.name.as_str()];
            let validator = compile(&port.schema).unwrap();
            let errors = instance_errors(&validator, value);
            assert!(!value.is_null() && errors.is_empty(), "{}", definition.key);
        }
    }

    let usable = catalog::definitions_map(&mut conn).await.unwrap();
    let blueprints = catalog::list_blueprints(&mut conn).await.unwrap();
    let keys: Vec<_> = blueprints.iter().map(|b| b.key.as_str()).collect();
    assert_eq!(
        keys,
        ["corqty.data-processing", "corqty.simulated-infrastructure"]
    );
    for blueprint in &blueprints {
        let diagnostics = validate_graph(&blueprint.graph, &usable);
        assert!(diagnostics.is_empty(), "{}: {diagnostics:?}", blueprint.key);
        for node in &blueprint.graph.nodes {
            let definition = definitions
                .iter()
                .find(|definition| definition.key == node.definition.key)
                .unwrap();
            assert_eq!(node.display_name, definition.display_name);
            assert_eq!(node.config, definition.spec.defaults);
        }
    }
}
```

Replace the whole of `crates/persistence/tests/test_reset.rs`:

```rust
use corqty_test_support::TestDatabase;
use serial_test::serial;

/// Rows that `TestDatabase::reset()` puts back after truncating a table.
fn seeded_rows(table: &str) -> i64 {
    match table {
        "node_definitions" => 6,
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

- [ ] **Step 2: Run the tests and verify RED**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-persistence --test workflow_catalog
```

Expected: compilation of the test fails with 2 errors. The first is ``error: couldn't read
`crates\persistence\tests\../../../migrations/202610250001_workflow_catalog.sql` `` because the migration does
not exist yet; on Windows it ends with `The system cannot find the file specified. (os error 2)`. The second is
``error[E0432]: unresolved imports `corqty_persistence::catalog`, `corqty_persistence::workflows` ``. Until the
migration exists, `test_reset` still passes, because no table named `node_definitions` or `blueprints` is
counted yet.

- [ ] **Step 3: Add the migration and the system seeds**

Create `migrations/202610250001_workflow_catalog.sql`:

```sql
CREATE FUNCTION corqty.can_edit(target_organization_id uuid)
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
          AND role IN ('admin', 'editor')
    )
$$;

REVOKE ALL ON FUNCTION corqty.can_edit(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.can_edit(uuid) TO corqty_api;

CREATE TABLE node_definitions (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    organization_id uuid REFERENCES organizations(id) ON DELETE CASCADE,
    scope text NOT NULL CHECK (scope IN ('system', 'organization')),
    key text NOT NULL CHECK (key ~ '^[a-z0-9]+([.-][a-z0-9]+)*$' AND char_length(key) <= 100),
    version integer NOT NULL CHECK (version >= 1),
    display_name text NOT NULL CHECK (char_length(display_name) BETWEEN 1 AND 100),
    description text NOT NULL DEFAULT '' CHECK (char_length(description) <= 1000),
    category text NOT NULL CHECK (char_length(category) BETWEEN 1 AND 60),
    spec jsonb NOT NULL,
    deprecated_at timestamptz,
    created_by uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT node_definitions_scope_org CHECK ((scope = 'system') = (organization_id IS NULL)),
    CONSTRAINT node_definitions_scope_key CHECK ((scope = 'system') = (key LIKE 'corqty.%')),
    CONSTRAINT node_definitions_key_version UNIQUE NULLS NOT DISTINCT (organization_id, key, version)
);

CREATE TABLE blueprints (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    organization_id uuid REFERENCES organizations(id) ON DELETE CASCADE,
    scope text NOT NULL CHECK (scope IN ('system', 'organization')),
    key text NOT NULL CHECK (key ~ '^[a-z0-9]+([.-][a-z0-9]+)*$' AND char_length(key) <= 100),
    version integer NOT NULL CHECK (version >= 1),
    display_name text NOT NULL CHECK (char_length(display_name) BETWEEN 1 AND 100),
    description text NOT NULL DEFAULT '' CHECK (char_length(description) <= 1000),
    graph jsonb NOT NULL,
    created_by uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT blueprints_scope_org CHECK ((scope = 'system') = (organization_id IS NULL)),
    CONSTRAINT blueprints_scope_key CHECK ((scope = 'system') = (key LIKE 'corqty.%')),
    CONSTRAINT blueprints_key_version UNIQUE NULLS NOT DISTINCT (organization_id, key, version)
);

CREATE TABLE workflows (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    slug text NOT NULL,
    published_version integer,
    created_by uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT workflows_org_slug UNIQUE (organization_id, slug),
    CONSTRAINT workflows_org_id UNIQUE (organization_id, id)
);
CREATE INDEX workflows_org_updated_idx ON workflows (organization_id, updated_at DESC, id DESC);

CREATE TABLE workflow_drafts (
    workflow_id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    graph jsonb NOT NULL,
    revision bigint NOT NULL CHECK (revision >= 1),
    updated_by uuid REFERENCES users(id) ON DELETE SET NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (organization_id, workflow_id) REFERENCES workflows (organization_id, id) ON DELETE CASCADE
);

CREATE TABLE workflow_versions (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    organization_id uuid NOT NULL,
    workflow_id uuid NOT NULL,
    version integer NOT NULL CHECK (version >= 1),
    graph jsonb NOT NULL,
    checksum text NOT NULL CHECK (checksum ~ '^[0-9a-f]{64}$'),
    draft_revision bigint NOT NULL,
    published_by uuid REFERENCES users(id) ON DELETE SET NULL,
    published_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (organization_id, workflow_id) REFERENCES workflows (organization_id, id) ON DELETE CASCADE,
    CONSTRAINT workflow_versions_number UNIQUE (workflow_id, version),
    CONSTRAINT workflow_versions_draft_revision UNIQUE (workflow_id, draft_revision)
);

ALTER TABLE workflows ADD CONSTRAINT workflows_published_version_fk
    FOREIGN KEY (id, published_version) REFERENCES workflow_versions (workflow_id, version);

-- Definitions and blueprints are immutable apart from deprecation, versions are
-- immutable, and nothing here is ever deleted.
GRANT SELECT, INSERT ON node_definitions, blueprints, workflow_versions TO corqty_api;
GRANT UPDATE (deprecated_at) ON node_definitions TO corqty_api;
GRANT SELECT, INSERT, UPDATE ON workflows, workflow_drafts TO corqty_api;

-- System rows (organization_id NULL) are visible to everyone; organization rows only
-- to members. The (SELECT ...) wrappers run each helper once per statement.
ALTER TABLE node_definitions ENABLE ROW LEVEL SECURITY;
ALTER TABLE node_definitions FORCE ROW LEVEL SECURITY;
CREATE POLICY node_definitions_select ON node_definitions
    FOR SELECT TO corqty_api
    USING (
        organization_id IS NULL
        OR (
            organization_id = corqty.current_organization_id()
            AND (SELECT corqty.is_member(corqty.current_organization_id()))
        )
    );
CREATE POLICY node_definitions_insert ON node_definitions
    FOR INSERT TO corqty_api
    WITH CHECK (
        scope = 'organization'
        AND organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_edit(corqty.current_organization_id()))
    );
CREATE POLICY node_definitions_update ON node_definitions
    FOR UPDATE TO corqty_api
    USING (
        scope = 'organization'
        AND organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_edit(corqty.current_organization_id()))
    )
    WITH CHECK (
        scope = 'organization'
        AND organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_edit(corqty.current_organization_id()))
    );

ALTER TABLE blueprints ENABLE ROW LEVEL SECURITY;
ALTER TABLE blueprints FORCE ROW LEVEL SECURITY;
CREATE POLICY blueprints_select ON blueprints
    FOR SELECT TO corqty_api
    USING (
        organization_id IS NULL
        OR (
            organization_id = corqty.current_organization_id()
            AND (SELECT corqty.is_member(corqty.current_organization_id()))
        )
    );
CREATE POLICY blueprints_insert ON blueprints
    FOR INSERT TO corqty_api
    WITH CHECK (
        scope = 'organization'
        AND organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_edit(corqty.current_organization_id()))
    );

ALTER TABLE workflows ENABLE ROW LEVEL SECURITY;
ALTER TABLE workflows FORCE ROW LEVEL SECURITY;
CREATE POLICY workflows_select ON workflows
    FOR SELECT TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.is_member(corqty.current_organization_id()))
    );
CREATE POLICY workflows_insert ON workflows
    FOR INSERT TO corqty_api
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_edit(corqty.current_organization_id()))
    );
CREATE POLICY workflows_update ON workflows
    FOR UPDATE TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_edit(corqty.current_organization_id()))
    )
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_edit(corqty.current_organization_id()))
    );

ALTER TABLE workflow_drafts ENABLE ROW LEVEL SECURITY;
ALTER TABLE workflow_drafts FORCE ROW LEVEL SECURITY;
CREATE POLICY workflow_drafts_select ON workflow_drafts
    FOR SELECT TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.is_member(corqty.current_organization_id()))
    );
CREATE POLICY workflow_drafts_insert ON workflow_drafts
    FOR INSERT TO corqty_api
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_edit(corqty.current_organization_id()))
    );
CREATE POLICY workflow_drafts_update ON workflow_drafts
    FOR UPDATE TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_edit(corqty.current_organization_id()))
    )
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_edit(corqty.current_organization_id()))
    );

ALTER TABLE workflow_versions ENABLE ROW LEVEL SECURITY;
ALTER TABLE workflow_versions FORCE ROW LEVEL SECURITY;
CREATE POLICY workflow_versions_select ON workflow_versions
    FOR SELECT TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.is_member(corqty.current_organization_id()))
    );
CREATE POLICY workflow_versions_insert ON workflow_versions
    FOR INSERT TO corqty_api
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_edit(corqty.current_organization_id()))
    );

-- The system catalog. Migrations and test resets run it as corqty_owner, which
-- bypasses row-level security; ON CONFLICT makes a second run change nothing.
CREATE FUNCTION corqty.seed_system_catalog()
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
    -- Every system definition runs on the mock executor with this config form.
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
END;
$$;

REVOKE ALL ON FUNCTION corqty.seed_system_catalog() FROM PUBLIC;

SELECT corqty.seed_system_catalog();
```

- [ ] **Step 4: Seed the system catalog again after every test reset**

Replace the whole of `crates/test-support/src/lib.rs`:

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
            .connect("postgres://corqty_owner:corqty_owner_dev@127.0.0.1:55432/corqty_test")
            .await
            .expect("connect owner test database");
        MIGRATOR.run(&owner).await.expect("run test migrations");

        // Every table the migrations created, so a new table needs no change here.
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT format('%I.%I', schemaname, tablename)
             FROM pg_tables
             WHERE schemaname = 'public' AND tablename <> '_sqlx_migrations'",
        )
        .fetch_all(&owner)
        .await
        .expect("list test tables");
        sqlx::query(&format!(
            "TRUNCATE TABLE {} RESTART IDENTITY CASCADE",
            tables.join(", ")
        ))
        .execute(&owner)
        .await
        .expect("reset test data");
        // The truncate removed the system catalog that the migration seeded.
        sqlx::query("SELECT corqty.seed_system_catalog()")
            .execute(&owner)
            .await
            .expect("seed system catalog");

        let api = PgPoolOptions::new()
            // A single API connection makes the context-leak test prove that
            // SET LOCAL values disappear before this connection is reused.
            .max_connections(1)
            .connect("postgres://corqty_api:corqty_api_dev@127.0.0.1:55432/corqty_test")
            .await
            .expect("connect api test database");

        Self { owner, api }
    }
}
```

- [ ] **Step 5: Add the catalog and workflow repositories**

Create `crates/persistence/src/catalog.rs`:

```rust
use chrono::{DateTime, Utc};
use corqty_domain::{
    catalog::{NewNodeDefinition, NodeDefinitionSpec},
    graph::GraphSnapshot,
    publish::Definitions,
};
use serde::Serialize;
use serde_json::json;
use sqlx::{PgConnection, types::Json};
use uuid::Uuid;

/// One node definition version. The spec fields sit at the top level of its JSON.
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct NodeDefinitionRecord {
    pub id: Uuid,
    pub scope: String,
    pub key: String,
    pub version: i32,
    pub display_name: String,
    pub description: String,
    pub category: String,
    #[serde(flatten)]
    #[sqlx(json)]
    pub spec: NodeDefinitionSpec,
    pub deprecated: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintRecord {
    pub id: Uuid,
    pub scope: String,
    pub key: String,
    pub version: i32,
    pub display_name: String,
    pub description: String,
    #[sqlx(json)]
    pub graph: GraphSnapshot,
    pub created_at: DateTime<Utc>,
}

const DEFINITION: &str = "id, scope, key, version, display_name, description, category, spec,
     deprecated_at IS NOT NULL AS deprecated, created_at";
const BLUEPRINT: &str = "id, scope, key, version, display_name, description, graph, created_at";

/// The system catalog and the current organization's definitions, by key and version.
pub async fn list_node_definitions(
    conn: &mut PgConnection,
) -> Result<Vec<NodeDefinitionRecord>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {DEFINITION} FROM node_definitions ORDER BY key, version"
    ))
    .fetch_all(conn)
    .await
}

/// Stores the next version of the key in the organization. A concurrent insert of the same
/// key fails with a unique violation on `node_definitions_key_version`.
pub async fn insert_node_definition(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    definition: &NewNodeDefinition,
) -> Result<NodeDefinitionRecord, sqlx::Error> {
    let record: NodeDefinitionRecord = sqlx::query_as(&format!(
        "INSERT INTO node_definitions(
            organization_id, scope, key, version, display_name,
            description, category, spec, created_by
         )
         SELECT $1, 'organization', $2, COALESCE(MAX(version), 0) + 1, $3, $4, $5, $6, $7
         FROM node_definitions
         WHERE organization_id = $1 AND key = $2
         RETURNING {DEFINITION}"
    ))
    .bind(organization_id)
    .bind(&definition.key)
    .bind(&definition.display_name)
    .bind(&definition.description)
    .bind(&definition.category)
    .bind(Json(&definition.spec))
    .bind(actor_user_id)
    .fetch_one(&mut *conn)
    .await?;
    audit_definition(
        conn,
        organization_id,
        actor_user_id,
        "node_definition.created",
        &record,
    )
    .await?;
    Ok(record)
}

/// Deprecates one of the organization's own definitions. System and foreign ids give None.
pub async fn deprecate_node_definition(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    id: Uuid,
) -> Result<Option<NodeDefinitionRecord>, sqlx::Error> {
    let record: Option<NodeDefinitionRecord> = sqlx::query_as(&format!(
        "UPDATE node_definitions
         SET deprecated_at = COALESCE(deprecated_at, now())
         WHERE id = $1 AND organization_id = $2
         RETURNING {DEFINITION}"
    ))
    .bind(id)
    .bind(organization_id)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(record) = &record {
        audit_definition(
            conn,
            organization_id,
            actor_user_id,
            "node_definition.deprecated",
            record,
        )
        .await?;
    }
    Ok(record)
}

/// Every definition a graph in the current organization may use, deprecated ones included.
pub async fn definitions_map(conn: &mut PgConnection) -> Result<Definitions, sqlx::Error> {
    let rows: Vec<(String, i32, Json<NodeDefinitionSpec>)> =
        sqlx::query_as("SELECT key, version, spec FROM node_definitions")
            .fetch_all(conn)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(key, version, Json(spec))| ((key, version), spec))
        .collect())
}

/// The system blueprints and the current organization's, by key and version.
pub async fn list_blueprints(conn: &mut PgConnection) -> Result<Vec<BlueprintRecord>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {BLUEPRINT} FROM blueprints ORDER BY key, version"
    ))
    .fetch_all(conn)
    .await
}

pub async fn get_blueprint(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<Option<BlueprintRecord>, sqlx::Error> {
    sqlx::query_as(&format!("SELECT {BLUEPRINT} FROM blueprints WHERE id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// Stores the next version of the key in the organization, like `insert_node_definition`.
pub async fn insert_blueprint(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    key: &str,
    display_name: &str,
    description: &str,
    graph: &GraphSnapshot,
) -> Result<BlueprintRecord, sqlx::Error> {
    let record: BlueprintRecord = sqlx::query_as(&format!(
        "INSERT INTO blueprints(
            organization_id, scope, key, version, display_name, description, graph, created_by
         )
         SELECT $1, 'organization', $2, COALESCE(MAX(version), 0) + 1, $3, $4, $5, $6
         FROM blueprints
         WHERE organization_id = $1 AND key = $2
         RETURNING {BLUEPRINT}"
    ))
    .bind(organization_id)
    .bind(key)
    .bind(display_name)
    .bind(description)
    .bind(Json(graph))
    .bind(actor_user_id)
    .fetch_one(&mut *conn)
    .await?;
    crate::audit::insert_in_transaction(
        conn,
        organization_id,
        actor_user_id,
        "blueprint.created",
        "blueprint",
        &record.id.to_string(),
        json!({ "key": record.key, "version": record.version }),
    )
    .await?;
    Ok(record)
}

async fn audit_definition(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    action: &str,
    record: &NodeDefinitionRecord,
) -> Result<(), sqlx::Error> {
    crate::audit::insert_in_transaction(
        conn,
        organization_id,
        actor_user_id,
        action,
        "node_definition",
        &record.id.to_string(),
        json!({ "key": record.key, "version": record.version }),
    )
    .await
}
```

Create `crates/persistence/src/workflows.rs`:

```rust
use chrono::{DateTime, Utc};
use corqty_domain::graph::GraphSnapshot;
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, types::Json};
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSummaryRecord {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub draft_revision: i64,
    pub published_version: Option<i32>,
    pub published_draft_revision: Option<i64>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRecord {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub published_version: Option<i32>,
    pub published_draft_revision: Option<i64>,
    #[sqlx(flatten)]
    pub draft: DraftRecord,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct DraftRecord {
    pub revision: i64,
    #[sqlx(json)]
    pub graph: GraphSnapshot,
    pub updated_at: DateTime<Utc>,
}

/// The draft that a publish reads while it holds the workflow's row locks.
#[derive(Debug, sqlx::FromRow)]
pub struct LockedWorkflow {
    pub revision: i64,
    #[sqlx(json)]
    pub graph: GraphSnapshot,
    pub published_version: Option<i32>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct VersionSummaryRecord {
    pub id: Uuid,
    pub version: i32,
    pub checksum: String,
    pub draft_revision: i64,
    pub published_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct VersionRecord {
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub summary: VersionSummaryRecord,
    #[sqlx(json)]
    pub graph: GraphSnapshot,
}

/// Each workflow with its draft and, through `published_version`, its published version.
const WORKFLOWS: &str = "workflows w
     JOIN workflow_drafts d ON d.workflow_id = w.id
     LEFT JOIN workflow_versions v ON v.workflow_id = w.id AND v.version = w.published_version";
const VERSION_SUMMARY: &str = "id, version, checksum, draft_revision, published_at";

/// The current organization's workflows, most recently changed first.
pub async fn list_workflows(
    conn: &mut PgConnection,
) -> Result<Vec<WorkflowSummaryRecord>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT w.id, w.name, w.slug, d.revision AS draft_revision, w.published_version,
                v.draft_revision AS published_draft_revision, w.updated_at
         FROM {WORKFLOWS}
         ORDER BY w.updated_at DESC, w.id DESC"
    ))
    .fetch_all(conn)
    .await
}

pub async fn get_workflow(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<Option<WorkflowRecord>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT w.id, w.name, w.slug, w.published_version,
                v.draft_revision AS published_draft_revision,
                d.revision, d.graph, d.updated_at
         FROM {WORKFLOWS}
         WHERE w.id = $1"
    ))
    .bind(id)
    .fetch_optional(conn)
    .await
}

/// Creates a workflow whose draft starts at revision 1 with `graph`.
pub async fn insert_workflow(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    name: &str,
    slug: &str,
    graph: &GraphSnapshot,
    blueprint_id: Option<Uuid>,
) -> Result<WorkflowRecord, sqlx::Error> {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO workflows(organization_id, name, slug, created_by)
         VALUES ($1, $2, $3, $4)
         RETURNING id",
    )
    .bind(organization_id)
    .bind(name)
    .bind(slug)
    .bind(actor_user_id)
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO workflow_drafts(workflow_id, organization_id, graph, revision, updated_by)
         VALUES ($1, $2, $3, 1, $4)",
    )
    .bind(id)
    .bind(organization_id)
    .bind(Json(graph))
    .bind(actor_user_id)
    .execute(&mut *conn)
    .await?;
    let metadata = blueprint_id.map_or_else(|| json!({}), |id| json!({ "blueprintId": id }));
    audit(
        conn,
        organization_id,
        actor_user_id,
        "workflow.created",
        id,
        metadata,
    )
    .await?;
    get_workflow(conn, id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

/// Replaces the draft if it is still at `expected_revision` and returns the new revision.
/// None means the revision moved on or the workflow is not visible; nothing is written then.
pub async fn save_draft(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    workflow_id: Uuid,
    expected_revision: i64,
    graph: &GraphSnapshot,
) -> Result<Option<i64>, sqlx::Error> {
    let revision: Option<i64> = sqlx::query_scalar(
        "UPDATE workflow_drafts
         SET graph = $3, revision = revision + 1, updated_by = $4, updated_at = now()
         WHERE workflow_id = $1 AND revision = $2
         RETURNING revision",
    )
    .bind(workflow_id)
    .bind(expected_revision)
    .bind(Json(graph))
    .bind(actor_user_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(revision) = revision else {
        return Ok(None);
    };
    sqlx::query("UPDATE workflows SET updated_at = now() WHERE id = $1")
        .bind(workflow_id)
        .execute(&mut *conn)
        .await?;
    let metadata = json!({ "revision": revision });
    audit(
        conn,
        organization_id,
        actor_user_id,
        "workflow.draft_saved",
        workflow_id,
        metadata,
    )
    .await?;
    Ok(Some(revision))
}

pub async fn workflow_exists(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM workflows WHERE id = $1)")
        .bind(id)
        .fetch_one(conn)
        .await
}

/// Locks the workflow and its draft until the transaction ends, so publishes and saves of
/// one workflow run one at a time.
pub async fn lock_workflow(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<Option<LockedWorkflow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT d.revision, d.graph, w.published_version
         FROM workflows w
         JOIN workflow_drafts d ON d.workflow_id = w.id
         WHERE w.id = $1
         FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(conn)
    .await
}

pub async fn version_exists_for_revision(
    conn: &mut PgConnection,
    workflow_id: Uuid,
    revision: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM workflow_versions WHERE workflow_id = $1 AND draft_revision = $2
         )",
    )
    .bind(workflow_id)
    .bind(revision)
    .fetch_one(conn)
    .await
}

/// Stores `graph` as the workflow's next version and makes it the published one. Callers
/// hold `lock_workflow`'s locks. A second version of one draft revision fails with a unique
/// violation on `workflow_versions_draft_revision`.
pub async fn insert_version(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    workflow_id: Uuid,
    draft_revision: i64,
    graph: &GraphSnapshot,
    checksum: &str,
) -> Result<VersionSummaryRecord, sqlx::Error> {
    let version: VersionSummaryRecord = sqlx::query_as(&format!(
        "INSERT INTO workflow_versions(
            organization_id, workflow_id, version, graph, checksum, draft_revision, published_by
         )
         SELECT $1, $2, COALESCE(MAX(version), 0) + 1, $3, $4, $5, $6
         FROM workflow_versions
         WHERE workflow_id = $2
         RETURNING {VERSION_SUMMARY}"
    ))
    .bind(organization_id)
    .bind(workflow_id)
    .bind(Json(graph))
    .bind(checksum)
    .bind(draft_revision)
    .bind(actor_user_id)
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query("UPDATE workflows SET published_version = $2, updated_at = now() WHERE id = $1")
        .bind(workflow_id)
        .bind(version.version)
        .execute(&mut *conn)
        .await?;
    let metadata = json!({
        "version": version.version,
        "draftRevision": draft_revision,
        "checksum": checksum,
    });
    audit(
        conn,
        organization_id,
        actor_user_id,
        "workflow.published",
        workflow_id,
        metadata,
    )
    .await?;
    Ok(version)
}

/// The workflow's versions, newest first.
pub async fn list_versions(
    conn: &mut PgConnection,
    workflow_id: Uuid,
) -> Result<Vec<VersionSummaryRecord>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {VERSION_SUMMARY} FROM workflow_versions
         WHERE workflow_id = $1
         ORDER BY version DESC"
    ))
    .bind(workflow_id)
    .fetch_all(conn)
    .await
}

pub async fn get_version(
    conn: &mut PgConnection,
    workflow_id: Uuid,
    version: i32,
) -> Result<Option<VersionRecord>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {VERSION_SUMMARY}, graph FROM workflow_versions
         WHERE workflow_id = $1 AND version = $2"
    ))
    .bind(workflow_id)
    .bind(version)
    .fetch_optional(conn)
    .await
}

async fn audit(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    action: &str,
    workflow_id: Uuid,
    metadata: Value,
) -> Result<(), sqlx::Error> {
    crate::audit::insert_in_transaction(
        conn,
        organization_id,
        actor_user_id,
        action,
        "workflow",
        &workflow_id.to_string(),
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
pub mod store;
pub mod tenant;
pub mod workflows;
```

- [ ] **Step 6: Verify GREEN**

```text
cargo test -p corqty-persistence
```

Expected: `auth_store` 2, `catalog_guards` 3, `final_admin` 2, `migrations` 1, `tenant_isolation` 1,
`test_reset` 1 and `workflow_catalog` 4 tests pass. `catalog_guards` passing shows that the new tables force
row-level security, have policies and grant nothing to `corqty_worker`, and that `corqty.can_edit` is guarded.

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

Expected: every suite passes. corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 2,
`members_http` 3, `organizations_http` 1, `problems_http` 2. corqty-domain: `account_and_rbac` 6, `catalog_rules` 5,
`graph_rules` 8, `publish_rules` 9. corqty-persistence: `auth_store` 2, `catalog_guards` 3, `final_admin` 2,
`migrations` 1, `tenant_isolation` 1, `test_reset` 1, `workflow_catalog` 4. 58 tests in total.

- [ ] **Step 8: Commit**

```text
git add migrations/202610250001_workflow_catalog.sql crates/persistence crates/test-support
git commit -m "feat: store the workflow catalog with tenant isolation and system seeds"
```

Expected: `git show --stat HEAD` lists exactly the 7 files above (1562 insertions, 2 deletions).

## Task 12: Expose node definition and blueprint APIs

**Files:**

- Create: `crates/api/src/catalog.rs`
- Modify: `crates/api/src/lib.rs`
- Modify: `crates/api/src/router.rs`
- Modify: `crates/api/tests/authorization_matrix.rs`
- Test: `crates/api/tests/catalog_http.rs`

This task puts the Task 11 catalog repositories behind five routes under `/api/v1/orgs/{organization_id}`:

| method + path | capability | success | errors |
|---|---|---|---|
| GET `/node-definitions` | `ViewOrganization` | 200 `{items}` | 404 |
| POST `/node-definitions` | `EditWorkflows` | 201 node definition | 422 `node_definition_invalid`, 409 `node_definition_version_conflict` |
| POST `/node-definitions/{definition_id}/deprecate` | `EditWorkflows` | 200 node definition | 404 for a system or foreign id |
| GET `/blueprints` | `ViewOrganization` | 200 `{items}` | 404 |
| POST `/blueprints` | `EditWorkflows` | 201 blueprint | 422 `blueprint_invalid` |

Every handler in the new `crates/api/src/catalog.rs` follows the Task 1 pattern. `authorize` opens the tenant
transaction and checks the capability, so an outsider gets 404 and a member without the capability gets 403 before
any validation runs. The handler then validates, calls the repository with `&mut access.tx`, and finishes with
`access.commit()`. An early `return Err(...)` drops the transaction, which rolls it back, so a failed call never
leaves a row or an audit event behind. The repositories write the D9 audit rows (`node_definition.created`,
`node_definition.deprecated`, `blueprint.created`, each with `{"key", "version"}` metadata) in the same transaction.

- The lists return system rows plus the organization's own, ordered by key and version. Row-level security does
  the filtering, and deprecated definitions stay in the list with `"deprecated": true`.
- `POST /node-definitions` runs `validate_new_definition`. Any diagnostic gives 422 `node_definition_invalid`
  with the diagnostics, whose pointers point into the request body (`reserved_key` at `/key`, `unknown_widget` at
  `/uiSchema/mode/widget`). The next version of a key is `max + 1` inside the INSERT. Two concurrent requests for
  the same key can both compute the same number. The later INSERT then fails on the unique constraint
  `node_definitions_key_version` and becomes 409 `node_definition_version_conflict`. Retrying gets the next
  version.
- `POST /node-definitions/{definition_id}/deprecate` takes no body. The repository returns `None` for a system
  definition, an unknown id and another organization's id, and all three become 404.
- `POST /blueprints` takes `{key, displayName, description?, graph}`. The handler collects
  `validate_blueprint_metadata` (key, display name, description) and `validate_graph` against `definitions_map`.
  Each graph pointer gets a `/graph` prefix so it points into the request body. Any diagnostic gives 422
  `blueprint_invalid`. `definitions_map` only holds the system catalog and the organization's own definitions, so
  a blueprint cannot use another organization's definition. Design §9 defines no conflict code for blueprints,
  which only the API creates in this milestone. A concurrent insert of the same blueprint key therefore fails on
  `blueprints_key_version` and returns 500.

`Items<T>` serializes a list as `{"items": [...]}`. It is public, so the workflow routes in Task 13 can reuse it.

- [ ] **Step 1: Write the failing tests**

`crates/api/tests/catalog_http.rs` has three tests. Each helper asserts the response status before reading the
body:

- **Node definitions.** The admin invites an editor through the API. The editor creates `acme.thing` twice and
  gets versions 1 and 2. The test compares the whole JSON of each one, which shows that the spec fields sit at
  the top level and that omitted defaults come back filled in. `corqty.custom` gives the full 422 problem with
  `reserved_key` at `/key`, and the widget `dropdown` gives `unknown_widget` at `/uiSchema/mode/widget`.
  Deprecating version 1 gives 200 with `deprecated: true`. A second organization creates its own `acme.thing`,
  and deprecating either that id or the system `corqty.mock.task` through the first organization gives 404. The
  list then holds both versions followed by the six system definitions. The organization's catalog audit events
  are exactly the three D9 rows, which also shows that the four failed calls wrote nothing.
- **Blueprints.** The editor lists the two system blueprints and posts a copy of the data-processing graph as
  `acme.pipeline`, which gives version 1 with the same graph. A body with the key `Acme Pipeline` and an unknown
  definition key on the first node gives 422 `blueprint_invalid` with `invalid_key` at `/key` and
  `unknown_definition` at `/graph/nodes/0/definition`. The list and the audit log show only the valid blueprint.
- **Version conflict.** The test needs an owner connection beside the fixture. It calls `TestDatabase::reset()`
  once to keep the owner pool, and `AuthFixture::registered` then resets the database again. In an open owner
  transaction it inserts `acme.thing` version 1. The request computes version 1 as well, because it cannot see
  the uncommitted row, and then waits on the row's unique key. The test polls `pg_locks` until a transaction lock
  is not granted, commits the owner transaction and expects 409 `node_definition_version_conflict` with no audit
  row. A retry gets version 2. `tokio::select!` makes the test fail at once if the request returns without
  waiting, instead of polling forever.

Create `crates/api/tests/catalog_http.rs`:

```rust
mod support;

use std::pin::pin;

use axum::{body::to_bytes, http::StatusCode, response::Response};
use corqty_test_support::TestDatabase;
use serde_json::{Value, json};
use serial_test::serial;
use support::AuthFixture;
use uuid::Uuid;

const SYSTEM_DEFINITION_ID: &str = "00000000-0000-7000-8000-000000000001";

fn definition(key: &str) -> Value {
    json!({
        "key": key,
        "displayName": "Thing",
        "category": "Testing",
        "inputs": [{ "name": "input", "dataType": "core.any", "schema": {} }],
        "outputs": [{ "name": "result", "dataType": "core.any", "schema": {} }],
        "configSchema": {
            "type": "object",
            "properties": { "mode": { "type": "string", "enum": ["fixed", "echo"] } },
            "required": ["mode"],
        },
        "uiSchema": { "mode": { "widget": "select" } },
        "defaults": { "mode": "fixed" },
        "executor": { "key": "mock", "version": "1.0.0" },
    })
}

/// Asserts the status and returns the JSON body.
async fn body(response: Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// The id of a created record and its JSON without the id and the creation time.
async fn created(response: Response) -> (String, Value) {
    let mut record = body(response, StatusCode::CREATED).await;
    let object = record.as_object_mut().unwrap();
    assert!(object.remove("createdAt").unwrap().is_string());
    let id = object.remove("id").unwrap().as_str().unwrap().to_owned();
    (id, record)
}

/// The (code, pointer) pairs of a 422 problem with the given code.
async fn diagnostics(response: Response, code: &str) -> Value {
    let problem = body(response, StatusCode::UNPROCESSABLE_ENTITY).await;
    assert_eq!(problem["code"], code);
    problem["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| json!([diagnostic["code"], diagnostic["pointer"]]))
        .collect()
}

/// Each item of an `{items}` body as an array of the given fields.
fn items(listed: &Value, fields: &[&str]) -> Vec<Value> {
    listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| fields.iter().map(|field| item[field].clone()).collect())
        .collect()
}

/// The organization's catalog audit events, newest first, without their ids and times.
async fn catalog_events(member: &AuthFixture, organization_id: Uuid) -> Vec<Value> {
    let path = format!("/api/v1/orgs/{organization_id}/audit-events");
    let events = body(member.get(&path).await, StatusCode::OK).await;
    let fields = [
        "actorUserId",
        "action",
        "resourceType",
        "resourceId",
        "metadata",
    ];
    items(&events, &fields)
        .into_iter()
        .filter(|event| event[2] == "node_definition" || event[2] == "blueprint")
        .collect()
}

/// Registers editor@example.com and adds it to the admin's organization as an editor.
async fn editor_of(admin: &AuthFixture) -> AuthFixture {
    let invitations = format!("/api/v1/orgs/{}/invitations", admin.organization_id);
    let invitation = body(
        admin
            .post_json(
                &invitations,
                r#"{"email":"editor@example.com","role":"editor"}"#,
            )
            .await,
        StatusCode::CREATED,
    )
    .await;
    let editor = admin
        .register_another("editor@example.com", "Personal")
        .await;
    let accept = json!({ "token": invitation["token"] }).to_string();
    let accepted = editor
        .post_json("/api/v1/invitations/accept", &accept)
        .await;
    assert_eq!(accepted.status(), StatusCode::OK);
    editor
}

#[tokio::test]
#[serial]
async fn editors_add_and_deprecate_node_definition_versions() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let editor = editor_of(&admin).await;
    let organization_id = admin.organization_id;
    let definitions = format!("/api/v1/orgs/{organization_id}/node-definitions");
    let thing = definition("acme.thing").to_string();

    let mut ids = Vec::new();
    for version in [1, 2] {
        let (id, record) = created(editor.post_json(&definitions, &thing).await).await;
        assert_eq!(
            record,
            json!({
                "scope": "organization",
                "key": "acme.thing",
                "version": version,
                "displayName": "Thing",
                "description": "",
                "category": "Testing",
                "inputs": [{
                    "name": "input",
                    "dataType": "core.any",
                    "schema": {},
                    "required": false,
                    "description": "",
                }],
                "outputs": [{
                    "name": "result",
                    "dataType": "core.any",
                    "schema": {},
                    "required": false,
                    "description": "",
                }],
                "configSchema": {
                    "type": "object",
                    "properties": { "mode": { "type": "string", "enum": ["fixed", "echo"] } },
                    "required": ["mode"],
                },
                "uiSchema": { "mode": { "widget": "select", "label": null, "help": null } },
                "defaults": { "mode": "fixed" },
                "executor": { "key": "mock", "version": "1.0.0" },
                "deprecated": false,
            })
        );
        ids.push(id);
    }

    let reserved = definition("corqty.custom").to_string();
    assert_eq!(
        body(
            editor.post_json(&definitions, &reserved).await,
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await,
        json!({
            "type": "about:blank",
            "title": "Validation failed",
            "status": 422,
            "code": "node_definition_invalid",
            "diagnostics": [{
                "code": "reserved_key",
                "pointer": "/key",
                "message": "Keys starting with \"corqty.\" belong to the system catalog.",
            }],
        })
    );
    let mut dropdown = definition("acme.thing");
    dropdown["uiSchema"]["mode"]["widget"] = json!("dropdown");
    assert_eq!(
        diagnostics(
            editor.post_json(&definitions, &dropdown.to_string()).await,
            "node_definition_invalid",
        )
        .await,
        json!([["unknown_widget", "/uiSchema/mode/widget"]])
    );

    let deprecate = format!("{definitions}/{}/deprecate", ids[0]);
    let deprecated = body(editor.post_json(&deprecate, "").await, StatusCode::OK).await;
    assert_eq!(deprecated["id"], ids[0]);
    assert_eq!(deprecated["version"], 1);
    assert_eq!(deprecated["deprecated"], true);

    let outsider = admin
        .register_another("outsider@example.com", "Outside")
        .await;
    let foreign = format!("/api/v1/orgs/{}/node-definitions", outsider.organization_id);
    let (foreign_id, _) = created(outsider.post_json(&foreign, &thing).await).await;
    for id in [SYSTEM_DEFINITION_ID, foreign_id.as_str()] {
        let response = editor
            .post_json(&format!("{definitions}/{id}/deprecate"), "")
            .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{id}");
    }

    let listed = body(editor.get(&definitions).await, StatusCode::OK).await;
    assert_eq!(
        items(&listed, &["scope", "key", "version", "deprecated"]),
        [
            json!(["organization", "acme.thing", 1, true]),
            json!(["organization", "acme.thing", 2, false]),
            json!(["system", "corqty.mock.normalize-payload", 1, false]),
            json!(["system", "corqty.mock.produce-summary", 1, false]),
            json!(["system", "corqty.mock.task", 1, false]),
            json!(["system", "corqty.simulated.create-rds", 1, false]),
            json!(["system", "corqty.simulated.create-vpc", 1, false]),
            json!(["system", "corqty.simulated.deploy-service", 1, false]),
        ]
    );

    let event = |action: &str, id: &str, version: i32| {
        json!([
            editor.user_id,
            action,
            "node_definition",
            id,
            { "key": "acme.thing", "version": version },
        ])
    };
    assert_eq!(
        catalog_events(&editor, organization_id).await,
        [
            event("node_definition.deprecated", &ids[0], 1),
            event("node_definition.created", &ids[1], 2),
            event("node_definition.created", &ids[0], 1),
        ]
    );
}

#[tokio::test]
#[serial]
async fn editors_add_blueprints_whose_graphs_pass_publish_validation() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let editor = editor_of(&admin).await;
    let organization_id = admin.organization_id;
    let blueprints = format!("/api/v1/orgs/{organization_id}/blueprints");

    let system = body(editor.get(&blueprints).await, StatusCode::OK).await;
    assert_eq!(
        items(&system, &["scope", "key"]),
        [
            json!(["system", "corqty.data-processing"]),
            json!(["system", "corqty.simulated-infrastructure"]),
        ]
    );
    let graph = system["items"][0]["graph"].clone();

    let pipeline = json!({
        "key": "acme.pipeline",
        "displayName": "Pipeline",
        "description": "Normalizes and summarizes a payload.",
        "graph": graph,
    });
    let (id, record) = created(editor.post_json(&blueprints, &pipeline.to_string()).await).await;
    assert_eq!(
        record,
        json!({
            "scope": "organization",
            "key": "acme.pipeline",
            "version": 1,
            "displayName": "Pipeline",
            "description": "Normalizes and summarizes a payload.",
            "graph": graph,
        })
    );

    let mut invalid = pipeline.clone();
    invalid["key"] = json!("Acme Pipeline");
    invalid["graph"]["nodes"][0]["definition"]["key"] = json!("acme.missing");
    assert_eq!(
        diagnostics(
            editor.post_json(&blueprints, &invalid.to_string()).await,
            "blueprint_invalid",
        )
        .await,
        json!([
            ["invalid_key", "/key"],
            ["unknown_definition", "/graph/nodes/0/definition"],
        ])
    );

    let listed = body(editor.get(&blueprints).await, StatusCode::OK).await;
    assert_eq!(
        items(&listed, &["scope", "key", "version"]),
        [
            json!(["organization", "acme.pipeline", 1]),
            json!(["system", "corqty.data-processing", 1]),
            json!(["system", "corqty.simulated-infrastructure", 1]),
        ]
    );
    assert_eq!(
        catalog_events(&editor, organization_id).await,
        [json!([
            editor.user_id,
            "blueprint.created",
            "blueprint",
            id,
            { "key": "acme.pipeline", "version": 1 },
        ])]
    );
}

#[tokio::test]
#[serial]
async fn a_concurrent_version_of_the_same_key_is_a_conflict() {
    // `registered` resets the database again; this pool only has to stay connected.
    let owner = TestDatabase::reset().await.owner;
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let definitions = format!("/api/v1/orgs/{}/node-definitions", admin.organization_id);
    let thing = definition("acme.thing").to_string();
    let mut other = owner.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO node_definitions(organization_id, scope, key, version, display_name, category, spec)
         VALUES ($1, 'organization', 'acme.thing', 1, 'Thing', 'Testing', '{}')",
    )
    .bind(admin.organization_id)
    .execute(&mut *other)
    .await
    .unwrap();

    // The request also computes version 1, then waits on the uncommitted row's unique key.
    let mut request = pin!(admin.post_json(&definitions, &thing));
    let waiting =
        "SELECT EXISTS (SELECT 1 FROM pg_locks WHERE locktype = 'transactionid' AND NOT granted)";
    tokio::select! {
        response = &mut request => panic!("the request did not wait: {}", response.status()),
        () = async {
            while !sqlx::query_scalar::<_, bool>(waiting).fetch_one(&owner).await.unwrap() {}
        } => other.commit().await.unwrap(),
    }
    let conflict = body(request.await, StatusCode::CONFLICT).await;
    assert_eq!(conflict["code"], "node_definition_version_conflict");
    assert_eq!(
        catalog_events(&admin, admin.organization_id).await,
        Vec::<Value>::new()
    );

    let (_, retried) = created(admin.post_json(&definitions, &thing).await).await;
    assert_eq!(retried["version"], 2);
}
```

Then add the authorization rows. `Matrix::assert_row` expects 200 from every role on a GET, 403 from each role
that lacks the capability on a write, and 404 from the outsider. JSON is extracted before the handler runs, so a
write row needs a body that deserializes even though no holder sends it. The deprecate row uses the id of the
system definition `corqty.mock.task`.

In `crates/api/tests/authorization_matrix.rs`:

Replace:

```rust
        (Method::DELETE, member, "", Capability::ManageMembers),
    ] {
        matrix.assert_row(method, &path, body, capability).await;
    }
}
```

With:

```rust
        (Method::DELETE, member, "", Capability::ManageMembers),
    ] {
        matrix.assert_row(method, &path, body, capability).await;
    }
}

#[tokio::test]
#[serial]
async fn catalog_routes_enforce_capabilities() {
    let matrix = Matrix::new().await;
    let org = format!("/api/v1/orgs/{}", matrix.organization_id);
    let definition = r#"{"key":"acme.thing","displayName":"Thing","category":"Testing",
        "inputs":[],"outputs":[],"configSchema":{"type":"object"},"uiSchema":{},"defaults":{},
        "executor":{"key":"mock","version":"1.0.0"}}"#;
    let blueprint = r#"{"key":"acme.pipeline","displayName":"Pipeline","graph":{
        "schemaVersion":1,"workflowInputSchema":{"type":"object"},"nodes":[],"edges":[],
        "viewport":{"x":0,"y":0,"zoom":1}}}"#;
    for (method, path, body, capability) in [
        (
            Method::GET,
            format!("{org}/node-definitions"),
            "",
            Capability::ViewOrganization,
        ),
        (
            Method::POST,
            format!("{org}/node-definitions"),
            definition,
            Capability::EditWorkflows,
        ),
        (
            Method::POST,
            format!("{org}/node-definitions/00000000-0000-7000-8000-000000000001/deprecate"),
            "",
            Capability::EditWorkflows,
        ),
        (
            Method::GET,
            format!("{org}/blueprints"),
            "",
            Capability::ViewOrganization,
        ),
        (
            Method::POST,
            format!("{org}/blueprints"),
            blueprint,
            Capability::EditWorkflows,
        ),
    ] {
        matrix.assert_row(method, &path, body, capability).await;
    }
}
```

- [ ] **Step 2: Run the tests and verify RED**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-api --no-fail-fast --test catalog_http --test authorization_matrix
```

Expected: both test binaries compile, because the tests only talk HTTP. No catalog route exists yet, so every
request falls through to the 404 fallback:

- In `authorization_matrix`, 2 tests pass. `catalog_routes_enforce_capabilities` fails with
  ``assertion `left == right` failed: admin GET /api/v1/orgs/<organization id>/node-definitions``, left 404 and
  right 200.
- In `catalog_http`, all 3 tests fail. `editors_add_and_deprecate_node_definition_versions` stops in `body` with
  left 404 and right 201, and `editors_add_blueprints_whose_graphs_pass_publish_validation` stops with left 404
  and right 200. `a_concurrent_version_of_the_same_key_is_a_conflict` panics with
  `the request did not wait: 404 Not Found`.

- [ ] **Step 3: Add the catalog handlers**

Create `crates/api/src/catalog.rs`:

```rust
use axum::{Extension, Json, extract::State, http::StatusCode};
use corqty_domain::{
    catalog::{NewNodeDefinition, validate_blueprint_metadata, validate_new_definition},
    diagnostic::Diagnostic,
    graph::GraphSnapshot,
    publish::validate_graph,
    rbac::Capability,
};
use corqty_persistence::{
    catalog::{self, BlueprintRecord, NodeDefinitionRecord},
    models::SessionRecord,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    authz::authorize,
    error::ApiError,
    extract::{ApiJson, ApiPath},
    state::AppState,
};

/// A list response: `{"items": [...]}`.
#[derive(Serialize)]
pub struct Items<T> {
    pub items: Vec<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewBlueprint {
    key: String,
    display_name: String,
    #[serde(default)]
    description: String,
    graph: GraphSnapshot,
}

pub async fn list_node_definitions(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(organization_id): ApiPath<Uuid>,
) -> Result<Json<Items<NodeDefinitionRecord>>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let items = catalog::list_node_definitions(&mut access.tx).await?;
    access.commit().await?;
    Ok(Json(Items { items }))
}

pub async fn create_node_definition(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(organization_id): ApiPath<Uuid>,
    ApiJson(definition): ApiJson<NewNodeDefinition>,
) -> Result<(StatusCode, Json<NodeDefinitionRecord>), ApiError> {
    let mut access =
        authorize(&state, &session, organization_id, Capability::EditWorkflows).await?;
    let diagnostics = validate_new_definition(&definition);
    if !diagnostics.is_empty() {
        return Err(ApiError::Invalid {
            code: "node_definition_invalid",
            diagnostics,
        });
    }
    let record = catalog::insert_node_definition(
        &mut access.tx,
        organization_id,
        access.user_id,
        &definition,
    )
    .await
    .map_err(map_version_conflict)?;
    access.commit().await?;
    Ok((StatusCode::CREATED, Json(record)))
}

/// Only the organization's own definitions can be deprecated: system and foreign ids are 404.
pub async fn deprecate_node_definition(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, definition_id)): ApiPath<(Uuid, Uuid)>,
) -> Result<Json<NodeDefinitionRecord>, ApiError> {
    let mut access =
        authorize(&state, &session, organization_id, Capability::EditWorkflows).await?;
    let record = catalog::deprecate_node_definition(
        &mut access.tx,
        organization_id,
        access.user_id,
        definition_id,
    )
    .await?
    .ok_or(ApiError::NotFound)?;
    access.commit().await?;
    Ok(Json(record))
}

pub async fn list_blueprints(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(organization_id): ApiPath<Uuid>,
) -> Result<Json<Items<BlueprintRecord>>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let items = catalog::list_blueprints(&mut access.tx).await?;
    access.commit().await?;
    Ok(Json(Items { items }))
}

/// A blueprint's graph must pass the same validation as a publish, against the definitions
/// the organization can use.
pub async fn create_blueprint(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(organization_id): ApiPath<Uuid>,
    ApiJson(blueprint): ApiJson<NewBlueprint>,
) -> Result<(StatusCode, Json<BlueprintRecord>), ApiError> {
    let mut access =
        authorize(&state, &session, organization_id, Capability::EditWorkflows).await?;
    let mut diagnostics = validate_blueprint_metadata(
        &blueprint.key,
        &blueprint.display_name,
        &blueprint.description,
    );
    let definitions = catalog::definitions_map(&mut access.tx).await?;
    diagnostics.extend(
        validate_graph(&blueprint.graph, &definitions)
            .into_iter()
            .map(|diagnostic| Diagnostic {
                pointer: format!("/graph{}", diagnostic.pointer),
                ..diagnostic
            }),
    );
    if !diagnostics.is_empty() {
        return Err(ApiError::Invalid {
            code: "blueprint_invalid",
            diagnostics,
        });
    }
    let record = catalog::insert_blueprint(
        &mut access.tx,
        organization_id,
        access.user_id,
        &blueprint.key,
        &blueprint.display_name,
        &blueprint.description,
        &blueprint.graph,
    )
    .await?;
    access.commit().await?;
    Ok((StatusCode::CREATED, Json(record)))
}

/// Two requests computed the same next version; the later insert fails on the unique key.
fn map_version_conflict(error: sqlx::Error) -> ApiError {
    if error
        .as_database_error()
        .is_some_and(|database| database.constraint() == Some("node_definitions_key_version"))
    {
        ApiError::Conflict("node_definition_version_conflict")
    } else {
        error.into()
    }
}
```

Replace the whole of `crates/api/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod audit;
pub mod auth;
pub mod authz;
pub mod catalog;
pub mod config;
pub mod error;
pub mod extract;
pub mod organizations;
pub mod router;
pub mod state;
```

- [ ] **Step 4: Route the catalog endpoints**

All five routes go on the protected router, so they need a session, and the POST routes also need the CSRF
token.

Replace the whole of `crates/api/src/router.rs`:

```rust
use axum::{
    Router,
    http::StatusCode,
    middleware,
    routing::{get, post},
};
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::{
    audit,
    auth::{handlers, middleware as auth_middleware},
    catalog,
    error::ApiError,
    organizations,
    state::AppState,
};

pub fn build_router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/auth/me", get(handlers::me))
        .route("/auth/logout", post(handlers::logout))
        .route("/organizations", post(organizations::handlers::create))
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
        .route("/orgs/{organization_id}/audit-events", get(audit::list))
        .route(
            "/orgs/{organization_id}/members/{target_user_id}",
            axum::routing::patch(organizations::members::change_role)
                .delete(organizations::members::remove_member),
        )
        .route(
            "/orgs/{organization_id}/node-definitions",
            get(catalog::list_node_definitions).post(catalog::create_node_definition),
        )
        .route(
            "/orgs/{organization_id}/node-definitions/{definition_id}/deprecate",
            post(catalog::deprecate_node_definition),
        )
        .route(
            "/orgs/{organization_id}/blueprints",
            get(catalog::list_blueprints).post(catalog::create_blueprint),
        )
        .route(
            "/auth/password",
            axum::routing::patch(handlers::change_password),
        )
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
        // `method_not_allowed_fallback` only reaches routes added before it.
        .fallback(|| async { ApiError::NotFound })
        .method_not_allowed_fallback(|| async { ApiError::MethodNotAllowed })
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
}
```

- [ ] **Step 5: Verify GREEN**

```text
cargo test -p corqty-api --no-fail-fast --test catalog_http --test authorization_matrix
```

Expected: `authorization_matrix` 3 and `catalog_http` 3 tests pass.

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

Expected: every suite passes. corqty-api: unit 4, `audit_http` 1, `auth_http` 3, `authorization_matrix` 3,
`catalog_http` 3, `members_http` 3, `organizations_http` 1, `problems_http` 2. corqty-domain: `account_and_rbac` 6,
`catalog_rules` 5, `graph_rules` 8, `publish_rules` 9. corqty-persistence: `auth_store` 2, `catalog_guards` 3,
`final_admin` 2, `migrations` 1, `tenant_isolation` 1, `test_reset` 1, `workflow_catalog` 4. 62 tests in total.

- [ ] **Step 7: Commit**

```text
git add crates/api
git commit -m "feat: expose node definition and blueprint APIs"
```

Expected: `git show --stat HEAD` lists exactly the 5 files above (587 insertions).

## Task 13: Expose workflow and draft APIs with revision preconditions

**Files:**

- Create: `crates/api/src/workflows.rs`
- Modify: `crates/api/src/lib.rs`
- Modify: `crates/api/src/router.rs`
- Modify: `crates/api/tests/authorization_matrix.rs`
- Test: `crates/api/tests/workflows_http.rs`

This task puts the Task 11 workflow repositories behind six routes under `/api/v1/orgs/{organization_id}`:

| method + path | capability | success | errors |
|---|---|---|---|
| GET `/workflows` | `ViewOrganization` | 200 `{items}`, most recently changed first | 404 |
| POST `/workflows` `{name, blueprintId?}` | `EditWorkflows` | 201 workflow | 422 `invalid_workflow_name`, 404 unknown blueprint |
| GET `/workflows/{workflow_id}` | `ViewOrganization` | 200 workflow | 404 |
| PUT `/workflows/{workflow_id}/draft` `{revision, graph}` | `EditWorkflows` | 200 `{revision}` | 409 `draft_revision_conflict`, 422 `graph_invalid`, 404 |
| POST `/workflows/{workflow_id}/validate` `{graph}` | `PublishWorkflows` | 200 `{valid, diagnostics}` | 404 |
| GET `/workflows/{workflow_id}/versions` | `ViewOrganization` | 200 `{items}`, newest first | 404 |

The handlers in the new `crates/api/src/workflows.rs` follow the Task 1 pattern, like the catalog handlers in Task
12. `authorize` opens the tenant transaction and checks the capability, so an outsider gets 404 and a member
without the capability gets 403 before anything else runs. The handler then validates, calls the repository with
`&mut access.tx`, and finishes with `access.commit()`. An early `return Err(...)` drops the transaction, which
rolls it back, so a failed call never leaves a change or an audit event behind. Row-level security only shows the
current organization's workflows, so another organization's workflow id looks exactly like an unknown one: 404.

- `POST /workflows` trims the name, which must then have 1 to 100 characters, or the response is 422
  `invalid_workflow_name` (no diagnostics, like `invalid_organization_name`). The slug is
  `slugify(name, "workflow", Uuid::new_v4())`. Without `blueprintId` the draft starts as
  `GraphSnapshot::empty()`. With one, the handler loads the blueprint through the tenant transaction, so only
  system blueprints and the organization's own are found and anything else is 404. Its graph is copied with a
  fresh UUID for every node and edge, and each edge's endpoints are renamed with their nodes (D14), so the new
  workflow never shares an id with its blueprint. `insert_workflow` stores the draft at revision 1 and writes the
  D9 audit row `workflow.created` with `{}` or `{"blueprintId": "<uuid>"}`.
- `PUT /workflows/{workflow_id}/draft` only checks the size limits (D13). A graph over 500 nodes or 1000 edges
  gives 422 `graph_invalid` with `too_many_nodes` at `/nodes` or `too_many_edges` at `/edges`. Any graph that
  deserializes and fits is stored, because full validation waits for validate and publish. `save_draft` updates
  the draft only while its revision still equals the request's `revision`, and returns the new revision. When it
  returns `None`, `workflow_exists` tells a stale revision (409 `draft_revision_conflict`) from a workflow that is
  not visible (404). Two saves at the same revision cannot both win: the second UPDATE waits on the first one's
  row lock, then re-checks `revision = $2` against the committed row, matches nothing and gets 409.
  Each successful save writes `workflow.draft_saved` with `{"revision": N}` and moves the workflow to the top of
  the list.
- `POST /workflows/{workflow_id}/validate` runs the publish validation (`validate_graph` against
  `definitions_map`) on the graph in the request body and writes nothing. The diagnostics point into that graph.
  It needs `PublishWorkflows`, because it exists to check a graph before publishing it.
- `GET /workflows/{workflow_id}/versions` checks that the workflow exists and lists its versions. The list stays
  empty until Task 14 adds publishing, together with `GET /workflows/{workflow_id}/versions/{version}`.

The list routes reuse Task 12's `Items<T>` for their `{"items": [...]}` bodies. The PUT and POST routes need the
CSRF token like every other write on the protected router.

- [ ] **Step 1: Write the failing tests**

`crates/api/tests/workflows_http.rs` has four tests. Each helper asserts the response status before reading the
body:

- **Create and apply.** The admin invites an editor through the API. The editor creates `  Nightly report  ` and
  gets the whole workflow JSON back: the trimmed name, a `nightly-report-` slug with an 8-character suffix, no
  published version and an empty draft graph at revision 1. A GET of the workflow returns the same JSON. A blank
  name and a 101-character name give 422 `invalid_workflow_name`, and a random `blueprintId` gives 404. Applying
  the system blueprint `corqty.simulated-infrastructure` gives node and edge ids that are UUIDs and disjoint from
  the blueprint's. The test then replaces every node id with its definition key in both graphs and drops the
  edge ids. The two graphs must then be equal, which also shows that every edge still names a node of the new
  graph. The workflow audit events are exactly the two D9 `workflow.created` rows.
- **Draft revisions.** Two workflows are listed newest first. Saving a graph with a workflow input and the
  viewport `{x: 12.5, y: -40, zoom: 1.5}` at revisions 1 and 2 returns revisions 2 and 3, and a GET returns the
  graph unchanged, viewport included. The saved workflow moves to the top of the list. Saving at the stale
  revision 2 gives 409 `draft_revision_conflict`. A graph with 501 nodes gives the full 422 `graph_invalid`
  problem with `too_many_nodes` at `/nodes`. A PUT with the session cookie but no `x-csrf-token` header gives
  403, and a PUT to a random workflow id gives 404. Afterwards the draft is still at revision 3, and the audit
  log holds only the two `workflow.created` and two `workflow.draft_saved` rows.
- **Validation.** The editor applies the infrastructure blueprint and validates its graph: `{"valid": true,
  "diagnostics": []}`. With an unknown definition key on the first node and `maxAttempts` 9 on the second, the
  response is 200 with `valid: false`, `unknown_definition` at `/nodes/0/definition` and `invalid_retry` at
  `/nodes/1/retry/maxAttempts`. The workflow JSON, including its revision and `updatedAt`, is unchanged, and
  there is still only one workflow audit event.
- **Another organization.** The admin creates a second organization, Beta, with a workflow and an organization
  blueprint. Through Acme's URLs the same admin gets 404 for Beta's workflow on GET, PUT draft, validate and
  versions, and for applying Beta's blueprint. Beta's workflow is unchanged afterwards. Acme's list then holds
  only Acme's new workflow, and the test compares that list item as a whole, which pins the summary JSON. Its
  versions list is `{"items": []}`.

Create `crates/api/tests/workflows_http.rs`:

```rust
mod support;

use std::collections::{HashMap, HashSet};

use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use serde_json::{Value, json};
use serial_test::serial;
use support::AuthFixture;
use tower::ServiceExt;
use uuid::Uuid;

const INFRASTRUCTURE_BLUEPRINT_ID: &str = "00000000-0000-7000-8000-000000000101";

fn empty_graph() -> Value {
    json!({
        "schemaVersion": 1,
        "workflowInputSchema": { "type": "object", "properties": {} },
        "nodes": [],
        "edges": [],
        "viewport": { "x": 0.0, "y": 0.0, "zoom": 1.0 },
    })
}

/// A mock task node; only the number of nodes matters to the limits.
fn node(id: &str) -> Value {
    json!({
        "id": id,
        "definition": { "key": "corqty.mock.task", "version": 1 },
        "displayName": "Mock task",
        "position": { "x": 0, "y": 0 },
        "config": {},
        "inputs": {},
        "retry": { "maxAttempts": 1 },
        "timeoutSeconds": 900,
    })
}

/// Asserts the status and returns the JSON body.
async fn body(response: Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// The (code, pointer) pairs of a diagnostics array.
fn pairs(diagnostics: &Value) -> Vec<Value> {
    diagnostics
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| json!([diagnostic["code"], diagnostic["pointer"]]))
        .collect()
}

/// The name and draft revision of each listed workflow, in list order.
async fn listed(member: &AuthFixture, workflows: &str) -> Vec<Value> {
    let listed = body(member.get(workflows).await, StatusCode::OK).await;
    listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| json!([item["name"], item["draftRevision"]]))
        .collect()
}

/// The organization's workflow audit events, newest first, as
/// [actorUserId, action, resourceId, metadata].
async fn workflow_events(member: &AuthFixture, organization_id: Uuid) -> Vec<Value> {
    let path = format!("/api/v1/orgs/{organization_id}/audit-events");
    let events = body(member.get(&path).await, StatusCode::OK).await;
    events["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["resourceType"] == "workflow")
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

/// The node and edge ids of a graph.
fn ids(graph: &Value) -> HashSet<String> {
    ["nodes", "edges"]
        .iter()
        .flat_map(|list| graph[*list].as_array().unwrap())
        .map(|item| item["id"].as_str().unwrap().to_owned())
        .collect()
}

/// The graph with each node id replaced by its definition key and without edge ids, so
/// graphs that differ only in their ids compare equal. Panics on an edge to a missing node.
fn by_definition_key(graph: &Value) -> Value {
    let mut graph = graph.clone();
    let keys: HashMap<String, Value> = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| {
            let id = node["id"].as_str().unwrap().to_owned();
            (id, node["definition"]["key"].clone())
        })
        .collect();
    for node in graph["nodes"].as_array_mut().unwrap() {
        node["id"] = node["definition"]["key"].clone();
    }
    for edge in graph["edges"].as_array_mut().unwrap() {
        edge.as_object_mut().unwrap().remove("id");
        for end in ["source", "target"] {
            edge[end]["nodeId"] = keys[edge[end]["nodeId"].as_str().unwrap()].clone();
        }
    }
    graph
}

/// Registers editor@example.com and adds it to the admin's organization as an editor.
async fn editor_of(admin: &AuthFixture) -> AuthFixture {
    let invitations = format!("/api/v1/orgs/{}/invitations", admin.organization_id);
    let invitation = body(
        admin
            .post_json(
                &invitations,
                r#"{"email":"editor@example.com","role":"editor"}"#,
            )
            .await,
        StatusCode::CREATED,
    )
    .await;
    let editor = admin
        .register_another("editor@example.com", "Personal")
        .await;
    let accept = json!({ "token": invitation["token"] }).to_string();
    let accepted = editor
        .post_json("/api/v1/invitations/accept", &accept)
        .await;
    assert_eq!(accepted.status(), StatusCode::OK);
    editor
}

#[tokio::test]
#[serial]
async fn editors_create_empty_workflows_and_apply_blueprints() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let editor = editor_of(&admin).await;
    let organization_id = admin.organization_id;
    let workflows = format!("/api/v1/orgs/{organization_id}/workflows");

    let empty = body(
        editor
            .post_json(&workflows, r#"{"name":"  Nightly report  "}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let empty_id = empty["id"].as_str().unwrap();
    let slug = empty["slug"].as_str().unwrap();
    // The name's words, then an 8-character random suffix.
    assert!(slug.starts_with("nightly-report-"), "{slug}");
    assert_eq!(slug.len(), "nightly-report-".len() + 8, "{slug}");
    assert!(empty["draft"]["updatedAt"].is_string());
    assert_eq!(
        empty,
        json!({
            "id": empty_id,
            "name": "Nightly report",
            "slug": slug,
            "publishedVersion": null,
            "publishedDraftRevision": null,
            "draft": {
                "revision": 1,
                "graph": empty_graph(),
                "updatedAt": empty["draft"]["updatedAt"],
            },
        })
    );
    let path = format!("{workflows}/{empty_id}");
    assert_eq!(body(editor.get(&path).await, StatusCode::OK).await, empty);

    for name in ["   ".to_owned(), "x".repeat(101)] {
        let request = json!({ "name": name }).to_string();
        let problem = body(
            editor.post_json(&workflows, &request).await,
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
        assert_eq!(problem["code"], "invalid_workflow_name");
    }
    let unknown = json!({ "name": "Copy", "blueprintId": Uuid::new_v4() }).to_string();
    let response = editor.post_json(&workflows, &unknown).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let blueprints = format!("/api/v1/orgs/{organization_id}/blueprints");
    let blueprint = body(editor.get(&blueprints).await, StatusCode::OK).await["items"][1].clone();
    assert_eq!(blueprint["id"], INFRASTRUCTURE_BLUEPRINT_ID);
    let request = json!({ "name": "Provision staging", "blueprintId": blueprint["id"] });
    let applied = body(
        editor.post_json(&workflows, &request.to_string()).await,
        StatusCode::CREATED,
    )
    .await;
    let graph = &applied["draft"]["graph"];
    // Fresh UUIDs that the blueprint does not use, wired up exactly like the blueprint.
    let (old, new) = (ids(&blueprint["graph"]), ids(graph));
    assert_eq!(new.len(), old.len());
    for id in &new {
        assert!(Uuid::parse_str(id).is_ok(), "{id}");
        assert!(!old.contains(id), "{id}");
    }
    assert_eq!(
        by_definition_key(graph),
        by_definition_key(&blueprint["graph"])
    );

    assert_eq!(
        workflow_events(&editor, organization_id).await,
        [
            json!([
                editor.user_id,
                "workflow.created",
                applied["id"],
                { "blueprintId": INFRASTRUCTURE_BLUEPRINT_ID },
            ]),
            json!([editor.user_id, "workflow.created", empty_id, {}]),
        ]
    );
}

#[tokio::test]
#[serial]
async fn drafts_save_only_at_the_current_revision() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let workflows = format!("/api/v1/orgs/{}/workflows", admin.organization_id);
    let mut created = Vec::new();
    for name in ["First", "Second"] {
        let request = json!({ "name": name }).to_string();
        let workflow = body(
            admin.post_json(&workflows, &request).await,
            StatusCode::CREATED,
        )
        .await;
        created.push(workflow["id"].as_str().unwrap().to_owned());
    }
    assert_eq!(
        listed(&admin, &workflows).await,
        [json!(["Second", 1]), json!(["First", 1])]
    );

    let first = format!("{workflows}/{}", created[0]);
    let draft = format!("{first}/draft");
    let mut graph = empty_graph();
    graph["workflowInputSchema"]["properties"]["region"] = json!({ "type": "string" });
    graph["viewport"] = json!({ "x": 12.5, "y": -40.0, "zoom": 1.5 });
    for revision in [1, 2] {
        let request = json!({ "revision": revision, "graph": graph }).to_string();
        let saved = body(
            admin.request_json(Method::PUT, &draft, &request).await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(saved, json!({ "revision": revision + 1 }));
    }
    let stored = body(admin.get(&first).await, StatusCode::OK).await;
    assert_eq!(stored["draft"]["revision"], 3);
    assert_eq!(stored["draft"]["graph"], graph);
    // Saving moved the first workflow to the top.
    assert_eq!(
        listed(&admin, &workflows).await,
        [json!(["First", 3]), json!(["Second", 1])]
    );

    let stale = json!({ "revision": 2, "graph": graph }).to_string();
    let conflict = body(
        admin.request_json(Method::PUT, &draft, &stale).await,
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(conflict["code"], "draft_revision_conflict");

    let mut large = graph.clone();
    large["nodes"] = (0..501).map(|index| node(&format!("n{index}"))).collect();
    let request = json!({ "revision": 3, "graph": large }).to_string();
    assert_eq!(
        body(
            admin.request_json(Method::PUT, &draft, &request).await,
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await,
        json!({
            "type": "about:blank",
            "title": "Validation failed",
            "status": 422,
            "code": "graph_invalid",
            "diagnostics": [{
                "code": "too_many_nodes",
                "pointer": "/nodes",
                "message": "A graph can have at most 500 nodes.",
            }],
        })
    );

    // The session cookie alone is not enough to write.
    let current = json!({ "revision": 3, "graph": graph }).to_string();
    let response = admin
        .app
        .clone()
        .oneshot(
            Request::put(&draft)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, admin.cookie_header())
                .body(Body::from(current.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let unknown = format!("{workflows}/{}/draft", Uuid::new_v4());
    let response = admin.request_json(Method::PUT, &unknown, &current).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // Only the two successful saves changed the draft or wrote an audit event.
    let stored = body(admin.get(&first).await, StatusCode::OK).await;
    assert_eq!(stored["draft"]["revision"], 3);
    assert_eq!(
        workflow_events(&admin, admin.organization_id).await,
        [
            json!([admin.user_id, "workflow.draft_saved", created[0], { "revision": 3 }]),
            json!([admin.user_id, "workflow.draft_saved", created[0], { "revision": 2 }]),
            json!([admin.user_id, "workflow.created", created[1], {}]),
            json!([admin.user_id, "workflow.created", created[0], {}]),
        ]
    );
}

#[tokio::test]
#[serial]
async fn validation_reports_diagnostics_without_saving() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let editor = editor_of(&admin).await;
    let workflows = format!("/api/v1/orgs/{}/workflows", admin.organization_id);
    let request = json!({
        "name": "Provision staging",
        "blueprintId": INFRASTRUCTURE_BLUEPRINT_ID,
    });
    let applied = body(
        editor.post_json(&workflows, &request.to_string()).await,
        StatusCode::CREATED,
    )
    .await;
    let path = format!("{workflows}/{}", applied["id"].as_str().unwrap());
    let validate = format!("{path}/validate");

    let graph = applied["draft"]["graph"].clone();
    let request = json!({ "graph": graph }).to_string();
    assert_eq!(
        body(editor.post_json(&validate, &request).await, StatusCode::OK).await,
        json!({ "valid": true, "diagnostics": [] })
    );

    let mut invalid = graph;
    invalid["nodes"][0]["definition"]["key"] = json!("acme.missing");
    invalid["nodes"][1]["retry"]["maxAttempts"] = json!(9);
    let request = json!({ "graph": invalid }).to_string();
    let report = body(editor.post_json(&validate, &request).await, StatusCode::OK).await;
    assert_eq!(report["valid"], false);
    assert_eq!(
        pairs(&report["diagnostics"]),
        [
            json!(["unknown_definition", "/nodes/0/definition"]),
            json!(["invalid_retry", "/nodes/1/retry/maxAttempts"]),
        ]
    );

    // Validating wrote nothing: the same draft and revision, and no new audit event.
    assert_eq!(body(editor.get(&path).await, StatusCode::OK).await, applied);
    assert_eq!(
        workflow_events(&editor, admin.organization_id).await.len(),
        1
    );
}

#[tokio::test]
#[serial]
async fn workflows_of_another_organization_are_not_found() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let beta = body(
        admin
            .post_json("/api/v1/organizations", r#"{"name":"Beta"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let beta = format!("/api/v1/orgs/{}", beta["organizationId"].as_str().unwrap());
    let foreign = body(
        admin
            .post_json(&format!("{beta}/workflows"), r#"{"name":"Beta flow"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let blueprint = json!({ "key": "beta.empty", "displayName": "Empty", "graph": empty_graph() });
    let foreign_blueprint = body(
        admin
            .post_json(&format!("{beta}/blueprints"), &blueprint.to_string())
            .await,
        StatusCode::CREATED,
    )
    .await;

    // The admin of both organizations asks through Acme for Beta's workflow and blueprint.
    let workflows = format!("/api/v1/orgs/{}/workflows", admin.organization_id);
    let foreign_id = foreign["id"].as_str().unwrap();
    let foreign_path = format!("{workflows}/{foreign_id}");
    let graph = &foreign["draft"]["graph"];
    let copy = json!({ "name": "Copy", "blueprintId": foreign_blueprint["id"] });
    for (method, path, request) in [
        (Method::GET, foreign_path.clone(), String::new()),
        (
            Method::PUT,
            format!("{foreign_path}/draft"),
            json!({ "revision": 1, "graph": graph }).to_string(),
        ),
        (
            Method::POST,
            format!("{foreign_path}/validate"),
            json!({ "graph": graph }).to_string(),
        ),
        (
            Method::GET,
            format!("{foreign_path}/versions"),
            String::new(),
        ),
        (Method::POST, workflows.clone(), copy.to_string()),
    ] {
        let response = admin.request_json(method.clone(), &path, &request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {path}");
    }
    let unchanged = format!("{beta}/workflows/{foreign_id}");
    assert_eq!(
        body(admin.get(&unchanged).await, StatusCode::OK).await,
        foreign
    );

    // Acme lists only its own workflow, which has no versions yet.
    let own = body(
        admin.post_json(&workflows, r#"{"name":"Acme flow"}"#).await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(
        body(admin.get(&workflows).await, StatusCode::OK).await,
        json!({
            "items": [{
                "id": own["id"],
                "name": "Acme flow",
                "slug": own["slug"],
                "draftRevision": 1,
                "publishedVersion": null,
                "publishedDraftRevision": null,
                "updatedAt": own["draft"]["updatedAt"],
            }],
        })
    );
    let versions = format!("{workflows}/{}/versions", own["id"].as_str().unwrap());
    assert_eq!(
        body(admin.get(&versions).await, StatusCode::OK).await,
        json!({ "items": [] })
    );
}
```

Then add the authorization rows in a new test function. `Matrix::assert_row` expects 200 from every role on a GET,
403 from each role that lacks the capability on a write, and 404 from the outsider. The admin first creates a
workflow, so the rows for a single workflow have a real id. JSON is extracted before the handler runs, so the
write rows send bodies that deserialize even though no role holding the capability sends them.

In `crates/api/tests/authorization_matrix.rs`:

Replace:

```rust
            blueprint,
            Capability::EditWorkflows,
        ),
    ] {
        matrix.assert_row(method, &path, body, capability).await;
    }
}
```

With:

```rust
            blueprint,
            Capability::EditWorkflows,
        ),
    ] {
        matrix.assert_row(method, &path, body, capability).await;
    }
}

#[tokio::test]
#[serial]
async fn workflow_routes_enforce_capabilities() {
    let matrix = Matrix::new().await;
    let org = format!("/api/v1/orgs/{}", matrix.organization_id);
    let (_, admin) = &matrix.members[0];
    let created = admin
        .post_json(&format!("{org}/workflows"), r#"{"name":"Matrix"}"#)
        .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let body = to_bytes(created.into_body(), 64 * 1024).await.unwrap();
    let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let workflow = format!("{org}/workflows/{}", created["id"].as_str().unwrap());
    let graph = r#"{"schemaVersion":1,"workflowInputSchema":{"type":"object"},"nodes":[],
        "edges":[],"viewport":{"x":0,"y":0,"zoom":1}}"#;
    let draft = format!(r#"{{"revision":1,"graph":{graph}}}"#);
    let validate = format!(r#"{{"graph":{graph}}}"#);
    for (method, path, body, capability) in [
        (
            Method::GET,
            format!("{org}/workflows"),
            "",
            Capability::ViewOrganization,
        ),
        (
            Method::POST,
            format!("{org}/workflows"),
            r#"{"name":"Other"}"#,
            Capability::EditWorkflows,
        ),
        (
            Method::GET,
            workflow.clone(),
            "",
            Capability::ViewOrganization,
        ),
        (
            Method::PUT,
            format!("{workflow}/draft"),
            &draft,
            Capability::EditWorkflows,
        ),
        (
            Method::POST,
            format!("{workflow}/validate"),
            &validate,
            Capability::PublishWorkflows,
        ),
        (
            Method::GET,
            format!("{workflow}/versions"),
            "",
            Capability::ViewOrganization,
        ),
    ] {
        matrix.assert_row(method, &path, body, capability).await;
    }
}
```

- [ ] **Step 2: Run the tests and verify RED**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-api --no-fail-fast --test workflows_http --test authorization_matrix
```

Expected: both test binaries compile, because the tests only talk HTTP. No workflow route exists yet, so every
workflow request falls through to the 404 fallback:

- In `authorization_matrix`, 3 tests pass. `workflow_routes_enforce_capabilities` fails when the admin creates
  its workflow: ``assertion `left == right` failed``, left 404 and right 201.
- In `workflows_http`, all 4 tests fail in `body` on their first workflow request, with left 404 and right 201.

- [ ] **Step 3: Add the workflow handlers**

Create `crates/api/src/workflows.rs`:

```rust
use std::collections::HashMap;

use axum::{Extension, Json, extract::State, http::StatusCode};
use corqty_domain::{
    graph::{GraphSnapshot, check_limits},
    publish::validate_graph,
    rbac::Capability,
    slug::slugify,
};
use corqty_persistence::{
    catalog,
    models::SessionRecord,
    workflows::{self, VersionSummaryRecord, WorkflowRecord, WorkflowSummaryRecord},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    authz::authorize,
    catalog::Items,
    error::ApiError,
    extract::{ApiJson, ApiPath},
    state::AppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewWorkflow {
    name: String,
    blueprint_id: Option<Uuid>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftSave {
    revision: i64,
    graph: GraphSnapshot,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphBody {
    graph: GraphSnapshot,
}

pub async fn list_workflows(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(organization_id): ApiPath<Uuid>,
) -> Result<Json<Items<WorkflowSummaryRecord>>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let items = workflows::list_workflows(&mut access.tx).await?;
    access.commit().await?;
    Ok(Json(Items { items }))
}

/// Creates an empty workflow, or one whose draft is a copy of a blueprint's graph.
pub async fn create_workflow(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath(organization_id): ApiPath<Uuid>,
    ApiJson(workflow): ApiJson<NewWorkflow>,
) -> Result<(StatusCode, Json<WorkflowRecord>), ApiError> {
    let mut access =
        authorize(&state, &session, organization_id, Capability::EditWorkflows).await?;
    let name = workflow.name.trim();
    if !(1..=100).contains(&name.chars().count()) {
        return Err(ApiError::Validation("invalid_workflow_name"));
    }
    let graph = match workflow.blueprint_id {
        Some(blueprint_id) => {
            let blueprint = catalog::get_blueprint(&mut access.tx, blueprint_id)
                .await?
                .ok_or(ApiError::NotFound)?;
            with_fresh_ids(blueprint.graph)
        }
        None => GraphSnapshot::empty(),
    };
    let slug = slugify(name, "workflow", Uuid::new_v4());
    let record = workflows::insert_workflow(
        &mut access.tx,
        organization_id,
        access.user_id,
        name,
        &slug,
        &graph,
        workflow.blueprint_id,
    )
    .await?;
    access.commit().await?;
    Ok((StatusCode::CREATED, Json(record)))
}

pub async fn get_workflow(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, workflow_id)): ApiPath<(Uuid, Uuid)>,
) -> Result<Json<WorkflowRecord>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let record = workflows::get_workflow(&mut access.tx, workflow_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    access.commit().await?;
    Ok(Json(record))
}

/// Replaces the draft only while it is still at the revision the client last saw.
/// Full validation waits for validate and publish; a save only enforces the size limits.
pub async fn save_draft(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, workflow_id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(draft): ApiJson<DraftSave>,
) -> Result<Json<Value>, ApiError> {
    let mut access =
        authorize(&state, &session, organization_id, Capability::EditWorkflows).await?;
    let diagnostics = check_limits(&draft.graph);
    if !diagnostics.is_empty() {
        return Err(ApiError::Invalid {
            code: "graph_invalid",
            diagnostics,
        });
    }
    let saved = workflows::save_draft(
        &mut access.tx,
        organization_id,
        access.user_id,
        workflow_id,
        draft.revision,
        &draft.graph,
    )
    .await?;
    let Some(revision) = saved else {
        return Err(
            if workflows::workflow_exists(&mut access.tx, workflow_id).await? {
                ApiError::Conflict("draft_revision_conflict")
            } else {
                ApiError::NotFound
            },
        );
    };
    access.commit().await?;
    Ok(Json(json!({ "revision": revision })))
}

/// Checks a graph as a publish would, without storing anything.
pub async fn validate(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, workflow_id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(body): ApiJson<GraphBody>,
) -> Result<Json<Value>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::PublishWorkflows,
    )
    .await?;
    if !workflows::workflow_exists(&mut access.tx, workflow_id).await? {
        return Err(ApiError::NotFound);
    }
    let definitions = catalog::definitions_map(&mut access.tx).await?;
    let diagnostics = validate_graph(&body.graph, &definitions);
    access.commit().await?;
    Ok(Json(json!({
        "valid": diagnostics.is_empty(),
        "diagnostics": diagnostics,
    })))
}

pub async fn list_versions(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, workflow_id)): ApiPath<(Uuid, Uuid)>,
) -> Result<Json<Items<VersionSummaryRecord>>, ApiError> {
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
    let items = workflows::list_versions(&mut access.tx, workflow_id).await?;
    access.commit().await?;
    Ok(Json(Items { items }))
}

/// A blueprint's graph with new UUIDs for its nodes and edges, the edges following
/// their nodes, so a workflow never shares ids with the blueprint it came from.
fn with_fresh_ids(mut graph: GraphSnapshot) -> GraphSnapshot {
    let mut ids = HashMap::new();
    for node in &mut graph.nodes {
        let id = Uuid::new_v4().to_string();
        ids.insert(std::mem::replace(&mut node.id, id.clone()), id);
    }
    for edge in &mut graph.edges {
        edge.id = Uuid::new_v4().to_string();
        for end in [&mut edge.source, &mut edge.target] {
            if let Some(id) = ids.get(&end.node_id) {
                end.node_id.clone_from(id);
            }
        }
    }
    graph
}
```

Replace the whole of `crates/api/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod audit;
pub mod auth;
pub mod authz;
pub mod catalog;
pub mod config;
pub mod error;
pub mod extract;
pub mod organizations;
pub mod router;
pub mod state;
pub mod workflows;
```

- [ ] **Step 4: Route the workflow endpoints**

All six routes go on the protected router, so they need a session, and the PUT and POST routes also need the CSRF
token.

Replace the whole of `crates/api/src/router.rs`:

```rust
use axum::{
    Router,
    http::StatusCode,
    middleware,
    routing::{get, post, put},
};
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::{
    audit,
    auth::{handlers, middleware as auth_middleware},
    catalog,
    error::ApiError,
    organizations,
    state::AppState,
    workflows,
};

pub fn build_router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/auth/me", get(handlers::me))
        .route("/auth/logout", post(handlers::logout))
        .route("/organizations", post(organizations::handlers::create))
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
        .route("/orgs/{organization_id}/audit-events", get(audit::list))
        .route(
            "/orgs/{organization_id}/members/{target_user_id}",
            axum::routing::patch(organizations::members::change_role)
                .delete(organizations::members::remove_member),
        )
        .route(
            "/orgs/{organization_id}/node-definitions",
            get(catalog::list_node_definitions).post(catalog::create_node_definition),
        )
        .route(
            "/orgs/{organization_id}/node-definitions/{definition_id}/deprecate",
            post(catalog::deprecate_node_definition),
        )
        .route(
            "/orgs/{organization_id}/blueprints",
            get(catalog::list_blueprints).post(catalog::create_blueprint),
        )
        .route(
            "/orgs/{organization_id}/workflows",
            get(workflows::list_workflows).post(workflows::create_workflow),
        )
        .route(
            "/orgs/{organization_id}/workflows/{workflow_id}",
            get(workflows::get_workflow),
        )
        .route(
            "/orgs/{organization_id}/workflows/{workflow_id}/draft",
            put(workflows::save_draft),
        )
        .route(
            "/orgs/{organization_id}/workflows/{workflow_id}/validate",
            post(workflows::validate),
        )
        .route(
            "/orgs/{organization_id}/workflows/{workflow_id}/versions",
            get(workflows::list_versions),
        )
        .route(
            "/auth/password",
            axum::routing::patch(handlers::change_password),
        )
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
        // `method_not_allowed_fallback` only reaches routes added before it.
        .fallback(|| async { ApiError::NotFound })
        .method_not_allowed_fallback(|| async { ApiError::MethodNotAllowed })
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
}
```

- [ ] **Step 5: Verify GREEN**

```text
cargo test -p corqty-api --no-fail-fast --test workflows_http --test authorization_matrix
```

Expected: `authorization_matrix` 4 and `workflows_http` 4 tests pass.

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
`catalog_http` 3, `members_http` 3, `organizations_http` 1, `problems_http` 2, `workflows_http` 4. corqty-domain:
`account_and_rbac` 6, `catalog_rules` 5, `graph_rules` 8, `publish_rules` 9. corqty-persistence: `auth_store` 2,
`catalog_guards` 3, `final_admin` 2, `migrations` 1, `tenant_isolation` 1, `test_reset` 1, `workflow_catalog` 4.
67 tests in total.

- [ ] **Step 7: Commit**

```text
git add crates/api
git commit -m "feat: expose workflow and draft APIs with revision preconditions"
```

Expected: `git show --stat HEAD` lists exactly the 5 files above (782 insertions, 1 deletion).

## Task 14: Publish immutable workflow versions atomically

**Files:**

- Modify: `crates/api/src/workflows.rs`
- Modify: `crates/api/src/router.rs`
- Modify: `crates/api/tests/authorization_matrix.rs`
- Modify: `crates/persistence/tests/workflow_catalog.rs`
- Test: `crates/api/tests/publish_http.rs`

This task adds the last two workflow routes under `/api/v1/orgs/{organization_id}`:

| method + path | capability | success | errors |
|---|---|---|---|
| POST `/workflows/{workflow_id}/publish` `{revision}` | `PublishWorkflows` | 201 version summary | 409 `draft_revision_conflict`, 409 `draft_already_published`, 422 `graph_invalid`, 404 |
| GET `/workflows/{workflow_id}/versions/{version}` | `ViewOrganization` | 200 version | 404 |

A publish turns the stored draft into the workflow's next immutable version. It never takes a graph from the
request: the body names only the draft revision the client saw (D13). The handler runs in one transaction, in
this order:

1. `authorize(..., Capability::PublishWorkflows)` opens the tenant transaction. An outsider gets 404, and a
   runner or viewer gets 403.
2. `workflows::lock_workflow` reads the draft with `FOR UPDATE` on the workflow and draft rows. It returns `None`
   for a workflow that row-level security hides, which is 404.
3. When the locked draft's revision differs from the request's `revision`, the answer is 409
   `draft_revision_conflict`. The client must save or reload before it publishes.
4. When a version already exists for that revision, the answer is 409 `draft_already_published`, so each draft
   revision becomes at most one version.
5. `validate_graph` checks the stored graph against `catalog::definitions_map`, the system definitions plus the
   organization's own. Any diagnostic gives 422 `graph_invalid` with the sorted diagnostics, whose pointers lead
   into the stored draft. Because the map never holds another organization's definitions, a node that uses one
   gets `unknown_definition` at `/nodes/<i>/definition`.
6. `workflows::insert_version` stores the graph with its `canonical_checksum`, numbers it one above the current
   highest version, makes it the published version and writes the D9 audit row `workflow.published` with
   `{"version": V, "draftRevision": R, "checksum": "<hex>"}`.
7. `access.commit()`, then 201 with the version summary `{id, version, checksum, draftRevision, publishedAt}`.

Every early `return Err(...)` drops the transaction, which rolls it back and releases the lock, so a refused
publish stores no version, leaves `publishedVersion` alone and writes no audit row.

The lock is what makes concurrent publishes safe. A second publish of the same workflow waits in
`lock_workflow` until the first commits. Under READ COMMITTED its next statement sees the committed version, so
it gets 409 `draft_already_published`. The unique key `workflow_versions_draft_revision` backs this up: a
second version of one revision fails with SQLSTATE 23505, and the private `map_already_published` turns that
into the same 409, as `map_version_conflict` does for node definitions in Task 12. While the lock and the check
in step 4 stay in place, nothing reaches that path.

`GET /workflows/{workflow_id}/versions/{version}` returns the version summary plus its `graph`. Row-level
security hides other organizations' versions, so they are 404, like an unknown version number. A version
segment that is not an `i32` is 404 too, through `ApiPath`.

- [ ] **Step 1: Write the failing tests**

`crates/api/tests/publish_http.rs` has two tests. Its helpers are copies of the Task 13 ones, plus `checksum`,
which deserializes a graph from an API response and hashes it with `canonical_checksum`:

- **Publish each revision once.** The admin invites an editor, and the editor applies the infrastructure
  blueprint, whose graph is valid. Publishing revision 1 returns 201 with version 1, the checksum of the draft
  graph and `draftRevision` 1. A second publish of revision 1 gives 409 `draft_already_published`. The editor
  then saves the same graph with zoom 2 as revision 2. Publishing revision 1 now gives 409
  `draft_revision_conflict`, and publishing revision 2 gives version 2 with a different checksum, because the
  viewport is part of it. The versions list is exactly `[v2, v1]`, so the refused publishes stored nothing.
  `versions/1` returns the v1 summary plus the revision 1 graph, `versions/3` is 404, and the workflow reports
  published version 2 from draft revision 2. The workflow audit events are exactly `workflow.created`, the
  first `workflow.published`, `workflow.draft_saved` and the second `workflow.published`, with the D9 metadata.
- **Invalid and foreign drafts.** The admin creates a second organization, Beta, with the definition
  `beta.thing` v1. In Acme, the empty draft publishes as version 1. Revision 2 adds a node that uses
  `beta.thing`, and its publish gives the full 422 `graph_invalid` problem with exactly one diagnostic,
  `unknown_definition` at `/nodes/0/definition`. Afterwards there is still one version, the workflow still
  reports version 1 from revision 1, and the workflow audit events are unchanged. Beta's own workflow has
  version 1 and a saved revision 2. Through Acme's URLs its publish and its `versions/1` are 404, as are both
  routes for a random workflow id, and Beta's workflow still has one version.

Create `crates/api/tests/publish_http.rs`:

```rust
mod support;

use axum::{
    body::to_bytes,
    http::{Method, StatusCode},
    response::Response,
};
use corqty_domain::{graph::GraphSnapshot, publish::canonical_checksum};
use serde_json::{Value, json};
use serial_test::serial;
use support::AuthFixture;
use uuid::Uuid;

const INFRASTRUCTURE_BLUEPRINT_ID: &str = "00000000-0000-7000-8000-000000000101";

/// Asserts the status and returns the JSON body.
async fn body(response: Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// The server's checksum of a graph as the API returned it.
fn checksum(graph: &Value) -> String {
    let graph: GraphSnapshot = serde_json::from_value(graph.clone()).unwrap();
    canonical_checksum(&graph)
}

/// The organization's workflow audit events, newest first, as
/// [actorUserId, action, resourceId, metadata].
async fn workflow_events(member: &AuthFixture, organization_id: Uuid) -> Vec<Value> {
    let path = format!("/api/v1/orgs/{organization_id}/audit-events");
    let events = body(member.get(&path).await, StatusCode::OK).await;
    events["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["resourceType"] == "workflow")
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

/// Registers editor@example.com and adds it to the admin's organization as an editor.
async fn editor_of(admin: &AuthFixture) -> AuthFixture {
    let invitations = format!("/api/v1/orgs/{}/invitations", admin.organization_id);
    let invitation = body(
        admin
            .post_json(
                &invitations,
                r#"{"email":"editor@example.com","role":"editor"}"#,
            )
            .await,
        StatusCode::CREATED,
    )
    .await;
    let editor = admin
        .register_another("editor@example.com", "Personal")
        .await;
    let accept = json!({ "token": invitation["token"] }).to_string();
    let accepted = editor
        .post_json("/api/v1/invitations/accept", &accept)
        .await;
    assert_eq!(accepted.status(), StatusCode::OK);
    editor
}

#[tokio::test]
#[serial]
async fn editors_publish_each_draft_revision_once() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let editor = editor_of(&admin).await;
    let workflows = format!("/api/v1/orgs/{}/workflows", admin.organization_id);
    let request = json!({
        "name": "Provision staging",
        "blueprintId": INFRASTRUCTURE_BLUEPRINT_ID,
    });
    let applied = body(
        editor.post_json(&workflows, &request.to_string()).await,
        StatusCode::CREATED,
    )
    .await;
    let workflow_id = applied["id"].as_str().unwrap();
    let path = format!("{workflows}/{workflow_id}");
    let publish = format!("{path}/publish");

    let graph = applied["draft"]["graph"].clone();
    let first = body(
        editor.post_json(&publish, r#"{"revision":1}"#).await,
        StatusCode::CREATED,
    )
    .await;
    assert!(Uuid::parse_str(first["id"].as_str().unwrap()).is_ok());
    assert!(first["publishedAt"].is_string());
    assert_eq!(
        first,
        json!({
            "id": first["id"],
            "version": 1,
            "checksum": checksum(&graph),
            "draftRevision": 1,
            "publishedAt": first["publishedAt"],
        })
    );
    let again = body(
        editor.post_json(&publish, r#"{"revision":1}"#).await,
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(again["code"], "draft_already_published");

    // Once the draft moves on, its old revision can no longer be published.
    let mut edited = graph.clone();
    edited["viewport"]["zoom"] = json!(2.0);
    let save = json!({ "revision": 1, "graph": edited }).to_string();
    let saved = body(
        editor
            .request_json(Method::PUT, &format!("{path}/draft"), &save)
            .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(saved, json!({ "revision": 2 }));
    let stale = body(
        editor.post_json(&publish, r#"{"revision":1}"#).await,
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(stale["code"], "draft_revision_conflict");
    let second = body(
        editor.post_json(&publish, r#"{"revision":2}"#).await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(second["version"], 2);
    assert_eq!(second["draftRevision"], 2);
    assert_eq!(second["checksum"], checksum(&edited));
    // The viewport is part of the checksum.
    assert_ne!(checksum(&edited), checksum(&graph));

    // Versions are listed newest first, and each keeps the graph it was published from.
    let versions = format!("{path}/versions");
    assert_eq!(
        body(editor.get(&versions).await, StatusCode::OK).await,
        json!({ "items": [second, first] })
    );
    let mut version = first.clone();
    version["graph"] = graph;
    let detail = format!("{versions}/1");
    assert_eq!(
        body(editor.get(&detail).await, StatusCode::OK).await,
        version
    );
    let missing = editor.get(&format!("{versions}/3")).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let stored = body(editor.get(&path).await, StatusCode::OK).await;
    assert_eq!(stored["publishedVersion"], 2);
    assert_eq!(stored["publishedDraftRevision"], 2);

    // Only the two successful publishes wrote an audit event.
    let published = |number: u32, revision: u32, graph: &Value| {
        json!([
            editor.user_id,
            "workflow.published",
            workflow_id,
            { "version": number, "draftRevision": revision, "checksum": checksum(graph) },
        ])
    };
    assert_eq!(
        workflow_events(&editor, admin.organization_id).await,
        [
            published(2, 2, &edited),
            json!([editor.user_id, "workflow.draft_saved", workflow_id, { "revision": 2 }]),
            published(1, 1, &version["graph"]),
            json!([
                editor.user_id,
                "workflow.created",
                workflow_id,
                { "blueprintId": INFRASTRUCTURE_BLUEPRINT_ID },
            ]),
        ]
    );
}

#[tokio::test]
#[serial]
async fn invalid_and_foreign_drafts_are_not_published() {
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let beta = body(
        admin
            .post_json("/api/v1/organizations", r#"{"name":"Beta"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let beta = format!("/api/v1/orgs/{}", beta["organizationId"].as_str().unwrap());
    let definition = r#"{"key":"beta.thing","displayName":"Thing","category":"Testing",
        "inputs":[],"outputs":[],"configSchema":{"type":"object"},"uiSchema":{},"defaults":{},
        "executor":{"key":"mock","version":"1.0.0"}}"#;
    let created = admin
        .post_json(&format!("{beta}/node-definitions"), definition)
        .await;
    assert_eq!(created.status(), StatusCode::CREATED);

    let workflows = format!("/api/v1/orgs/{}/workflows", admin.organization_id);
    let created = body(
        admin.post_json(&workflows, r#"{"name":"Acme flow"}"#).await,
        StatusCode::CREATED,
    )
    .await;
    let path = format!("{workflows}/{}", created["id"].as_str().unwrap());
    let publish = format!("{path}/publish");
    // The empty draft is valid, so revision 1 becomes version 1.
    body(
        admin.post_json(&publish, r#"{"revision":1}"#).await,
        StatusCode::CREATED,
    )
    .await;

    // Revision 2 uses Beta's definition, which Acme's catalog does not contain.
    let mut graph = created["draft"]["graph"].clone();
    graph["nodes"] = json!([{
        "id": "thing",
        "definition": { "key": "beta.thing", "version": 1 },
        "displayName": "Thing",
        "position": { "x": 0, "y": 0 },
        "config": {},
        "inputs": {},
        "retry": { "maxAttempts": 1 },
        "timeoutSeconds": 900,
    }]);
    let save = json!({ "revision": 1, "graph": graph }).to_string();
    let saved = admin
        .request_json(Method::PUT, &format!("{path}/draft"), &save)
        .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let events = workflow_events(&admin, admin.organization_id).await;
    assert_eq!(
        body(
            admin.post_json(&publish, r#"{"revision":2}"#).await,
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await,
        json!({
            "type": "about:blank",
            "title": "Validation failed",
            "status": 422,
            "code": "graph_invalid",
            "diagnostics": [{
                "code": "unknown_definition",
                "pointer": "/nodes/0/definition",
                "message": "The node definition does not exist.",
            }],
        })
    );
    // Still one version, version 1 is still the published one, and no audit event was added.
    let versions = body(admin.get(&format!("{path}/versions")).await, StatusCode::OK).await;
    assert_eq!(versions["items"].as_array().unwrap().len(), 1);
    let stored = body(admin.get(&path).await, StatusCode::OK).await;
    assert_eq!(stored["draft"]["revision"], 2);
    assert_eq!(stored["publishedVersion"], 1);
    assert_eq!(stored["publishedDraftRevision"], 1);
    assert_eq!(workflow_events(&admin, admin.organization_id).await, events);

    // Beta's workflow has version 1 and an unpublished revision 2. Through Acme's URLs
    // neither is found, and an unknown workflow id is not found either.
    let foreign = body(
        admin
            .post_json(&format!("{beta}/workflows"), r#"{"name":"Beta flow"}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let beta_path = format!("{beta}/workflows/{}", foreign["id"].as_str().unwrap());
    body(
        admin
            .post_json(&format!("{beta_path}/publish"), r#"{"revision":1}"#)
            .await,
        StatusCode::CREATED,
    )
    .await;
    let save = json!({ "revision": 1, "graph": foreign["draft"]["graph"] }).to_string();
    let saved = admin
        .request_json(Method::PUT, &format!("{beta_path}/draft"), &save)
        .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let foreign_path = format!("{workflows}/{}", foreign["id"].as_str().unwrap());
    let unknown_path = format!("{workflows}/{}", Uuid::new_v4());
    for (method, path, request) in [
        (
            Method::POST,
            format!("{foreign_path}/publish"),
            r#"{"revision":2}"#,
        ),
        (Method::GET, format!("{foreign_path}/versions/1"), ""),
        (
            Method::POST,
            format!("{unknown_path}/publish"),
            r#"{"revision":1}"#,
        ),
        (Method::GET, format!("{unknown_path}/versions/1"), ""),
    ] {
        let response = admin.request_json(method.clone(), &path, request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {path}");
    }
    let versions = format!("{beta_path}/versions");
    let versions = body(admin.get(&versions).await, StatusCode::OK).await;
    assert_eq!(versions["items"].as_array().unwrap().len(), 1);
}
```

Add the matrix rows. The admin publishes the matrix workflow's empty draft first, so `versions/1` exists and
every role must get 200 from it. The publish row sends `{"revision":1}`: the admin and the editor, who hold
`PublishWorkflows`, never send it, while the runner and the viewer get 403 and the outsider 404.

In `crates/api/tests/authorization_matrix.rs`, apply these replacements in order:

Replace:

```rust
    let workflow = format!("{org}/workflows/{}", created["id"].as_str().unwrap());
```

With:

```rust
    let workflow = format!("{org}/workflows/{}", created["id"].as_str().unwrap());
    // Publish version 1, so every role has a version to read.
    let published = admin
        .post_json(&format!("{workflow}/publish"), r#"{"revision":1}"#)
        .await;
    assert_eq!(published.status(), StatusCode::CREATED);
```

Replace:

```rust
            format!("{workflow}/versions"),
            "",
            Capability::ViewOrganization,
        ),
    ] {
        matrix.assert_row(method, &path, body, capability).await;
    }
}
```

With:

```rust
            format!("{workflow}/versions"),
            "",
            Capability::ViewOrganization,
        ),
        (
            Method::POST,
            format!("{workflow}/publish"),
            r#"{"revision":1}"#,
            Capability::PublishWorkflows,
        ),
        (
            Method::GET,
            format!("{workflow}/versions/1"),
            "",
            Capability::ViewOrganization,
        ),
    ] {
        matrix.assert_row(method, &path, body, capability).await;
    }
}
```

Finally, the persistence test publishes one draft revision twice at the same time. The helper `publish` does
what the handler does: it locks the workflow, returns `None` when the revision already has a version, and
otherwise inserts the version and commits. `TestDatabase::reset()` gives a single-connection API pool, so the
test opens a second pool with the same connect options. The two publishes run concurrently through
`tokio::join!`, as the editor and the admin of organization A. Exactly one must store version 1 and the other
must be refused. The database then holds one version row, `published_version` is 1, and the audit log holds
`workflow.created` and one `workflow.published`.

In `crates/persistence/tests/workflow_catalog.rs`, apply these replacements in order:

Replace:

```rust
use sqlx::{Postgres, Transaction};
```

With:

```rust
use sqlx::{PgPool, Postgres, Transaction};
```

Replace:

```rust
            assert_eq!(node.config, definition.spec.defaults);
        }
    }
}
```

With:

```rust
            assert_eq!(node.config, definition.spec.defaults);
        }
    }
}

/// Publishes the draft the way the API does: lock the workflow, refuse a revision that
/// already has a version, then store the next version. None means it was refused.
async fn publish(pool: &PgPool, user: Uuid, organization: Uuid, workflow: Uuid) -> Option<i32> {
    let mut tx = begin_tenant(pool, user, organization).await.unwrap();
    let locked = workflows::lock_workflow(&mut tx, workflow)
        .await
        .unwrap()
        .unwrap();
    let published = workflows::version_exists_for_revision(&mut tx, workflow, locked.revision);
    if published.await.unwrap() {
        return None;
    }
    let checksum = canonical_checksum(&locked.graph);
    let version = workflows::insert_version(
        &mut tx,
        organization,
        user,
        workflow,
        locked.revision,
        &locked.graph,
        &checksum,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    Some(version.version)
}

#[tokio::test]
#[serial]
async fn concurrent_publishes_of_one_revision_store_one_version() {
    let fixture = Fixture::new().await;
    let (a, editor) = (fixture.org_a, fixture.editor);
    let empty = GraphSnapshot::empty();
    let mut tx = fixture.tx(editor, a).await;
    let workflow = workflows::insert_workflow(&mut tx, a, editor, "Race", "race", &empty, None)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    // A second pool, so that each publish runs on its own connection at the same time.
    let options = fixture.database.api.connect_options();
    let other = PgPool::connect_with((*options).clone()).await.unwrap();
    let (first, second) = tokio::join!(
        publish(&fixture.database.api, editor, a, workflow.id),
        publish(&other, fixture.admin, a, workflow.id),
    );
    let mut outcomes = [first, second];
    outcomes.sort_unstable();
    assert_eq!(outcomes, [None, Some(1)]);

    let stored: Vec<(i32, i64, Option<i32>)> = sqlx::query_as(
        "SELECT v.version, v.draft_revision, w.published_version
         FROM workflow_versions v JOIN workflows w ON w.id = v.workflow_id",
    )
    .fetch_all(&fixture.database.owner)
    .await
    .unwrap();
    assert_eq!(stored, [(1, 1, Some(1))]);
    let checksum = canonical_checksum(&empty);
    assert_eq!(
        audit_rows(&fixture.database).await,
        [
            audit("workflow.created", workflow.id, json!({})),
            audit(
                "workflow.published",
                workflow.id,
                json!({"version": 1, "draftRevision": 1, "checksum": checksum})
            ),
        ]
    );
}
```

- [ ] **Step 2: Run the tests and verify RED**

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-api --no-fail-fast --test publish_http --test authorization_matrix
```

Expected: both test binaries compile, because the tests only talk HTTP. No publish route exists yet, so each
publish falls through to the 404 fallback:

- In `authorization_matrix`, 3 tests pass. `workflow_routes_enforce_capabilities` fails when the admin
  publishes version 1: ``assertion `left == right` failed``, left 404 and right 201.
- In `publish_http`, both tests fail in `body` on their first publish, with left 404 and right 201.

```text
cargo test -p corqty-persistence --test workflow_catalog
```

Expected: all 5 tests pass, `concurrent_publishes_of_one_revision_store_one_version` included. Task 11 already
built `lock_workflow` and `insert_version`, so this test has no RED of its own. It guards the lock: with
`FOR UPDATE` removed from `lock_workflow`, it failed in 5 runs out of 5, because both publishes inserted
version 1 and the second hit the unique key `workflow_versions_number` (23505).

- [ ] **Step 3: Add the publish and version handlers**

`PublishDraft` is the request body. Like every request DTO it denies unknown fields, so a body that also sends a
graph is 422 `invalid_json`.

In `crates/api/src/workflows.rs`, apply these replacements in order:

Replace:

```rust
use corqty_domain::{
    graph::{GraphSnapshot, check_limits},
    publish::validate_graph,
    rbac::Capability,
    slug::slugify,
};
use corqty_persistence::{
    catalog,
    models::SessionRecord,
    workflows::{self, VersionSummaryRecord, WorkflowRecord, WorkflowSummaryRecord},
};
```

With:

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

Replace:

```rust
pub struct GraphBody {
    graph: GraphSnapshot,
}
```

With:

```rust
pub struct GraphBody {
    graph: GraphSnapshot,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishDraft {
    revision: i64,
}
```

Replace:

```rust
    let items = workflows::list_versions(&mut access.tx, workflow_id).await?;
    access.commit().await?;
    Ok(Json(Items { items }))
}
```

With:

```rust
    let items = workflows::list_versions(&mut access.tx, workflow_id).await?;
    access.commit().await?;
    Ok(Json(Items { items }))
}

/// Publishes the stored draft, at exactly the revision the client saw, as the workflow's
/// next version. The workflow stays locked until the commit, so publishes of one workflow
/// run one at a time and each draft revision becomes at most one version.
pub async fn publish(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, workflow_id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(request): ApiJson<PublishDraft>,
) -> Result<(StatusCode, Json<VersionSummaryRecord>), ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::PublishWorkflows,
    )
    .await?;
    let draft = workflows::lock_workflow(&mut access.tx, workflow_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if draft.revision != request.revision {
        return Err(ApiError::Conflict("draft_revision_conflict"));
    }
    if workflows::version_exists_for_revision(&mut access.tx, workflow_id, draft.revision).await? {
        return Err(ApiError::Conflict("draft_already_published"));
    }
    let definitions = catalog::definitions_map(&mut access.tx).await?;
    let diagnostics = validate_graph(&draft.graph, &definitions);
    if !diagnostics.is_empty() {
        return Err(ApiError::Invalid {
            code: "graph_invalid",
            diagnostics,
        });
    }
    let version = workflows::insert_version(
        &mut access.tx,
        organization_id,
        access.user_id,
        workflow_id,
        draft.revision,
        &draft.graph,
        &canonical_checksum(&draft.graph),
    )
    .await
    .map_err(map_already_published)?;
    access.commit().await?;
    Ok((StatusCode::CREATED, Json(version)))
}

pub async fn get_version(
    State(state): State<AppState>,
    Extension(session): Extension<SessionRecord>,
    ApiPath((organization_id, workflow_id, version)): ApiPath<(Uuid, Uuid, i32)>,
) -> Result<Json<VersionRecord>, ApiError> {
    let mut access = authorize(
        &state,
        &session,
        organization_id,
        Capability::ViewOrganization,
    )
    .await?;
    let record = workflows::get_version(&mut access.tx, workflow_id, version)
        .await?
        .ok_or(ApiError::NotFound)?;
    access.commit().await?;
    Ok(Json(record))
}

/// The unique key that keeps a draft revision from becoming two versions. The workflow
/// lock already stops a second publish before it gets here; this keeps the answer a 409
/// if that ever changes.
fn map_already_published(error: sqlx::Error) -> ApiError {
    if error
        .as_database_error()
        .is_some_and(|database| database.constraint() == Some("workflow_versions_draft_revision"))
    {
        ApiError::Conflict("draft_already_published")
    } else {
        error.into()
    }
}
```

- [ ] **Step 4: Route the new endpoints**

Both routes go on the protected router, so they need a session, and the publish POST also needs the CSRF token.
The version detail route sits next to the versions list.

Replace the whole of `crates/api/src/router.rs`:

```rust
use axum::{
    Router,
    http::StatusCode,
    middleware,
    routing::{get, post, put},
};
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::{
    audit,
    auth::{handlers, middleware as auth_middleware},
    catalog,
    error::ApiError,
    organizations,
    state::AppState,
    workflows,
};

pub fn build_router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/auth/me", get(handlers::me))
        .route("/auth/logout", post(handlers::logout))
        .route("/organizations", post(organizations::handlers::create))
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
        .route("/orgs/{organization_id}/audit-events", get(audit::list))
        .route(
            "/orgs/{organization_id}/members/{target_user_id}",
            axum::routing::patch(organizations::members::change_role)
                .delete(organizations::members::remove_member),
        )
        .route(
            "/orgs/{organization_id}/node-definitions",
            get(catalog::list_node_definitions).post(catalog::create_node_definition),
        )
        .route(
            "/orgs/{organization_id}/node-definitions/{definition_id}/deprecate",
            post(catalog::deprecate_node_definition),
        )
        .route(
            "/orgs/{organization_id}/blueprints",
            get(catalog::list_blueprints).post(catalog::create_blueprint),
        )
        .route(
            "/orgs/{organization_id}/workflows",
            get(workflows::list_workflows).post(workflows::create_workflow),
        )
        .route(
            "/orgs/{organization_id}/workflows/{workflow_id}",
            get(workflows::get_workflow),
        )
        .route(
            "/orgs/{organization_id}/workflows/{workflow_id}/draft",
            put(workflows::save_draft),
        )
        .route(
            "/orgs/{organization_id}/workflows/{workflow_id}/validate",
            post(workflows::validate),
        )
        .route(
            "/orgs/{organization_id}/workflows/{workflow_id}/publish",
            post(workflows::publish),
        )
        .route(
            "/orgs/{organization_id}/workflows/{workflow_id}/versions",
            get(workflows::list_versions),
        )
        .route(
            "/orgs/{organization_id}/workflows/{workflow_id}/versions/{version}",
            get(workflows::get_version),
        )
        .route(
            "/auth/password",
            axum::routing::patch(handlers::change_password),
        )
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
        // `method_not_allowed_fallback` only reaches routes added before it.
        .fallback(|| async { ApiError::NotFound })
        .method_not_allowed_fallback(|| async { ApiError::MethodNotAllowed })
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
}
```

- [ ] **Step 5: Verify GREEN**

```text
cargo test -p corqty-api --no-fail-fast --test publish_http --test authorization_matrix
```

Expected: `authorization_matrix` 4 and `publish_http` 2 tests pass.

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
`catalog_http` 3, `members_http` 3, `organizations_http` 1, `problems_http` 2, `publish_http` 2,
`workflows_http` 4. corqty-domain: `account_and_rbac` 6, `catalog_rules` 5, `graph_rules` 8, `publish_rules` 9.
corqty-persistence: `auth_store` 2, `catalog_guards` 3, `final_admin` 2, `migrations` 1, `tenant_isolation` 1,
`test_reset` 1, `workflow_catalog` 5. 70 tests in total.

- [ ] **Step 7: Commit**

```text
git add crates/api crates/persistence/tests/workflow_catalog.rs
git commit -m "feat: publish immutable workflow versions atomically"
```

Expected: `git show --stat HEAD` lists exactly the 5 files above (503 insertions, 3 deletions).

## Task 15: List workflows, browse the catalog, and create workflows from blueprints

**Files:**

- Create: `apps/web/src/components/status/status-badge.tsx`
- Create: `apps/web/src/studio/graph-model.ts`
- Create: `apps/web/src/studio/studio.tsx`
- Create: `apps/web/src/views/workflows-view.tsx`
- Create: `apps/web/src/views/catalog-view.tsx`
- Create: `apps/web/src/app/(app)/orgs/[organizationId]/workflows/page.tsx`
- Create: `apps/web/src/app/(app)/orgs/[organizationId]/workflows/[workflowId]/page.tsx`
- Create: `apps/web/src/app/(app)/orgs/[organizationId]/catalog/page.tsx`
- Modify: `apps/web/src/api/types.ts`
- Modify: `apps/web/src/api/queries.ts`
- Modify: `apps/web/src/layout/app-shell.tsx`
- Modify: `apps/web/src/test/handlers.ts`
- Test: `apps/web/src/views/workflow-views.test.tsx` (new)

This task adds two organization pages and the first workflow write from the UI. `/orgs/[organizationId]/workflows`
lists the workflows in the order the API returns them (updatedAt descending). Users with `edit_workflows` also get a "New workflow"
dialog that takes a name and an optional blueprint, POSTs `{name, blueprintId}`, and opens the new workflow.
`/orgs/[organizationId]/catalog` is read-only. It shows one row per definition key with the highest
non-deprecated version, lists deprecated versions muted in the same row, and lists blueprints with their
descriptions. Simulated entries carry the Simulated badge from the new `components/status/status-badge.tsx`,
which is the only place outside `components/ui` allowed to use raw palette colors (D17). It already has every
status the studio needs (saved, saving, conflict, error, simulated, read-only). `isSimulated(key)` starts
`src/studio/graph-model.ts`, and Task 16 adds the graph helpers next to it. The studio route renders a
placeholder that loads the workflow until Task 16 replaces `src/studio/studio.tsx`.

The new API types list only the fields this task reads. Task 16 adds the port specs, schemas and graphs it
needs. The MSW fixtures mirror the seeded system catalog (§8) in API order (key, then version), so later
tests can reuse them.

- [ ] **Step 1: Add the API types and the test fixtures**

Replace the contents of `apps/web/src/api/types.ts` with:

```ts
export type Role = "admin" | "editor" | "runner" | "viewer"

export type Capability =
  | "view_organization"
  | "edit_workflows"
  | "publish_workflows"
  | "run_workflows"
  | "manage_members"
  | "manage_service_tokens"
  | "manage_secrets"
  | "manage_plugins"
  | "enable_shell_executor"

export interface OrganizationMembership {
  organizationId: string
  name: string
  slug: string
  role: Role
  capabilities: Capability[]
}

export interface Me {
  user: { id: string; email: string }
  organizations: OrganizationMembership[]
}

export interface Diagnostic {
  code: string
  pointer: string
  message: string
}

export interface Member {
  userId: string
  email: string
  role: Role
}

export interface Invitation {
  token: string
  inviteUrl?: string
  expiresAt: string
}

export interface AuditEvent {
  id: number
  actorUserId: string | null
  action: string
  resourceType: string
  resourceId: string
  metadata: Record<string, unknown>
  occurredAt: string
}

// The catalog and workflow types list only the fields the UI reads so far;
// the API sends more (port specs, schemas, graphs).
export interface NodeDefinition {
  id: string
  key: string
  version: number
  displayName: string
  description: string
  category: string
  deprecated: boolean
}

export interface Blueprint {
  id: string
  key: string
  displayName: string
  description: string
}

export interface WorkflowSummary {
  id: string
  name: string
  draftRevision: number
  publishedVersion: number | null
  updatedAt: string
}

export interface Workflow {
  id: string
  name: string
  draft: { revision: number }
}
```

In `apps/web/src/test/handlers.ts`, apply these replacements in order:

Replace:

```ts
import type { AuditEvent, Capability, Me, Member, Role } from "@/api/types"
```

With:

```ts
import type {
  AuditEvent,
  Blueprint,
  Capability,
  Me,
  Member,
  NodeDefinition,
  Role,
  WorkflowSummary,
} from "@/api/types"
```

Replace:

```ts
// A signed-in admin of org-1 reading data; tests override writes and failures.
export const handlers = [
  http.get("/api/v1/auth/me", () => HttpResponse.json(meAs("admin"))),
  http.get("/api/v1/orgs/:organizationId/members", () =>
    HttpResponse.json(members),
  ),
  http.get("/api/v1/orgs/:organizationId/audit-events", () =>
    HttpResponse.json({ items: auditEvents }),
  ),
]
```

With:

```ts
// Ordered by updatedAt descending, as the API returns them.
export const workflows: WorkflowSummary[] = [
  {
    id: "wf-2",
    name: "Provision staging",
    draftRevision: 3,
    publishedVersion: 1,
    updatedAt: "2026-10-25T12:00:00Z",
  },
  {
    id: "wf-1",
    name: "Nightly report",
    draftRevision: 1,
    publishedVersion: null,
    updatedAt: "2026-10-24T08:00:00Z",
  },
]

// The seeded system catalog, in API order (key, then version).
export const nodeDefinitions: NodeDefinition[] = [
  {
    id: "00000000-0000-7000-8000-000000000005",
    key: "corqty.mock.normalize-payload",
    version: 1,
    displayName: "Normalize payload",
    description: "Normalizes a JSON payload.",
    category: "Data processing",
    deprecated: false,
  },
  {
    id: "00000000-0000-7000-8000-000000000006",
    key: "corqty.mock.produce-summary",
    version: 1,
    displayName: "Produce summary",
    description: "Produces a text summary of normalized data.",
    category: "Data processing",
    deprecated: false,
  },
  {
    id: "00000000-0000-7000-8000-000000000001",
    key: "corqty.mock.task",
    version: 1,
    displayName: "Mock task",
    description: "Returns a fixed JSON result or echoes its input.",
    category: "Testing",
    deprecated: false,
  },
  {
    id: "00000000-0000-7000-8000-000000000003",
    key: "corqty.simulated.create-rds",
    version: 1,
    displayName: "Create RDS (simulated)",
    description: "Simulated: pretends to create a database inside a VPC.",
    category: "Infrastructure (simulated)",
    deprecated: false,
  },
  {
    id: "00000000-0000-7000-8000-000000000002",
    key: "corqty.simulated.create-vpc",
    version: 1,
    displayName: "Create VPC (simulated)",
    description: "Simulated: pretends to create a VPC and returns its id.",
    category: "Infrastructure (simulated)",
    deprecated: false,
  },
  {
    id: "00000000-0000-7000-8000-000000000004",
    key: "corqty.simulated.deploy-service",
    version: 1,
    displayName: "Deploy service (simulated)",
    description: "Simulated: pretends to deploy a service into a VPC.",
    category: "Infrastructure (simulated)",
    deprecated: false,
  },
]

export const blueprints: Blueprint[] = [
  {
    id: "00000000-0000-7000-8000-000000000102",
    key: "corqty.data-processing",
    displayName: "Data processing",
    description:
      "Normalizes a JSON payload and produces a summary with the mock executor.",
  },
  {
    id: "00000000-0000-7000-8000-000000000101",
    key: "corqty.simulated-infrastructure",
    displayName: "Simulated infrastructure",
    description:
      "Simulated: every step uses the mock executor. Creates a VPC, then a database and a service in parallel.",
  },
]

// A signed-in admin of org-1 reading data; tests override writes and failures.
export const handlers = [
  http.get("/api/v1/auth/me", () => HttpResponse.json(meAs("admin"))),
  http.get("/api/v1/orgs/:organizationId/members", () =>
    HttpResponse.json(members),
  ),
  http.get("/api/v1/orgs/:organizationId/audit-events", () =>
    HttpResponse.json({ items: auditEvents }),
  ),
  http.get("/api/v1/orgs/:organizationId/workflows", () =>
    HttpResponse.json({ items: workflows }),
  ),
  http.get("/api/v1/orgs/:organizationId/node-definitions", () =>
    HttpResponse.json({ items: nodeDefinitions }),
  ),
  http.get("/api/v1/orgs/:organizationId/blueprints", () =>
    HttpResponse.json({ items: blueprints }),
  ),
]
```

- [ ] **Step 2: Write the failing view tests**

The tests render each view inside `AuthGate` and `AppShell`, the same way the `(app)` and
`orgs/[organizationId]` layouts do, so `useCan` reads the capabilities of the signed-in role from MSW.

Create `apps/web/src/views/workflow-views.test.tsx`:

```tsx
import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { http, HttpResponse } from "msw/http"
import type { ReactElement } from "react"
import { expect, it } from "vitest"

import { AuthGate } from "@/auth/auth-gate"
import { AppShell } from "@/layout/app-shell"
import { meAs, nodeDefinitions } from "@/test/handlers"
import { renderWithProviders, router } from "@/test/render"
import { server } from "@/test/server"
import { CatalogView } from "@/views/catalog-view"
import { WorkflowsView } from "@/views/workflows-view"

// Mirrors the (app) and orgs/[organizationId] layouts around a page.
function renderInShell(view: ReactElement, route: string) {
  return renderWithProviders(
    <AuthGate>
      <AppShell organizationId="org-1">{view}</AppShell>
    </AuthGate>,
    { route },
  )
}

function signedInAs(role: "editor" | "viewer") {
  server.use(http.get("/api/v1/auth/me", () => HttpResponse.json(meAs(role))))
}

it("lists workflows in the order the API returns them", async () => {
  renderInShell(
    <WorkflowsView organizationId="org-1" />,
    "/orgs/org-1/workflows",
  )
  expect(
    await screen.findByRole("link", { name: "Provision staging" }),
  ).toHaveAttribute("href", "/orgs/org-1/workflows/wf-2")
  const rows = within(screen.getByRole("table")).getAllByRole("row").slice(1)
  expect(rows.map((row) => within(row).getByRole("link").textContent)).toEqual(
    ["Provision staging", "Nightly report"],
  )
  expect(within(rows[0]).getByText("v1")).toBeInTheDocument()
  expect(within(rows[1]).getByText("Not published")).toBeInTheDocument()
  expect(screen.getByRole("link", { name: "Workflows" })).toHaveAttribute(
    "aria-current",
    "page",
  )
})

it("a viewer sees the workflows but cannot create one", async () => {
  signedInAs("viewer")
  renderInShell(
    <WorkflowsView organizationId="org-1" />,
    "/orgs/org-1/workflows",
  )
  expect(
    await screen.findByRole("link", { name: "Provision staging" }),
  ).toBeInTheDocument()
  expect(
    screen.queryByRole("button", { name: "New workflow" }),
  ).not.toBeInTheDocument()
})

it("an editor creates a workflow from a blueprint and opens it", async () => {
  signedInAs("editor")
  let created: unknown
  server.use(
    http.post("/api/v1/orgs/:organizationId/workflows", async ({ request }) => {
      created = await request.json()
      return HttpResponse.json(
        { id: "wf-new", name: "Staging", draft: { revision: 1 } },
        { status: 201 },
      )
    }),
  )
  renderInShell(
    <WorkflowsView organizationId="org-1" />,
    "/orgs/org-1/workflows",
  )
  await userEvent.click(
    await screen.findByRole("button", { name: "New workflow" }),
  )
  await userEvent.type(screen.getByLabelText("Name"), "Staging")
  const simulated = await screen.findByRole("radio", {
    name: /^Simulated infrastructure/,
  })
  const simulatedOption = simulated.closest("label") as HTMLElement
  expect(within(simulatedOption).getByText("Simulated")).toBeInTheDocument()
  expect(
    within(simulatedOption).getByText(/^Simulated: every step uses the mock/),
  ).toBeInTheDocument()
  const dataOption = screen
    .getByRole("radio", { name: /^Data processing/ })
    .closest("label") as HTMLElement
  expect(within(dataOption).queryByText("Simulated")).not.toBeInTheDocument()
  await userEvent.click(simulated)
  await userEvent.click(screen.getByRole("button", { name: "Create workflow" }))
  await waitFor(() =>
    expect(router.push).toHaveBeenCalledWith("/orgs/org-1/workflows/wf-new"),
  )
  expect(created).toEqual({
    name: "Staging",
    blueprintId: "00000000-0000-7000-8000-000000000101",
  })
})

it("the catalog shows the latest definitions and marks simulated entries", async () => {
  server.use(
    http.get("/api/v1/orgs/:organizationId/node-definitions", () =>
      HttpResponse.json({
        items: [
          {
            id: "acme-thing-1",
            key: "acme.thing",
            version: 1,
            displayName: "Old thing",
            description: "",
            category: "Acme",
            deprecated: true,
          },
          {
            id: "acme-thing-2",
            key: "acme.thing",
            version: 2,
            displayName: "Thing",
            description: "Does the thing.",
            category: "Acme",
            deprecated: false,
          },
          ...nodeDefinitions,
        ],
      }),
    ),
  )
  renderInShell(<CatalogView organizationId="org-1" />, "/orgs/org-1/catalog")
  const definitions = await screen.findByRole("table", {
    name: "Node definitions",
  })
  const thing = await within(definitions).findByRole("row", {
    name: /acme\.thing/,
  })
  expect(within(definitions).getAllByRole("row")).toHaveLength(1 + 7)
  expect(within(thing).getByText("Thing")).toBeInTheDocument()
  expect(within(thing).getByText("v2")).toBeInTheDocument()
  expect(within(thing).getByText("v1 deprecated")).toHaveClass(
    "text-muted-foreground",
  )
  expect(within(thing).queryByText("Old thing")).not.toBeInTheDocument()
  for (const definition of nodeDefinitions) {
    const row = within(definitions).getByRole("row", {
      name: new RegExp(definition.key.replaceAll(".", "\\.")),
    })
    expect(within(row).queryAllByText("Simulated")).toHaveLength(
      definition.key.startsWith("corqty.simulated") ? 1 : 0,
    )
  }
  const blueprints = screen.getByRole("table", { name: "Blueprints" })
  const simulated = await within(blueprints).findByRole("row", {
    name: /Simulated infrastructure/,
  })
  expect(within(simulated).getByText("Simulated")).toBeInTheDocument()
  expect(
    within(
      within(blueprints).getByRole("row", { name: /Data processing/ }),
    ).queryByText("Simulated"),
  ).not.toBeInTheDocument()
  expect(screen.getByRole("link", { name: "Catalog" })).toHaveAttribute(
    "aria-current",
    "page",
  )
})
```

- [ ] **Step 3: Run the tests and verify RED**

Run:

```text
pnpm web:test
```

Expected: `Test Files  1 failed | 4 passed (5)` and `Tests  12 passed (12)`. The new file fails to load with
`Error: Failed to resolve import "@/views/catalog-view" from "src/views/workflow-views.test.tsx". Does the file exist?`

- [ ] **Step 4: Add the status badge and the simulated check**

Create `apps/web/src/components/status/status-badge.tsx`:

```tsx
import { Badge } from "@/components/ui/badge"

// Status hues keep a fixed meaning, so this is the one place outside
// components/ui that may use raw palette colors.
const statuses = {
  saved: {
    label: "Saved",
    className: "border-emerald-600/40 text-emerald-700 dark:text-emerald-400",
  },
  saving: { label: "Saving…", className: "text-muted-foreground" },
  conflict: {
    label: "Conflict",
    className: "border-amber-600/40 text-amber-700 dark:text-amber-400",
  },
  error: {
    label: "Error",
    className: "border-red-600/40 text-red-700 dark:text-red-400",
  },
  simulated: {
    label: "Simulated",
    className: "border-blue-600/40 text-blue-700 dark:text-blue-400",
  },
  "read-only": { label: "Read-only", className: "text-muted-foreground" },
}

export type Status = keyof typeof statuses

export function StatusBadge({ status }: { status: Status }) {
  const { label, className } = statuses[status]
  return (
    <Badge variant="outline" className={className}>
      {label}
    </Badge>
  )
}
```

Create `apps/web/src/studio/graph-model.ts`:

```ts
export function isSimulated(key: string) {
  return key.startsWith("corqty.simulated")
}
```

- [ ] **Step 5: Add the workflow and catalog queries**

Every key sits under `["orgs", organizationId]`. Creating a workflow goes through `useOrganizationMutation`,
which invalidates that whole prefix, because the API also appends a `workflow.created` audit event.

Replace the contents of `apps/web/src/api/queries.ts` with:

```ts
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"

import { api } from "@/api/client"
import type {
  AuditEvent,
  Blueprint,
  Invitation,
  Member,
  NodeDefinition,
  Role,
  Workflow,
  WorkflowSummary,
} from "@/api/types"

export const queryKeys = {
  organization: (organizationId: string) => ["orgs", organizationId] as const,
  members: (organizationId: string) =>
    ["orgs", organizationId, "members"] as const,
  auditEvents: (organizationId: string) =>
    ["orgs", organizationId, "audit-events"] as const,
  workflows: (organizationId: string) =>
    ["orgs", organizationId, "workflows"] as const,
  workflow: (organizationId: string, workflowId: string) =>
    ["orgs", organizationId, "workflows", workflowId] as const,
  nodeDefinitions: (organizationId: string) =>
    ["orgs", organizationId, "node-definitions"] as const,
  blueprints: (organizationId: string) =>
    ["orgs", organizationId, "blueprints"] as const,
}

export function useMembers(organizationId: string) {
  return useQuery({
    queryKey: queryKeys.members(organizationId),
    queryFn: ({ signal }) =>
      api<Member[]>(`/orgs/${organizationId}/members`, { signal }),
  })
}

export function useAuditEvents(organizationId: string) {
  return useQuery({
    queryKey: queryKeys.auditEvents(organizationId),
    queryFn: ({ signal }) =>
      api<{ items: AuditEvent[] }>(
        `/orgs/${organizationId}/audit-events?limit=50`,
        { signal },
      ),
  })
}

// Writes also append audit events: refresh the whole organization.
function useOrganizationMutation<Variables, Result>(
  organizationId: string,
  mutationFn: (variables: Variables) => Promise<Result>,
) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn,
    onSuccess: () =>
      queryClient.invalidateQueries({
        queryKey: queryKeys.organization(organizationId),
      }),
  })
}

export function useCreateInvitation(organizationId: string) {
  return useOrganizationMutation(
    organizationId,
    (body: { email: string; role: Role }) =>
      api<Invitation>(`/orgs/${organizationId}/invitations`, {
        method: "POST",
        body: JSON.stringify(body),
      }),
  )
}

export function useChangeMemberRole(organizationId: string) {
  return useOrganizationMutation(
    organizationId,
    ({ userId, role }: { userId: string; role: Role }) =>
      api<void>(`/orgs/${organizationId}/members/${userId}`, {
        method: "PATCH",
        body: JSON.stringify({ role }),
      }),
  )
}

export function useRemoveMember(organizationId: string) {
  return useOrganizationMutation(organizationId, (userId: string) =>
    api<void>(`/orgs/${organizationId}/members/${userId}`, {
      method: "DELETE",
    }),
  )
}

export function useWorkflows(organizationId: string) {
  return useQuery({
    queryKey: queryKeys.workflows(organizationId),
    queryFn: ({ signal }) =>
      api<{ items: WorkflowSummary[] }>(`/orgs/${organizationId}/workflows`, {
        signal,
      }),
  })
}

export function useWorkflow(organizationId: string, workflowId: string) {
  return useQuery({
    queryKey: queryKeys.workflow(organizationId, workflowId),
    queryFn: ({ signal }) =>
      api<Workflow>(`/orgs/${organizationId}/workflows/${workflowId}`, {
        signal,
      }),
  })
}

export function useNodeDefinitions(organizationId: string) {
  return useQuery({
    queryKey: queryKeys.nodeDefinitions(organizationId),
    queryFn: ({ signal }) =>
      api<{ items: NodeDefinition[] }>(
        `/orgs/${organizationId}/node-definitions`,
        { signal },
      ),
  })
}

export function useBlueprints(organizationId: string) {
  return useQuery({
    queryKey: queryKeys.blueprints(organizationId),
    queryFn: ({ signal }) =>
      api<{ items: Blueprint[] }>(`/orgs/${organizationId}/blueprints`, {
        signal,
      }),
  })
}

export function useCreateWorkflow(organizationId: string) {
  return useOrganizationMutation(
    organizationId,
    (body: { name: string; blueprintId?: string }) =>
      api<Workflow>(`/orgs/${organizationId}/workflows`, {
        method: "POST",
        body: JSON.stringify(body),
      }),
  )
}
```

- [ ] **Step 6: Add the workflows view, the catalog view and the studio placeholder**

`NewWorkflowForm` renders inside `DialogContent`, so it mounts when the dialog opens and starts empty each time.
Its blueprint query only runs while the dialog is open.

Create `apps/web/src/views/workflows-view.tsx`:

```tsx
"use client"

import Link from "next/link"
import { useRouter } from "next/navigation"
import { useState } from "react"
import type { FormEvent } from "react"

import { ApiProblem } from "@/api/client"
import { useBlueprints, useCreateWorkflow, useWorkflows } from "@/api/queries"
import { useCan } from "@/auth/use-can"
import { StatusBadge } from "@/components/status/status-badge"
import { Button } from "@/components/ui/button"
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
import { isSimulated } from "@/studio/graph-model"

export function WorkflowsView({ organizationId }: { organizationId: string }) {
  const canEdit = useCan(organizationId, "edit_workflows")
  const workflows = useWorkflows(organizationId)
  const [creating, setCreating] = useState(false)

  return (
    <section className="flex flex-col gap-5 p-4 md:p-6">
      <header className="flex flex-col items-start justify-between gap-3 sm:flex-row sm:items-center">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Workflows</h1>
          <p className="text-sm text-muted-foreground">
            Drafts and published versions in this organization.
          </p>
        </div>
        {canEdit && (
          <Button onClick={() => setCreating(true)}>New workflow</Button>
        )}
      </header>
      {workflows.isPending && (
        <p className="text-sm text-muted-foreground">Loading workflows…</p>
      )}
      {workflows.isError && (
        <p role="alert" className="text-sm text-destructive">
          The workflows could not be loaded.
        </p>
      )}
      {workflows.data?.items.length === 0 && (
        <p className="text-sm text-muted-foreground">No workflows yet.</p>
      )}
      {canEdit && (
        <Dialog open={creating} onOpenChange={setCreating}>
          <DialogContent className="sm:max-w-md">
            <NewWorkflowForm organizationId={organizationId} />
          </DialogContent>
        </Dialog>
      )}
      <div className="overflow-x-auto rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Name</TableHead>
              <TableHead>Draft</TableHead>
              <TableHead>Published</TableHead>
              <TableHead>Updated</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {workflows.data?.items.map((workflow) => (
              <TableRow key={workflow.id}>
                <TableCell>
                  <Link
                    href={`/orgs/${organizationId}/workflows/${workflow.id}`}
                    className="font-medium underline-offset-4 hover:underline"
                  >
                    {workflow.name}
                  </Link>
                </TableCell>
                <TableCell>rev {workflow.draftRevision}</TableCell>
                <TableCell>
                  {workflow.publishedVersion === null ? (
                    <span className="text-muted-foreground">Not published</span>
                  ) : (
                    `v${workflow.publishedVersion}`
                  )}
                </TableCell>
                <TableCell>
                  {new Date(workflow.updatedAt).toLocaleString()}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </section>
  )
}

// Mounted only while the dialog is open, so it starts empty each time.
function NewWorkflowForm({ organizationId }: { organizationId: string }) {
  const router = useRouter()
  const blueprints = useBlueprints(organizationId)
  const createWorkflow = useCreateWorkflow(organizationId)
  const [name, setName] = useState("")
  const [blueprintId, setBlueprintId] = useState("")
  const [error, setError] = useState("")
  const options = [
    {
      id: "",
      key: "",
      displayName: "Empty workflow",
      description: "Start from a blank canvas.",
    },
    ...(blueprints.data?.items ?? []),
  ]

  async function create(event: FormEvent) {
    event.preventDefault()
    setError("")
    try {
      const workflow = await createWorkflow.mutateAsync({
        name,
        blueprintId: blueprintId || undefined,
      })
      router.push(`/orgs/${organizationId}/workflows/${workflow.id}`)
    } catch (problem) {
      setError(
        problem instanceof ApiProblem && problem.code === "invalid_workflow_name"
          ? "Enter a name of 1 to 100 characters."
          : "The workflow could not be created.",
      )
    }
  }

  return (
    <>
      <DialogHeader>
        <DialogTitle>New workflow</DialogTitle>
        <DialogDescription>
          Start from a blank canvas or copy a blueprint.
        </DialogDescription>
      </DialogHeader>
      <form className="grid gap-4" onSubmit={create}>
        <Field>
          <FieldLabel htmlFor="workflow-name">Name</FieldLabel>
          <Input
            id="workflow-name"
            required
            maxLength={100}
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
        </Field>
        <fieldset className="grid gap-2">
          <legend className="mb-2 text-sm font-medium">Start from</legend>
          {blueprints.isPending && (
            <p className="text-sm text-muted-foreground">
              Loading blueprints…
            </p>
          )}
          {blueprints.isError && (
            <p role="alert" className="text-sm text-destructive">
              The blueprints could not be loaded.
            </p>
          )}
          {options.map((option) => (
            <label
              key={option.id}
              className="flex gap-3 rounded-md border p-3 text-sm has-checked:border-ring has-checked:bg-muted/50"
            >
              <input
                type="radio"
                name="blueprint"
                className="mt-0.5"
                checked={blueprintId === option.id}
                onChange={() => setBlueprintId(option.id)}
              />
              <span className="grid gap-1">
                <span className="flex items-center gap-2 font-medium">
                  {option.displayName}
                  {isSimulated(option.key) && (
                    <StatusBadge status="simulated" />
                  )}
                </span>
                <span className="text-muted-foreground">
                  {option.description}
                </span>
              </span>
            </label>
          ))}
        </fieldset>
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
        <Button
          className="sm:justify-self-end"
          type="submit"
          disabled={createWorkflow.isPending}
        >
          Create workflow
        </Button>
      </form>
    </>
  )
}
```

Create `apps/web/src/views/catalog-view.tsx`:

```tsx
"use client"

import { useBlueprints, useNodeDefinitions } from "@/api/queries"
import type { NodeDefinition } from "@/api/types"
import { StatusBadge } from "@/components/status/status-badge"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { isSimulated } from "@/studio/graph-model"

// The API orders definitions by key, then version.
function versionsByKey(definitions: NodeDefinition[]) {
  const groups = new Map<string, NodeDefinition[]>()
  for (const definition of definitions) {
    groups.set(definition.key, [
      ...(groups.get(definition.key) ?? []),
      definition,
    ])
  }
  return [...groups.values()]
}

export function CatalogView({ organizationId }: { organizationId: string }) {
  const definitions = useNodeDefinitions(organizationId)
  const blueprints = useBlueprints(organizationId)

  return (
    <section className="flex flex-col gap-5 p-4 md:p-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Catalog</h1>
        <p className="text-sm text-muted-foreground">
          Node definitions and blueprints available to this organization.
        </p>
      </div>
      {(definitions.isPending || blueprints.isPending) && (
        <p className="text-sm text-muted-foreground">Loading catalog…</p>
      )}
      {(definitions.isError || blueprints.isError) && (
        <p role="alert" className="text-sm text-destructive">
          The catalog could not be loaded.
        </p>
      )}
      <h2 id="catalog-definitions" className="text-lg font-semibold">
        Node definitions
      </h2>
      <div className="overflow-x-auto rounded-md border">
        <Table aria-labelledby="catalog-definitions">
          <TableHeader>
            <TableRow>
              <TableHead>Name</TableHead>
              <TableHead>Key</TableHead>
              <TableHead>Version</TableHead>
              <TableHead>Category</TableHead>
              <TableHead>Description</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {versionsByKey(definitions.data?.items ?? []).map((versions) => {
              const current = versions.findLast(
                (version) => !version.deprecated,
              )
              const shown = current ?? versions[versions.length - 1]
              return (
                <TableRow key={shown.key}>
                  <TableCell>
                    <span className="flex items-center gap-2 font-medium">
                      {shown.displayName}
                      {isSimulated(shown.key) && (
                        <StatusBadge status="simulated" />
                      )}
                    </span>
                  </TableCell>
                  <TableCell className="font-mono text-xs">
                    {shown.key}
                  </TableCell>
                  <TableCell>
                    {current && <span>v{current.version}</span>}
                    {versions
                      .filter((version) => version.deprecated)
                      .map((version) => (
                        <span
                          key={version.id}
                          className="block text-xs text-muted-foreground"
                        >
                          v{version.version} deprecated
                        </span>
                      ))}
                  </TableCell>
                  <TableCell>{shown.category}</TableCell>
                  <TableCell className="min-w-64 whitespace-normal text-muted-foreground">
                    {shown.description}
                  </TableCell>
                </TableRow>
              )
            })}
          </TableBody>
        </Table>
      </div>
      <h2 id="catalog-blueprints" className="text-lg font-semibold">
        Blueprints
      </h2>
      <div className="overflow-x-auto rounded-md border">
        <Table aria-labelledby="catalog-blueprints">
          <TableHeader>
            <TableRow>
              <TableHead>Name</TableHead>
              <TableHead>Key</TableHead>
              <TableHead>Description</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {blueprints.data?.items.map((blueprint) => (
              <TableRow key={blueprint.id}>
                <TableCell>
                  <span className="flex items-center gap-2 font-medium">
                    {blueprint.displayName}
                    {isSimulated(blueprint.key) && (
                      <StatusBadge status="simulated" />
                    )}
                  </span>
                </TableCell>
                <TableCell className="font-mono text-xs">
                  {blueprint.key}
                </TableCell>
                <TableCell className="min-w-64 whitespace-normal text-muted-foreground">
                  {blueprint.description}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </section>
  )
}
```

Create `apps/web/src/studio/studio.tsx`:

```tsx
"use client"

import { useWorkflow } from "@/api/queries"

// Placeholder until the canvas lands: it only proves the workflow loads.
export function Studio({
  organizationId,
  workflowId,
}: {
  organizationId: string
  workflowId: string
}) {
  const workflow = useWorkflow(organizationId, workflowId)

  return (
    <section className="flex flex-col gap-2 p-4 md:p-6">
      {workflow.isPending && (
        <p className="text-sm text-muted-foreground">Loading workflow…</p>
      )}
      {workflow.isError && (
        <p role="alert" className="text-sm text-destructive">
          The workflow could not be loaded.
        </p>
      )}
      {workflow.data && (
        <>
          <h1 className="text-2xl font-semibold tracking-tight">
            {workflow.data.name}
          </h1>
          <p className="text-sm text-muted-foreground">
            Draft revision {workflow.data.draft.revision}. The editor is not
            available yet.
          </p>
        </>
      )}
    </section>
  )
}
```

- [ ] **Step 7: Add the routes and the sidebar links**

Create `apps/web/src/app/(app)/orgs/[organizationId]/workflows/page.tsx`:

```tsx
import { WorkflowsView } from "@/views/workflows-view"

export default async function WorkflowsPage({
  params,
}: PageProps<"/orgs/[organizationId]/workflows">) {
  const { organizationId } = await params
  return <WorkflowsView organizationId={organizationId} />
}
```

Create `apps/web/src/app/(app)/orgs/[organizationId]/workflows/[workflowId]/page.tsx`:

```tsx
import { Studio } from "@/studio/studio"

export default async function WorkflowPage({
  params,
}: PageProps<"/orgs/[organizationId]/workflows/[workflowId]">) {
  const { organizationId, workflowId } = await params
  return <Studio organizationId={organizationId} workflowId={workflowId} />
}
```

Create `apps/web/src/app/(app)/orgs/[organizationId]/catalog/page.tsx`:

```tsx
import { CatalogView } from "@/views/catalog-view"

export default async function CatalogPage({
  params,
}: PageProps<"/orgs/[organizationId]/catalog">) {
  const { organizationId } = await params
  return <CatalogView organizationId={organizationId} />
}
```

The links are active by prefix, so the studio route also marks "Workflows" as the current page.

Replace the contents of `apps/web/src/layout/app-shell.tsx` with:

```tsx
"use client"

import Link from "next/link"
import { usePathname, useRouter } from "next/navigation"
import type { ReactNode } from "react"

import { api } from "@/api/client"
import { useAuth } from "@/auth/auth-context"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Separator } from "@/components/ui/separator"

const links = [
  { label: "Overview", suffix: "" },
  { label: "Workflows", suffix: "/workflows" },
  { label: "Catalog", suffix: "/catalog" },
  { label: "Members", suffix: "/members" },
  { label: "Audit log", suffix: "/audit" },
]

export function AppShell({
  organizationId,
  children,
}: {
  organizationId: string
  children: ReactNode
}) {
  const { me, refresh } = useAuth()
  const router = useRouter()
  const pathname = usePathname()
  const membership = me?.organizations.find(
    (organization) => organization.organizationId === organizationId,
  )
  if (!me || !membership) {
    return <p className="p-6">Organization not found.</p>
  }

  async function logout() {
    await api("/auth/logout", { method: "POST" })
    // With `me` cleared, the auth gate sends the browser to /login.
    await refresh()
  }

  return (
    <div className="min-h-screen bg-background md:grid md:grid-cols-[220px_1fr]">
      <a
        href="#main-content"
        className="sr-only z-50 rounded-md bg-background px-3 py-2 text-sm shadow focus:not-sr-only focus:fixed focus:top-3 focus:left-3"
      >
        Skip to content
      </a>
      <aside className="flex flex-wrap items-center border-b bg-sidebar p-3 text-sidebar-foreground md:min-h-screen md:flex-nowrap md:flex-col md:items-stretch md:border-r md:border-b-0">
        <div className="flex items-center gap-2 px-2 py-3">
          <span className="grid size-7 place-items-center rounded-md bg-foreground text-xs font-bold text-background">
            C
          </span>
          <span className="font-semibold tracking-tight">Corqty</span>
        </div>
        <select
          aria-label="Organization"
          className="ml-auto h-9 max-w-48 rounded-md border bg-background px-2 text-sm md:ml-0 md:max-w-none"
          value={membership.organizationId}
          onChange={(event) => router.push(`/orgs/${event.target.value}`)}
        >
          {me.organizations.map((organization) => (
            <option
              key={organization.organizationId}
              value={organization.organizationId}
            >
              {organization.name}
            </option>
          ))}
        </select>
        <Separator className="mx-3 hidden md:my-3 md:block" />
        <nav
          aria-label="Organization"
          className="order-3 mt-3 flex basis-full gap-1 overflow-x-auto md:order-none md:mt-0 md:flex-col"
        >
          {links.map((link) => {
            const href = `/orgs/${membership.organizationId}${link.suffix}`
            const active = link.suffix
              ? pathname.startsWith(href)
              : pathname === href
            return (
              <Link
                key={link.label}
                href={href}
                aria-current={active ? "page" : undefined}
                className={`rounded-md px-3 py-2 text-sm ${
                  active
                    ? "bg-sidebar-accent font-medium"
                    : "text-muted-foreground"
                }`}
              >
                {link.label}
              </Link>
            )
          })}
        </nav>
        <div className="ml-2 flex items-center gap-2 md:mt-auto md:ml-0 md:flex-col md:items-stretch md:gap-2 md:border-t md:pt-3">
          <span className="hidden truncate px-2 text-xs text-muted-foreground md:block">
            {me.user.email}
          </span>
          <div className="flex items-center justify-between gap-2 px-2">
            <Badge variant="secondary">{membership.role}</Badge>
            <Button variant="ghost" size="sm" onClick={() => void logout()}>
              Log out
            </Button>
          </div>
        </div>
      </aside>
      <main id="main-content" className="min-w-0">
        {children}
      </main>
    </div>
  )
}
```

- [ ] **Step 8: Verify GREEN**

Run:

```text
pnpm web:test
```

Expected: `Test Files  5 passed (5)` and `Tests  16 passed (16)` with no unhandled errors. On Node 25 or newer,
each worker may print `ExperimentalWarning: localStorage is not available because --localstorage-file was not
provided.`; this is harmless.

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

Expected: `✓ Compiled successfully`, `Finished TypeScript`, and a route table that adds
`ƒ /orgs/[organizationId]/catalog`, `ƒ /orgs/[organizationId]/workflows` and
`ƒ /orgs/[organizationId]/workflows/[workflowId]` to the Task 7 routes. It exits 0.

- [ ] **Step 10: Commit**

```text
git add apps/web
git commit -m "feat: list workflows, browse the catalog, and create workflows from blueprints"
```

## Task 16: Build the studio canvas with typed, cycle-safe connections

**Files:**

- Create: `apps/web/src/studio/studio-node.tsx`
- Create: `apps/web/src/studio/palette.tsx`
- Create: `apps/web/src/studio/studio-canvas.tsx`
- Modify: `apps/web/package.json`
- Modify: `pnpm-lock.yaml`
- Modify: `apps/web/src/api/types.ts`
- Modify: `apps/web/src/test/handlers.ts`
- Modify: `apps/web/src/test/setup.ts`
- Modify: `apps/web/src/studio/graph-model.ts`
- Modify: `apps/web/src/studio/studio.tsx`
- Modify: `apps/web/src/app/globals.css`
- Test: `apps/web/src/studio/graph-model.test.ts` (new)
- Test: `apps/web/src/studio/studio.test.tsx` (new)

The studio route gets its editor, built on `@xyflow/react` 12.12.0. `src/studio/graph-model.ts` holds the pure
graph rules: `toFlow` and `fromFlow` map a `GraphSnapshot` onto XYFlow nodes and edges and fold positions and
edges back; `compatible`, `hasCycle` and `wouldCreateCycle` mirror the Rust domain; `connectionError` returns
`"self"`, `"occupied"`, `"bound"` (the target node already has `inputs[port]`), `"incompatible"` or `"cycle"`
in that order, or null, and `refusalMessage` turns it into the sentence the studio announces;
`latestDefinitions` keeps the highest non-deprecated version of each key; `newNodeFromDefinition` and
`insertBlueprint` (D14) create nodes and edges with fresh `crypto.randomUUID()` ids. The compatibility and
cycle tests read the shared fixture `crates/domain/tests/fixtures/graph-rules.json` with `readFileSync`
(D15), so the web app and the Rust domain check one table.

`Studio` loads the workflow, the node definitions and the blueprints through the Task 15 queries. The canvas
is client-only (`next/dynamic(..., { ssr: false })` inside the `"use client"` studio file) because XYFlow
measures the DOM and follows the system color scheme (`colorMode="system"`, D17); `globals.css` points
XYFlow's background, node border and edge stroke at the semantic tokens. Each canvas node keeps its
`GraphNode` in `data`, XYFlow owns positions and edges while editing, and edits stay in local state until
Task 18 saves them. The canvas opens at the snapshot's stored viewport. Ports render as `in:<port>` and
`out:<port>` handles labelled with the port name and data type. `isValidConnection` accepts a connection only
when `connectionError` returns null; when a dragged connection is dropped on a port and refused, the
`aria-live="polite"` status line under the canvas says why (for example `Incompatible types: core.string ->
aws.vpc-id`). The palette lists `latestDefinitions` and the blueprints, filtered case-insensitively on display
name and key, and its items carry their id as `application/x-corqty-definition` or
`application/x-corqty-blueprint`. Without `edit_workflows`, or when `matchMedia("(min-width: 1024px)")` does
not match, the studio is read-only: no palette and no drops, nodes are neither draggable nor connectable,
the delete key is off, and a Read-only badge shows the reason.

The API types gain the definition spec, the graph snapshot types, `Blueprint.graph` and
`Workflow.draft.graph`. The MSW fixtures gain the seeded specs and blueprint graphs (§8) and a default
`GET /api/v1/orgs/:organizationId/workflows/:workflowId` that returns "Provision staging" (wf-2), created
from the simulated infrastructure blueprint. jsdom lacks the layout APIs XYFlow uses, so `setup.ts` stubs
them, and its `matchMedia` answers `min-width` queries from `window.innerWidth`, which tests can change.

- [ ] **Step 1: Add XYFlow**

Run:

```text
pnpm --filter @corqty/web add @xyflow/react@12.12.0
```

Expected: `Done`, the exact version (no `^`) in `apps/web/package.json`, and an updated `pnpm-lock.yaml`. pnpm
repeats the known `✕ unmet peer msw@^2.4.9: found 3.0.3` warning for `@vitest/mocker 4.1.10` (Task 6); it
only concerns Vitest browser mode.

`apps/web/package.json` is now:

```json
{
  "name": "@corqty/web",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "build": "next build",
    "dev": "next dev -H 0.0.0.0 -p 3000",
    "lint": "next typegen && tsc --noEmit && oxlint",
    "test": "vitest run"
  },
  "dependencies": {
    "@base-ui/react": "^1.6.0",
    "@tanstack/react-query": "5.104.5",
    "@xyflow/react": "12.12.0",
    "class-variance-authority": "^0.7.1",
    "clsx": "^2.1.1",
    "lucide-react": "^1.27.0",
    "next": "16.4.3",
    "react": "^19.3.1",
    "react-dom": "^19.3.1",
    "tailwind-merge": "^3.6.0"
  },
  "devDependencies": {
    "@tailwindcss/postcss": "4.3.3",
    "@testing-library/jest-dom": "^7.0.0",
    "@testing-library/react": "^16.3.2",
    "@testing-library/user-event": "^14.6.1",
    "@types/node": "^26.1.2",
    "@types/react": "^19.3.0",
    "@types/react-dom": "^19.3.0",
    "@vitejs/plugin-react": "^6.0.4",
    "jsdom": "^30.0.0",
    "msw": "3.0.3",
    "oxlint": "1.89.0",
    "postcss": "8.5.31",
    "shadcn": "^4.16.0",
    "tailwindcss": "^4.3.3",
    "tw-animate-css": "^1.4.0",
    "typescript": "^7.0.2",
    "vite": "^8.1.5",
    "vitest": "^4.1.10"
  }
}
```

- [ ] **Step 2: Add the graph types and the catalog fixtures**

Replace the contents of `apps/web/src/api/types.ts` with:

```ts
export type Role = "admin" | "editor" | "runner" | "viewer"

export type Capability =
  | "view_organization"
  | "edit_workflows"
  | "publish_workflows"
  | "run_workflows"
  | "manage_members"
  | "manage_service_tokens"
  | "manage_secrets"
  | "manage_plugins"
  | "enable_shell_executor"

export interface OrganizationMembership {
  organizationId: string
  name: string
  slug: string
  role: Role
  capabilities: Capability[]
}

export interface Me {
  user: { id: string; email: string }
  organizations: OrganizationMembership[]
}

export interface Diagnostic {
  code: string
  pointer: string
  message: string
}

export interface Member {
  userId: string
  email: string
  role: Role
}

export interface Invitation {
  token: string
  inviteUrl?: string
  expiresAt: string
}

export interface AuditEvent {
  id: number
  actorUserId: string | null
  action: string
  resourceType: string
  resourceId: string
  metadata: Record<string, unknown>
  occurredAt: string
}

// The catalog and workflow types list only the fields the UI reads; the API
// also sends scope, slugs and timestamps.
export type JsonSchema = {
  [keyword: string]: unknown
  properties?: Record<string, JsonSchema>
  required?: string[]
}

export interface PortSpec {
  name: string
  dataType: string
  schema: JsonSchema
  required: boolean
  description: string
}

export interface UiField {
  widget: string
  label?: string | null
  help?: string | null
}

export interface NodeDefinition {
  id: string
  key: string
  version: number
  displayName: string
  description: string
  category: string
  deprecated: boolean
  inputs: PortSpec[]
  outputs: PortSpec[]
  configSchema: JsonSchema
  uiSchema: Record<string, UiField>
  defaults: Record<string, unknown>
  executor: { key: string; version: string }
}

export type InputBinding =
  | { kind: "literal"; value: unknown }
  | { kind: "workflowInput"; pointer: string }
  | { kind: "secret"; secretId: string }

export interface GraphNode {
  id: string
  definition: { key: string; version: number }
  displayName: string
  position: { x: number; y: number }
  config: Record<string, unknown>
  inputs: Record<string, InputBinding>
  retry: { maxAttempts: number }
  timeoutSeconds: number
}

export interface GraphEdge {
  id: string
  source: { nodeId: string; port: string }
  target: { nodeId: string; port: string }
}

export interface GraphSnapshot {
  schemaVersion: number
  workflowInputSchema: JsonSchema
  nodes: GraphNode[]
  edges: GraphEdge[]
  viewport: { x: number; y: number; zoom: number }
}

export interface Blueprint {
  id: string
  key: string
  displayName: string
  description: string
  graph: GraphSnapshot
}

export interface WorkflowSummary {
  id: string
  name: string
  draftRevision: number
  publishedVersion: number | null
  updatedAt: string
}

export interface Workflow {
  id: string
  name: string
  draft: { revision: number; graph: GraphSnapshot }
}
```

In `apps/web/src/test/handlers.ts`, apply these replacements in order:

Replace:

```ts
import type {
  AuditEvent,
  Blueprint,
  Capability,
  Me,
  Member,
  NodeDefinition,
  Role,
  WorkflowSummary,
} from "@/api/types"
```

With:

```ts
import type {
  AuditEvent,
  Blueprint,
  Capability,
  GraphEdge,
  GraphNode,
  JsonSchema,
  Me,
  Member,
  NodeDefinition,
  PortSpec,
  Role,
  Workflow,
  WorkflowSummary,
} from "@/api/types"
```

Replace:

```ts
// The seeded system catalog, in API order (key, then version).
export const nodeDefinitions: NodeDefinition[] = [
  {
    id: "00000000-0000-7000-8000-000000000005",
    key: "corqty.mock.normalize-payload",
    version: 1,
    displayName: "Normalize payload",
    description: "Normalizes a JSON payload.",
    category: "Data processing",
    deprecated: false,
  },
  {
    id: "00000000-0000-7000-8000-000000000006",
    key: "corqty.mock.produce-summary",
    version: 1,
    displayName: "Produce summary",
    description: "Produces a text summary of normalized data.",
    category: "Data processing",
    deprecated: false,
  },
  {
    id: "00000000-0000-7000-8000-000000000001",
    key: "corqty.mock.task",
    version: 1,
    displayName: "Mock task",
    description: "Returns a fixed JSON result or echoes its input.",
    category: "Testing",
    deprecated: false,
  },
  {
    id: "00000000-0000-7000-8000-000000000003",
    key: "corqty.simulated.create-rds",
    version: 1,
    displayName: "Create RDS (simulated)",
    description: "Simulated: pretends to create a database inside a VPC.",
    category: "Infrastructure (simulated)",
    deprecated: false,
  },
  {
    id: "00000000-0000-7000-8000-000000000002",
    key: "corqty.simulated.create-vpc",
    version: 1,
    displayName: "Create VPC (simulated)",
    description: "Simulated: pretends to create a VPC and returns its id.",
    category: "Infrastructure (simulated)",
    deprecated: false,
  },
  {
    id: "00000000-0000-7000-8000-000000000004",
    key: "corqty.simulated.deploy-service",
    version: 1,
    displayName: "Deploy service (simulated)",
    description: "Simulated: pretends to deploy a service into a VPC.",
    category: "Infrastructure (simulated)",
    deprecated: false,
  },
]

export const blueprints: Blueprint[] = [
  {
    id: "00000000-0000-7000-8000-000000000102",
    key: "corqty.data-processing",
    displayName: "Data processing",
    description:
      "Normalizes a JSON payload and produces a summary with the mock executor.",
  },
  {
    id: "00000000-0000-7000-8000-000000000101",
    key: "corqty.simulated-infrastructure",
    displayName: "Simulated infrastructure",
    description:
      "Simulated: every step uses the mock executor. Creates a VPC, then a database and a service in parallel.",
  },
]

// A signed-in admin of org-1 reading data; tests override writes and failures.
export const handlers = [
  http.get("/api/v1/auth/me", () => HttpResponse.json(meAs("admin"))),
  http.get("/api/v1/orgs/:organizationId/members", () =>
    HttpResponse.json(members),
  ),
  http.get("/api/v1/orgs/:organizationId/audit-events", () =>
    HttpResponse.json({ items: auditEvents }),
  ),
  http.get("/api/v1/orgs/:organizationId/workflows", () =>
    HttpResponse.json({ items: workflows }),
  ),
  http.get("/api/v1/orgs/:organizationId/node-definitions", () =>
    HttpResponse.json({ items: nodeDefinitions }),
  ),
  http.get("/api/v1/orgs/:organizationId/blueprints", () =>
    HttpResponse.json({ items: blueprints }),
  ),
]
```

With:

```ts
const mockConfigSchema: JsonSchema = {
  type: "object",
  properties: {
    mode: { type: "string", enum: ["fixed", "echo"], title: "Mode" },
    output: { type: "object", title: "Fixed output" },
    delayMs: {
      type: "integer",
      minimum: 0,
      maximum: 60000,
      title: "Delay (ms)",
    },
    failureMessage: {
      type: ["string", "null"],
      maxLength: 500,
      title: "Fail with message",
    },
  },
  required: ["mode", "output", "delayMs"],
  additionalProperties: false,
}

function port(
  name: string,
  dataType: string,
  schema: JsonSchema,
  required = false,
): PortSpec {
  return { name, dataType, schema, required, description: "" }
}

// Every seeded definition is version 1 and runs on the mock executor.
function seeded(
  definition: Pick<
    NodeDefinition,
    | "id"
    | "key"
    | "displayName"
    | "description"
    | "category"
    | "inputs"
    | "outputs"
  >,
  output: Record<string, unknown>,
  delayMs = 500,
): NodeDefinition {
  return {
    ...definition,
    version: 1,
    deprecated: false,
    configSchema: mockConfigSchema,
    uiSchema: {
      mode: { widget: "select" },
      output: { widget: "code" },
      delayMs: { widget: "number" },
      failureMessage: { widget: "text" },
    },
    defaults: { mode: "fixed", output, delayMs, failureMessage: null },
    executor: { key: "mock", version: "1.0.0" },
  }
}

const vpcIdSchema: JsonSchema = { type: "string", pattern: "^vpc-" }

// The seeded system catalog, in API order (key, then version).
export const nodeDefinitions: NodeDefinition[] = [
  seeded(
    {
      id: "00000000-0000-7000-8000-000000000005",
      key: "corqty.mock.normalize-payload",
      displayName: "Normalize payload",
      description: "Normalizes a JSON payload.",
      category: "Data processing",
      inputs: [port("payload", "core.object", { type: "object" }, true)],
      outputs: [port("normalized", "core.object", { type: "object" })],
    },
    { normalized: { records: 3 } },
  ),
  seeded(
    {
      id: "00000000-0000-7000-8000-000000000006",
      key: "corqty.mock.produce-summary",
      displayName: "Produce summary",
      description: "Produces a text summary of normalized data.",
      category: "Data processing",
      inputs: [port("normalized", "core.object", { type: "object" }, true)],
      outputs: [port("summary", "core.string", { type: "string" })],
    },
    { summary: "3 records processed" },
  ),
  seeded(
    {
      id: "00000000-0000-7000-8000-000000000001",
      key: "corqty.mock.task",
      displayName: "Mock task",
      description: "Returns a fixed JSON result or echoes its input.",
      category: "Testing",
      inputs: [port("input", "core.any", {})],
      outputs: [port("result", "core.any", {})],
    },
    { result: { ok: true } },
    0,
  ),
  seeded(
    {
      id: "00000000-0000-7000-8000-000000000003",
      key: "corqty.simulated.create-rds",
      displayName: "Create RDS (simulated)",
      description: "Simulated: pretends to create a database inside a VPC.",
      category: "Infrastructure (simulated)",
      inputs: [port("vpcId", "aws.vpc-id", vpcIdSchema, true)],
      outputs: [port("endpoint", "core.string", { type: "string" })],
    },
    { endpoint: "db.simulated.internal:5432" },
  ),
  seeded(
    {
      id: "00000000-0000-7000-8000-000000000002",
      key: "corqty.simulated.create-vpc",
      displayName: "Create VPC (simulated)",
      description: "Simulated: pretends to create a VPC and returns its id.",
      category: "Infrastructure (simulated)",
      inputs: [
        port("region", "core.string", { type: "string", minLength: 1 }, true),
      ],
      outputs: [port("vpcId", "aws.vpc-id", vpcIdSchema)],
    },
    { vpcId: "vpc-0simulated" },
  ),
  seeded(
    {
      id: "00000000-0000-7000-8000-000000000004",
      key: "corqty.simulated.deploy-service",
      displayName: "Deploy service (simulated)",
      description: "Simulated: pretends to deploy a service into a VPC.",
      category: "Infrastructure (simulated)",
      inputs: [port("vpcId", "aws.vpc-id", vpcIdSchema, true)],
      outputs: [port("url", "core.string", { type: "string" })],
    },
    { url: "https://service.simulated.internal" },
  ),
]

// A blueprint node as the seed writes it: the definition's name and defaults.
function blueprintNode(
  id: string,
  key: string,
  [x, y]: [number, number],
  inputs: GraphNode["inputs"] = {},
): GraphNode {
  const definition = nodeDefinitions.find((candidate) => candidate.key === key)
  if (!definition) throw new Error(`No seeded definition ${key}`)
  return {
    id,
    definition: { key, version: 1 },
    displayName: definition.displayName,
    position: { x, y },
    config: structuredClone(definition.defaults),
    inputs,
    retry: { maxAttempts: 1 },
    timeoutSeconds: 900,
  }
}

// Seeded edges join ports that share a name.
function blueprintEdge(
  source: string,
  target: string,
  port: string,
): GraphEdge {
  return {
    id: `${source}-${target}`,
    source: { nodeId: source, port },
    target: { nodeId: target, port },
  }
}

export const blueprints: Blueprint[] = [
  {
    id: "00000000-0000-7000-8000-000000000102",
    key: "corqty.data-processing",
    displayName: "Data processing",
    description:
      "Normalizes a JSON payload and produces a summary with the mock executor.",
    graph: {
      schemaVersion: 1,
      workflowInputSchema: {
        type: "object",
        properties: { payload: { type: "object" } },
        required: ["payload"],
        additionalProperties: false,
      },
      nodes: [
        blueprintNode("normalize", "corqty.mock.normalize-payload", [0, 0], {
          payload: { kind: "workflowInput", pointer: "/payload" },
        }),
        blueprintNode("summary", "corqty.mock.produce-summary", [300, 0]),
      ],
      edges: [blueprintEdge("normalize", "summary", "normalized")],
      viewport: { x: 0, y: 0, zoom: 1 },
    },
  },
  {
    id: "00000000-0000-7000-8000-000000000101",
    key: "corqty.simulated-infrastructure",
    displayName: "Simulated infrastructure",
    description:
      "Simulated: every step uses the mock executor. Creates a VPC, then a database and a service in parallel.",
    graph: {
      schemaVersion: 1,
      workflowInputSchema: {
        type: "object",
        properties: {
          region: { type: "string", minLength: 1, default: "us-east-1" },
        },
        required: ["region"],
        additionalProperties: false,
      },
      nodes: [
        blueprintNode("vpc", "corqty.simulated.create-vpc", [0, 0], {
          region: { kind: "workflowInput", pointer: "/region" },
        }),
        blueprintNode("rds", "corqty.simulated.create-rds", [300, -100]),
        blueprintNode("deploy", "corqty.simulated.deploy-service", [300, 100]),
      ],
      edges: [
        blueprintEdge("vpc", "rds", "vpcId"),
        blueprintEdge("vpc", "deploy", "vpcId"),
      ],
      viewport: { x: 0, y: 0, zoom: 1 },
    },
  },
]

// "Provision staging" was created from the simulated infrastructure blueprint.
export const workflow: Workflow = {
  id: "wf-2",
  name: "Provision staging",
  draft: { revision: 3, graph: blueprints[1].graph },
}

// A signed-in admin of org-1 reading data; tests override writes and failures.
export const handlers = [
  http.get("/api/v1/auth/me", () => HttpResponse.json(meAs("admin"))),
  http.get("/api/v1/orgs/:organizationId/members", () =>
    HttpResponse.json(members),
  ),
  http.get("/api/v1/orgs/:organizationId/audit-events", () =>
    HttpResponse.json({ items: auditEvents }),
  ),
  http.get("/api/v1/orgs/:organizationId/workflows", () =>
    HttpResponse.json({ items: workflows }),
  ),
  http.get("/api/v1/orgs/:organizationId/workflows/:workflowId", () =>
    HttpResponse.json(workflow),
  ),
  http.get("/api/v1/orgs/:organizationId/node-definitions", () =>
    HttpResponse.json({ items: nodeDefinitions }),
  ),
  http.get("/api/v1/orgs/:organizationId/blueprints", () =>
    HttpResponse.json({ items: blueprints }),
  ),
]
```

- [ ] **Step 3: Give jsdom the layout APIs XYFlow needs**

XYFlow measures nodes with `ResizeObserver`, `DOMMatrixReadOnly`, `offsetWidth`/`offsetHeight` and
`getBBox`. The observer stub must report a `contentRect`, or XYFlow's pan-zoom throws on `width`. Elements
report the size of their inline style (1 px otherwise), which is enough to measure nodes and handles and to
draw edges. jsdom also has no `matchMedia`; the stub answers `min-width` queries from `window.innerWidth`
(1024 in jsdom), and every test starts at 1024 again.

Replace the contents of `apps/web/src/test/setup.ts` with:

```ts
import "@testing-library/jest-dom/vitest"
import { cleanup } from "@testing-library/react"
import { afterAll, afterEach, beforeAll, vi } from "vitest"

import { server } from "@/test/server"

vi.mock("next/navigation", async () => {
  const { router } = await import("@/test/render")
  return {
    usePathname: () => window.location.pathname,
    useRouter: () => router,
  }
})

// XYFlow measures nodes with layout APIs that jsdom lacks. Elements report
// the size of their inline style (1 px otherwise), and the observer reports it.
class ResizeObserverStub {
  callback: ResizeObserverCallback
  constructor(callback: ResizeObserverCallback) {
    this.callback = callback
  }
  observe(target: HTMLElement) {
    setTimeout(() => {
      const { offsetWidth: width, offsetHeight: height } = target
      const contentRect = { x: 0, y: 0, top: 0, left: 0, width, height }
      this.callback(
        [{ target, contentRect } as unknown as ResizeObserverEntry],
        this as unknown as ResizeObserver,
      )
    })
  }
  unobserve() {}
  disconnect() {}
}

class DOMMatrixReadOnlyStub {
  m22: number
  constructor(transform: string) {
    const scale = /scale\(([\d.]+)\)/.exec(transform)?.[1]
    this.m22 = scale === undefined ? 1 : Number(scale)
  }
}

Object.assign(globalThis, {
  ResizeObserver: ResizeObserverStub,
  DOMMatrixReadOnly: DOMMatrixReadOnlyStub,
})
Object.defineProperties(HTMLElement.prototype, {
  offsetHeight: {
    get(this: HTMLElement) {
      return parseFloat(this.style.height) || 1
    },
  },
  offsetWidth: {
    get(this: HTMLElement) {
      return parseFloat(this.style.width) || 1
    },
  },
})
Object.assign(SVGElement.prototype, {
  getBBox: () => ({ x: 0, y: 0, width: 0, height: 0 }),
})

// jsdom has no matchMedia either. min-width queries compare with
// window.innerWidth (1024 in jsdom), which tests may change; nothing else
// matches.
Object.assign(window, {
  matchMedia: (query: string) => ({
    media: query,
    matches:
      window.innerWidth >=
      Number(/\(min-width: (\d+)px\)/.exec(query)?.[1] ?? Infinity),
    addEventListener() {},
    removeEventListener() {},
  }),
})

beforeAll(() => server.listen({ onUnhandledFrame: "error" }))
afterEach(() => {
  cleanup()
  server.resetHandlers()
  sessionStorage.clear()
  vi.clearAllMocks()
  Object.assign(window, { innerWidth: 1024 })
})
afterAll(() => server.close())
```

- [ ] **Step 4: Write the failing graph-model tests**

Create `apps/web/src/studio/graph-model.test.ts`:

```ts
import { readFileSync } from "node:fs"
import { resolve } from "node:path"
import { expect, it } from "vitest"

import type {
  GraphNode,
  GraphSnapshot,
  NodeDefinition,
  PortSpec,
} from "@/api/types"
import {
  compatible,
  connectionError,
  fromFlow,
  hasCycle,
  insertBlueprint,
  latestDefinitions,
  newNodeFromDefinition,
  refusalMessage,
  toFlow,
  wouldCreateCycle,
} from "@/studio/graph-model"
import { blueprints, nodeDefinitions } from "@/test/handlers"

// The Rust domain tests read the same file, so both sides share one rule set.
const rules: {
  compatibility: { source: string; target: string; compatible: boolean }[]
  cycles: { name: string; edges: [string, string][]; hasCycle: boolean }[]
} = JSON.parse(
  readFileSync(
    resolve(
      import.meta.dirname,
      "../../../../crates/domain/tests/fixtures/graph-rules.json",
    ),
    "utf8",
  ),
)

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/
const infrastructure = blueprints[1] // corqty.simulated-infrastructure

function definition(key: string) {
  const found = nodeDefinitions.find((candidate) => candidate.key === key)
  if (!found) throw new Error(`No seeded definition ${key}`)
  return found
}

function node(
  id: string,
  from: NodeDefinition,
  inputs: GraphNode["inputs"] = {},
): GraphNode {
  return { ...newNodeFromDefinition(from, { x: 0, y: 0 }), id, inputs }
}

function port(name: string, dataType: string): PortSpec {
  return { name, dataType, schema: {}, required: false, description: "" }
}

function connect(
  source: string,
  output: string,
  target: string,
  input: string,
) {
  return {
    source,
    sourceHandle: `out:${output}`,
    target,
    targetHandle: `in:${input}`,
  }
}

it("follows the shared compatibility and cycle rules", () => {
  expect(rules.compatibility.length).toBeGreaterThan(0)
  for (const rule of rules.compatibility) {
    expect(
      compatible(rule.source, rule.target),
      `${rule.source} -> ${rule.target}`,
    ).toBe(rule.compatible)
  }
  expect(rules.cycles.length).toBeGreaterThan(0)
  for (const rule of rules.cycles) {
    expect(hasCycle(rule.edges), rule.name).toBe(rule.hasCycle)
  }
  const chain: [string, string][] = [
    ["a", "b"],
    ["b", "c"],
  ]
  expect(wouldCreateCycle(chain, ["c", "a"])).toBe(true)
  expect(wouldCreateCycle(chain, ["a", "c"])).toBe(false)
})

it("names the reason a connection is refused", () => {
  const mock = definition("corqty.mock.task")
  // Organization definitions with numeric ports, for integer -> number.
  const counter = {
    ...mock,
    id: "acme-counter",
    key: "acme.counter",
    inputs: [port("limit", "core.integer")],
    outputs: [port("count", "core.integer")],
  }
  const average = {
    ...mock,
    id: "acme-average",
    key: "acme.average",
    inputs: [port("value", "core.number")],
    outputs: [port("mean", "core.number")],
  }
  const definitions = [...nodeDefinitions, counter, average]
  // vpc (region bound) feeds rds and deploy; a feeds b.
  const graph: GraphSnapshot = {
    ...infrastructure.graph,
    nodes: [
      ...infrastructure.graph.nodes,
      node("a", mock),
      node("b", mock),
      node("summary", definition("corqty.mock.produce-summary")),
      node("counter", counter),
      node("average", average),
    ],
    edges: [
      ...infrastructure.graph.edges,
      {
        id: "a-b",
        source: { nodeId: "a", port: "result" },
        target: { nodeId: "b", port: "input" },
      },
    ],
  }
  const check = (connection: ReturnType<typeof connect>) =>
    connectionError(graph, definitions, connection)

  expect(check(connect("a", "result", "a", "input"))).toBe("self")
  expect(check(connect("vpc", "vpcId", "rds", "vpcId"))).toBe("occupied")
  expect(check(connect("summary", "summary", "vpc", "region"))).toBe("bound")
  expect(check(connect("rds", "endpoint", "summary", "normalized"))).toBe(
    "incompatible",
  )
  expect(check(connect("average", "mean", "counter", "limit"))).toBe(
    "incompatible",
  )
  expect(check(connect("b", "result", "a", "input"))).toBe("cycle")
  expect(check(connect("counter", "count", "average", "value"))).toBeNull()
  expect(check(connect("rds", "endpoint", "a", "input"))).toBeNull()

  const message = (connection: ReturnType<typeof connect>) =>
    refusalMessage(graph, definitions, connection)
  expect(message(connect("rds", "endpoint", "summary", "normalized"))).toBe(
    "Incompatible types: core.string -> core.object",
  )
  expect(message(connect("b", "result", "a", "input"))).toBe(
    "This connection would create a cycle",
  )
  expect(message(connect("rds", "endpoint", "a", "input"))).toBeNull()
})

it("offers the highest non-deprecated version of each key", () => {
  const mock = definition("corqty.mock.task")
  const versions = [
    { ...mock, id: "old-1", key: "acme.old", deprecated: true },
    { ...mock, id: "thing-1", key: "acme.thing" },
    { ...mock, id: "thing-2", key: "acme.thing", version: 2 },
    { ...mock, id: "thing-3", key: "acme.thing", version: 3, deprecated: true },
    mock,
  ]
  expect(latestDefinitions(versions).map(({ id }) => id)).toEqual([
    "thing-2",
    mock.id,
  ])
})

it("creates a node from a definition with a fresh id and its defaults", () => {
  const vpc = definition("corqty.simulated.create-vpc")
  const first = newNodeFromDefinition(vpc, { x: 40, y: 80 })
  expect(first).toEqual({
    id: expect.stringMatching(uuid),
    definition: { key: "corqty.simulated.create-vpc", version: 1 },
    displayName: "Create VPC (simulated)",
    position: { x: 40, y: 80 },
    config: {
      mode: "fixed",
      output: { vpcId: "vpc-0simulated" },
      delayMs: 500,
      failureMessage: null,
    },
    inputs: {},
    retry: { maxAttempts: 1 },
    timeoutSeconds: 900,
  })
  expect(first.config.output).not.toBe(vpc.defaults.output)
  expect(newNodeFromDefinition(vpc, { x: 0, y: 0 }).id).not.toBe(first.id)
})

it("inserts a blueprint at the drop point with fresh ids and merged inputs", () => {
  const existing = node("existing", definition("corqty.mock.task"))
  const graph: GraphSnapshot = {
    schemaVersion: 1,
    workflowInputSchema: {
      type: "object",
      properties: {
        region: { type: "string", default: "eu-west-1" },
        size: { type: "integer" },
      },
      required: ["size"],
    },
    nodes: [existing],
    edges: [],
    viewport: { x: 10, y: 20, zoom: 2 },
  }

  const result = insertBlueprint(graph, infrastructure, { x: 100, y: 50 })

  const [vpc, rds, deploy] = result.nodes.slice(1)
  expect(result.nodes[0]).toBe(existing)
  expect([vpc, rds, deploy].map(({ position }) => position)).toEqual([
    { x: 100, y: 50 },
    { x: 400, y: -50 },
    { x: 400, y: 150 },
  ])
  const ids = [vpc.id, rds.id, deploy.id, ...result.edges.map(({ id }) => id)]
  expect(new Set(ids).size).toBe(5)
  for (const id of ids) expect(id).toMatch(uuid)
  expect({ ...vpc, id: "vpc", position: { x: 0, y: 0 } }).toEqual(
    infrastructure.graph.nodes[0],
  )
  expect(vpc.config).not.toBe(infrastructure.graph.nodes[0].config)
  expect(result.edges.map(({ source, target }) => [source, target])).toEqual([
    [
      { nodeId: vpc.id, port: "vpcId" },
      { nodeId: rds.id, port: "vpcId" },
    ],
    [
      { nodeId: vpc.id, port: "vpcId" },
      { nodeId: deploy.id, port: "vpcId" },
    ],
  ])
  expect(result.workflowInputSchema).toEqual({
    type: "object",
    properties: {
      region: { type: "string", default: "eu-west-1" },
      size: { type: "integer" },
    },
    required: ["size", "region"],
  })
  expect(result.viewport).toEqual(graph.viewport)
  expect(graph.nodes).toEqual([existing])
})

it("maps a graph onto the canvas and positions back into the graph", () => {
  const { graph } = infrastructure
  const flow = toFlow(graph, nodeDefinitions)
  expect(flow.nodes[0]).toMatchObject({
    id: "vpc",
    type: "step",
    position: { x: 0, y: 0 },
    data: {
      node: graph.nodes[0],
      definition: definition("corqty.simulated.create-vpc"),
    },
  })
  expect(flow.edges[0]).toEqual({
    id: "vpc-rds",
    source: "vpc",
    sourceHandle: "out:vpcId",
    target: "rds",
    targetHandle: "in:vpcId",
  })

  const moved = flow.nodes.map((flowNode) => ({
    ...flowNode,
    position: { x: flowNode.position.x + 5, y: flowNode.position.y - 5 },
  }))

  expect(fromFlow(graph, moved, flow.edges)).toEqual({
    ...graph,
    nodes: graph.nodes.map((graphNode) => ({
      ...graphNode,
      position: { x: graphNode.position.x + 5, y: graphNode.position.y - 5 },
    })),
  })
})
```

- [ ] **Step 5: Write the failing studio tests**

The tests render `Studio` inside `AuthGate` and `AppShell`, as the `(app)` and `orgs/[organizationId]` layouts
do. jsdom has neither `DragEvent` nor `DataTransfer`, so `drop` fires a plain event with a `getData` stub and
sets `clientX`/`clientY` on it. It has no `elementFromPoint` either, so `dragConnection` points XYFlow's hit
test at the target handle, presses the source handle and releases next to the target node. The canvas module
is imported up front so `next/dynamic` resolves within the `findBy` timeouts on a cold start.

Create `apps/web/src/studio/studio.test.tsx`:

```tsx
import {
  createEvent,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { http, HttpResponse } from "msw/http"
import { expect, it } from "vitest"

import type { GraphSnapshot } from "@/api/types"
import { AuthGate } from "@/auth/auth-gate"
import { AppShell } from "@/layout/app-shell"
import { Studio } from "@/studio/studio"
// Loaded up front so next/dynamic resolves within the findBy timeouts even
// on a cold start.
import "@/studio/studio-canvas"
import { blueprints, meAs, nodeDefinitions, workflow } from "@/test/handlers"
import { renderWithProviders } from "@/test/render"
import { server } from "@/test/server"

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/
const mockTask = nodeDefinitions[2] // corqty.mock.task
const infrastructure = blueprints[1] // corqty.simulated-infrastructure

// Mirrors the (app) and orgs/[organizationId] layouts around the studio page.
function renderStudio() {
  return renderWithProviders(
    <AuthGate>
      <AppShell organizationId="org-1">
        <Studio organizationId="org-1" workflowId="wf-2" />
      </AppShell>
    </AuthGate>,
    { route: "/orgs/org-1/workflows/wf-2" },
  )
}

function withDraft(graph: GraphSnapshot) {
  server.use(
    http.get("/api/v1/orgs/:organizationId/workflows/:workflowId", () =>
      HttpResponse.json({ ...workflow, draft: { revision: 1, graph } }),
    ),
  )
}

async function findCanvas() {
  return screen.findByTestId("rf__wrapper")
}

// jsdom has no DataTransfer or DragEvent: hand XYFlow what the palette would
// put on a drag, on a plain event that carries the pointer position.
function drop(
  pane: HTMLElement,
  format: string,
  id: string,
  [clientX, clientY] = [40, 60],
) {
  const dataTransfer = {
    getData: (requested: string) => (requested === format ? id : ""),
    dropEffect: "none",
  }
  fireEvent.dragOver(pane, { dataTransfer })
  const event = createEvent.drop(pane, { dataTransfer })
  Object.assign(event, { clientX, clientY })
  fireEvent(pane, event)
}

function handle(nodeId: string, handleId: string) {
  const found = document.querySelector<HTMLElement>(
    `[data-nodeid="${nodeId}"][data-handleid="${handleId}"]`,
  )
  if (!found) throw new Error(`No handle ${nodeId} ${handleId}`)
  return found
}

// jsdom has no layout either, so XYFlow's hit test is pointed at the target
// handle while the pointer moves next to the target node at `at`.
function dragConnection(
  [sourceNode, sourceHandle]: [string, string],
  [targetNode, targetHandle]: [string, string],
  [clientX, clientY]: [number, number],
) {
  const target = handle(targetNode, targetHandle)
  document.elementFromPoint = () => target
  fireEvent.mouseDown(handle(sourceNode, sourceHandle), { button: 0 })
  fireEvent.mouseMove(document, { clientX, clientY })
  fireEvent.mouseUp(document, { clientX, clientY })
}

it("shows the draft with typed ports on an editable canvas", async () => {
  const { container } = renderStudio()
  const vpc = await screen.findByRole("group", {
    name: "Create VPC (simulated)",
  })
  expect(
    screen.getByRole("heading", { name: "Provision staging" }),
  ).toBeInTheDocument()
  expect(within(vpc).getByText("Simulated")).toBeInTheDocument()
  expect(vpc).toHaveClass("draggable")
  const output = handle("vpc", "out:vpcId")
  expect(output).toHaveClass("source", "connectable")
  expect(output.parentElement).toHaveTextContent(/^vpcIdaws\.vpc-id$/)
  expect(handle("rds", "in:vpcId")).toHaveClass("target", "connectable")
  await waitFor(() =>
    expect(container.querySelectorAll(".react-flow__edge")).toHaveLength(2),
  )
  expect(screen.queryByText("Read-only")).not.toBeInTheDocument()
})

it("the palette lists the latest definitions and blueprints and filters them", async () => {
  server.use(
    http.get("/api/v1/orgs/:organizationId/node-definitions", () =>
      HttpResponse.json({
        items: [
          {
            ...mockTask,
            id: "acme-old",
            key: "acme.old",
            displayName: "Old thing",
            deprecated: true,
          },
          ...nodeDefinitions,
          {
            ...mockTask,
            id: "mock-task-2",
            version: 2,
            displayName: "Mock task v2",
            deprecated: true,
          },
        ],
      }),
    ),
  )
  renderStudio()
  const definitions = await screen.findByRole("list", {
    name: "Node definitions",
  })
  const blueprintList = screen.getByRole("list", { name: "Blueprints" })
  const items = (list: HTMLElement) => within(list).queryAllByRole("listitem")
  expect(items(definitions)).toHaveLength(6)
  expect(within(definitions).getByText("Mock task")).toBeInTheDocument()
  expect(
    within(definitions).queryByText("Mock task v2"),
  ).not.toBeInTheDocument()
  expect(within(definitions).queryByText("Old thing")).not.toBeInTheDocument()
  expect(items(blueprintList)).toHaveLength(2)

  const filter = screen.getByRole("searchbox", { name: "Filter the palette" })
  await userEvent.type(filter, "VPC")
  expect(items(definitions)).toHaveLength(1)
  expect(
    within(definitions).getByText("Create VPC (simulated)"),
  ).toBeInTheDocument()
  expect(items(blueprintList)).toHaveLength(0)

  await userEvent.clear(filter)
  await userEvent.type(filter, "create-rds")
  expect(items(definitions)).toHaveLength(1)
  expect(
    within(definitions).getByText("corqty.simulated.create-rds"),
  ).toBeInTheDocument()

  await userEvent.clear(filter)
  await userEvent.type(filter, "data")
  expect(items(definitions)).toHaveLength(0)
  expect(items(blueprintList)).toHaveLength(1)
  expect(within(blueprintList).getByText("Data processing")).toBeInTheDocument()
})

it("dropping a definition adds a node with a fresh id at the drop point", async () => {
  withDraft({ ...infrastructure.graph, nodes: [], edges: [] })
  renderStudio()
  drop(await findCanvas(), "application/x-corqty-definition", mockTask.id)
  const node = await screen.findByRole("group", { name: "Mock task" })
  expect(node.dataset.id).toMatch(uuid)
  expect(node.style.transform).toBe("translate(40px,60px)")
})

it("dropping a blueprint inserts its nodes and edges with fresh ids", async () => {
  const { container } = renderStudio()
  await screen.findByRole("group", { name: "Create VPC (simulated)" })
  drop(
    await findCanvas(),
    "application/x-corqty-blueprint",
    infrastructure.id,
    [600, 0],
  )
  await waitFor(() =>
    expect(
      screen.getAllByRole("group", { name: "Create VPC (simulated)" }),
    ).toHaveLength(2),
  )
  const ids = [
    ...container.querySelectorAll<HTMLElement>(".react-flow__node"),
  ].map((node) => node.dataset.id)
  expect(ids.slice(0, 3)).toEqual(["vpc", "rds", "deploy"])
  expect(ids.slice(3)).toEqual([
    expect.stringMatching(uuid),
    expect.stringMatching(uuid),
    expect.stringMatching(uuid),
  ])
  await waitFor(() =>
    expect(container.querySelectorAll(".react-flow__edge")).toHaveLength(4),
  )
  expect(
    screen.getByRole("group", { name: `Edge from ${ids[3]} to ${ids[4]}` }),
  ).toBeInTheDocument()
})

it("connects compatible ports and announces refused connections", async () => {
  withDraft({ ...infrastructure.graph, edges: [] })
  const { container } = renderStudio()
  const deploy = await screen.findByRole("group", {
    name: "Deploy service (simulated)",
  })
  await waitFor(() => expect(deploy).toBeVisible())
  const status = screen.getByRole("status")

  dragConnection(["rds", "out:endpoint"], ["deploy", "in:vpcId"], [300, 100])
  expect(status).toHaveTextContent(
    "Incompatible types: core.string -> aws.vpc-id",
  )
  expect(container.querySelectorAll(".react-flow__edge")).toHaveLength(0)

  dragConnection(["vpc", "out:vpcId"], ["deploy", "in:vpcId"], [300, 100])
  await waitFor(() =>
    expect(container.querySelectorAll(".react-flow__edge")).toHaveLength(1),
  )
  expect(status).toBeEmptyDOMElement()
})

it.each([
  {
    role: "viewer" as const,
    width: 1280,
    reason: "Your role can view this workflow but not edit it.",
  },
  {
    role: "admin" as const,
    width: 800,
    reason: "Editing needs a screen at least 1024 pixels wide.",
  },
])(
  "the canvas is read-only for $role at $width px",
  async ({ role, width, reason }) => {
    server.use(
      http.get("/api/v1/auth/me", () => HttpResponse.json(meAs(role))),
    )
    Object.assign(window, { innerWidth: width })
    const { container } = renderStudio()
    const vpc = await screen.findByRole("group", {
      name: "Create VPC (simulated)",
    })
    expect(screen.getByText("Read-only")).toBeInTheDocument()
    expect(screen.getByText(reason)).toBeInTheDocument()
    expect(
      screen.queryByRole("complementary", { name: "Palette" }),
    ).not.toBeInTheDocument()
    expect(vpc).not.toHaveClass("draggable")
    expect(handle("vpc", "out:vpcId")).not.toHaveClass("connectable")
    drop(await findCanvas(), "application/x-corqty-definition", mockTask.id)
    expect(container.querySelectorAll(".react-flow__node")).toHaveLength(3)
  },
)
```

- [ ] **Step 6: Run the tests and verify RED**

Run:

```text
pnpm web:test
```

Expected: `Test Files  2 failed | 5 passed (7)` and `Tests  6 failed | 16 passed (22)`. Every
`graph-model.test.ts` test fails with a `TypeError` such as `TypeError: compatible is not a function` or
`TypeError: newNodeFromDefinition is not a function`, and `studio.test.tsx` fails to load with
`Error: Failed to resolve import "@/studio/studio-canvas" from "src/studio/studio.test.tsx". Does the file exist?`

- [ ] **Step 7: Implement the graph model**

In `apps/web/src/studio/graph-model.ts`, apply these replacements in order:

Replace:

```ts
export function isSimulated(key: string) {
  return key.startsWith("corqty.simulated")
}
```

With:

```ts
import type { Edge, Node } from "@xyflow/react"

import type {
  Blueprint,
  GraphNode,
  GraphSnapshot,
  NodeDefinition,
} from "@/api/types"

// A canvas node carries its graph node; XYFlow owns the position while editing.
export type StudioNode = Node<
  { node: GraphNode; definition: NodeDefinition | undefined },
  "step"
>

// Node handles are named `in:<port>` and `out:<port>`.
export type PortConnection = {
  source: string
  sourceHandle?: string | null
  target: string
  targetHandle?: string | null
}

export type ConnectionError =
  | "self"
  | "occupied"
  | "bound"
  | "incompatible"
  | "cycle"

export function isSimulated(key: string) {
  return key.startsWith("corqty.simulated")
}

function portName(handle: string | null | undefined) {
  return handle?.split(":")[1] ?? ""
}

export function findDefinition(
  definitions: NodeDefinition[],
  { key, version }: GraphNode["definition"],
) {
  return definitions.find(
    (definition) => definition.key === key && definition.version === version,
  )
}

export function toFlowNode(
  node: GraphNode,
  definitions: NodeDefinition[],
): StudioNode {
  return {
    id: node.id,
    type: "step",
    position: node.position,
    ariaLabel: node.displayName,
    data: { node, definition: findDefinition(definitions, node.definition) },
  }
}

export function toFlow(graph: GraphSnapshot, definitions: NodeDefinition[]) {
  return {
    nodes: graph.nodes.map((node) => toFlowNode(node, definitions)),
    edges: graph.edges.map(
      (edge): Edge => ({
        id: edge.id,
        source: edge.source.nodeId,
        sourceHandle: `out:${edge.source.port}`,
        target: edge.target.nodeId,
        targetHandle: `in:${edge.target.port}`,
      }),
    ),
  }
}

// Nodes and edges come from the canvas, everything else from `graph`.
export function fromFlow(
  graph: GraphSnapshot,
  nodes: StudioNode[],
  edges: Edge[],
): GraphSnapshot {
  return {
    ...graph,
    nodes: nodes.map(({ position, data }) => ({
      ...data.node,
      position: { x: position.x, y: position.y },
    })),
    edges: edges.map((edge) => ({
      id: edge.id,
      source: { nodeId: edge.source, port: portName(edge.sourceHandle) },
      target: { nodeId: edge.target, port: portName(edge.targetHandle) },
    })),
  }
}

// Mirrors corqty_domain::graph::compatible.
export function compatible(source: string, target: string) {
  return (
    source === target ||
    (source === "core.integer" && target === "core.number") ||
    target === "core.any"
  )
}

// Kahn's algorithm: nodes on a cycle never run out of incoming edges.
export function hasCycle(edges: [string, string][]) {
  const incoming = new Map<string, number>()
  const outgoing = new Map<string, string[]>()
  for (const [source, target] of edges) {
    incoming.set(source, incoming.get(source) ?? 0)
    incoming.set(target, (incoming.get(target) ?? 0) + 1)
    outgoing.set(source, [...(outgoing.get(source) ?? []), target])
  }
  const ready = [...incoming.keys()].filter((id) => incoming.get(id) === 0)
  // `ready` grows while it is walked.
  for (const id of ready) {
    for (const target of outgoing.get(id) ?? []) {
      const count = (incoming.get(target) ?? 0) - 1
      incoming.set(target, count)
      if (count === 0) ready.push(target)
    }
  }
  return ready.length < incoming.size
}

export function wouldCreateCycle(
  edges: [string, string][],
  candidate: [string, string],
) {
  return hasCycle([...edges, candidate])
}

// The data type behind an `in:<port>` or `out:<port>` handle.
export function portType(
  graph: GraphSnapshot,
  definitions: NodeDefinition[],
  nodeId: string,
  handle: string | null | undefined,
) {
  const node = graph.nodes.find((candidate) => candidate.id === nodeId)
  const definition = node && findDefinition(definitions, node.definition)
  const ports = handle?.startsWith("in:")
    ? definition?.inputs
    : definition?.outputs
  return ports?.find((port) => port.name === portName(handle))?.dataType
}

// Checked in this order; null means the connection may become an edge.
export function connectionError(
  graph: GraphSnapshot,
  definitions: NodeDefinition[],
  connection: PortConnection,
): ConnectionError | null {
  const { source, sourceHandle, target, targetHandle } = connection
  const port = portName(targetHandle)
  if (source === target) return "self"
  const occupied = graph.edges.some(
    (edge) => edge.target.nodeId === target && edge.target.port === port,
  )
  if (occupied) return "occupied"
  const targetNode = graph.nodes.find((node) => node.id === target)
  if (targetNode && Object.hasOwn(targetNode.inputs, port)) return "bound"
  const sourceType = portType(graph, definitions, source, sourceHandle)
  const targetType = portType(graph, definitions, target, targetHandle)
  if (!sourceType || !targetType || !compatible(sourceType, targetType)) {
    return "incompatible"
  }
  const edges = graph.edges.map(
    (edge): [string, string] => [edge.source.nodeId, edge.target.nodeId],
  )
  return wouldCreateCycle(edges, [source, target]) ? "cycle" : null
}

const refusals = {
  self: "A node cannot connect to itself",
  occupied: "This input already has a connection",
  bound: "This input already has a bound value",
  cycle: "This connection would create a cycle",
}

export function refusalMessage(
  graph: GraphSnapshot,
  definitions: NodeDefinition[],
  connection: PortConnection,
) {
  const error = connectionError(graph, definitions, connection)
  if (error !== "incompatible") return error && refusals[error]
  const { source, sourceHandle, target, targetHandle } = connection
  const sourceType = portType(graph, definitions, source, sourceHandle)
  const targetType = portType(graph, definitions, target, targetHandle)
  return `Incompatible types: ${sourceType} -> ${targetType}`
}

// The palette offers the highest non-deprecated version of each key.
export function latestDefinitions(definitions: NodeDefinition[]) {
  const latest = new Map<string, NodeDefinition>()
  for (const definition of definitions) {
    const current = latest.get(definition.key)
    if (
      !definition.deprecated &&
      (!current || definition.version > current.version)
    ) {
      latest.set(definition.key, definition)
    }
  }
  return [...latest.values()]
}

export function newNodeFromDefinition(
  definition: NodeDefinition,
  position: GraphNode["position"],
): GraphNode {
  return {
    id: crypto.randomUUID(),
    definition: { key: definition.key, version: definition.version },
    displayName: definition.displayName,
    position,
    config: structuredClone(definition.defaults),
    inputs: {},
    retry: { maxAttempts: 1 },
    timeoutSeconds: 900,
  }
}

// D14: the blueprint's origin lands on the drop point, every node and edge gets
// a fresh id, and its workflow inputs join the graph's (existing entries win).
export function insertBlueprint(
  graph: GraphSnapshot,
  blueprint: Blueprint,
  position: GraphNode["position"],
): GraphSnapshot {
  const ids = new Map(
    blueprint.graph.nodes.map((node) => [node.id, crypto.randomUUID()]),
  )
  const freshId = (id: string) => ids.get(id) ?? id
  const schema = graph.workflowInputSchema
  const added = blueprint.graph.workflowInputSchema
  const required = [
    ...new Set([...(schema.required ?? []), ...(added.required ?? [])]),
  ]
  return {
    ...graph,
    workflowInputSchema: {
      ...schema,
      properties: { ...added.properties, ...schema.properties },
      ...(required.length > 0 && { required }),
    },
    nodes: [
      ...graph.nodes,
      ...blueprint.graph.nodes.map((node) => ({
        ...structuredClone(node),
        id: freshId(node.id),
        position: {
          x: position.x + node.position.x,
          y: position.y + node.position.y,
        },
      })),
    ],
    edges: [
      ...graph.edges,
      ...blueprint.graph.edges.map((edge) => ({
        id: crypto.randomUUID(),
        source: { ...edge.source, nodeId: freshId(edge.source.nodeId) },
        target: { ...edge.target, nodeId: freshId(edge.target.nodeId) },
      })),
    ],
  }
}
```

- [ ] **Step 8: Add the node, the palette and the canvas**

Create `apps/web/src/studio/studio-node.tsx`:

```tsx
import { Handle, Position, type NodeProps } from "@xyflow/react"

import type { PortSpec } from "@/api/types"
import { StatusBadge } from "@/components/status/status-badge"
import { isSimulated, type StudioNode } from "@/studio/graph-model"

function Port({
  port,
  side,
  isConnectable,
}: {
  port: PortSpec
  side: "in" | "out"
  isConnectable: boolean
}) {
  return (
    <li className="relative px-3 py-1">
      <Handle
        id={`${side}:${port.name}`}
        type={side === "in" ? "target" : "source"}
        position={side === "in" ? Position.Left : Position.Right}
        isConnectable={isConnectable}
      />
      {port.name}
      <span className="block text-muted-foreground">{port.dataType}</span>
    </li>
  )
}

export function StudioNodeView({ data, isConnectable }: NodeProps<StudioNode>) {
  const { node, definition } = data
  return (
    <div className="min-w-52 rounded-lg border bg-card text-sm text-card-foreground shadow-sm">
      <div className="flex items-center justify-between gap-2 border-b px-3 py-2">
        <span className="font-medium">{node.displayName}</span>
        {isSimulated(node.definition.key) && <StatusBadge status="simulated" />}
      </div>
      {definition ? (
        <div className="grid grid-cols-2 gap-4 py-2 text-xs">
          <ul>
            {definition.inputs.map((port) => (
              <Port
                key={port.name}
                port={port}
                side="in"
                isConnectable={isConnectable}
              />
            ))}
          </ul>
          <ul className="text-right">
            {definition.outputs.map((port) => (
              <Port
                key={port.name}
                port={port}
                side="out"
                isConnectable={isConnectable}
              />
            ))}
          </ul>
        </div>
      ) : (
        <p className="px-3 py-2 text-xs text-destructive">
          Unknown definition {node.definition.key} v{node.definition.version}
        </p>
      )}
    </div>
  )
}
```

Create `apps/web/src/studio/palette.tsx`:

```tsx
"use client"

import { useId, useState } from "react"

import type { Blueprint, NodeDefinition } from "@/api/types"
import { StatusBadge } from "@/components/status/status-badge"
import { Input } from "@/components/ui/input"
import { isSimulated, latestDefinitions } from "@/studio/graph-model"

export const DEFINITION_TYPE = "application/x-corqty-definition"
export const BLUEPRINT_TYPE = "application/x-corqty-blueprint"

type Item = { id: string; key: string; displayName: string }

// ponytail: mouse drag and drop only; a keyboard "add" action can follow.
function PaletteList({
  title,
  format,
  items,
}: {
  title: string
  format: string
  items: Item[]
}) {
  const headingId = useId()
  return (
    <section className="grid gap-2">
      <h2 id={headingId} className="text-xs font-medium text-muted-foreground">
        {title}
      </h2>
      <ul aria-labelledby={headingId} className="grid gap-1.5">
        {items.map((item) => (
          <li
            key={item.id}
            draggable
            onDragStart={(event) => {
              event.dataTransfer.setData(format, item.id)
              event.dataTransfer.effectAllowed = "move"
            }}
            className="cursor-grab rounded-md border bg-card px-3 py-2 text-sm"
          >
            <span className="flex items-center gap-2 font-medium">
              {item.displayName}
              {isSimulated(item.key) && <StatusBadge status="simulated" />}
            </span>
            <span className="block truncate font-mono text-xs text-muted-foreground">
              {item.key}
            </span>
          </li>
        ))}
      </ul>
      {items.length === 0 && (
        <p className="text-sm text-muted-foreground">No matches.</p>
      )}
    </section>
  )
}

export function Palette({
  definitions,
  blueprints,
}: {
  definitions: NodeDefinition[]
  blueprints: Blueprint[]
}) {
  const [filter, setFilter] = useState("")
  const query = filter.trim().toLowerCase()
  const matches = (item: Item) =>
    item.displayName.toLowerCase().includes(query) ||
    item.key.toLowerCase().includes(query)

  return (
    <aside
      aria-label="Palette"
      className="flex min-h-0 flex-col gap-4 overflow-y-auto border-r p-3"
    >
      <Input
        type="search"
        aria-label="Filter the palette"
        placeholder="Filter by name or key"
        value={filter}
        onChange={(event) => setFilter(event.target.value)}
      />
      <PaletteList
        title="Node definitions"
        format={DEFINITION_TYPE}
        items={latestDefinitions(definitions).filter(matches)}
      />
      <PaletteList
        title="Blueprints"
        format={BLUEPRINT_TYPE}
        items={blueprints.filter(matches)}
      />
    </aside>
  )
}
```

`insertBlueprint` appends, so after a blueprint drop the canvas keeps its existing node objects (with their
measured size and selection) and adds only the new ones.

Create `apps/web/src/studio/studio-canvas.tsx`:

```tsx
"use client"

import "@xyflow/react/dist/style.css"

import {
  Background,
  Controls,
  ReactFlow,
  ReactFlowProvider,
  addEdge,
  useEdgesState,
  useNodesState,
  useReactFlow,
  type Connection,
  type IsValidConnection,
  type NodeTypes,
  type OnConnectEnd,
} from "@xyflow/react"
import { useState, type DragEvent } from "react"

import type { Blueprint, GraphSnapshot, NodeDefinition } from "@/api/types"
import {
  connectionError,
  fromFlow,
  insertBlueprint,
  newNodeFromDefinition,
  refusalMessage,
  toFlow,
  toFlowNode,
} from "@/studio/graph-model"
import { BLUEPRINT_TYPE, DEFINITION_TYPE } from "@/studio/palette"
import { StudioNodeView } from "@/studio/studio-node"

const nodeTypes = { step: StudioNodeView } satisfies NodeTypes

type CanvasProps = {
  graph: GraphSnapshot
  definitions: NodeDefinition[]
  blueprints: Blueprint[]
  readOnly: boolean
}

function Canvas({ graph, definitions, blueprints, readOnly }: CanvasProps) {
  const [initial] = useState(() => toFlow(graph, definitions))
  // XYFlow holds the nodes and edges; this holds the rest of the snapshot.
  // ponytail: edits stay in local state until drafts are saved automatically.
  const [snapshot, setSnapshot] = useState(graph)
  const [nodes, setNodes, onNodesChange] = useNodesState(initial.nodes)
  const [edges, setEdges, onEdgesChange] = useEdgesState(initial.edges)
  const [announcement, setAnnouncement] = useState("")
  const { screenToFlowPosition } = useReactFlow()

  const current = () => fromFlow(snapshot, nodes, edges)

  const isValidConnection: IsValidConnection = (connection) =>
    connectionError(current(), definitions, connection) === null

  function onConnect(connection: Connection) {
    setEdges((existing) =>
      addEdge({ ...connection, id: crypto.randomUUID() }, existing),
    )
  }

  // Explain why a connection dropped on a port was refused.
  const onConnectEnd: OnConnectEnd = (_, { fromHandle, toHandle }) => {
    if (!fromHandle || !toHandle || fromHandle.type === toHandle.type) return
    const [source, target] =
      fromHandle.type === "source"
        ? [fromHandle, toHandle]
        : [toHandle, fromHandle]
    const message = refusalMessage(current(), definitions, {
      source: source.nodeId,
      sourceHandle: source.id,
      target: target.nodeId,
      targetHandle: target.id,
    })
    setAnnouncement(message ?? "")
  }

  function onDragOver(event: DragEvent) {
    event.preventDefault()
    event.dataTransfer.dropEffect = "move"
  }

  function onDrop(event: DragEvent) {
    event.preventDefault()
    const position = screenToFlowPosition({
      x: event.clientX,
      y: event.clientY,
    })
    const definitionId = event.dataTransfer.getData(DEFINITION_TYPE)
    const definition = definitions.find(({ id }) => id === definitionId)
    if (definition) {
      const node = toFlowNode(
        newNodeFromDefinition(definition, position),
        definitions,
      )
      setNodes((existing) => [...existing, node])
      return
    }
    const blueprintId = event.dataTransfer.getData(BLUEPRINT_TYPE)
    const blueprint = blueprints.find(({ id }) => id === blueprintId)
    if (blueprint) {
      const next = insertBlueprint(current(), blueprint, position)
      const flow = toFlow(next, definitions)
      setSnapshot(next)
      // insertBlueprint appends, so the nodes already on the canvas keep their
      // measured size and selection.
      setNodes([...nodes, ...flow.nodes.slice(nodes.length)])
      setEdges([...edges, ...flow.edges.slice(edges.length)])
    }
  }

  return (
    <div className="flex min-h-96 flex-col">
      <div className="min-h-0 flex-1">
        <ReactFlow
          nodes={nodes}
          edges={edges}
          nodeTypes={nodeTypes}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onConnect={onConnect}
          onConnectEnd={onConnectEnd}
          isValidConnection={isValidConnection}
          onDragOver={readOnly ? undefined : onDragOver}
          onDrop={readOnly ? undefined : onDrop}
          nodesDraggable={!readOnly}
          nodesConnectable={!readOnly}
          deleteKeyCode={readOnly ? null : ["Backspace", "Delete"]}
          defaultViewport={graph.viewport}
          colorMode="system"
        >
          <Background />
          <Controls showInteractive={false} />
        </ReactFlow>
      </div>
      <p
        role="status"
        aria-live="polite"
        className="min-h-9 border-t px-4 py-2 text-sm text-muted-foreground"
      >
        {announcement}
      </p>
    </div>
  )
}

export function StudioCanvas(props: CanvasProps) {
  return (
    <ReactFlowProvider>
      <Canvas {...props} />
    </ReactFlowProvider>
  )
}
```

Replace the contents of `apps/web/src/app/globals.css` with:

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
  --radius-sm: calc(var(--radius) * 0.6);
  --radius-md: calc(var(--radius) * 0.8);
  --radius-lg: var(--radius);
}

:root {
  --radius: 0.625rem;
  --background: oklch(1 0 0);
  --foreground: oklch(0.145 0 0);
  --card: oklch(1 0 0);
  --card-foreground: oklch(0.145 0 0);
  --popover: oklch(1 0 0);
  --popover-foreground: oklch(0.145 0 0);
  --primary: oklch(0.205 0 0);
  --primary-foreground: oklch(0.985 0 0);
  --secondary: oklch(0.97 0 0);
  --secondary-foreground: oklch(0.205 0 0);
  --muted: oklch(0.97 0 0);
  --muted-foreground: oklch(0.556 0 0);
  --accent: oklch(0.97 0 0);
  --accent-foreground: oklch(0.205 0 0);
  --destructive: oklch(0.577 0.245 27.325);
  --border: oklch(0.922 0 0);
  --input: oklch(0.922 0 0);
  --ring: oklch(0.708 0 0);
  --sidebar: oklch(0.985 0 0);
  --sidebar-foreground: oklch(0.145 0 0);
  --sidebar-accent: oklch(0.97 0 0);
}

.dark {
  --background: oklch(0.145 0 0);
  --foreground: oklch(0.985 0 0);
  --card: oklch(0.205 0 0);
  --card-foreground: oklch(0.985 0 0);
  --popover: oklch(0.205 0 0);
  --popover-foreground: oklch(0.985 0 0);
  --primary: oklch(0.922 0 0);
  --primary-foreground: oklch(0.205 0 0);
  --secondary: oklch(0.269 0 0);
  --secondary-foreground: oklch(0.985 0 0);
  --muted: oklch(0.269 0 0);
  --muted-foreground: oklch(0.708 0 0);
  --accent: oklch(0.269 0 0);
  --accent-foreground: oklch(0.985 0 0);
  --destructive: oklch(0.704 0.191 22.216);
  --border: oklch(1 0 0 / 10%);
  --input: oklch(1 0 0 / 15%);
  --ring: oklch(0.556 0 0);
  --sidebar: oklch(0.205 0 0);
  --sidebar-foreground: oklch(0.985 0 0);
  --sidebar-accent: oklch(0.269 0 0);
}

/* The studio canvas uses the same semantic tokens in both themes. */
.react-flow {
  --xy-background-color: var(--background);
  --xy-node-border: 1px solid var(--border);
  --xy-edge-stroke: var(--muted-foreground);
}

@layer base {
  * {
    @apply border-border outline-ring/50;
  }

  body {
    @apply bg-background text-foreground antialiased;
  }
}

@media (prefers-reduced-motion: reduce) {
  *,
  *::before,
  *::after {
    scroll-behavior: auto !important;
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
  }
}
```

- [ ] **Step 9: Replace the studio placeholder**

`useSyncExternalStore` reads `matchMedia` only in the browser and assumes a narrow screen on the server, so
the studio switches between editing and read-only live when the window crosses 1024 px.

Replace the contents of `apps/web/src/studio/studio.tsx` with:

```tsx
"use client"

import dynamic from "next/dynamic"
import { useSyncExternalStore } from "react"

import { useBlueprints, useNodeDefinitions, useWorkflow } from "@/api/queries"
import { useCan } from "@/auth/use-can"
import { StatusBadge } from "@/components/status/status-badge"
import { cn } from "@/lib/utils"
import { Palette } from "@/studio/palette"

// XYFlow measures the DOM and follows the system color scheme, so the canvas
// renders only in the browser.
const StudioCanvas = dynamic(
  () => import("@/studio/studio-canvas").then((module) => module.StudioCanvas),
  {
    ssr: false,
    loading: () => (
      <p className="p-4 text-sm text-muted-foreground">Loading editor…</p>
    ),
  },
)

const wideScreen = "(min-width: 1024px)"

function subscribe(onChange: () => void) {
  const query = window.matchMedia(wideScreen)
  query.addEventListener("change", onChange)
  return () => query.removeEventListener("change", onChange)
}

// Full editing needs 1024 px; the server render assumes a narrow screen.
function useWideScreen() {
  return useSyncExternalStore(
    subscribe,
    () => window.matchMedia(wideScreen).matches,
    () => false,
  )
}

export function Studio({
  organizationId,
  workflowId,
}: {
  organizationId: string
  workflowId: string
}) {
  const workflow = useWorkflow(organizationId, workflowId)
  const definitions = useNodeDefinitions(organizationId)
  const blueprints = useBlueprints(organizationId)
  const canEdit = useCan(organizationId, "edit_workflows")
  const wide = useWideScreen()
  const readOnlyReason = !canEdit
    ? "Your role can view this workflow but not edit it."
    : !wide
      ? "Editing needs a screen at least 1024 pixels wide."
      : null

  if (workflow.isError || definitions.isError || blueprints.isError) {
    return (
      <p role="alert" className="p-4 text-sm text-destructive md:p-6">
        The workflow could not be loaded.
      </p>
    )
  }
  if (!workflow.data || !definitions.data || !blueprints.data) {
    return (
      <p className="p-4 text-sm text-muted-foreground md:p-6">
        Loading workflow…
      </p>
    )
  }

  return (
    <section className="flex h-dvh flex-col">
      <header className="flex flex-wrap items-center gap-3 border-b px-4 py-3">
        <h1 className="text-lg font-semibold tracking-tight">
          {workflow.data.name}
        </h1>
        {readOnlyReason && (
          <>
            <StatusBadge status="read-only" />
            <span className="text-sm text-muted-foreground">
              {readOnlyReason}
            </span>
          </>
        )}
      </header>
      <div
        className={cn(
          "grid min-h-0 flex-1",
          !readOnlyReason && "grid-cols-[16rem_1fr]",
        )}
      >
        {!readOnlyReason && (
          <Palette
            definitions={definitions.data.items}
            blueprints={blueprints.data.items}
          />
        )}
        <StudioCanvas
          graph={workflow.data.draft.graph}
          definitions={definitions.data.items}
          blueprints={blueprints.data.items}
          readOnly={readOnlyReason !== null}
        />
      </div>
    </section>
  )
}
```

- [ ] **Step 10: Verify GREEN**

Run:

```text
pnpm web:test
```

Expected: `Test Files  7 passed (7)` and `Tests  29 passed (29)` with no unhandled errors. On Node 25 or newer,
each worker may print `ExperimentalWarning: localStorage is not available because --localstorage-file was not
provided.`; this is harmless.

- [ ] **Step 11: Run lint and build**

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

Expected: `✓ Compiled successfully`, `Finished TypeScript`, and the same route table as Task 15, with
`ƒ /orgs/[organizationId]/workflows/[workflowId]` still dynamic. It exits 0.

- [ ] **Step 12: Commit**

```text
git add apps/web pnpm-lock.yaml
git commit -m "feat: build the studio canvas with typed, cycle-safe connections"
```

## Task 17: Add the schema-driven inspector and widgets

**Files:**

- Create: `apps/web/src/components/ui/textarea.tsx`
- Create: `apps/web/src/studio/widgets.tsx`
- Create: `apps/web/src/studio/inspector.tsx`
- Modify: `apps/web/src/studio/studio-canvas.tsx`
- Test: `apps/web/src/studio/widgets.test.tsx` (new)
- Test: `apps/web/src/studio/inspector.test.tsx` (new)
- Test: `apps/web/src/studio/studio.test.tsx`

The studio gets its right-hand inspector, generated from the selected node's definition. `src/studio/widgets.tsx`
is the widget registry: `widgets` maps each of the ten uiSchema widget names of
`corqty_domain::catalog::WIDGETS` to an editor that takes `{ label, help, schema, value, onChange }`, renders
its own label, and writes the §5 value contract. text, textarea, select and fileText write strings; number
writes a number (a whole number when the schema type is `integer`, null when the field is empty); boolean
writes a boolean; multiSelect writes the checked enum strings in enum order; keyValue writes an object of
strings; secretReference writes `{"kind":"secret","secretId":"<uuid>"}`, or null when empty. code edits any
JSON as text: invalid JSON, or anything but a JSON object when the schema type is `object`, keeps the last valid
value and shows an inline error. fileText imports a file as text and rejects one over 262,144 bytes
(`FILE_TEXT_MAX_BYTES`, the domain cap) with an inline error. `widgetFor(schema, field?)` returns the uiSchema
widget, or else the §5 default for the schema (string enum: select, string: text, integer and number: number,
boolean: boolean, array of string enum: multiSelect, anything else: code). `blankValue(schema)` is the value a
new literal starts from: the schema's `default`, or else an empty value for its widget. The widgets use the
existing `Input` and `Button`, a new shadcn `Textarea`, and native `<select>` and checkbox elements instead of
the base-ui Select and Checkbox (fewer moving parts, and jsdom drives them directly), all styled with semantic
tokens (D17).

`src/studio/inspector.tsx` exports `Inspector`. With exactly one node selected on the canvas it shows that
node's display name, one field per `configSchema` property (the label comes from the uiSchema entry, then the
property's `title`, then its name; the help from the uiSchema entry, then the `description`), one fieldset per
input port, and the execution settings. A port fed by an edge reads `Connected from <node>.<port>` and has no
editor. Any other port has a Binding select (Not bound, Literal value, Workflow input, Secret):
- A literal edits its value with the widget that its port schema picks.
- A workflow input picks one of the top-level `workflowInputSchema` properties as an RFC 6901 pointer (`/a~1b`
  for the property `a/b`).
- A secret takes a UUID, with the help text `Secret picker arrives with secrets.` (D11).

Choosing Workflow input or Secret unbinds the port until a pointer or a valid UUID is chosen, because a draft
whose secret binding has no UUID would not deserialize on the server. For the same reason Max attempts (1 to
5) and Timeout (seconds) (1 to 14400) write only whole numbers in range and otherwise say `Enter a whole
number from <min> to <max>.` With no node selected (or several), the inspector edits `workflowInputSchema` as
JSON with the code widget, which follows outside changes such as a blueprint drop that adds workflow inputs.
In read-only mode the inspector stays visible inside a disabled `<fieldset>`, which disables every control in
it.

The canvas renders the inspector in a 20rem column beside the flow from 1024 px (`lg`) and below the flow on
narrower screens; `min-h-0` lets the column scroll on its own. Node edits replace the canvas node's
`data.node` and `ariaLabel`, and input schema edits go into the snapshot, so `fromFlow` sees both and Task 18
can save them. XYFlow ignores its delete key while focus is in an input, so Backspace in the inspector edits
the field and never deletes the selected node.

- [ ] **Step 1: Write the failing widget tests**

The table renders each widget in a harness that keeps the value in state, as the inspector does, edits it
the way a person would, and checks the last value written.

Create `apps/web/src/studio/widgets.test.tsx`:

```tsx
import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useState } from "react"
import { expect, it, vi } from "vitest"

import type { JsonSchema } from "@/api/types"
import { widgetFor, widgets, type WidgetName } from "@/studio/widgets"

const secretId = "0190f5c2-7d1e-7a00-8000-0000000000aa"

// Holds the value like the inspector does and records every write.
function renderWidget(name: WidgetName, schema: JsonSchema, initial?: unknown) {
  const onChange = vi.fn()
  const Widget = widgets[name]
  function Harness() {
    const [value, setValue] = useState(initial)
    return (
      <Widget
        label="Field"
        schema={schema}
        value={value}
        onChange={(next) => {
          setValue(next)
          onChange(next)
        }}
      />
    )
  }
  render(<Harness />)
  return onChange
}

const field = () => screen.getByLabelText("Field")

it("registers exactly the catalog's ten widgets", () => {
  expect(Object.keys(widgets)).toEqual([
    "text",
    "textarea",
    "number",
    "boolean",
    "select",
    "multiSelect",
    "secretReference",
    "code",
    "keyValue",
    "fileText",
  ])
})

it.each<{
  widget: WidgetName
  schema: JsonSchema
  initial?: unknown
  edit: () => Promise<void>
  expected: unknown
}>([
  {
    widget: "text",
    schema: { type: "string" },
    edit: () => userEvent.type(field(), "us-east-1"),
    expected: "us-east-1",
  },
  {
    widget: "textarea",
    schema: { type: "string" },
    edit: () => userEvent.type(field(), "one{Enter}two"),
    expected: "one\ntwo",
  },
  {
    widget: "number",
    schema: { type: "integer" },
    edit: () => userEvent.type(field(), "42"),
    expected: 42,
  },
  {
    widget: "boolean",
    schema: { type: "boolean" },
    initial: false,
    edit: () => userEvent.click(field()),
    expected: true,
  },
  {
    widget: "select",
    schema: { type: "string", enum: ["fixed", "echo"] },
    initial: "fixed",
    edit: () => userEvent.selectOptions(field(), "echo"),
    expected: "echo",
  },
  {
    widget: "multiSelect",
    schema: { type: "array", items: { type: "string", enum: ["a", "b", "c"] } },
    edit: async () => {
      expect(screen.getByRole("group", { name: "Field" })).toBeInTheDocument()
      await userEvent.click(screen.getByLabelText("c"))
      await userEvent.click(screen.getByLabelText("a"))
    },
    expected: ["a", "c"],
  },
  {
    widget: "secretReference",
    schema: { type: ["object", "null"] },
    initial: null,
    edit: async () => {
      await userEvent.click(field())
      await userEvent.paste(secretId)
    },
    expected: { kind: "secret", secretId },
  },
  {
    widget: "code",
    schema: { type: "object" },
    initial: {},
    edit: async () => {
      await userEvent.clear(field())
      await userEvent.paste('{"ok": true}')
    },
    expected: { ok: true },
  },
  {
    widget: "keyValue",
    schema: { type: "object" },
    edit: async () => {
      expect(screen.getByRole("group", { name: "Field" })).toBeInTheDocument()
      await userEvent.click(screen.getByRole("button", { name: "Add entry" }))
      await userEvent.type(screen.getByLabelText("Key 1"), "region")
      await userEvent.type(screen.getByLabelText("Value 1"), "us-east-1")
    },
    expected: { region: "us-east-1" },
  },
  {
    widget: "fileText",
    schema: { type: "string" },
    edit: () =>
      userEvent.upload(
        screen.getByLabelText("Import Field from a file"),
        new File(["#!/bin/sh\necho ok\n"], "run.sh"),
      ),
    expected: "#!/bin/sh\necho ok\n",
  },
])(
  "$widget writes its value contract",
  async ({ widget, schema, initial, edit, expected }) => {
    const onChange = renderWidget(widget, schema, initial)
    await edit()
    await waitFor(() => expect(onChange).toHaveBeenLastCalledWith(expected))
  },
)

it("keyValue removes an entry", async () => {
  const onChange = renderWidget(
    "keyValue",
    { type: "object" },
    { region: "us-east-1", tier: "gold" },
  )
  expect(screen.getByLabelText("Key 2")).toHaveValue("tier")
  await userEvent.click(screen.getByRole("button", { name: "Remove entry 1" }))
  expect(onChange).toHaveBeenLastCalledWith({ tier: "gold" })
})

it("code keeps the last valid value and explains why", async () => {
  const onChange = renderWidget("code", { type: "object" }, { ok: true })
  expect(field()).toHaveValue('{\n  "ok": true\n}')
  await userEvent.type(field(), "{{")
  expect(screen.getByRole("alert")).toHaveTextContent(
    "This is not valid JSON for this field. The last valid value is kept.",
  )
  // An object field accepts only a JSON object.
  await userEvent.clear(field())
  await userEvent.paste("[1]")
  expect(screen.getByRole("alert")).toBeInTheDocument()
  expect(onChange).not.toHaveBeenCalled()
})

it("fileText rejects a file over 262,144 bytes", async () => {
  const onChange = renderWidget("fileText", { type: "string" }, "kept")
  const importer = screen.getByLabelText("Import Field from a file")
  await userEvent.upload(importer, new File(["x".repeat(262_145)], "big.txt"))
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Choose a file of at most 262,144 bytes.",
  )
  expect(onChange).not.toHaveBeenCalled()
  expect(field()).toHaveValue("kept")

  const largest = "x".repeat(262_144)
  await userEvent.upload(importer, new File([largest], "largest.txt"))
  await waitFor(() => expect(onChange).toHaveBeenCalledOnce())
  expect(onChange.mock.lastCall?.[0]).toHaveLength(262_144)
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
})

it("widgetFor uses the uiSchema widget or the default for the schema", () => {
  expect(widgetFor({ type: "string" }, { widget: "textarea" })).toBe("textarea")
  const defaults: [JsonSchema, WidgetName][] = [
    [{ type: "string", enum: ["fixed", "echo"] }, "select"],
    [{ type: "string" }, "text"],
    [{ type: "integer" }, "number"],
    [{ type: "number" }, "number"],
    [{ type: "boolean" }, "boolean"],
    [{ type: "array", items: { type: "string", enum: ["a"] } }, "multiSelect"],
    [{ type: "array", items: { type: "string" } }, "code"],
    [{ type: "object" }, "code"],
    [{ type: ["string", "null"] }, "code"],
    [{}, "code"],
  ]
  for (const [schema, widget] of defaults) {
    expect(widgetFor(schema)).toBe(widget)
  }
})
```

- [ ] **Step 2: Write the failing inspector tests**

The harness keeps the graph in state, as the canvas does, and records every node and input schema the
inspector writes.

Create `apps/web/src/studio/inspector.test.tsx`:

```tsx
import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useState } from "react"
import { expect, it, vi } from "vitest"

import type { GraphNode, GraphSnapshot, JsonSchema } from "@/api/types"
import { Inspector } from "@/studio/inspector"
import { blueprints, nodeDefinitions } from "@/test/handlers"

const infrastructure = blueprints[1].graph // vpc -> rds, vpc -> deploy
const secretId = "0190f5c2-7d1e-7a00-8000-0000000000aa"

// Holds the graph like the canvas does and records what the inspector writes.
function renderInspector(
  nodeId: string | null,
  { graph = infrastructure, readOnly = false } = {},
) {
  const nodeChanges = vi.fn<(node: GraphNode) => void>()
  const schemaChanges = vi.fn<(schema: JsonSchema) => void>()
  function Harness() {
    const [current, setCurrent] = useState<GraphSnapshot>(graph)
    return (
      <Inspector
        graph={current}
        definitions={nodeDefinitions}
        nodeId={nodeId}
        readOnly={readOnly}
        onNodeChange={(node) => {
          nodeChanges(node)
          setCurrent((previous) => ({
            ...previous,
            nodes: previous.nodes.map((candidate) =>
              candidate.id === node.id ? node : candidate,
            ),
          }))
        }}
        onInputSchemaChange={(schema) => {
          schemaChanges(schema)
          setCurrent((previous) => ({
            ...previous,
            workflowInputSchema: schema,
          }))
        }}
      />
    )
  }
  render(<Harness />)
  return {
    nodeChanges,
    schemaChanges,
    lastNode: () => nodeChanges.mock.lastCall?.[0],
  }
}

// Each input port is a group named after the port.
const port = (name: string) =>
  screen.getByRole("group", { name: new RegExp(`^${name} `) })

it("edits the node's name and its configuration through the uiSchema widgets", async () => {
  const { lastNode } = renderInspector("vpc")
  expect(
    screen.getByRole("heading", { name: "Create VPC (simulated)" }),
  ).toBeInTheDocument()
  const name = screen.getByLabelText("Display name")
  await userEvent.clear(name)
  await userEvent.type(name, "Network")
  expect(lastNode()?.displayName).toBe("Network")
  expect(screen.getByRole("heading", { name: "Network" })).toBeInTheDocument()

  expect(screen.getByLabelText("Mode")).toHaveValue("fixed")
  expect(screen.getByLabelText("Fixed output")).toHaveValue(
    '{\n  "vpcId": "vpc-0simulated"\n}',
  )
  expect(screen.getByLabelText("Delay (ms)")).toHaveValue(500)
  await userEvent.selectOptions(screen.getByLabelText("Mode"), "echo")
  await userEvent.type(screen.getByLabelText("Fail with message"), "boom")
  expect(lastNode()?.config).toEqual({
    mode: "echo",
    output: { vpcId: "vpc-0simulated" },
    delayMs: 500,
    failureMessage: "boom",
  })
})

it("shows where a connected input comes from instead of an editor", () => {
  renderInspector("rds")
  const vpcId = port("vpcId")
  expect(vpcId).toHaveTextContent("Connected from Create VPC (simulated).vpcId")
  expect(within(vpcId).queryByRole("combobox")).not.toBeInTheDocument()
})

it("binds an input to a workflow input, a literal or a secret", async () => {
  const graph = {
    ...infrastructure,
    workflowInputSchema: {
      type: "object",
      properties: { region: { type: "string" }, "a/b": { type: "string" } },
    },
  }
  const { lastNode } = renderInspector("vpc", { graph })
  const region = port("region")
  const binding = within(region).getByLabelText("Binding")
  expect(binding).toHaveValue("workflowInput")
  const picker = within(region).getByLabelText("Workflow input")
  expect(picker).toHaveValue("/region")
  expect(
    within(picker)
      .getAllByRole("option")
      .map((option) => option.textContent),
  ).toEqual(["Choose an input…", "/region", "/a~1b"])
  await userEvent.selectOptions(picker, "/a~1b")
  expect(lastNode()?.inputs).toEqual({
    region: { kind: "workflowInput", pointer: "/a~1b" },
  })

  await userEvent.selectOptions(binding, "Literal value")
  expect(lastNode()?.inputs).toEqual({ region: { kind: "literal", value: "" } })
  await userEvent.type(within(region).getByLabelText("Value"), "eu-west-1")
  expect(lastNode()?.inputs).toEqual({
    region: { kind: "literal", value: "eu-west-1" },
  })

  await userEvent.selectOptions(binding, "Secret")
  expect(lastNode()?.inputs).toEqual({})
  expect(
    within(region).getByText("Secret picker arrives with secrets."),
  ).toBeInTheDocument()
  const secret = within(region).getByLabelText("Secret id")
  await userEvent.type(secret, "not-a-uuid")
  expect(within(region).getByRole("alert")).toHaveTextContent(
    "Enter the secret id as a UUID.",
  )
  expect(lastNode()?.inputs).toEqual({})
  await userEvent.clear(secret)
  await userEvent.paste(secretId)
  expect(lastNode()?.inputs).toEqual({ region: { kind: "secret", secretId } })

  await userEvent.selectOptions(binding, "Not bound")
  expect(lastNode()?.inputs).toEqual({})
})

it("retry and timeout take whole numbers within their bounds", async () => {
  const { nodeChanges, lastNode } = renderInspector("vpc")
  const attempts = screen.getByLabelText("Max attempts")
  expect(attempts).toHaveValue(1)
  await userEvent.clear(attempts)
  await userEvent.type(attempts, "0")
  expect(screen.getByRole("alert")).toHaveTextContent(
    "Enter a whole number from 1 to 5.",
  )
  await userEvent.type(attempts, "{Backspace}6")
  expect(screen.getByRole("alert")).toBeInTheDocument()
  expect(nodeChanges).not.toHaveBeenCalled()
  await userEvent.type(attempts, "{Backspace}5")
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  expect(lastNode()?.retry).toEqual({ maxAttempts: 5 })

  const timeout = screen.getByLabelText("Timeout (seconds)")
  expect(timeout).toHaveValue(900)
  await userEvent.clear(timeout)
  await userEvent.type(timeout, "14401")
  expect(screen.getByRole("alert")).toHaveTextContent(
    "Enter a whole number from 1 to 14400.",
  )
  expect(lastNode()?.timeoutSeconds).toBe(1440)
  await userEvent.type(timeout, "{Backspace}0")
  expect(lastNode()?.timeoutSeconds).toBe(14_400)
  expect(lastNode()?.retry).toEqual({ maxAttempts: 5 })
})

it("without a selected node it edits the workflow input schema as JSON", async () => {
  const { schemaChanges } = renderInspector(null)
  expect(
    screen.getByRole("heading", { name: "Workflow inputs" }),
  ).toBeInTheDocument()
  const editor = screen.getByLabelText("Input schema (JSON)")
  expect(editor).toHaveValue(
    JSON.stringify(infrastructure.workflowInputSchema, null, 2),
  )
  await userEvent.clear(editor)
  await userEvent.paste("null")
  expect(screen.getByRole("alert")).toBeInTheDocument()
  expect(schemaChanges).not.toHaveBeenCalled()
  await userEvent.clear(editor)
  await userEvent.paste('{"type": "object", "properties": {}}')
  expect(schemaChanges).toHaveBeenLastCalledWith({
    type: "object",
    properties: {},
  })
})

it.each(["vpc", null])(
  "read-only disables every control (node %s)",
  (nodeId) => {
    renderInspector(nodeId, { readOnly: true })
    const controls = screen
      .getByRole("complementary", { name: "Inspector" })
      .querySelectorAll("input, select, textarea, button")
    expect(controls.length).toBeGreaterThan(0)
    for (const control of controls) {
      expect(control).toBeDisabled()
    }
  },
)
```

- [ ] **Step 3: Extend the studio tests**

The new test selects nodes with a click (XYFlow selects on click) and clears the selection with a click on
the pane. The read-only rows also check that the inspector shows the selected node with disabled controls.

In `apps/web/src/studio/studio.test.tsx`, apply these replacements in order:

Replace:

```tsx
  expect(status).toBeEmptyDOMElement()
})

it.each([
```

With:

```tsx
  expect(status).toBeEmptyDOMElement()
})

it("the inspector edits the selected node, or the workflow inputs when none is", async () => {
  const { container } = renderStudio()
  const vpc = await screen.findByRole("group", {
    name: "Create VPC (simulated)",
  })
  const inspector = screen.getByRole("complementary", { name: "Inspector" })
  expect(within(inspector).getByLabelText("Input schema (JSON)")).toHaveValue(
    JSON.stringify(infrastructure.graph.workflowInputSchema, null, 2),
  )

  fireEvent.click(vpc)
  const name = await within(inspector).findByLabelText("Display name")
  expect(name).toHaveValue("Create VPC (simulated)")
  // Backspace edits the field; it does not delete the selected node.
  await userEvent.clear(name)
  await userEvent.type(name, "Networks{Backspace}")
  expect(screen.getByRole("group", { name: "Network" })).toBeInTheDocument()
  expect(container.querySelectorAll(".react-flow__node")).toHaveLength(3)

  fireEvent.click(screen.getByRole("group", { name: "Create RDS (simulated)" }))
  expect(
    await within(inspector).findByText("Connected from Network.vpcId"),
  ).toBeInTheDocument()

  const pane = container.querySelector<HTMLElement>(".react-flow__pane")
  if (!pane) throw new Error("No pane")
  fireEvent.click(pane)
  expect(
    await within(inspector).findByLabelText("Input schema (JSON)"),
  ).toBeInTheDocument()
})

it.each([
```

Replace:

```tsx
    drop(await findCanvas(), "application/x-corqty-definition", mockTask.id)
    expect(container.querySelectorAll(".react-flow__node")).toHaveLength(3)
  },
)
```

With:

```tsx
    drop(await findCanvas(), "application/x-corqty-definition", mockTask.id)
    expect(container.querySelectorAll(".react-flow__node")).toHaveLength(3)

    // The inspector shows the selected node but cannot change it.
    fireEvent.click(vpc)
    expect(await screen.findByLabelText("Display name")).toBeDisabled()
    expect(screen.getByLabelText("Max attempts")).toBeDisabled()
  },
)
```

- [ ] **Step 4: Run the tests and verify RED**

Run:

```text
pnpm web:test
```

Expected: `Test Files  3 failed | 6 passed (9)` and `Tests  3 failed | 27 passed (30)`.
`widgets.test.tsx` fails to load with `Error: Failed to resolve import "@/studio/widgets" from
"src/studio/widgets.test.tsx". Does the file exist?`, and `inspector.test.tsx` with the same error for
`"@/studio/inspector"`. In `studio.test.tsx` the new test fails with `TestingLibraryElementError: Unable to
find an accessible element with the role "complementary" and name "Inspector"`, and both read-only rows fail
with `TestingLibraryElementError: Unable to find a label with the text of: Display name`.

- [ ] **Step 5: Add the textarea component**

This is the shadcn base-nova `textarea` registry file, written by hand with `cn` imported from
`@/lib/utils`. Running `shadcn add textarea` would import `cn` from the `cn` package and add it as a
dependency (D3).

Create `apps/web/src/components/ui/textarea.tsx`:

```tsx
import * as React from "react"

import { cn } from "@/lib/utils"

function Textarea({ className, ...props }: React.ComponentProps<"textarea">) {
  return (
    <textarea
      data-slot="textarea"
      className={cn(
        "flex field-sizing-content min-h-16 w-full rounded-lg border border-input bg-transparent px-2.5 py-2 text-base transition-colors outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:bg-input/50 disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20 md:text-sm dark:bg-input/30 dark:disabled:bg-input/80 dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40",
        className
      )}
      {...props}
    />
  )
}

export { Textarea }
```

- [ ] **Step 6: Implement the widgets**

Create `apps/web/src/studio/widgets.tsx`:

```tsx
"use client"

import { XIcon } from "lucide-react"
import { useId, useState, type ReactNode } from "react"

import type { JsonSchema, UiField } from "@/api/types"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"

// Mirrors corqty_domain::catalog::FILE_TEXT_MAX_BYTES.
export const FILE_TEXT_MAX_BYTES = 262_144

// Native selects, styled like Input.
export const selectClassName =
  "h-8 w-full min-w-0 rounded-lg border border-input bg-transparent px-2.5 py-1 text-base outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50 md:text-sm dark:bg-input/30"

const checkboxClassName = "size-4 accent-primary"

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

export type WidgetProps = {
  label: string
  help?: string | null
  schema: JsonSchema
  value: unknown
  onChange: (value: unknown) => void
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
}

function stringEnum(schema: unknown): string[] | undefined {
  const values = isRecord(schema) ? schema.enum : undefined
  return Array.isArray(values) &&
    values.every((value) => typeof value === "string")
    ? values
    : undefined
}

function Help({ children }: { children: ReactNode }) {
  return <p className="text-xs text-muted-foreground">{children}</p>
}

function Problem({ children }: { children: ReactNode }) {
  return (
    <p role="alert" className="text-xs text-destructive">
      {children}
    </p>
  )
}

function Labelled({
  id,
  label,
  help,
  children,
}: {
  id: string
  label: string
  help?: string | null
  children: ReactNode
}) {
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      {children}
      {help && <Help>{help}</Help>}
    </div>
  )
}

function TextWidget({ label, help, value, onChange }: WidgetProps) {
  const id = useId()
  return (
    <Labelled id={id} label={label} help={help}>
      <Input
        id={id}
        value={typeof value === "string" ? value : ""}
        onChange={(event) => onChange(event.target.value)}
      />
    </Labelled>
  )
}

function TextareaWidget({ label, help, value, onChange }: WidgetProps) {
  const id = useId()
  return (
    <Labelled id={id} label={label} help={help}>
      <Textarea
        id={id}
        className="max-h-64"
        value={typeof value === "string" ? value : ""}
        onChange={(event) => onChange(event.target.value)}
      />
    </Labelled>
  )
}

// An empty field writes null; integer schemas get whole numbers.
function NumberWidget({ label, help, schema, value, onChange }: WidgetProps) {
  const id = useId()
  const integer = schema.type === "integer"
  return (
    <Labelled id={id} label={label} help={help}>
      <Input
        id={id}
        type="number"
        step={integer ? 1 : "any"}
        value={typeof value === "number" ? value : ""}
        onChange={({ target }) => {
          const number = Number(target.value)
          onChange(
            target.value === "" ? null : integer ? Math.trunc(number) : number,
          )
        }}
      />
    </Labelled>
  )
}

function BooleanWidget({ label, help, value, onChange }: WidgetProps) {
  const id = useId()
  return (
    <div className="grid gap-1.5">
      <div className="flex items-center gap-2">
        <input
          id={id}
          type="checkbox"
          className={checkboxClassName}
          checked={value === true}
          onChange={(event) => onChange(event.target.checked)}
        />
        <Label htmlFor={id}>{label}</Label>
      </div>
      {help && <Help>{help}</Help>}
    </div>
  )
}

function SelectWidget({ label, help, schema, value, onChange }: WidgetProps) {
  const id = useId()
  return (
    <Labelled id={id} label={label} help={help}>
      <select
        id={id}
        className={selectClassName}
        value={typeof value === "string" ? value : ""}
        onChange={(event) => onChange(event.target.value)}
      >
        <option value="" disabled>
          Choose…
        </option>
        {stringEnum(schema)?.map((option) => (
          <option key={option}>{option}</option>
        ))}
      </select>
    </Labelled>
  )
}

function MultiSelectWidget({
  label,
  help,
  schema,
  value,
  onChange,
}: WidgetProps) {
  const options = stringEnum(schema.items) ?? []
  const selected = Array.isArray(value) ? value : []
  return (
    <fieldset className="grid gap-1.5">
      <legend className="mb-1.5 text-sm font-medium">{label}</legend>
      {options.map((option) => (
        <label key={option} className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            className={checkboxClassName}
            checked={selected.includes(option)}
            onChange={(event) =>
              onChange(
                options.filter((candidate) =>
                  candidate === option
                    ? event.target.checked
                    : selected.includes(candidate),
                ),
              )
            }
          />
          {option}
        </label>
      ))}
      {help && <Help>{help}</Help>}
    </fieldset>
  )
}

// ponytail: a UUID field until secrets exist. The text is local: only this
// widget changes the value while it is shown.
function SecretReferenceWidget({ label, help, value, onChange }: WidgetProps) {
  const id = useId()
  const [text, setText] = useState(
    isRecord(value) && typeof value.secretId === "string" ? value.secretId : "",
  )
  const invalid = text !== "" && !uuid.test(text)
  return (
    <Labelled id={id} label={label} help={help}>
      <Input
        id={id}
        value={text}
        aria-invalid={invalid || undefined}
        onChange={({ target }) => {
          setText(target.value)
          if (target.value === "") onChange(null)
          else if (uuid.test(target.value)) {
            onChange({ kind: "secret", secretId: target.value })
          }
        }}
      />
      {invalid && <Problem>Enter the secret id as a UUID.</Problem>}
      <Help>Secret picker arrives with secrets.</Help>
    </Labelled>
  )
}

function json(value: unknown) {
  return value === undefined ? "" : JSON.stringify(value, null, 2)
}

// Object fields accept only a JSON object.
function parse(text: string, schema: JsonSchema) {
  try {
    const value: unknown = JSON.parse(text)
    return schema.type !== "object" || isRecord(value) ? { value } : undefined
  } catch {
    return undefined
  }
}

function CodeWidget({ label, help, schema, value, onChange }: WidgetProps) {
  const id = useId()
  const [text, setText] = useState(() => json(value))
  const [shown, setShown] = useState(value)
  // Follow changes made elsewhere, such as a blueprint adding workflow inputs.
  if (value !== shown) {
    setShown(value)
    setText(json(value))
  }
  const invalid = text !== json(value) && !parse(text, schema)
  return (
    <Labelled id={id} label={label} help={help}>
      <Textarea
        id={id}
        className="max-h-64 font-mono text-xs"
        spellCheck={false}
        value={text}
        aria-invalid={invalid || undefined}
        onChange={({ target }) => {
          setText(target.value)
          const parsed = parse(target.value, schema)
          if (parsed) {
            setShown(parsed.value)
            onChange(parsed.value)
          }
        }}
      />
      {invalid && (
        <Problem>
          This is not valid JSON for this field. The last valid value is kept.
        </Problem>
      )}
    </Labelled>
  )
}

// The rows are local: only this widget changes the value while it is shown.
function KeyValueWidget({ label, help, value, onChange }: WidgetProps) {
  const [rows, setRows] = useState(() =>
    Object.entries(isRecord(value) ? value : {}).map(
      ([key, entry]): [string, string] => [key, String(entry)],
    ),
  )
  function update(next: [string, string][]) {
    setRows(next)
    onChange(Object.fromEntries(next.filter(([key]) => key !== "")))
  }
  return (
    <fieldset className="grid gap-2">
      <legend className="mb-1.5 text-sm font-medium">{label}</legend>
      {rows.map(([key, entry], index) => (
        // Rows have no identity besides their position.
        <div key={index} className="grid grid-cols-[1fr_1fr_auto] gap-2">
          <Input
            aria-label={`Key ${index + 1}`}
            value={key}
            onChange={({ target }) =>
              update(
                rows.map((row, other) =>
                  other === index ? [target.value, entry] : row,
                ),
              )
            }
          />
          <Input
            aria-label={`Value ${index + 1}`}
            value={entry}
            onChange={({ target }) =>
              update(
                rows.map((row, other) =>
                  other === index ? [key, target.value] : row,
                ),
              )
            }
          />
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label={`Remove entry ${index + 1}`}
            onClick={() => update(rows.filter((_, other) => other !== index))}
          >
            <XIcon />
          </Button>
        </div>
      ))}
      <Button
        type="button"
        variant="outline"
        size="sm"
        className="justify-self-start"
        onClick={() => setRows([...rows, ["", ""]])}
      >
        Add entry
      </Button>
      {help && <Help>{help}</Help>}
    </fieldset>
  )
}

function FileTextWidget({ label, help, value, onChange }: WidgetProps) {
  const id = useId()
  const [tooLarge, setTooLarge] = useState(false)
  async function load(file: File | undefined) {
    if (!file) return
    setTooLarge(file.size > FILE_TEXT_MAX_BYTES)
    if (file.size <= FILE_TEXT_MAX_BYTES) onChange(await file.text())
  }
  return (
    <Labelled id={id} label={label} help={help}>
      <Textarea
        id={id}
        className="max-h-64 font-mono text-xs"
        spellCheck={false}
        value={typeof value === "string" ? value : ""}
        onChange={(event) => onChange(event.target.value)}
      />
      <Input
        type="file"
        aria-label={`Import ${label} from a file`}
        onChange={(event) => void load(event.target.files?.[0])}
      />
      {tooLarge && <Problem>Choose a file of at most 262,144 bytes.</Problem>}
    </Labelled>
  )
}

// One editor per uiSchema widget name (corqty_domain::catalog::WIDGETS), each
// writing its value contract.
export const widgets = {
  text: TextWidget,
  textarea: TextareaWidget,
  number: NumberWidget,
  boolean: BooleanWidget,
  select: SelectWidget,
  multiSelect: MultiSelectWidget,
  secretReference: SecretReferenceWidget,
  code: CodeWidget,
  keyValue: KeyValueWidget,
  fileText: FileTextWidget,
} satisfies Record<string, (props: WidgetProps) => ReactNode>

export type WidgetName = keyof typeof widgets

// The uiSchema widget, or else the default for the schema.
export function widgetFor(schema: JsonSchema, field?: UiField): WidgetName {
  if (field && Object.hasOwn(widgets, field.widget)) {
    return field.widget as WidgetName
  }
  switch (schema.type) {
    case "string":
      return stringEnum(schema) ? "select" : "text"
    case "integer":
    case "number":
      return "number"
    case "boolean":
      return "boolean"
    case "array":
      return stringEnum(schema.items) ? "multiSelect" : "code"
    default:
      return "code"
  }
}

const blanks: Partial<Record<WidgetName, unknown>> = {
  text: "",
  select: "",
  number: 0,
  boolean: false,
  multiSelect: [],
}

// A new literal starts from the schema's default or its widget's empty value.
export function blankValue(schema: JsonSchema): unknown {
  return structuredClone(schema.default ?? blanks[widgetFor(schema)] ?? null)
}
```

- [ ] **Step 7: Implement the inspector**

Create `apps/web/src/studio/inspector.tsx`:

```tsx
"use client"

import { useId, useState } from "react"

import type {
  GraphNode,
  GraphSnapshot,
  InputBinding,
  JsonSchema,
  NodeDefinition,
  PortSpec,
} from "@/api/types"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { findDefinition } from "@/studio/graph-model"
import {
  blankValue,
  selectClassName,
  widgetFor,
  widgets,
} from "@/studio/widgets"

const bindingKinds = {
  none: "Not bound",
  literal: "Literal value",
  workflowInput: "Workflow input",
  secret: "Secret",
}

type BindingKind = keyof typeof bindingKinds

function text(value: unknown) {
  return typeof value === "string" ? value : undefined
}

// Writes only whole numbers in range, so the draft always deserializes.
function WholeNumber({
  label,
  min,
  max,
  value,
  onChange,
}: {
  label: string
  min: number
  max: number
  value: number
  onChange: (value: number) => void
}) {
  const id = useId()
  const [entered, setEntered] = useState(String(value))
  const valid = (candidate: string) =>
    /^\d+$/.test(candidate) &&
    Number(candidate) >= min &&
    Number(candidate) <= max
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      <Input
        id={id}
        type="number"
        min={min}
        max={max}
        step={1}
        value={entered}
        aria-invalid={!valid(entered) || undefined}
        onChange={({ target }) => {
          setEntered(target.value)
          if (valid(target.value)) onChange(Number(target.value))
        }}
      />
      {!valid(entered) && (
        <p role="alert" className="text-xs text-destructive">
          Enter a whole number from {min} to {max}.
        </p>
      )}
    </div>
  )
}

function Binding({
  graph,
  node,
  port,
  onChange,
}: {
  graph: GraphSnapshot
  node: GraphNode
  port: PortSpec
  onChange: (node: GraphNode) => void
}) {
  const kindId = useId()
  const pointerId = useId()
  const binding = node.inputs[port.name]
  const [kind, setKind] = useState<BindingKind>(binding?.kind ?? "none")
  const edge = graph.edges.find(
    ({ target }) => target.nodeId === node.id && target.port === port.name,
  )
  const source = graph.nodes.find(({ id }) => id === edge?.source.nodeId)
  const pointer = binding?.kind === "workflowInput" ? binding.pointer : ""
  // ponytail: the picker offers top-level workflow inputs only.
  const pointers = Object.keys(graph.workflowInputSchema.properties ?? {}).map(
    (name) => `/${name.replaceAll("~", "~0").replaceAll("/", "~1")}`,
  )
  if (pointer && !pointers.includes(pointer)) pointers.push(pointer)
  const Literal = widgets[widgetFor(port.schema)]

  // null leaves the port unbound.
  function bind(next: InputBinding | null) {
    const inputs = Object.fromEntries(
      Object.entries(node.inputs).filter(([name]) => name !== port.name),
    )
    onChange({
      ...node,
      inputs: next ? { ...inputs, [port.name]: next } : inputs,
    })
  }

  function choose(next: BindingKind) {
    setKind(next)
    // A workflow input or a secret binds once one is chosen.
    bind(
      next === "literal"
        ? { kind: "literal", value: blankValue(port.schema) }
        : null,
    )
  }

  return (
    <fieldset className="grid gap-2 rounded-lg border p-3">
      <legend className="px-1 text-sm font-medium">
        {port.name}{" "}
        <span className="font-mono text-xs font-normal text-muted-foreground">
          {port.dataType}
          {port.required && ", required"}
        </span>
      </legend>
      {edge ? (
        <p className="text-sm text-muted-foreground">
          Connected from {source?.displayName ?? edge.source.nodeId}.
          {edge.source.port}
        </p>
      ) : (
        <>
          <Label htmlFor={kindId}>Binding</Label>
          <select
            id={kindId}
            className={selectClassName}
            value={kind}
            onChange={(event) => choose(event.target.value as BindingKind)}
          >
            {Object.entries(bindingKinds).map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
          {binding?.kind === "literal" && (
            <Literal
              label="Value"
              schema={port.schema}
              value={binding.value}
              onChange={(value) => bind({ kind: "literal", value })}
            />
          )}
          {kind === "workflowInput" && (
            <>
              <Label htmlFor={pointerId}>Workflow input</Label>
              <select
                id={pointerId}
                className={selectClassName}
                value={pointer}
                onChange={(event) =>
                  bind({ kind: "workflowInput", pointer: event.target.value })
                }
              >
                <option value="" disabled>
                  Choose an input…
                </option>
                {pointers.map((option) => (
                  <option key={option}>{option}</option>
                ))}
              </select>
            </>
          )}
          {kind === "secret" && (
            <widgets.secretReference
              label="Secret id"
              schema={{}}
              value={binding?.kind === "secret" ? binding : null}
              onChange={(value) => bind(value as InputBinding | null)}
            />
          )}
        </>
      )}
    </fieldset>
  )
}

function NodeSettings({
  graph,
  node,
  definition,
  onChange,
}: {
  graph: GraphSnapshot
  node: GraphNode
  definition: NodeDefinition | undefined
  onChange: (node: GraphNode) => void
}) {
  const properties = Object.entries(definition?.configSchema.properties ?? {})
  return (
    <>
      <header className="grid gap-0.5">
        <h2 className="font-semibold">{node.displayName}</h2>
        <p className="font-mono text-xs text-muted-foreground">
          {node.definition.key} v{node.definition.version}
        </p>
      </header>
      <widgets.text
        label="Display name"
        schema={{ type: "string" }}
        value={node.displayName}
        onChange={(value) => onChange({ ...node, displayName: String(value) })}
      />
      {properties.length > 0 && (
        <section className="grid gap-3">
          <h3 className="text-xs font-medium text-muted-foreground">
            Configuration
          </h3>
          {properties.map(([name, schema]) => {
            const field = definition?.uiSchema[name]
            const Widget = widgets[widgetFor(schema, field)]
            return (
              <Widget
                key={name}
                label={field?.label ?? text(schema.title) ?? name}
                help={field?.help ?? text(schema.description)}
                schema={schema}
                value={node.config[name]}
                onChange={(value) =>
                  onChange({
                    ...node,
                    config: { ...node.config, [name]: value },
                  })
                }
              />
            )
          })}
        </section>
      )}
      {definition && definition.inputs.length > 0 && (
        <section className="grid gap-3">
          <h3 className="text-xs font-medium text-muted-foreground">Inputs</h3>
          {definition.inputs.map((port) => (
            <Binding
              key={port.name}
              graph={graph}
              node={node}
              port={port}
              onChange={onChange}
            />
          ))}
        </section>
      )}
      <section className="grid gap-3">
        <h3 className="text-xs font-medium text-muted-foreground">Execution</h3>
        <WholeNumber
          label="Max attempts"
          min={1}
          max={5}
          value={node.retry.maxAttempts}
          onChange={(maxAttempts) =>
            onChange({ ...node, retry: { maxAttempts } })
          }
        />
        <WholeNumber
          label="Timeout (seconds)"
          min={1}
          max={14_400}
          value={node.timeoutSeconds}
          onChange={(timeoutSeconds) => onChange({ ...node, timeoutSeconds })}
        />
      </section>
    </>
  )
}

export function Inspector({
  graph,
  definitions,
  nodeId,
  readOnly,
  onNodeChange,
  onInputSchemaChange,
}: {
  graph: GraphSnapshot
  definitions: NodeDefinition[]
  nodeId: string | null
  readOnly: boolean
  onNodeChange: (node: GraphNode) => void
  onInputSchemaChange: (schema: JsonSchema) => void
}) {
  const node = graph.nodes.find(({ id }) => id === nodeId)
  return (
    <aside
      aria-label="Inspector"
      className="overflow-y-auto border-t p-4 lg:border-t-0 lg:border-l"
    >
      {/* A disabled fieldset disables every control inside it. */}
      <fieldset disabled={readOnly} className="grid gap-5">
        {node ? (
          <NodeSettings
            key={node.id}
            graph={graph}
            node={node}
            definition={findDefinition(definitions, node.definition)}
            onChange={onNodeChange}
          />
        ) : (
          <>
            <h2 className="font-semibold">Workflow inputs</h2>
            <widgets.code
              label="Input schema (JSON)"
              help="Node inputs can bind to its top-level properties. Select a node to edit it."
              schema={{ type: "object" }}
              value={graph.workflowInputSchema}
              onChange={(value) => onInputSchemaChange(value as JsonSchema)}
            />
          </>
        )}
      </fieldset>
    </aside>
  )
}
```

- [ ] **Step 8: Show the inspector beside the canvas**

In `apps/web/src/studio/studio-canvas.tsx`, apply these replacements in order:

Replace:

```tsx
import type { Blueprint, GraphSnapshot, NodeDefinition } from "@/api/types"
```

With:

```tsx
import type {
  Blueprint,
  GraphNode,
  GraphSnapshot,
  NodeDefinition,
} from "@/api/types"
```

Replace:

```tsx
} from "@/studio/graph-model"
import { BLUEPRINT_TYPE, DEFINITION_TYPE } from "@/studio/palette"
```

With:

```tsx
} from "@/studio/graph-model"
import { Inspector } from "@/studio/inspector"
import { BLUEPRINT_TYPE, DEFINITION_TYPE } from "@/studio/palette"
```

Replace:

```tsx
  const current = () => fromFlow(snapshot, nodes, edges)
```

With:

```tsx
  const current = () => fromFlow(snapshot, nodes, edges)
  // The inspector edits a single selected node, otherwise the workflow inputs.
  const selected = nodes.filter((node) => node.selected)

  function updateNode(node: GraphNode) {
    setNodes((existing) =>
      existing.map((candidate) =>
        candidate.id === node.id
          ? {
              ...candidate,
              ariaLabel: node.displayName,
              data: { ...candidate.data, node },
            }
          : candidate,
      ),
    )
  }
```

Replace:

```tsx
  return (
    <div className="flex min-h-96 flex-col">
      <div className="min-h-0 flex-1">
        <ReactFlow
          nodes={nodes}
          edges={edges}
          nodeTypes={nodeTypes}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onConnect={onConnect}
          onConnectEnd={onConnectEnd}
          isValidConnection={isValidConnection}
          onDragOver={readOnly ? undefined : onDragOver}
          onDrop={readOnly ? undefined : onDrop}
          nodesDraggable={!readOnly}
          nodesConnectable={!readOnly}
          deleteKeyCode={readOnly ? null : ["Backspace", "Delete"]}
          defaultViewport={graph.viewport}
          colorMode="system"
        >
          <Background />
          <Controls showInteractive={false} />
        </ReactFlow>
      </div>
      <p
        role="status"
        aria-live="polite"
        className="min-h-9 border-t px-4 py-2 text-sm text-muted-foreground"
      >
        {announcement}
      </p>
    </div>
  )
}
```

With:

```tsx
  return (
    <div className="grid min-h-0 lg:grid-cols-[1fr_20rem]">
      <div className="flex min-h-96 flex-col">
        <div className="min-h-0 flex-1">
          <ReactFlow
            nodes={nodes}
            edges={edges}
            nodeTypes={nodeTypes}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            onConnect={onConnect}
            onConnectEnd={onConnectEnd}
            isValidConnection={isValidConnection}
            onDragOver={readOnly ? undefined : onDragOver}
            onDrop={readOnly ? undefined : onDrop}
            nodesDraggable={!readOnly}
            nodesConnectable={!readOnly}
            deleteKeyCode={readOnly ? null : ["Backspace", "Delete"]}
            defaultViewport={graph.viewport}
            colorMode="system"
          >
            <Background />
            <Controls showInteractive={false} />
          </ReactFlow>
        </div>
        <p
          role="status"
          aria-live="polite"
          className="min-h-9 border-t px-4 py-2 text-sm text-muted-foreground"
        >
          {announcement}
        </p>
      </div>
      <Inspector
        graph={current()}
        definitions={definitions}
        nodeId={selected.length === 1 ? selected[0].id : null}
        readOnly={readOnly}
        onNodeChange={updateNode}
        onInputSchemaChange={(workflowInputSchema) =>
          setSnapshot((existing) => ({ ...existing, workflowInputSchema }))
        }
      />
    </div>
  )
}
```

- [ ] **Step 9: Verify GREEN**

Run:

```text
pnpm web:test
```

Expected: `Test Files  9 passed (9)` and `Tests  52 passed (52)` with no unhandled errors (widgets 15,
inspector 7, studio 8). On Node 25 or newer, each worker may print `ExperimentalWarning: localStorage is not
available because --localstorage-file was not provided.`; this is harmless.

- [ ] **Step 10: Run lint and build**

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

Expected: `✓ Compiled successfully`, `Finished TypeScript`, and the same route table as Task 16, with
`ƒ /orgs/[organizationId]/workflows/[workflowId]` still dynamic. It exits 0.

- [ ] **Step 11: Commit**

```text
git add apps/web
git commit -m "feat: add the schema-driven inspector and widgets"
```

## Task 18: Save drafts automatically and validate and publish from the studio

**Files:**

- Create: `apps/web/src/studio/diagnostics.ts`
- Create: `apps/web/src/studio/use-draft-autosave.ts`
- Modify: `apps/web/src/api/types.ts`
- Modify: `apps/web/src/api/queries.ts`
- Modify: `apps/web/src/studio/studio-node.tsx`
- Modify: `apps/web/src/studio/studio-canvas.tsx`
- Modify: `apps/web/src/studio/studio.tsx`
- Modify: `apps/web/src/test/handlers.ts`
- Test: `apps/web/src/studio/diagnostics.test.ts` (new)
- Test: `apps/web/src/studio/use-draft-autosave.test.ts` (new)
- Test: `apps/web/src/test/raw-colors.test.ts` (new)
- Test: `apps/web/src/studio/studio.test.tsx`

The studio saves the draft on its own and can validate and publish it. `src/studio/use-draft-autosave.ts`
exports `useDraftAutosave(organizationId, workflowId, draft, readOnly)`. The canvas reports its graph after
every render (`onChange`, which now also carries viewport moves through `onMoveEnd`). The first report is the
loaded draft as the canvas holds it, and later reports that serialize to the same JSON (selection, node
measuring) change nothing. A real change starts an 800 ms timer (`AUTOSAVE_DELAY_MS`), and each further change
restarts it. When the timer fires, the hook sends `PUT .../draft {revision, graph}`. Saves form a queue, so one
request is in flight at a time and the next one carries the revision the last one returned. The status is
`saving` while a timer or a save is pending, else `saved`. A 409 sets `conflict` and stops saving until the
studio reloads the draft. Any other failure (413 `payload_too_large`, 422 `graph_invalid` for the size limits, a
500 or a network error, shown as `network_error`) sets `error` with its code and stops saving until the next
edit. In read-only mode nothing is saved. `flush()` sends a pending edit at once, waits for every save and
resolves to the saved `{revision, graph}`, or null after a conflict or an error. `conflict()` lets a publish
409 stop saving as well. Leaving the studio flushes a pending edit instead of dropping it.

The top bar shows a breadcrumb (`Workflows` / the workflow name as the page heading) and the save status badge.
Read-only mode shows the read-only badge and its reason instead. The bar also shows `rev N`, `vN published`
when the workflow has a published version, a polite live notice, and, only with `publish_workflows`, the
Validate and Publish buttons. Both buttons first `flush()` the autosave and then act on the saved draft:
- Validate posts that graph, and the notice says `No problems found.` or `N problem(s) found.`
- Publish posts `{revision}`. A 201 says `Published vN.` and refreshes the organization's queries, which
  updates `vN published`. A 422 `graph_invalid` lists the diagnostics. A 409 `draft_revision_conflict` shows the
  conflict banner. A 409 `draft_already_published` refetches the workflow and says `Already published as vN.`
- Any other failure says `Validation failed (<code>).` or `Publishing failed (<code>).`

The conflict banner (`role="alert"`) explains that the draft was saved elsewhere and offers Reload. Reload
refetches the workflow and remounts the editor under a new key, which resets the canvas, the inspector and the
autosave to the latest draft. The page only falls back to `The workflow could not be loaded.` while it has no
data, so a failed background refetch no longer replaces a loaded editor.

`src/studio/diagnostics.ts` exports `mapDiagnostics(sentGraph, diagnostics)`. It resolves each RFC 6901 pointer
against the graph that was sent:
- `/nodes/<i>...` gives `nodeId`, and `/nodes/<i>/inputs/<name>...` also gives `port: "in:<name>"`, with the
  name unescaped.
- `/edges/<i>/source` gives `edgeId` plus the source node and its `out:` port.
- `/edges/<i>/id` gives only `edgeId`.
- Any other `/edges/<i>` pointer gives `edgeId` plus the target node and its `in:` port.
- Pointers outside the nodes and edges, or past their ends, stay unplaced.

`port` is the canvas handle id, so an input and an output with the same name stay apart. The studio lists the
problems in a `Problems` panel as `<node display name> · <port>` (or the pointer), the message and the code. The
canvas shares them through `DiagnosticsContext`, and a node with problems gets `data-invalid` with a
destructive ring. Each affected port row gets `data-invalid` and a dot marker named `Has a problem`.

`src/test/raw-colors.test.ts` is the D17 guard. It fails on raw palette classes (a `bg`, `text`, `border`,
`ring`, `fill` or `stroke` prefix with `red`, `green`, `amber`, `yellow`, `blue`, `emerald`, `orange` or `rose`
and a shade) and on color literals (hex, `rgb()`, `hsl()` and `oklch()`) in any `.ts` or `.tsx` file under
`src` outside `components/status` and `components/ui`.

- [ ] **Step 1: Write the failing diagnostics tests**

Create `apps/web/src/studio/diagnostics.test.ts`:

```ts
import { expect, it } from "vitest"

import { mapDiagnostics } from "@/studio/diagnostics"
import { blueprints } from "@/test/handlers"

// Nodes vpc, rds and deploy; edges vpc-rds and vpc-deploy.
const graph = blueprints[1].graph

it.each([
  ["/nodes/1", { nodeId: "rds" }],
  ["/nodes/0/config/delayMs", { nodeId: "vpc" }],
  ["/nodes/2/inputs/vpcId", { nodeId: "deploy", port: "in:vpcId" }],
  ["/nodes/0/inputs/a~1b~0c/value/0", { nodeId: "vpc", port: "in:a/b~c" }],
  ["/edges/1", { edgeId: "vpc-deploy", nodeId: "deploy", port: "in:vpcId" }],
  ["/edges/0/source", { edgeId: "vpc-rds", nodeId: "vpc", port: "out:vpcId" }],
  ["/edges/0/target", { edgeId: "vpc-rds", nodeId: "rds", port: "in:vpcId" }],
  ["/edges/1/id", { edgeId: "vpc-deploy" }],
  ["/nodes/3/id", {}],
  ["/edges/2", {}],
  ["/nodes", {}],
  ["/workflowInputSchema/properties/region", {}],
])("places %s on the graph that was sent", (pointer, placement) => {
  const diagnostic = { code: "some_code", pointer, message: "A message." }
  expect(mapDiagnostics(graph, [diagnostic])).toEqual([
    { ...diagnostic, ...placement },
  ])
})
```

- [ ] **Step 2: Write the failing autosave tests**

The hook is rendered on its own with fake `setTimeout`/`clearTimeout`, so the 800 ms debounce is measured
exactly. Testing Library's `waitFor` drains its work with a `setTimeout(0)`, which a fake timer never fires.
The tests therefore wait for saves with `flush()` inside `act`, or with `vi.waitUntil`, which uses real timers.

Create `apps/web/src/studio/use-draft-autosave.test.ts`:

```ts
import { act, renderHook } from "@testing-library/react"
import { http, HttpResponse } from "msw/http"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import type { GraphSnapshot } from "@/api/types"
import {
  AUTOSAVE_DELAY_MS,
  useDraftAutosave,
} from "@/studio/use-draft-autosave"
import { workflow } from "@/test/handlers"
import { server } from "@/test/server"

type Save = { revision: number; graph: GraphSnapshot }

const { draft } = workflow // revision 3

function named(displayName: string): GraphSnapshot {
  const [first, ...rest] = draft.graph.nodes
  return { ...draft.graph, nodes: [{ ...first, displayName }, ...rest] }
}

// Records every save and answers it with `respond`, by default with the next
// revision.
function recordSaves(
  respond: (save: Save) => Response | Promise<Response> = ({ revision }) =>
    HttpResponse.json({ revision: revision + 1 }),
) {
  const saves: Save[] = []
  server.use(
    http.put("/api/v1/orgs/org-1/workflows/wf-2/draft", async ({ request }) => {
      const save = (await request.json()) as Save
      saves.push(save)
      return respond(save)
    }),
  )
  return saves
}

function problem(status: number, code: string) {
  return HttpResponse.json({ status, code }, { status })
}

// The canvas reports its graph once on mount: that is the loaded draft.
function renderAutosave(readOnly = false) {
  const { result } = renderHook(() =>
    useDraftAutosave("org-1", "wf-2", draft, readOnly),
  )
  act(() => result.current.edit(draft.graph))
  return result
}

type Autosave = ReturnType<typeof renderAutosave>

async function wait(ms: number) {
  await act(() => vi.advanceTimersByTimeAsync(ms))
}

// Waits for the saves already started, so React sees their updates in act.
// (Testing Library's waitFor needs a real setTimeout.)
async function settle(autosave: Autosave) {
  await act(() => autosave.current.flush())
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] })
})

afterEach(() => {
  vi.useRealTimers()
})

it("saves once, 800 ms after the last edit", async () => {
  const saves = recordSaves()
  const autosave = renderAutosave()
  act(() => autosave.current.edit(named("A")))
  await wait(500)
  act(() => autosave.current.edit(named("B")))
  expect(autosave.current.status).toBe("saving")
  await wait(AUTOSAVE_DELAY_MS - 1)
  expect(saves).toEqual([])

  await wait(1)
  await settle(autosave)
  expect(autosave.current.status).toBe("saved")
  expect(saves).toEqual([{ revision: 3, graph: named("B") }])
  expect(autosave.current.revision).toBe(4)
})

it("sends one save at a time", async () => {
  let answer = () => {}
  const saves = recordSaves(async ({ revision }) => {
    await new Promise<void>((resolve) => {
      answer = resolve
    })
    return HttpResponse.json({ revision: revision + 1 })
  })
  const autosave = renderAutosave()
  act(() => autosave.current.edit(named("A")))
  await wait(AUTOSAVE_DELAY_MS)
  await vi.waitUntil(() => saves.length === 1)

  act(() => autosave.current.edit(named("B")))
  await wait(AUTOSAVE_DELAY_MS)
  expect(saves).toHaveLength(1)
  await act(async () => {
    answer()
    await vi.waitUntil(() => saves.length === 2)
    answer()
    await autosave.current.flush()
  })
  expect(autosave.current.status).toBe("saved")
  expect(saves).toEqual([
    { revision: 3, graph: named("A") },
    { revision: 4, graph: named("B") },
  ])
  expect(autosave.current.revision).toBe(5)
})

it("a conflict stops saving for good", async () => {
  const saves = recordSaves(() => problem(409, "draft_revision_conflict"))
  const autosave = renderAutosave()
  act(() => autosave.current.edit(named("A")))
  await wait(AUTOSAVE_DELAY_MS)
  await settle(autosave)
  expect(autosave.current.status).toBe("conflict")

  act(() => autosave.current.edit(named("B")))
  await wait(AUTOSAVE_DELAY_MS)
  expect(autosave.current.status).toBe("conflict")
  expect(await act(() => autosave.current.flush())).toBeNull()
  expect(saves).toHaveLength(1)
})

it.each([
  [413, "payload_too_large"],
  [422, "graph_invalid"],
])("a %i stops saving until the next edit", async (status, code) => {
  let failing = true
  const saves = recordSaves(({ revision }) =>
    failing
      ? problem(status, code)
      : HttpResponse.json({ revision: revision + 1 }),
  )
  const autosave = renderAutosave()
  act(() => autosave.current.edit(named("A")))
  await wait(AUTOSAVE_DELAY_MS)
  await settle(autosave)
  expect(autosave.current.status).toBe("error")
  expect(autosave.current.errorCode).toBe(code)
  await wait(AUTOSAVE_DELAY_MS * 2)
  expect(saves).toHaveLength(1)

  failing = false
  act(() => autosave.current.edit(named("B")))
  await wait(AUTOSAVE_DELAY_MS)
  await settle(autosave)
  expect(autosave.current.status).toBe("saved")
  expect(saves).toEqual([
    { revision: 3, graph: named("A") },
    { revision: 3, graph: named("B") },
  ])
})

it("flush saves pending edits at once and returns the saved draft", async () => {
  const saves = recordSaves()
  const autosave = renderAutosave()
  act(() => autosave.current.edit(named("A")))
  expect(await act(() => autosave.current.flush())).toEqual({
    revision: 4,
    graph: named("A"),
  })
  expect(saves).toEqual([{ revision: 3, graph: named("A") }])
  await wait(AUTOSAVE_DELAY_MS)
  expect(saves).toHaveLength(1)
})

it("read-only mode never saves", async () => {
  const saves = recordSaves()
  const autosave = renderAutosave(true)
  act(() => autosave.current.edit(named("A")))
  await wait(AUTOSAVE_DELAY_MS)
  expect(autosave.current.status).toBe("saved")
  expect(await act(() => autosave.current.flush())).toEqual(draft)
  expect(saves).toEqual([])
})
```

- [ ] **Step 3: Add the raw-color scan**

This guard passes on the existing code. Its last assertion proves that the pattern does match the status
badge's colors.

Create `apps/web/src/test/raw-colors.test.ts`:

```ts
import { readdirSync, readFileSync } from "node:fs"
import { join, resolve } from "node:path"
import { expect, it } from "vitest"

const src = resolve(import.meta.dirname, "..")
// D17: status badges keep fixed hues and shadcn primitives ship their own;
// everything else uses the semantic tokens from globals.css.
const exempt = ["components/status/", "components/ui/"]
const rawColor =
  /\b(?:bg|text|border|ring|fill|stroke)-(?:red|green|amber|yellow|blue|emerald|orange|rose)-\d{2,3}\b|#[\da-f]{3,8}\b|\b(?:rgba?|hsla?|oklch)\(/gi

function rawColors(path: string) {
  return readFileSync(join(src, path), "utf8").match(rawColor) ?? []
}

it("only status components use raw palette colors", () => {
  const offenders = readdirSync(src, { recursive: true, encoding: "utf8" })
    .map((path) => path.replaceAll("\\", "/"))
    .filter(
      (path) =>
        /\.tsx?$/.test(path) &&
        !exempt.some((folder) => path.startsWith(folder)),
    )
    .flatMap((path) => rawColors(path).map((color) => `${path}: ${color}`))
  expect(offenders).toEqual([])
  // The pattern does catch raw colors where they are allowed.
  expect(rawColors("components/status/status-badge.tsx")).not.toEqual([])
})
```

- [ ] **Step 4: Give the test API a published version and a draft save**

The workflow fixture gains `publishedVersion: 1`, matching the `wf-2` summary. A default handler answers every
draft save with the next revision, so tests that edit the canvas do not hit an unhandled request.

In `apps/web/src/test/handlers.ts`, apply these replacements in order:

Replace:

```ts
export const workflow: Workflow = {
  id: "wf-2",
  name: "Provision staging",
  draft: { revision: 3, graph: blueprints[1].graph },
}

// A signed-in admin of org-1 reading data; tests override writes and failures.
```

With:

```ts
export const workflow: Workflow = {
  id: "wf-2",
  name: "Provision staging",
  publishedVersion: 1,
  draft: { revision: 3, graph: blueprints[1].graph },
}

// A signed-in admin of org-1 who reads data and saves drafts; tests override
// other writes and failures.
```

Replace:

```ts
  http.get("/api/v1/orgs/:organizationId/workflows/:workflowId", () =>
    HttpResponse.json(workflow),
  ),
```

With:

```ts
  http.get("/api/v1/orgs/:organizationId/workflows/:workflowId", () =>
    HttpResponse.json(workflow),
  ),
  http.put(
    "/api/v1/orgs/:organizationId/workflows/:workflowId/draft",
    async ({ request }) => {
      const { revision } = (await request.json()) as { revision: number }
      return HttpResponse.json({ revision: revision + 1 })
    },
  ),
```

- [ ] **Step 5: Extend the studio tests**

These tests run on real timers. Waits on the first save allow 2 s, because the save fires 800 ms after the
last keystroke.

In `apps/web/src/studio/studio.test.tsx`, apply these replacements in order:

Replace:

```tsx
import type { GraphSnapshot } from "@/api/types"
```

With:

```tsx
import type { Diagnostic, GraphSnapshot } from "@/api/types"
```

Replace:

```tsx
const infrastructure = blueprints[1] // corqty.simulated-infrastructure
```

With:

```tsx
const infrastructure = blueprints[1] // corqty.simulated-infrastructure
const workflowUrl = "/api/v1/orgs/:organizationId/workflows/:workflowId"
```

Replace:

```tsx
function withDraft(graph: GraphSnapshot) {
  server.use(
    http.get("/api/v1/orgs/:organizationId/workflows/:workflowId", () =>
      HttpResponse.json({ ...workflow, draft: { revision: 1, graph } }),
    ),
  )
}
```

With:

```tsx
function withDraft(graph: GraphSnapshot) {
  server.use(
    http.get(workflowUrl, () =>
      HttpResponse.json({ ...workflow, draft: { revision: 1, graph } }),
    ),
  )
}

function problem(status: number, code: string, diagnostics?: Diagnostic[]) {
  return HttpResponse.json(
    { type: "about:blank", title: "Problem", status, code, diagnostics },
    { status },
  )
}

// The draft with its VPC node renamed.
function renamed(displayName: string): GraphSnapshot {
  const [vpc, ...rest] = infrastructure.graph.nodes
  return { ...infrastructure.graph, nodes: [{ ...vpc, displayName }, ...rest] }
}

async function renameVpc(displayName: string) {
  fireEvent.click(
    await screen.findByRole("group", { name: "Create VPC (simulated)" }),
  )
  const name = await screen.findByLabelText("Display name")
  await userEvent.clear(name)
  await userEvent.type(name, displayName)
}
```

Replace:

```tsx
  expect(within(vpc).getByText("Simulated")).toBeInTheDocument()
```

With:

```tsx
  const breadcrumb = screen.getByRole("navigation", { name: "Breadcrumb" })
  expect(
    within(breadcrumb).getByRole("link", { name: "Workflows" }),
  ).toHaveAttribute("href", "/orgs/org-1/workflows")
  expect(screen.getByText("Saved")).toBeInTheDocument()
  expect(screen.getByText("rev 3")).toBeInTheDocument()
  expect(screen.getByText("v1 published")).toBeInTheDocument()
  expect(screen.getByRole("button", { name: "Validate" })).toBeEnabled()
  expect(screen.getByRole("button", { name: "Publish" })).toBeEnabled()
  expect(within(vpc).getByText("Simulated")).toBeInTheDocument()
```

Replace:

```tsx
    expect(screen.getByLabelText("Max attempts")).toBeDisabled()
  },
)
```

With:

```tsx
    expect(screen.getByLabelText("Max attempts")).toBeDisabled()
  },
)

it("saves the draft once editing pauses", async () => {
  const saves: unknown[] = []
  server.use(
    http.put(`${workflowUrl}/draft`, async ({ request }) => {
      saves.push(await request.json())
      return HttpResponse.json({ revision: 4 })
    }),
  )
  renderStudio()
  await renameVpc("Network")
  expect(screen.getByText("Saving…")).toBeInTheDocument()
  expect(
    await screen.findByText("rev 4", {}, { timeout: 2000 }),
  ).toBeInTheDocument()
  expect(screen.getByText("Saved")).toBeInTheDocument()
  expect(saves).toEqual([{ revision: 3, graph: renamed("Network") }])
})

it("a conflict stops saving until Reload loads the latest draft", async () => {
  server.use(
    http.put(`${workflowUrl}/draft`, () =>
      problem(409, "draft_revision_conflict"),
    ),
  )
  renderStudio()
  await renameVpc("Network")
  const banner = await screen.findByRole("alert", {}, { timeout: 2000 })
  expect(banner).toHaveTextContent("This draft was saved somewhere else.")
  expect(screen.getByText("Conflict")).toBeInTheDocument()
  expect(screen.getByRole("button", { name: "Publish" })).toBeDisabled()

  server.use(
    http.get(workflowUrl, () =>
      HttpResponse.json({
        ...workflow,
        draft: { revision: 7, graph: renamed("Remote network") },
      }),
    ),
  )
  await userEvent.click(within(banner).getByRole("button", { name: "Reload" }))
  expect(
    await screen.findByRole("group", { name: "Remote network" }),
  ).toBeInTheDocument()
  expect(screen.getByText("rev 7")).toBeInTheDocument()
  expect(screen.getByText("Saved")).toBeInTheDocument()
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
})

it("Validate saves pending edits first and checks the saved graph", async () => {
  const requests: unknown[] = []
  server.use(
    http.put(`${workflowUrl}/draft`, async ({ request }) => {
      requests.push(["save", await request.json()])
      return HttpResponse.json({ revision: 4 })
    }),
    http.post(`${workflowUrl}/validate`, async ({ request }) => {
      requests.push(["validate", await request.json()])
      return HttpResponse.json({ valid: true, diagnostics: [] })
    }),
  )
  renderStudio()
  await renameVpc("Network")
  await userEvent.click(screen.getByRole("button", { name: "Validate" }))
  expect(await screen.findByText("No problems found.")).toBeInTheDocument()
  expect(requests).toEqual([
    ["save", { revision: 3, graph: renamed("Network") }],
    ["validate", { graph: renamed("Network") }],
  ])
  expect(screen.getByText("rev 4")).toBeInTheDocument()
})

it("publishing an invalid draft lists the problems and marks the node and port", async () => {
  withDraft({ ...infrastructure.graph, edges: [] })
  const message = "This input is required: connect an edge or add a binding."
  const published: unknown[] = []
  server.use(
    http.post(`${workflowUrl}/publish`, async ({ request }) => {
      published.push(await request.json())
      return problem(422, "graph_invalid", [
        {
          code: "missing_required_input",
          pointer: "/nodes/1/inputs/vpcId",
          message,
        },
        {
          code: "missing_required_input",
          pointer: "/nodes/2/inputs/vpcId",
          message,
        },
      ])
    }),
  )
  renderStudio()
  const rds = await screen.findByRole("group", {
    name: "Create RDS (simulated)",
  })
  await userEvent.click(screen.getByRole("button", { name: "Publish" }))
  const list = await screen.findByRole("list", { name: "Problems" })
  expect(
    within(list)
      .getAllByRole("listitem")
      .map((item) => item.textContent),
  ).toEqual([
    `Create RDS (simulated) · vpcId ${message} missing_required_input`,
    `Deploy service (simulated) · vpcId ${message} missing_required_input`,
  ])
  expect(screen.getByText("2 problems found.")).toBeInTheDocument()
  expect(published).toEqual([{ revision: 1 }])

  expect(rds.firstElementChild).toHaveAttribute("data-invalid")
  const port = handle("rds", "in:vpcId").parentElement
  expect(port).toHaveAttribute("data-invalid")
  expect(
    within(port as HTMLElement).getByRole("img", { name: "Has a problem" }),
  ).toBeInTheDocument()
  const vpc = screen.getByRole("group", { name: "Create VPC (simulated)" })
  expect(vpc.firstElementChild).not.toHaveAttribute("data-invalid")
})

it("publishing reports the new version, then that the draft already has one", async () => {
  let publishedVersion = 1
  server.use(
    http.get(workflowUrl, () =>
      HttpResponse.json({ ...workflow, publishedVersion }),
    ),
    http.post(`${workflowUrl}/publish`, () => {
      if (publishedVersion === 2) return problem(409, "draft_already_published")
      publishedVersion = 2
      return HttpResponse.json(
        {
          id: "version-2",
          version: 2,
          checksum: "0".repeat(64),
          draftRevision: 3,
          publishedAt: "2026-10-26T00:00:00Z",
        },
        { status: 201 },
      )
    }),
  )
  renderStudio()
  await screen.findByRole("group", { name: "Create VPC (simulated)" })
  expect(screen.getByText("v1 published")).toBeInTheDocument()

  await userEvent.click(screen.getByRole("button", { name: "Publish" }))
  expect(await screen.findByText("Published v2.")).toBeInTheDocument()
  expect(screen.getByText("v2 published")).toBeInTheDocument()

  await userEvent.click(screen.getByRole("button", { name: "Publish" }))
  expect(
    await screen.findByText("Already published as v2."),
  ).toBeInTheDocument()
})

it.each(["runner", "viewer"] as const)(
  "a %s cannot validate or publish",
  async (role) => {
    server.use(http.get("/api/v1/auth/me", () => HttpResponse.json(meAs(role))))
    renderStudio()
    await screen.findByRole("group", { name: "Create VPC (simulated)" })
    expect(screen.getByText("rev 3")).toBeInTheDocument()
    expect(
      screen.queryByRole("button", { name: "Validate" }),
    ).not.toBeInTheDocument()
    expect(
      screen.queryByRole("button", { name: "Publish" }),
    ).not.toBeInTheDocument()
  },
)
```

- [ ] **Step 6: Run the tests and verify RED**

Run:

```text
pnpm web:test
```

Expected: `Test Files  3 failed | 9 passed (12)` and `Tests  8 failed | 52 passed (60)`:
- `diagnostics.test.ts` fails to load with `Error: Failed to resolve import "@/studio/diagnostics" from
  "src/studio/diagnostics.test.ts". Does the file exist?`.
- `use-draft-autosave.test.ts` fails the same way for `"@/studio/use-draft-autosave"`.
- `raw-colors.test.ts` passes.
- In `studio.test.tsx`, the first test fails with `TestingLibraryElementError: Unable to find an accessible
  element with the role "navigation" and name "Breadcrumb"`.
- `saves the draft once editing pauses` fails with `Unable to find an element with the text: Saving…`.
- The conflict test fails with `Unable to find role="alert"`.
- The Validate test fails with `Unable to find an accessible element with the role "button" and name
  "Validate"`, and the invalid-publish test with the same error for `"Publish"`.
- The published-version test fails with `Unable to find an element with the text: v1 published`.
- Both runner and viewer rows fail with `Unable to find an element with the text: rev 3`.

- [ ] **Step 7: Add the published version and the publish result to the API types**

`Workflow` gains `publishedVersion`. `VersionSummary` lists the only field the studio reads from a publish.

Replace the contents of `apps/web/src/api/types.ts` with:

```ts
export type Role = "admin" | "editor" | "runner" | "viewer"

export type Capability =
  | "view_organization"
  | "edit_workflows"
  | "publish_workflows"
  | "run_workflows"
  | "manage_members"
  | "manage_service_tokens"
  | "manage_secrets"
  | "manage_plugins"
  | "enable_shell_executor"

export interface OrganizationMembership {
  organizationId: string
  name: string
  slug: string
  role: Role
  capabilities: Capability[]
}

export interface Me {
  user: { id: string; email: string }
  organizations: OrganizationMembership[]
}

export interface Diagnostic {
  code: string
  pointer: string
  message: string
}

export interface Member {
  userId: string
  email: string
  role: Role
}

export interface Invitation {
  token: string
  inviteUrl?: string
  expiresAt: string
}

export interface AuditEvent {
  id: number
  actorUserId: string | null
  action: string
  resourceType: string
  resourceId: string
  metadata: Record<string, unknown>
  occurredAt: string
}

// The catalog and workflow types list only the fields the UI reads; the API
// also sends scope, slugs and timestamps.
export type JsonSchema = {
  [keyword: string]: unknown
  properties?: Record<string, JsonSchema>
  required?: string[]
}

export interface PortSpec {
  name: string
  dataType: string
  schema: JsonSchema
  required: boolean
  description: string
}

export interface UiField {
  widget: string
  label?: string | null
  help?: string | null
}

export interface NodeDefinition {
  id: string
  key: string
  version: number
  displayName: string
  description: string
  category: string
  deprecated: boolean
  inputs: PortSpec[]
  outputs: PortSpec[]
  configSchema: JsonSchema
  uiSchema: Record<string, UiField>
  defaults: Record<string, unknown>
  executor: { key: string; version: string }
}

export type InputBinding =
  | { kind: "literal"; value: unknown }
  | { kind: "workflowInput"; pointer: string }
  | { kind: "secret"; secretId: string }

export interface GraphNode {
  id: string
  definition: { key: string; version: number }
  displayName: string
  position: { x: number; y: number }
  config: Record<string, unknown>
  inputs: Record<string, InputBinding>
  retry: { maxAttempts: number }
  timeoutSeconds: number
}

export interface GraphEdge {
  id: string
  source: { nodeId: string; port: string }
  target: { nodeId: string; port: string }
}

export interface GraphSnapshot {
  schemaVersion: number
  workflowInputSchema: JsonSchema
  nodes: GraphNode[]
  edges: GraphEdge[]
  viewport: { x: number; y: number; zoom: number }
}

export interface Blueprint {
  id: string
  key: string
  displayName: string
  description: string
  graph: GraphSnapshot
}

export interface WorkflowSummary {
  id: string
  name: string
  draftRevision: number
  publishedVersion: number | null
  updatedAt: string
}

export interface Workflow {
  id: string
  name: string
  publishedVersion: number | null
  draft: { revision: number; graph: GraphSnapshot }
}

export interface VersionSummary {
  version: number
}
```

- [ ] **Step 8: Add the validate and publish mutations**

Validation writes nothing, so it is a plain mutation. Publishing writes a version and an audit event, so it
refreshes the organization's queries through `useOrganizationMutation`, like the other writes.

In `apps/web/src/api/queries.ts`, apply these replacements in order:

Replace:

```ts
import type {
  AuditEvent,
  Blueprint,
  Invitation,
  Member,
  NodeDefinition,
  Role,
  Workflow,
  WorkflowSummary,
} from "@/api/types"
```

With:

```ts
import type {
  AuditEvent,
  Blueprint,
  Diagnostic,
  GraphSnapshot,
  Invitation,
  Member,
  NodeDefinition,
  Role,
  VersionSummary,
  Workflow,
  WorkflowSummary,
} from "@/api/types"
```

Replace:

```ts
export function useCreateWorkflow(organizationId: string) {
  return useOrganizationMutation(
    organizationId,
    (body: { name: string; blueprintId?: string }) =>
      api<Workflow>(`/orgs/${organizationId}/workflows`, {
        method: "POST",
        body: JSON.stringify(body),
      }),
  )
}
```

With:

```ts
export function useCreateWorkflow(organizationId: string) {
  return useOrganizationMutation(
    organizationId,
    (body: { name: string; blueprintId?: string }) =>
      api<Workflow>(`/orgs/${organizationId}/workflows`, {
        method: "POST",
        body: JSON.stringify(body),
      }),
  )
}

// Validation writes nothing, so nothing needs a refresh.
export function useValidateWorkflow(
  organizationId: string,
  workflowId: string,
) {
  return useMutation({
    mutationFn: (graph: GraphSnapshot) =>
      api<{ diagnostics: Diagnostic[] }>(
        `/orgs/${organizationId}/workflows/${workflowId}/validate`,
        { method: "POST", body: JSON.stringify({ graph }) },
      ),
  })
}

export function usePublishWorkflow(organizationId: string, workflowId: string) {
  return useOrganizationMutation(organizationId, (revision: number) =>
    api<VersionSummary>(
      `/orgs/${organizationId}/workflows/${workflowId}/publish`,
      { method: "POST", body: JSON.stringify({ revision }) },
    ),
  )
}
```

- [ ] **Step 9: Map diagnostics onto the graph that was sent**

Create `apps/web/src/studio/diagnostics.ts`:

```ts
import type { Diagnostic, GraphSnapshot } from "@/api/types"

// A diagnostic placed on the canvas. `port` is a handle id: `in:<port>` or
// `out:<port>`.
export type MappedDiagnostic = Diagnostic & {
  nodeId?: string
  port?: string
  edgeId?: string
}

// RFC 6901 escapes `~` as `~0` and `/` as `~1`.
function unescape(token: string) {
  return token.replaceAll("~1", "/").replaceAll("~0", "~")
}

// Pointers index into the graph that was sent, so they resolve against it:
// the canvas may have changed since. Problems with the graph as a whole stay
// unplaced.
export function mapDiagnostics(
  graph: GraphSnapshot,
  diagnostics: Diagnostic[],
): MappedDiagnostic[] {
  return diagnostics.map((diagnostic) => {
    const [, list, index, field, name] = diagnostic.pointer.split("/")
    if (list === "nodes") {
      const node = graph.nodes[Number(index)]
      if (!node) return diagnostic
      return field === "inputs" && name !== undefined
        ? { ...diagnostic, nodeId: node.id, port: `in:${unescape(name)}` }
        : { ...diagnostic, nodeId: node.id }
    }
    if (list === "edges") {
      const edge = graph.edges[Number(index)]
      if (!edge) return diagnostic
      if (field === "id") return { ...diagnostic, edgeId: edge.id }
      // An edge problem shows on the end it names, else on the input it feeds.
      const [end, port] =
        field === "source"
          ? [edge.source, `out:${edge.source.port}`]
          : [edge.target, `in:${edge.target.port}`]
      return { ...diagnostic, edgeId: edge.id, nodeId: end.nodeId, port }
    }
    return diagnostic
  })
}
```

- [ ] **Step 10: Implement the autosave hook**

The save logic is a plain closure created once per editor. It keeps the server's revision and graph, the
latest edit, the timer, the queue and the halt reason, and reports status changes through React state.

Create `apps/web/src/studio/use-draft-autosave.ts`:

```ts
import { useEffect, useState } from "react"

import { ApiProblem, api } from "@/api/client"
import type { GraphSnapshot } from "@/api/types"

export const AUTOSAVE_DELAY_MS = 800

export type SaveStatus = "saved" | "saving" | "conflict" | "error"

export type SavedDraft = { revision: number; graph: GraphSnapshot }

type Report = { status: SaveStatus; revision: number; errorCode: string }

function createAutosave(
  path: string,
  draft: SavedDraft,
  report: (state: Report) => void,
) {
  let { revision } = draft
  // What the server holds at `revision`, and the latest edit.
  let saved = { graph: draft.graph, json: JSON.stringify(draft.graph) }
  let latest: typeof saved | undefined
  let timer: ReturnType<typeof setTimeout> | undefined
  let queue = Promise.resolve()
  let queued = 0
  let halted: "conflict" | "error" | undefined
  let errorCode = ""

  function update() {
    const busy = timer !== undefined || queued > 0
    report({
      status: halted ?? (busy ? "saving" : "saved"),
      revision,
      errorCode,
    })
  }

  async function save() {
    const sending = latest
    try {
      if (halted || !sending || sending.json === saved.json) return
      const result = await api<{ revision: number }>(path, {
        method: "PUT",
        body: JSON.stringify({ revision, graph: sending.graph }),
      })
      revision = result.revision
      saved = sending
    } catch (error) {
      clearTimeout(timer)
      timer = undefined
      const problem = error instanceof ApiProblem ? error : undefined
      halted = problem?.status === 409 ? "conflict" : "error"
      errorCode = problem?.code ?? "network_error"
    } finally {
      queued -= 1
      update()
    }
  }

  // Each save waits for the one before it, so one request is in flight.
  function enqueue() {
    queued += 1
    queue = queue.then(save)
    update()
  }

  return {
    edit(graph: GraphSnapshot, readOnly: boolean) {
      const json = JSON.stringify(graph)
      // The canvas reports its graph after every render. The first report is
      // the loaded draft as the canvas holds it; later ones may change nothing.
      if (!latest) {
        latest = { graph, json }
        saved = latest
        return
      }
      if (json === latest.json) return
      latest = { graph, json }
      if (readOnly || halted === "conflict") return
      halted = undefined
      const idle = timer === undefined
      clearTimeout(timer)
      timer = setTimeout(() => {
        timer = undefined
        enqueue()
      }, AUTOSAVE_DELAY_MS)
      // Later edits in a burst leave the status as it is.
      if (idle) update()
    },

    // Saves pending edits now and waits for every save. Resolves to the saved
    // draft, or null when it could not be saved.
    async flush(): Promise<SavedDraft | null> {
      if (timer !== undefined) {
        clearTimeout(timer)
        timer = undefined
        enqueue()
      }
      await queue
      return halted ? null : { revision, graph: saved.graph }
    },

    conflict() {
      clearTimeout(timer)
      timer = undefined
      halted = "conflict"
      update()
    },
  }
}

// Saves the draft 800 ms after the last edit. A 409 stops saving until the
// studio reloads the draft; any other failure stops it until the next edit.
export function useDraftAutosave(
  organizationId: string,
  workflowId: string,
  draft: SavedDraft,
  readOnly: boolean,
) {
  const [state, setState] = useState<Report>({
    status: "saved",
    revision: draft.revision,
    errorCode: "",
  })
  const [autosave] = useState(() =>
    createAutosave(
      `/orgs/${organizationId}/workflows/${workflowId}/draft`,
      draft,
      setState,
    ),
  )
  // Leaving the studio saves pending edits at once instead of dropping them.
  useEffect(() => () => void autosave.flush(), [autosave])
  return {
    ...state,
    edit: (graph: GraphSnapshot) => autosave.edit(graph, readOnly),
    flush: autosave.flush,
    conflict: autosave.conflict,
  }
}
```

- [ ] **Step 11: Mark nodes and ports that have problems**

Replace the contents of `apps/web/src/studio/studio-node.tsx` with:

```tsx
import { Handle, Position, type NodeProps } from "@xyflow/react"
import { createContext, useContext } from "react"

import type { PortSpec } from "@/api/types"
import { StatusBadge } from "@/components/status/status-badge"
import type { MappedDiagnostic } from "@/studio/diagnostics"
import { isSimulated, type StudioNode } from "@/studio/graph-model"

// The problems found by the last Validate or Publish.
export const DiagnosticsContext = createContext<MappedDiagnostic[]>([])

function Port({
  port,
  side,
  isConnectable,
  invalid,
}: {
  port: PortSpec
  side: "in" | "out"
  isConnectable: boolean
  invalid: boolean
}) {
  return (
    <li data-invalid={invalid || undefined} className="relative px-3 py-1">
      <Handle
        id={`${side}:${port.name}`}
        type={side === "in" ? "target" : "source"}
        position={side === "in" ? Position.Left : Position.Right}
        isConnectable={isConnectable}
      />
      {port.name}
      {invalid && (
        <span
          role="img"
          aria-label="Has a problem"
          className="ml-1 inline-block size-2 rounded-full bg-destructive"
        />
      )}
      <span className="block text-muted-foreground">{port.dataType}</span>
    </li>
  )
}

export function StudioNodeView({
  id,
  data,
  isConnectable,
}: NodeProps<StudioNode>) {
  const { node, definition } = data
  const problems = useContext(DiagnosticsContext).filter(
    (problem) => problem.nodeId === id,
  )
  const invalid = (handle: string) =>
    problems.some((problem) => problem.port === handle)
  return (
    <div
      data-invalid={problems.length > 0 || undefined}
      className="min-w-52 rounded-lg border bg-card text-sm text-card-foreground shadow-sm data-invalid:border-destructive data-invalid:ring-2 data-invalid:ring-destructive/40"
    >
      <div className="flex items-center justify-between gap-2 border-b px-3 py-2">
        <span className="font-medium">{node.displayName}</span>
        {isSimulated(node.definition.key) && <StatusBadge status="simulated" />}
      </div>
      {definition ? (
        <div className="grid grid-cols-2 gap-4 py-2 text-xs">
          <ul>
            {definition.inputs.map((port) => (
              <Port
                key={port.name}
                port={port}
                side="in"
                isConnectable={isConnectable}
                invalid={invalid(`in:${port.name}`)}
              />
            ))}
          </ul>
          <ul className="text-right">
            {definition.outputs.map((port) => (
              <Port
                key={port.name}
                port={port}
                side="out"
                isConnectable={isConnectable}
                invalid={invalid(`out:${port.name}`)}
              />
            ))}
          </ul>
        </div>
      ) : (
        <p className="px-3 py-2 text-xs text-destructive">
          Unknown definition {node.definition.key} v{node.definition.version}
        </p>
      )}
    </div>
  )
}
```

- [ ] **Step 12: Report canvas changes and track the viewport**

The canvas reports `fromFlow(snapshot, nodes, edges)` after every change and provides the problems to the
node views. `onMoveEnd` writes the viewport into the snapshot, so pans and zooms are saved like edits; they are
part of the draft and its checksum (D12).

In `apps/web/src/studio/studio-canvas.tsx`, apply these replacements in order:

Replace:

```tsx
import { useState, type DragEvent } from "react"
```

With:

```tsx
import { useEffect, useState, type DragEvent } from "react"
```

Replace:

```tsx
} from "@/api/types"
```

With:

```tsx
} from "@/api/types"
import type { MappedDiagnostic } from "@/studio/diagnostics"
```

Replace:

```tsx
import { StudioNodeView } from "@/studio/studio-node"
```

With:

```tsx
import { DiagnosticsContext, StudioNodeView } from "@/studio/studio-node"
```

Replace:

```tsx
type CanvasProps = {
  graph: GraphSnapshot
  definitions: NodeDefinition[]
  blueprints: Blueprint[]
  readOnly: boolean
}

function Canvas({ graph, definitions, blueprints, readOnly }: CanvasProps) {
  const [initial] = useState(() => toFlow(graph, definitions))
  // XYFlow holds the nodes and edges; this holds the rest of the snapshot.
  // ponytail: edits stay in local state until drafts are saved automatically.
  const [snapshot, setSnapshot] = useState(graph)
```

With:

```tsx
type CanvasProps = {
  graph: GraphSnapshot
  definitions: NodeDefinition[]
  blueprints: Blueprint[]
  readOnly: boolean
  problems: MappedDiagnostic[]
  onChange: (graph: GraphSnapshot) => void
}

function Canvas({
  graph,
  definitions,
  blueprints,
  readOnly,
  problems,
  onChange,
}: CanvasProps) {
  const [initial] = useState(() => toFlow(graph, definitions))
  // XYFlow holds the nodes and edges; this holds the rest of the snapshot.
  const [snapshot, setSnapshot] = useState(graph)
```

Replace:

```tsx
  const current = () => fromFlow(snapshot, nodes, edges)
```

With:

```tsx
  const current = () => fromFlow(snapshot, nodes, edges)
  // The studio saves what the canvas reports.
  useEffect(() => {
    onChange(fromFlow(snapshot, nodes, edges))
  }, [onChange, snapshot, nodes, edges])
```

Replace:

```tsx
          <ReactFlow
            nodes={nodes}
            edges={edges}
            nodeTypes={nodeTypes}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            onConnect={onConnect}
            onConnectEnd={onConnectEnd}
            isValidConnection={isValidConnection}
            onDragOver={readOnly ? undefined : onDragOver}
            onDrop={readOnly ? undefined : onDrop}
            nodesDraggable={!readOnly}
            nodesConnectable={!readOnly}
            deleteKeyCode={readOnly ? null : ["Backspace", "Delete"]}
            defaultViewport={graph.viewport}
            colorMode="system"
          >
            <Background />
            <Controls showInteractive={false} />
          </ReactFlow>
```

With:

```tsx
          <DiagnosticsContext value={problems}>
            <ReactFlow
              nodes={nodes}
              edges={edges}
              nodeTypes={nodeTypes}
              onNodesChange={onNodesChange}
              onEdgesChange={onEdgesChange}
              onConnect={onConnect}
              onConnectEnd={onConnectEnd}
              isValidConnection={isValidConnection}
              onDragOver={readOnly ? undefined : onDragOver}
              onDrop={readOnly ? undefined : onDrop}
              onMoveEnd={(_, viewport) =>
                setSnapshot((existing) => ({ ...existing, viewport }))
              }
              nodesDraggable={!readOnly}
              nodesConnectable={!readOnly}
              deleteKeyCode={readOnly ? null : ["Backspace", "Delete"]}
              defaultViewport={graph.viewport}
              colorMode="system"
            >
              <Background />
              <Controls showInteractive={false} />
            </ReactFlow>
          </DiagnosticsContext>
```

- [ ] **Step 13: Add the top bar, the conflict banner and the problems panel**

In `apps/web/src/studio/studio.tsx`, apply these replacements in order:

Replace:

```tsx
"use client"

import dynamic from "next/dynamic"
import { useSyncExternalStore } from "react"

import { useBlueprints, useNodeDefinitions, useWorkflow } from "@/api/queries"
import { useCan } from "@/auth/use-can"
import { StatusBadge } from "@/components/status/status-badge"
import { cn } from "@/lib/utils"
import { Palette } from "@/studio/palette"
```

With:

```tsx
"use client"

import { useQueryClient } from "@tanstack/react-query"
import dynamic from "next/dynamic"
import Link from "next/link"
import { useId, useState, useSyncExternalStore } from "react"

import { ApiProblem } from "@/api/client"
import {
  queryKeys,
  useBlueprints,
  useNodeDefinitions,
  usePublishWorkflow,
  useValidateWorkflow,
  useWorkflow,
} from "@/api/queries"
import type {
  Blueprint,
  Diagnostic,
  GraphSnapshot,
  NodeDefinition,
  Workflow,
} from "@/api/types"
import { useCan } from "@/auth/use-can"
import { StatusBadge } from "@/components/status/status-badge"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"
import { mapDiagnostics, type MappedDiagnostic } from "@/studio/diagnostics"
import { Palette } from "@/studio/palette"
import { useDraftAutosave, type SavedDraft } from "@/studio/use-draft-autosave"
```

Replace:

```tsx
export function Studio({
  organizationId,
  workflowId,
}: {
  organizationId: string
  workflowId: string
}) {
  const workflow = useWorkflow(organizationId, workflowId)
  const definitions = useNodeDefinitions(organizationId)
  const blueprints = useBlueprints(organizationId)
  const canEdit = useCan(organizationId, "edit_workflows")
  const wide = useWideScreen()
  const readOnlyReason = !canEdit
    ? "Your role can view this workflow but not edit it."
    : !wide
      ? "Editing needs a screen at least 1024 pixels wide."
      : null

  if (workflow.isError || definitions.isError || blueprints.isError) {
    return (
      <p role="alert" className="p-4 text-sm text-destructive md:p-6">
        The workflow could not be loaded.
      </p>
    )
  }
  if (!workflow.data || !definitions.data || !blueprints.data) {
    return (
      <p className="p-4 text-sm text-muted-foreground md:p-6">
        Loading workflow…
      </p>
    )
  }

  return (
    <section className="flex h-dvh flex-col">
      <header className="flex flex-wrap items-center gap-3 border-b px-4 py-3">
        <h1 className="text-lg font-semibold tracking-tight">
          {workflow.data.name}
        </h1>
        {readOnlyReason && (
          <>
            <StatusBadge status="read-only" />
            <span className="text-sm text-muted-foreground">
              {readOnlyReason}
            </span>
          </>
        )}
      </header>
      <div
        className={cn(
          "grid min-h-0 flex-1",
          !readOnlyReason && "grid-cols-[16rem_1fr]",
        )}
      >
        {!readOnlyReason && (
          <Palette
            definitions={definitions.data.items}
            blueprints={blueprints.data.items}
          />
        )}
        <StudioCanvas
          graph={workflow.data.draft.graph}
          definitions={definitions.data.items}
          blueprints={blueprints.data.items}
          readOnly={readOnlyReason !== null}
        />
      </div>
    </section>
  )
}
```

With:

```tsx
const noProblems: MappedDiagnostic[] = []

// Names a problem's place with the names in the graph that was checked.
function place(graph: GraphSnapshot, problem: MappedDiagnostic) {
  const node = graph.nodes.find(({ id }) => id === problem.nodeId)
  if (!node) return problem.pointer
  const port = problem.port?.split(":")[1]
  return port ? `${node.displayName} · ${port}` : node.displayName
}

function Editor({
  organizationId,
  workflow,
  definitions,
  blueprints,
  onReload,
}: {
  organizationId: string
  workflow: Workflow
  definitions: NodeDefinition[]
  blueprints: Blueprint[]
  onReload: () => void
}) {
  const canEdit = useCan(organizationId, "edit_workflows")
  const canPublish = useCan(organizationId, "publish_workflows")
  const wide = useWideScreen()
  const readOnlyReason = !canEdit
    ? "Your role can view this workflow but not edit it."
    : !wide
      ? "Editing needs a screen at least 1024 pixels wide."
      : null
  const autosave = useDraftAutosave(
    organizationId,
    workflow.id,
    workflow.draft,
    readOnlyReason !== null,
  )
  const validate = useValidateWorkflow(organizationId, workflow.id)
  const publish = usePublishWorkflow(organizationId, workflow.id)
  const queryClient = useQueryClient()
  const [problems, setProblems] = useState<{
    graph: GraphSnapshot
    items: MappedDiagnostic[]
  } | null>(null)
  const [notice, setNotice] = useState("")
  const [busy, setBusy] = useState(false)
  const problemsId = useId()

  function show(graph: GraphSnapshot, diagnostics: Diagnostic[]) {
    const count = diagnostics.length
    setProblems(
      count > 0 ? { graph, items: mapDiagnostics(graph, diagnostics) } : null,
    )
    setNotice(
      count === 0
        ? "No problems found."
        : `${count} problem${count === 1 ? "" : "s"} found.`,
    )
  }

  async function validateDraft({ graph }: SavedDraft) {
    try {
      show(graph, (await validate.mutateAsync(graph)).diagnostics)
    } catch (error) {
      const code = error instanceof ApiProblem ? error.code : "network_error"
      setNotice(`Validation failed (${code}).`)
    }
  }

  async function publishDraft({ revision, graph }: SavedDraft) {
    try {
      const { version } = await publish.mutateAsync(revision)
      setProblems(null)
      setNotice(`Published v${version}.`)
    } catch (error) {
      const problem =
        error instanceof ApiProblem ? error : new ApiProblem(0, "network_error")
      if (problem.code === "graph_invalid") {
        show(graph, problem.diagnostics)
      } else if (problem.code === "draft_revision_conflict") {
        autosave.conflict()
      } else if (problem.code === "draft_already_published") {
        // That version is the latest one: read it from the workflow.
        const queryKey = queryKeys.workflow(organizationId, workflow.id)
        await queryClient.refetchQueries({ queryKey })
        const latest = queryClient.getQueryData<Workflow>(queryKey)
        setNotice(`Already published as v${latest?.publishedVersion}.`)
      } else {
        setNotice(`Publishing failed (${problem.code}).`)
      }
    }
  }

  // Both act on the saved draft, so pending edits are saved first.
  async function run(task: (saved: SavedDraft) => Promise<void>) {
    setBusy(true)
    setNotice("")
    const saved = await autosave.flush()
    if (saved) await task(saved)
    setBusy(false)
  }

  const conflict = autosave.status === "conflict"
  return (
    <section className="flex h-dvh flex-col">
      <header className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b px-4 py-3 text-sm">
        <nav aria-label="Breadcrumb">
          <ol className="flex items-center gap-2">
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
            <li aria-current="page">
              <h1 className="text-lg font-semibold tracking-tight">
                {workflow.name}
              </h1>
            </li>
          </ol>
        </nav>
        {readOnlyReason ? (
          <>
            <StatusBadge status="read-only" />
            <span className="text-muted-foreground">{readOnlyReason}</span>
          </>
        ) : (
          <StatusBadge status={autosave.status} />
        )}
        {autosave.status === "error" && (
          <span className="text-destructive">
            The draft could not be saved ({autosave.errorCode}). The next edit
            tries again.
          </span>
        )}
        <span className="text-muted-foreground">rev {autosave.revision}</span>
        {workflow.publishedVersion !== null && (
          <span className="text-muted-foreground">
            v{workflow.publishedVersion} published
          </span>
        )}
        <span aria-live="polite">{notice}</span>
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
      </header>
      {conflict && (
        <div
          role="alert"
          className="flex flex-wrap items-center gap-3 border-b bg-muted px-4 py-2 text-sm"
        >
          This draft was saved somewhere else. Reload it to keep editing;
          changes made here since the last save are discarded.
          <Button variant="outline" size="sm" onClick={onReload}>
            Reload
          </Button>
        </div>
      )}
      {problems && (
        <section
          aria-labelledby={problemsId}
          className="max-h-40 overflow-y-auto border-b px-4 py-2 text-sm"
        >
          <h2 id={problemsId} className="font-medium">
            Problems
          </h2>
          <ul aria-labelledby={problemsId} className="mt-1 grid gap-1">
            {problems.items.map((problem, index) => (
              <li key={index}>
                <span className="font-medium">
                  {place(problems.graph, problem)}
                </span>{" "}
                {problem.message}{" "}
                <span className="font-mono text-xs text-muted-foreground">
                  {problem.code}
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}
      <div
        className={cn(
          "grid min-h-0 flex-1",
          !readOnlyReason && "grid-cols-[16rem_1fr]",
        )}
      >
        {!readOnlyReason && (
          <Palette definitions={definitions} blueprints={blueprints} />
        )}
        <StudioCanvas
          graph={workflow.draft.graph}
          definitions={definitions}
          blueprints={blueprints}
          readOnly={readOnlyReason !== null}
          problems={problems?.items ?? noProblems}
          onChange={autosave.edit}
        />
      </div>
    </section>
  )
}

export function Studio({
  organizationId,
  workflowId,
}: {
  organizationId: string
  workflowId: string
}) {
  const workflow = useWorkflow(organizationId, workflowId)
  const definitions = useNodeDefinitions(organizationId)
  const blueprints = useBlueprints(organizationId)
  // Reloading after a conflict starts the editor over from the latest draft.
  const [generation, setGeneration] = useState(0)

  // A failed background refetch keeps the loaded studio on screen.
  if (!workflow.data || !definitions.data || !blueprints.data) {
    return workflow.isError || definitions.isError || blueprints.isError ? (
      <p role="alert" className="p-4 text-sm text-destructive md:p-6">
        The workflow could not be loaded.
      </p>
    ) : (
      <p className="p-4 text-sm text-muted-foreground md:p-6">
        Loading workflow…
      </p>
    )
  }

  async function reload() {
    const result = await workflow.refetch()
    if (result.isSuccess) setGeneration((current) => current + 1)
  }

  return (
    <Editor
      key={generation}
      organizationId={organizationId}
      workflow={workflow.data}
      definitions={definitions.data.items}
      blueprints={blueprints.data.items}
      onReload={() => void reload()}
    />
  )
}
```

- [ ] **Step 14: Verify GREEN**

Run:

```text
pnpm web:test
```

Expected: `Test Files  12 passed (12)` and `Tests  79 passed (79)` with no unhandled errors (studio 15,
diagnostics 12, use-draft-autosave 7, raw-colors 1, and the 44 tests of the other eight files unchanged). On Node
25 or newer, each worker may print `ExperimentalWarning: localStorage is not available because
--localstorage-file was not provided.`; this is harmless.

- [ ] **Step 15: Run lint and build**

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

Expected: `✓ Compiled successfully`, `Finished TypeScript`, and the same route table as Task 17, with
`ƒ /orgs/[organizationId]/workflows/[workflowId]` still dynamic. It exits 0.

- [ ] **Step 16: Commit**

```text
git add apps/web
git commit -m "feat: save drafts automatically and validate and publish from the studio"
```

## Task 19: Extend Compose, smoke tests, CI, and docs for Milestone 2

**Files:**

- Modify: `scripts/smoke-foundation.ps1`
- Modify: `README.md`
- Modify: `docs/superpowers/specs/2026-07-28-corqty-workflow-orchestrator-design.md`

This task closes Milestone 2. It runs after Task 18, on a tree that holds both the backend work (Tasks 1-5 and
8-14) and the studio (Tasks 6, 7 and 15-18). The Compose smoke test grows to cover the system catalog and the
draft and publish flow end to end, and the README and the master spec describe what Milestone 2 built.

Compose and CI need no change, because earlier tasks already gave them everything Milestone 2 uses:

- The `migrate` service runs every migration, and Task 11's migration ends with
  `SELECT corqty.seed_system_catalog();`. A fresh stack therefore starts with the 6 system node definitions and
  the 2 system blueprints. `docker/rust/Dockerfile` copies `migrations/`, so the migrator embeds the new file.
- Task 7 gave the `web` service `next dev` on port 3000 with a healthcheck, so `up --wait` returns only after
  Next.js has served a page.
- The CI job already runs, in order: the database role script from Task 4, `cargo fmt --check`, Clippy,
  `cargo test --workspace`, `pnpm install --frozen-lockfile`, `pnpm web:lint`, `pnpm web:test`, `pnpm web:build`,
  a fresh Compose stack, and this smoke script under `pwsh`. The web tests read the shared fixture
  `crates/domain/tests/fixtures/graph-rules.json`, which the checkout already contains.

- [ ] **Step 1: Extend the Compose smoke test**

The new checks run as the invited editor, whose role holds `edit_workflows` and `publish_workflows`:

1. A fresh organization sees exactly 6 node definitions and 2 blueprints with scope `system`.
2. Applying each blueprint with `POST /workflows {name, blueprintId}` returns 201 with draft revision 1 and as
   many nodes as the blueprint has.
3. For the simulated infrastructure workflow, saving the returned graph at revision 1 returns revision 2.
   Publishing revision 2 returns 201 with version 1 and a 64-character lowercase hex checksum. Publishing revision
   2 again returns 409 `draft_already_published`, and publishing revision 1 returns 409
   `draft_revision_conflict`, because Task 14 checks the revision before it looks for an existing version.
4. Publishing the data-processing workflow at revision 1 returns 201 with version 1.
5. The outsider is the admin of its own organization. It gets 404 when it asks for the infrastructure workflow
   under that organization, because row-level security hides the row.
6. The studio route `/orgs/{organizationId}/workflows/{workflowId}` on the web service returns the Corqty shell.
   The first request makes `next dev` compile the route, so this check catches a studio page that cannot render
   on the server.
7. The audit log also holds `workflow.created`, `workflow.draft_saved` and `workflow.published`.

`Invoke-WebRequest` throws on every 4xx response in both Windows PowerShell 5.1 and PowerShell 7. The helper
`Invoke-SmokeRequest` therefore catches the error, reads the status from `$_.Exception.Response` and parses the
problem body from `$_.ErrorDetails.Message`. A request without a response, such as a refused connection, is
rethrown. The helper sends the CSRF header and a JSON body only when it has a body. `ConvertTo-Json` gets
`-Depth 32` because its default depth of 2 would flatten the graph. `-cnotmatch` keeps the checksum check
case-sensitive. The existing web check (`<title>Corqty</title>` at port 3000) and the service-state check stay as
they are.

In `scripts/smoke-foundation.ps1`, apply these replacements in order:

Replace:

```powershell
    [pscustomobject]@{
        Session = $webSession
        Csrf = $csrf
    }
}

$suffix = [guid]::NewGuid().ToString("N")
```

With:

```powershell
    [pscustomobject]@{
        Session = $webSession
        Csrf = $csrf
    }
}

# Returns the status and the parsed JSON body, for problems as well as successes.
function Invoke-SmokeRequest {
    param(
        [pscustomobject]$User,
        [string]$Method,
        [string]$Path,
        [hashtable]$Body
    )

    $request = @{
        UseBasicParsing = $true
        WebSession = $User.Session
        Method = $Method
        Uri = "$ApiBase$Path"
    }
    if ($Body) {
        $request.ContentType = "application/json"
        $request.Headers = @{ "x-csrf-token" = $User.Csrf }
        $request.Body = ConvertTo-Json -InputObject $Body -Depth 32 -Compress
    }
    try {
        $response = Invoke-WebRequest @request
        $status = [int]$response.StatusCode
        $content = $response.Content
    } catch {
        if ($null -eq $_.Exception.Response) {
            throw
        }
        $status = [int]$_.Exception.Response.StatusCode
        $content = $_.ErrorDetails.Message
    }
    [pscustomobject]@{
        Status = $status
        Body = if ($content) { $content | ConvertFrom-Json } else { $null }
    }
}

$suffix = [guid]::NewGuid().ToString("N")
```

Replace:

```powershell
if ($hiddenStatus -ne 404) {
    throw "Cross-tenant member request returned $hiddenStatus, not 404"
}

$audit = Invoke-RestMethod `
```

With:

```powershell
if ($hiddenStatus -ne 404) {
    throw "Cross-tenant member request returned $hiddenStatus, not 404"
}

$organizationPath = "/orgs/$organizationId"
$definitions = Invoke-SmokeRequest `
    -User $editor `
    -Method Get `
    -Path "$organizationPath/node-definitions"
$blueprints = Invoke-SmokeRequest `
    -User $editor `
    -Method Get `
    -Path "$organizationPath/blueprints"
if (
    @($definitions.Body.items | Where-Object { $_.scope -eq "system" }).Count -ne 6 -or
    @($blueprints.Body.items | Where-Object { $_.scope -eq "system" }).Count -ne 2
) {
    throw "System catalog does not list 6 node definitions and 2 blueprints"
}

$workflows = @{}
foreach ($blueprint in $blueprints.Body.items) {
    $created = Invoke-SmokeRequest `
        -User $editor `
        -Method Post `
        -Path "$organizationPath/workflows" `
        -Body @{
            name = $blueprint.displayName
            blueprintId = $blueprint.id
        }
    if (
        $created.Status -ne 201 -or
        $created.Body.draft.revision -ne 1 -or
        @($created.Body.draft.graph.nodes).Count -ne @($blueprint.graph.nodes).Count
    ) {
        throw "Applying blueprint $($blueprint.key) returned $($created.Status)"
    }
    $workflows[$blueprint.key] = $created.Body
}

$infrastructure = $workflows["corqty.simulated-infrastructure"]
$infrastructurePath = "$organizationPath/workflows/$($infrastructure.id)"
$saved = Invoke-SmokeRequest `
    -User $editor `
    -Method Put `
    -Path "$infrastructurePath/draft" `
    -Body @{
        revision = 1
        graph = $infrastructure.draft.graph
    }
if ($saved.Status -ne 200 -or $saved.Body.revision -ne 2) {
    throw "Draft save returned $($saved.Status), not revision 2"
}
$published = Invoke-SmokeRequest `
    -User $editor `
    -Method Post `
    -Path "$infrastructurePath/publish" `
    -Body @{ revision = 2 }
if (
    $published.Status -ne 201 -or
    $published.Body.version -ne 1 -or
    $published.Body.checksum -cnotmatch "^[0-9a-f]{64}$"
) {
    throw "Publishing draft revision 2 did not create checksummed version 1"
}
foreach ($refusal in @(
    @{ revision = 2; code = "draft_already_published" },
    @{ revision = 1; code = "draft_revision_conflict" }
)) {
    $refused = Invoke-SmokeRequest `
        -User $editor `
        -Method Post `
        -Path "$infrastructurePath/publish" `
        -Body @{ revision = $refusal.revision }
    if ($refused.Status -ne 409 -or $refused.Body.code -ne $refusal.code) {
        throw "Publishing revision $($refusal.revision) did not return 409 $($refusal.code)"
    }
}

$dataProcessing = $workflows["corqty.data-processing"]
$published = Invoke-SmokeRequest `
    -User $editor `
    -Method Post `
    -Path "$organizationPath/workflows/$($dataProcessing.id)/publish" `
    -Body @{ revision = 1 }
if ($published.Status -ne 201 -or $published.Body.version -ne 1) {
    throw "Publishing the data-processing workflow returned $($published.Status)"
}

$outsiderMe = Invoke-RestMethod `
    -WebSession $outsider.Session `
    "$ApiBase/auth/me"
$outsiderOrganizationId = $outsiderMe.organizations[0].organizationId
$foreign = Invoke-SmokeRequest `
    -User $outsider `
    -Method Get `
    -Path "/orgs/$outsiderOrganizationId/workflows/$($infrastructure.id)"
if ($foreign.Status -ne 404) {
    throw "Cross-tenant workflow request returned $($foreign.Status), not 404"
}

$studio = Invoke-WebRequest `
    -UseBasicParsing `
    "$WebBase$infrastructurePath"
if ($studio.StatusCode -ne 200 -or $studio.Content -notmatch "<title>Corqty</title>") {
    throw "Web application did not serve the workflow studio"
}

$audit = Invoke-RestMethod `
```

Replace:

```powershell
    "invitation.created",
    "invitation.accepted"
)) {
```

With:

```powershell
    "invitation.created",
    "invitation.accepted",
    "workflow.created",
    "workflow.draft_saved",
    "workflow.published"
)) {
```

- [ ] **Step 2: Run the smoke test against a fresh stack**

Run:

```text
docker compose --project-name corqty-m2 up --build --wait
```

Expected: the command exits 0 after `Container corqty-m2-migrate-1 Exited`, `Container corqty-m2-api-1 Healthy`
and `Container corqty-m2-web-1 Healthy`.

Run:

```text
./scripts/smoke-foundation.ps1 -ProjectName corqty-m2
```

Expected: the script prints nothing and exits 0. In Windows PowerShell 5.1 the same check runs as
`powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-foundation.ps1 -ProjectName corqty-m2`.

There is no RED here. Tasks 11-18 already built everything the new checks exercise, so they pass on the first run.
To see that they can fail, run edited copies of the script against the same stack:

- With `code = "draft_mutated"` in place of `code = "draft_already_published"`, the copy stops with
  `Publishing revision 2 did not return 409 draft_mutated`.
- With `-ne 7` in place of `-ne 6` in the catalog check, it stops with
  `System catalog does not list 6 node definitions and 2 blueprints`.
- When the editor requests the workflow under the organization that owns it, instead of the outsider under its
  own organization, the copy stops with `Cross-tenant workflow request returned 200, not 404`.

Leave the stack running; Step 5 removes it.

- [ ] **Step 3: Describe Milestone 2 in the README**

Only the opening description changes. The `Local start` and `Test` sections already match the merged tree.

In `README.md`:

Replace:

```markdown
Generic visual workflow orchestration. Milestone 1 provides the multi-tenant
account, RBAC, audit, API, React shell, PostgreSQL, and Docker foundations.
```

With:

```markdown
Generic visual workflow orchestration. Milestone 1 provides the multi-tenant
account, RBAC, audit, API, React shell, PostgreSQL, and Docker foundations.
Milestone 2 adds the workflow catalog and studio: versioned node definitions
and blueprints, a Next.js and XYFlow editor with typed connections and
automatic draft saves, and validated, immutable published versions.
```

- [ ] **Step 4: Bring the master spec up to date**

- The status line names Milestone 2.
- The architecture diagram's browser box, the `apps/web` boundary and the Compose `web` service name Next.js. The
  `web` line no longer promises SSE proxying: Milestone 3 has to check whether server-sent events stream through
  the Next.js rewrite without buffering.
- Section 6.1 no longer says "SPA". The invitation token still lives only in the URL fragment, which browsers
  never send to the server, and the accept page reads it in the browser.
- Section 19 gains a "Milestone 2 decisions" note. It records the Next.js switch and the settled points on
  definitions and blueprints, audit, executors, secrets, checksums, draft saves and publishing, and blueprint
  application. It also says that the graph and timeout limits are fixed constants until Milestone 5.
- Section 21 lists the Next.js documentation.

In `docs/superpowers/specs/2026-07-28-corqty-workflow-orchestrator-design.md`, apply these replacements in order:

Replace:

```markdown
**Status:** Approved; Milestone 1 foundation implemented
```

With:

```markdown
**Status:** Approved; Milestone 1 foundation and Milestone 2 catalog and studio implemented
```

Replace:

```text
    Browser["React + XYFlow + shadcn/ui"]
```

With:

```text
    Browser["Next.js + React + XYFlow + shadcn/ui"]
```

Replace:

```markdown
- `apps/web`: React application, routes, XYFlow editor, schema widgets, API client, and tests.
```

With:

```markdown
- `apps/web`: Next.js App Router application with React client components, routes, XYFlow editor, schema widgets, API client, and tests.
```

Replace:

```markdown
- Invitation plaintext appears only in the SPA URL fragment and is submitted in an authenticated `POST` body; it is never placed in an HTTP path or query string.
```

With:

```markdown
- Invitation plaintext appears only in the web application's URL fragment, which the browser never sends to the server; the accept page reads it client-side and submits it in an authenticated `POST` body. It is never placed in an HTTP path or query string.
```

Replace:

```markdown
- `web`: Vite development server with API/SSE proxying
```

With:

```markdown
- `web`: Next.js development server whose rewrites proxy `/api/v1` to the API
```

Replace:

```markdown
Each milestone receives its own implementation plan and must leave the repository runnable. Later milestone plans may refine file-level details without changing the approved behavior in this specification.
```

With:

```markdown
Each milestone receives its own implementation plan and must leave the repository runnable. Later milestone plans may refine file-level details without changing the approved behavior in this specification.

### Milestone 2 decisions

- The web application uses the Next.js App Router instead of Vite and React Router. Every API call still runs in the browser against the same-origin `/api/v1`.
- Graphs reference node definitions by key and integer version. System keys start with `corqty.` and organization keys must not; versions count from 1 per organization and key. Definitions and blueprints are immutable and have no delete endpoint. A deprecated definition is hidden from the palette but stays valid in drafts and publishes.
- The audit events `workflow.created`, `workflow.draft_saved`, `workflow.published`, `node_definition.created`, `node_definition.deprecated`, and `blueprint.created` are written in the same transaction as the change and carry only identifiers, keys, versions, revisions, and checksums. Failed operations write nothing.
- The only built-in executor is `mock` 1.0.0, from a static compiled registry; any other executor makes a definition or graph invalid. The shell preset is deferred to Milestone 3.
- Drafts may hold secret references, but publishing reports every reference as not found until secrets arrive in Milestone 4. A plaintext value in a secret-reference field is always rejected.
- The server computes the version checksum as SHA-256 over the canonical graph JSON with sorted object keys. The viewport is part of the snapshot and therefore of the checksum.
- Draft saves accept any graph that parses and stays within the size limits; full validation runs only on Validate and Publish. Publishing names an exact draft revision, and each revision is published at most once.
- Applying a blueprint is `POST /orgs/{org_id}/workflows` with a `blueprintId`; the server copies the graph with fresh node and edge identifiers.
- Graph and timeout limits are fixed constants in Milestone 2; deployment configuration is deferred to Milestone 5.
```

Replace:

```markdown
- XYFlow custom nodes, handles, and connection validation: `@xyflow/react`
```

With:

```markdown
- XYFlow custom nodes, handles, and connection validation: `@xyflow/react`
- Next.js App Router route groups, client components, and rewrites to a same-origin API: `next`
```

- [ ] **Step 5: Run the complete verification**

Run:

```text
docker compose --project-name corqty-m2 down --volumes
```

Expected: `Volume corqty-m2_postgres-data Removed` and `Network corqty-m2_default Removed`.

Run:

```text
docker compose -f docker-compose.test.yml up -d --wait
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: `postgres-test` reports `Healthy`. Formatting and Clippy exit 0 with no warnings. All 70 Rust tests
pass:

- corqty-api: unit 4, audit_http 1, auth_http 3, authorization_matrix 4, catalog_http 3, members_http 3,
  organizations_http 1, problems_http 2, publish_http 2, workflows_http 4.
- corqty-domain: account_and_rbac 6, catalog_rules 5, graph_rules 8, publish_rules 9.
- corqty-persistence: auth_store 2, catalog_guards 3, final_admin 2, migrations 1, tenant_isolation 1,
  test_reset 1, workflow_catalog 5.

Run:

```text
pnpm install --frozen-lockfile
pnpm web:lint
pnpm web:test
pnpm web:build
```

Expected: `Lockfile is up to date`. `pnpm web:lint` prints `✓ Types generated successfully` and exits 0.
`pnpm web:test` reports `Test Files  12 passed (12)` and `Tests  79 passed (79)`. `pnpm web:build` prints
`✓ Compiled successfully` and a route table with `○ /`, `○ /_not-found`, `○ /invitations/accept`, `○ /login`,
`ƒ /orgs/[organizationId]`, `ƒ /orgs/[organizationId]/audit`, `ƒ /orgs/[organizationId]/catalog`,
`ƒ /orgs/[organizationId]/members`, `ƒ /orgs/[organizationId]/workflows`,
`ƒ /orgs/[organizationId]/workflows/[workflowId]` and `○ /register`.

Run:

```text
docker compose config --quiet
docker compose --project-name corqty-m2 up --build --wait
./scripts/smoke-foundation.ps1 -ProjectName corqty-m2
docker compose --project-name corqty-m2 run --rm migrate
./scripts/smoke-foundation.ps1 -ProjectName corqty-m2
docker compose --project-name corqty-m2 down --volumes
git diff --check
```

Expected:

- `docker compose config --quiet` prints nothing.
- The fresh stack starts with an empty volume and reports `Container corqty-m2-web-1 Healthy`.
- The smoke script prints nothing and exits 0.
- The migrator rerun may first print `Compiling` lines for a few crates. It ends with
  ``Running `target/debug/corqty-migrator` `` and exits 0 without applying anything, because all six migrations
  are already recorded.
- The second smoke run passes too, with a new set of users and organizations.
- The stack and its volume are removed.
- `git diff --check` prints nothing.

- [ ] **Step 6: Commit**

```text
git add scripts/smoke-foundation.ps1 README.md docs/superpowers/specs/2026-07-28-corqty-workflow-orchestrator-design.md
git commit -m "test: smoke-test the catalog and studio and document Milestone 2"
```

## Task 20: Bound validation cost and close API races

**Files:**

- Modify: `crates/domain/src/schema.rs`
- Modify: `crates/domain/src/diagnostic.rs`
- Modify: `crates/domain/src/catalog.rs`
- Modify: `crates/domain/src/publish.rs`
- Modify: `crates/api/src/error.rs`
- Modify: `crates/api/src/catalog.rs`
- Modify: `crates/api/src/workflows.rs`
- Modify: `crates/persistence/src/catalog.rs`
- Modify: `crates/persistence/src/workflows.rs`
- Modify: `docs/superpowers/specs/2026-07-28-corqty-workflow-orchestrator-design.md`
- Modify: `crates/domain/tests/catalog_rules.rs`
- Modify: `crates/domain/tests/publish_rules.rs`
- Modify: `crates/api/tests/catalog_http.rs`
- Modify: `crates/api/tests/workflows_http.rs`
- Modify: `crates/persistence/tests/workflow_catalog.rs`

An adversarial review of Tasks 1-19 confirmed two kinds of defect. Registration is open and makes the new user
the admin of their own organization, which grants `edit_workflows` and `publish_workflows`. Every route that
validates is therefore open to anyone, and validation cost had no bound:

- **Exponential schemas.** A config schema whose subschemas fan out through `$ref` (each level an `allOf` of two
  references to the next) doubles the validation time per level. At 40 levels, a 2.8 KB body keeps
  `validate_new_definition` busy for hours on a tokio worker, while the request also holds a pooled connection
  idle in its transaction. Even without references, a large schema times a large instance costs seconds.
- **Unbounded diagnostics.** A definition with 15,000 required input ports passes validation. A 500-node graph
  that uses it then produces 7,500,000 `missing_required_input` diagnostics, about 1 GB of JSON and a 9 GB peak
  working set.
- **Repeated compiles.** `validate_graph` compiled a definition's config schema once per node and a port schema
  once per literal binding, so a valid 500-node graph over one heavy definition took tens of seconds, partly
  while publish held the workflow row lock.

Four API paths also misbehaved under concurrency or odd input:

- `save_draft` updated `workflow_drafts` and then `workflows`, while publish locks `workflows` and then
  `workflow_drafts` (`FOR UPDATE` on the join). A save and a publish of one workflow could deadlock, and
  PostgreSQL then aborts one of them with 40P01, which reached the client as 500.
- PostgreSQL text and jsonb cannot store U+0000. A NUL in a name or in a draft, for example from a UTF-16 file
  imported through the `fileText` widget, failed with SQLSTATE 22P05 (jsonb) or 22021 (text), which became 500.
- Two concurrent `POST /blueprints` of one key both computed the same next version. The later insert failed on
  `blueprints_key_version` with 23505 and returned 500, while node definitions already map that race to 409.
- Deprecating an already deprecated definition wrote another `node_definition.deprecated` audit row although
  nothing changed.

The fixes:

1. `schema::compile` returns an error, which its callers already report as `invalid_schema`, when the schema
   serializes to more than `MAX_SCHEMA_BYTES = 16_384` bytes, when any object at any depth has a `$ref`,
   `$dynamicRef` or `$recursiveRef` key with a string value ("Schema references are not supported."), or when
   any object at any depth has a string `$schema` other than `DIALECT` ("Only JSON Schema Draft 2020-12 is
   supported."). The old check only looked at the root `$schema`, so a nested draft-07 resource with its own
   `$id` compiled with draft-07 semantics. Only string values count, so a property *named* `$ref` or `$schema`
   (its value is a schema object) stays allowed. Every user schema goes through `compile`: port schemas, config
   schemas and the graph's `workflowInputSchema`.
2. A definition has at most `MAX_PORTS = 50` ports per side. More gives one `too_many_ports` diagnostic at
   `/inputs` or `/outputs`, and that side's ports are not checked further.
3. One validation reports at most `MAX_DIAGNOSTICS = 200` diagnostics. `capped` cuts a longer list to the first
   200 and appends one `too_many_diagnostics` at pointer `""`. To keep memory bounded as well as the response,
   the loops stop collecting once the count passes 200 (`validate_spec`'s port and uiSchema loops,
   `validate_graph`'s edge, node and binding loops), and `schema_errors` and `instance_errors` take at most 200
   errors from their iterators. `validate_graph` sorts before it caps, so the marker always comes last.
4. `validate_graph` compiles each definition's config schema at most once per call, cached by `(key, version)`,
   and each port schema that a literal binding needs at most once, cached by `(key, version, port)`. The new
   `validate_compiled_config` takes the already compiled validator; `validate_config` keeps its signature and
   compiles for its caller.
5. The API runs `validate_new_definition` and every `validate_graph` call (create blueprint, validate, publish)
   on a blocking thread through `catalog::blocking`, a small wrapper around `tokio::task::spawn_blocking` that
   moves the owned input in and hands it back with the result. A `JoinError` becomes `ApiError::Internal`.
6. `save_draft` touches `workflows.updated_at` before it updates the draft row, so saves and publishes lock the
   two rows in the same order. When the revision precondition fails, the handler returns without committing,
   which rolls the touch back.
7. `From<sqlx::Error>` maps SQLSTATE 22P05 and 22021 to 422 `unsupported_character` (no diagnostics).
8. `map_version_conflict` also maps `blueprints_key_version` to 409 `blueprint_version_conflict`, and
   `create_blueprint` uses it.
9. `deprecate_node_definition` updates only a row whose `deprecated_at IS NULL` and audits only that update.
   Otherwise it returns the organization's row unchanged (an idempotent 200 with no audit row), or `None` (404)
   for a system, foreign or unknown id.

The spec's "Milestone 2 decisions" gain one bullet with the new limits.

- [ ] **Step 1: Write the failing tests**

The tests cover each fix that has observable behavior:

- `catalog_rules`: a `$ref` to local `$defs`, a `$dynamicRef`, a `$recursiveRef`, a nested draft-07 resource with
  `$id`, and a port schema of exactly 16,385 bytes are each `invalid_schema` at the schema's pointer. Properties
  named `$ref` and `$schema` and a schema of exactly 16,384 bytes have no diagnostics. 50 inputs pass, and 51
  inputs or 51 outputs give exactly `too_many_ports` at `/inputs` or `/outputs`. A definition with 400 unknown
  uiSchema fields gets 201 diagnostics, the last `too_many_diagnostics` at `""`. The existing compile test now
  expects a local `$ref` to fail with "Schema references are not supported." and a nested draft-07 resource to
  fail with "Only JSON Schema Draft 2020-12 is supported."
- `publish_rules`: 500 nodes of a definition with 50 required inputs, none bound, give exactly 201 diagnostics:
  200 `missing_required_input`, then `too_many_diagnostics` at `""`.
- `catalog_http`: deprecating the same definition twice returns the same 200 body both times, and the existing
  audit assertion still expects one `node_definition.deprecated` row. The race set-up from the node definition
  conflict test moves into the helper `post_during_insert`, and a new test runs it for blueprints: an owner
  transaction holds an uncommitted `acme.flow` v1, the request computes v1 too and waits on the unique key, the
  owner commits, and the request gets 409 `blueprint_version_conflict` with no audit row. A retry creates v2. The
  competing creator is an owner transaction rather than a second HTTP request because the test API pool has a
  single connection.
- `workflows_http`: saving a draft whose node display name contains U+0000 gives exactly the 422
  `unsupported_character` problem.
- `workflow_catalog`: an owner transaction locks the workflow row with `FOR UPDATE`, as publish does first. A
  draft save then waits on that row. The owner next takes the draft row with `FOR UPDATE NOWAIT`, which succeeds
  only if the waiting save has not locked the draft yet; with the old statement order it fails with 55P03. The
  owner commits and the save completes as revision 2.

In `crates/domain/tests/catalog_rules.rs`, apply these replacements in order:

Replace:

```rust
use corqty_domain::{
    catalog::{
        FILE_TEXT_MAX_BYTES, NewNodeDefinition, validate_blueprint_metadata, validate_config,
        validate_new_definition,
    },
    diagnostic::Diagnostic,
    schema::{DIALECT, compile, instance_errors, schema_errors},
};
```

With:

```rust
use corqty_domain::{
    catalog::{
        FILE_TEXT_MAX_BYTES, MAX_PORTS, NewNodeDefinition, validate_blueprint_metadata,
        validate_config, validate_new_definition,
    },
    diagnostic::{Diagnostic, MAX_DIAGNOSTICS},
    schema::{DIALECT, MAX_SCHEMA_BYTES, compile, instance_errors, schema_errors},
};
```

Replace:

```rust
fn text(length: usize) -> Value {
    json!("x".repeat(length))
}
```

With:

```rust
fn text(length: usize) -> Value {
    json!("x".repeat(length))
}

/// A string schema whose JSON has exactly `bytes` bytes.
fn schema_of(bytes: usize) -> Value {
    let padding = bytes
        - json!({"type": "string", "description": ""})
            .to_string()
            .len();
    json!({"type": "string", "description": "x".repeat(padding)})
}
```

Replace:

```rust
        ("/category", text(60)),
    ] {
```

With:

```rust
        ("/category", text(60)),
        // Properties may be named like keywords: their values are schemas, not strings.
        ("/configSchema/properties/$ref", json!({"type": "string"})),
        (
            "/configSchema/properties/$schema",
            json!({"type": "string"}),
        ),
        ("/outputs/0/schema", schema_of(MAX_SCHEMA_BYTES)),
    ] {
```

Replace:

```rust
        (
            "/outputs/0/schema/$schema",
            json!("http://json-schema.org/draft-07/schema#"),
            "invalid_schema",
            "/outputs/0/schema",
        ),
```

With:

```rust
        (
            "/outputs/0/schema/$schema",
            json!("http://json-schema.org/draft-07/schema#"),
            "invalid_schema",
            "/outputs/0/schema",
        ),
        (
            "/inputs/0/schema",
            json!({"$defs": {"region": {"type": "string"}}, "$ref": "#/$defs/region"}),
            "invalid_schema",
            "/inputs/0/schema",
        ),
        (
            "/outputs/0/schema/$dynamicRef",
            json!("#"),
            "invalid_schema",
            "/outputs/0/schema",
        ),
        (
            "/outputs/0/schema/$recursiveRef",
            json!("#"),
            "invalid_schema",
            "/outputs/0/schema",
        ),
        (
            "/configSchema/properties/script",
            json!({
                "$id": "https://example.com/script",
                "$schema": "http://json-schema.org/draft-07/schema#",
                "type": "string"
            }),
            "invalid_schema",
            "/configSchema",
        ),
        (
            "/outputs/0/schema",
            schema_of(MAX_SCHEMA_BYTES + 1),
            "invalid_schema",
            "/outputs/0/schema",
        ),
```

Replace:

```rust
#[test]
fn blueprint_metadata_follows_the_definition_rules() {
```

With:

```rust
#[test]
fn each_side_has_at_most_fifty_ports() {
    let ports = |count: usize| -> Value {
        (0..count)
            .map(|index| json!({"name": format!("p{index}"), "dataType": "core.any", "schema": {}}))
            .collect()
    };
    assert!(validate_new_definition(&with("/inputs", ports(MAX_PORTS))).is_empty());
    for side in ["inputs", "outputs"] {
        let pointer = format!("/{side}");
        let diagnostics = validate_new_definition(&with(&pointer, ports(MAX_PORTS + 1)));
        assert_eq!(codes(&diagnostics), [("too_many_ports", pointer.as_str())]);
    }
}

#[test]
fn a_definition_reports_at_most_two_hundred_diagnostics() {
    let fields: serde_json::Map<String, Value> = (0..2 * MAX_DIAGNOSTICS)
        .map(|index| (format!("f{index}"), json!({"widget": "text"})))
        .collect();
    let diagnostics = validate_new_definition(&with("/uiSchema", Value::Object(fields)));
    assert_eq!(diagnostics.len(), MAX_DIAGNOSTICS + 1);
    assert_eq!(
        codes(&diagnostics[MAX_DIAGNOSTICS..]),
        [("too_many_diagnostics", "")]
    );
}

#[test]
fn blueprint_metadata_follows_the_definition_rules() {
```

Replace:

```rust
fn schemas_compile_offline_as_draft_2020_12_only() {
    let local_ref =
        json!({"$schema": DIALECT, "$defs": {"id": {"type": "string"}}, "$ref": "#/$defs/id"});
    assert!(compile(&local_ref).is_ok());
    assert!(compile(&json!({"$schema": "http://json-schema.org/draft-07/schema#"})).is_err());
```

With:

```rust
fn schemas_compile_offline_as_draft_2020_12_only() {
    assert!(compile(&json!({"$schema": DIALECT, "type": "string"})).is_ok());
    // Even a local reference can make validation exponential, so none is accepted.
    let local_ref =
        json!({"$schema": DIALECT, "$defs": {"id": {"type": "string"}}, "$ref": "#/$defs/id"});
    assert_eq!(
        compile(&local_ref).err().as_deref(),
        Some("Schema references are not supported.")
    );
    assert!(compile(&json!({"$schema": "http://json-schema.org/draft-07/schema#"})).is_err());
    let nested_draft_07 = json!({"properties": {"a": {
        "$id": "https://example.com/a",
        "$schema": "http://json-schema.org/draft-07/schema#"
    }}});
    assert_eq!(
        compile(&nested_draft_07).err().as_deref(),
        Some("Only JSON Schema Draft 2020-12 is supported.")
    );
```

In `crates/domain/tests/publish_rules.rs`, apply these replacements in order:

Replace:

```rust
use corqty_domain::{
    catalog::FILE_TEXT_MAX_BYTES,
    diagnostic::Diagnostic,
    graph::GraphSnapshot,
    publish::{Definitions, canonical_checksum, validate_graph},
};
```

With:

```rust
use corqty_domain::{
    catalog::{FILE_TEXT_MAX_BYTES, MAX_PORTS},
    diagnostic::{Diagnostic, MAX_DIAGNOSTICS},
    graph::{GraphSnapshot, MAX_NODES},
    publish::{Definitions, canonical_checksum, validate_graph},
};
```

Replace:

```rust
#[test]
fn only_the_first_edge_that_closes_a_cycle_is_reported() {
```

With:

```rust
#[test]
fn a_graph_reports_at_most_two_hundred_diagnostics() {
    // Each node leaves 50 required inputs unbound: 25,000 problems in all.
    let ports: Vec<Value> = (0..MAX_PORTS)
        .map(|index| {
            json!({"name": format!("p{index}"), "dataType": "core.any", "schema": {}, "required": true})
        })
        .collect();
    let wide = json!({
        "inputs": ports,
        "outputs": [],
        "configSchema": {"type": "object"},
        "uiSchema": {},
        "defaults": {},
        "executor": {"key": "mock", "version": "1.0.0"}
    });
    let mut definitions = definitions();
    definitions.insert(
        ("acme.wide".to_owned(), 1),
        serde_json::from_value(wide).unwrap(),
    );
    let nodes: Vec<Value> = (0..MAX_NODES)
        .map(|index| {
            let mut node = node(&format!("n{index}"));
            node["definition"]["key"] = json!("acme.wide");
            node["inputs"] = json!({});
            node
        })
        .collect();
    let graph = with(&[("/nodes", json!(nodes)), ("/edges", json!([]))]);

    let diagnostics = validate_graph(&graph, &definitions);
    assert_eq!(diagnostics.len(), MAX_DIAGNOSTICS + 1);
    assert!(
        diagnostics[..MAX_DIAGNOSTICS]
            .iter()
            .all(|diagnostic| diagnostic.code == "missing_required_input")
    );
    assert_eq!(
        codes(&diagnostics[MAX_DIAGNOSTICS..]),
        [("too_many_diagnostics", "")]
    );
}

#[test]
fn only_the_first_edge_that_closes_a_cycle_is_reported() {
```

In `crates/api/tests/catalog_http.rs`, apply these replacements in order:

Replace:

```rust
use serial_test::serial;
use support::AuthFixture;
```

With:

```rust
use serial_test::serial;
use sqlx::PgPool;
use support::AuthFixture;
```

Replace:

```rust
    assert_eq!(deprecated["deprecated"], true);
```

With:

```rust
    assert_eq!(deprecated["deprecated"], true);
    // Deprecating again changes nothing, so it writes no second audit event.
    let again = body(editor.post_json(&deprecate, "").await, StatusCode::OK).await;
    assert_eq!(again, deprecated);
```

Replace:

```rust
#[tokio::test]
#[serial]
async fn a_concurrent_version_of_the_same_key_is_a_conflict() {
    // `registered` resets the database again; this pool only has to stay connected.
    let owner = TestDatabase::reset().await.owner;
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let definitions = format!("/api/v1/orgs/{}/node-definitions", admin.organization_id);
    let thing = definition("acme.thing").to_string();
    let mut other = owner.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO node_definitions(organization_id, scope, key, version, display_name, category, spec)
         VALUES ($1, 'organization', 'acme.thing', 1, 'Thing', 'Testing', '{}')",
    )
    .bind(admin.organization_id)
    .execute(&mut *other)
    .await
    .unwrap();

    // The request also computes version 1, then waits on the uncommitted row's unique key.
    let mut request = pin!(admin.post_json(&definitions, &thing));
    let waiting =
        "SELECT EXISTS (SELECT 1 FROM pg_locks WHERE locktype = 'transactionid' AND NOT granted)";
    tokio::select! {
        response = &mut request => panic!("the request did not wait: {}", response.status()),
        () = async {
            while !sqlx::query_scalar::<_, bool>(waiting).fetch_one(&owner).await.unwrap() {}
        } => other.commit().await.unwrap(),
    }
    let conflict = body(request.await, StatusCode::CONFLICT).await;
```

With:

```rust
/// Posts `request` to `path` while another transaction holds an uncommitted version 1 of the
/// same key, which `insert` adds with the organization id as $1. The request computes
/// version 1 too and waits on that row's unique key; the other transaction then commits.
async fn post_during_insert(
    owner: &PgPool,
    admin: &AuthFixture,
    path: &str,
    request: &str,
    insert: &str,
) -> Response {
    let mut other = owner.begin().await.unwrap();
    sqlx::query(insert)
        .bind(admin.organization_id)
        .execute(&mut *other)
        .await
        .unwrap();
    let mut response = pin!(admin.post_json(path, request));
    let waiting =
        "SELECT EXISTS (SELECT 1 FROM pg_locks WHERE locktype = 'transactionid' AND NOT granted)";
    tokio::select! {
        response = &mut response => panic!("the request did not wait: {}", response.status()),
        () = async {
            while !sqlx::query_scalar::<_, bool>(waiting).fetch_one(owner).await.unwrap() {}
        } => other.commit().await.unwrap(),
    }
    response.await
}

#[tokio::test]
#[serial]
async fn a_concurrent_version_of_the_same_key_is_a_conflict() {
    // `registered` resets the database again; this pool only has to stay connected.
    let owner = TestDatabase::reset().await.owner;
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let definitions = format!("/api/v1/orgs/{}/node-definitions", admin.organization_id);
    let thing = definition("acme.thing").to_string();
    let insert = "INSERT INTO node_definitions(
            organization_id, scope, key, version, display_name, category, spec
         )
         VALUES ($1, 'organization', 'acme.thing', 1, 'Thing', 'Testing', '{}')";
    let response = post_during_insert(&owner, &admin, &definitions, &thing, insert).await;
    let conflict = body(response, StatusCode::CONFLICT).await;
```

Replace:

```rust
    let (_, retried) = created(admin.post_json(&definitions, &thing).await).await;
    assert_eq!(retried["version"], 2);
}
```

With:

```rust
    let (_, retried) = created(admin.post_json(&definitions, &thing).await).await;
    assert_eq!(retried["version"], 2);
}

#[tokio::test]
#[serial]
async fn a_concurrent_blueprint_of_the_same_key_is_a_conflict() {
    // `registered` resets the database again; this pool only has to stay connected.
    let owner = TestDatabase::reset().await.owner;
    let admin = AuthFixture::registered("admin@example.com", "Acme").await;
    let blueprints = format!("/api/v1/orgs/{}/blueprints", admin.organization_id);
    let flow = json!({
        "key": "acme.flow",
        "displayName": "Flow",
        "graph": {
            "schemaVersion": 1,
            "workflowInputSchema": { "type": "object", "properties": {} },
            "nodes": [],
            "edges": [],
            "viewport": { "x": 0, "y": 0, "zoom": 1 },
        },
    })
    .to_string();
    let insert = "INSERT INTO blueprints(organization_id, scope, key, version, display_name, graph)
         VALUES ($1, 'organization', 'acme.flow', 1, 'Flow', '{}')";
    let response = post_during_insert(&owner, &admin, &blueprints, &flow, insert).await;
    let conflict = body(response, StatusCode::CONFLICT).await;
    assert_eq!(conflict["code"], "blueprint_version_conflict");
    assert_eq!(
        catalog_events(&admin, admin.organization_id).await,
        Vec::<Value>::new()
    );

    let (_, retried) = created(admin.post_json(&blueprints, &flow).await).await;
    assert_eq!(retried["version"], 2);
}
```

In `crates/api/tests/workflows_http.rs`:

Replace:

```rust
                "message": "A graph can have at most 500 nodes.",
            }],
        })
    );
```

With:

```rust
                "message": "A graph can have at most 500 nodes.",
            }],
        })
    );

    // PostgreSQL cannot store a NUL character in text or jsonb.
    let mut nul = graph.clone();
    nul["nodes"] = json!([node("n0")]);
    nul["nodes"][0]["displayName"] = json!("Mock\u{0}task");
    let request = json!({ "revision": 3, "graph": nul }).to_string();
    assert_eq!(
        body(
            admin.request_json(Method::PUT, &draft, &request).await,
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await,
        json!({
            "type": "about:blank",
            "title": "Validation failed",
            "status": 422,
            "code": "unsupported_character",
        })
    );
```

In `crates/persistence/tests/workflow_catalog.rs`, apply these replacements in order:

Replace:

```rust
use corqty_domain::{
    catalog::{NewNodeDefinition, validate_new_definition},
```

With:

```rust
use std::pin::pin;

use corqty_domain::{
    catalog::{NewNodeDefinition, validate_new_definition},
```

Replace:

```rust
                json!({"version": 1, "draftRevision": 1, "checksum": checksum})
            ),
        ]
    );
}
```

With:

```rust
                json!({"version": 1, "draftRevision": 1, "checksum": checksum})
            ),
        ]
    );
}

#[tokio::test]
#[serial]
async fn draft_saves_lock_the_workflow_before_the_draft_like_publishes() {
    let fixture = Fixture::new().await;
    let (a, editor) = (fixture.org_a, fixture.editor);
    let empty = GraphSnapshot::empty();
    let mut tx = fixture.tx(editor, a).await;
    let workflow = workflows::insert_workflow(&mut tx, a, editor, "Race", "race", &empty, None)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    // A publish has locked the workflow row and is about to lock the draft row.
    let owner = &fixture.database.owner;
    let mut publishing = owner.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM workflows WHERE id = $1 FOR UPDATE")
        .bind(workflow.id)
        .execute(&mut *publishing)
        .await
        .unwrap();
    let mut save = pin!(async {
        let mut tx = fixture.tx(editor, a).await;
        let saved = workflows::save_draft(&mut tx, a, editor, workflow.id, 1, &empty)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        saved
    });
    let waiting =
        "SELECT EXISTS (SELECT 1 FROM pg_locks WHERE locktype = 'transactionid' AND NOT granted)";
    tokio::select! {
        saved = &mut save => panic!("the save did not wait: {saved:?}"),
        () = async {
            while !sqlx::query_scalar::<_, bool>(waiting).fetch_one(owner).await.unwrap() {}
        } => {}
    }
    // The waiting save holds no lock on the draft yet, so the publish takes it at once
    // instead of deadlocking with the save.
    sqlx::query("SELECT 1 FROM workflow_drafts WHERE workflow_id = $1 FOR UPDATE NOWAIT")
        .bind(workflow.id)
        .execute(&mut *publishing)
        .await
        .unwrap();
    publishing.commit().await.unwrap();
    assert_eq!(save.await, Some(2));
}
```

- [ ] **Step 2: Run the tests and verify RED**

```text
cargo test -p corqty-domain --no-fail-fast --test catalog_rules --test publish_rules
```

Expected: neither test binary compiles. Both fail with `error[E0432]: unresolved imports`: `catalog_rules` names
`corqty_domain::catalog::MAX_PORTS`, `corqty_domain::diagnostic::MAX_DIAGNOSTICS` and
`corqty_domain::schema::MAX_SCHEMA_BYTES`, and `publish_rules` names the first two. With only the three
constants added, the same run fails on behavior: in `catalog_rules` 3 tests pass and 4 fail
(`schemas_compile_offline_as_draft_2020_12_only` gets `None` instead of the reference error,
`a_definition_reports_at_most_two_hundred_diagnostics` gets 400 diagnostics instead of 201,
`each_side_has_at_most_fifty_ports` gets `[]` instead of `too_many_ports`, and
`each_definition_rule_reports_its_code_at_its_pointer` gets `[]` for the local `$ref` at `/inputs/0/schema`), and
in `publish_rules` 9 pass while `a_graph_reports_at_most_two_hundred_diagnostics` gets 25000 diagnostics instead
of 201.

Start the test database if it is not running:

```text
docker compose -f docker-compose.test.yml up -d --wait
```

Expected: `postgres-test` reports `Healthy`.

```text
cargo test -p corqty-api --no-fail-fast --test catalog_http --test workflows_http
```

Expected: both binaries compile, because the tests only talk HTTP.

- In `catalog_http`, 2 tests pass, the refactored node definition race included.
  `editors_add_and_deprecate_node_definition_versions` fails on its audit assertion: the left list holds two
  `node_definition.deprecated` rows for v1, the right list one.
  `a_concurrent_blueprint_of_the_same_key_is_a_conflict` fails in `body`: ``assertion `left == right` failed``,
  left 500 and right 409.
- In `workflows_http`, 3 tests pass. `drafts_save_only_at_the_current_revision` fails in `body` on the NUL save,
  with left 500 and right 422.

```text
cargo test -p corqty-persistence --no-fail-fast --test workflow_catalog
```

Expected: 5 tests pass. `draft_saves_lock_the_workflow_before_the_draft_like_publishes` panics on the `unwrap`
of the `NOWAIT` statement with a `PgDatabaseError` whose code is `55P03` and whose message is
`could not obtain lock on row in relation "workflow_drafts"`, because the waiting save already holds the draft
row.

- [ ] **Step 3: Bound schemas and diagnostics in the domain**

`MAX_DIAGNOSTICS` and `capped` live in `diagnostic.rs`, next to `Diagnostic`, because both `catalog.rs` and
`publish.rs` use them. `check_keywords` walks objects and arrays recursively. `serde_json` refuses input nested
deeper than 128 levels, so the recursion is bounded. The size check runs first, so the walk and the compiler
only ever see schemas of at most 16 KiB.

Replace the whole of `crates/domain/src/diagnostic.rs`:

```rust
use serde::{Deserialize, Serialize};

/// The most diagnostics one validation reports, so a hostile document cannot make the
/// response huge. Validations stop collecting soon after they pass it.
pub const MAX_DIAGNOSTICS: usize = 200;

/// One problem in a submitted document, located by an RFC 6901 JSON Pointer
/// into that document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub pointer: String,
    pub message: String,
}

impl Diagnostic {
    pub fn new(code: &str, pointer: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            pointer: pointer.into(),
            message: message.into(),
        }
    }
}

/// The first `MAX_DIAGNOSTICS` of `diagnostics`, followed by one `too_many_diagnostics`
/// when there were more.
#[must_use]
pub fn capped(mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    if diagnostics.len() > MAX_DIAGNOSTICS {
        diagnostics.truncate(MAX_DIAGNOSTICS);
        diagnostics.push(Diagnostic::new(
            "too_many_diagnostics",
            "",
            format!("Only the first {MAX_DIAGNOSTICS} problems are listed."),
        ));
    }
    diagnostics
}

/// One RFC 6901 reference token: `~` becomes `~0` and `/` becomes `~1`.
#[must_use]
pub fn pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}
```

Replace the whole of `crates/domain/src/schema.rs`:

```rust
use jsonschema::{PatternOptions, Validator};
use serde_json::Value;

use crate::diagnostic::{Diagnostic, MAX_DIAGNOSTICS};

/// The only JSON Schema dialect Corqty accepts.
pub const DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";
/// The largest schema, as JSON, that Corqty compiles. It bounds what one validation costs.
pub const MAX_SCHEMA_BYTES: usize = 16_384;

/// Compiles a user-supplied schema as Draft 2020-12. A schema must be self-contained and
/// small: references are refused, because even local ones can make validation exponential.
/// Patterns use the linear-time `regex` engine.
pub fn compile(schema: &Value) -> Result<Validator, String> {
    if schema.to_string().len() > MAX_SCHEMA_BYTES {
        return Err(format!(
            "A schema can have at most {MAX_SCHEMA_BYTES} bytes."
        ));
    }
    check_keywords(schema)?;
    jsonschema::draft202012::options()
        .offline()
        .with_pattern_options(PatternOptions::regex())
        .build(schema)
        .map_err(|error| error.to_string())
}

/// Refuses reference keywords and any dialect but `DIALECT`, at every depth. Only string
/// values count: a property merely named `$ref` or `$schema` holds a schema object.
fn check_keywords(value: &Value) -> Result<(), String> {
    match value {
        Value::Object(object) => object.iter().try_for_each(|(key, value)| {
            match (key.as_str(), value) {
                ("$ref" | "$dynamicRef" | "$recursiveRef", Value::String(_)) => {
                    return Err("Schema references are not supported.".to_owned());
                }
                ("$schema", Value::String(dialect)) if dialect != DIALECT => {
                    return Err("Only JSON Schema Draft 2020-12 is supported.".to_owned());
                }
                _ => {}
            }
            check_keywords(value)
        }),
        Value::Array(items) => items.iter().try_for_each(check_keywords),
        _ => Ok(()),
    }
}

/// The first `MAX_DIAGNOSTICS` Draft 2020-12 meta-schema errors in `schema`, as (pointer
/// into the schema, message).
#[must_use]
pub fn schema_errors(schema: &Value) -> Vec<(String, String)> {
    let mut errors: Vec<(String, String)> = jsonschema::draft202012::meta::validator()
        .iter_errors(schema)
        .take(MAX_DIAGNOSTICS)
        .map(|error| (error.instance_path().to_string(), error.to_string()))
        .collect();
    // A schema that is neither an object nor a boolean fails every branch the same way.
    errors.dedup();
    errors
}

/// The first `MAX_DIAGNOSTICS` errors of `instance`, as (pointer into the instance, message).
/// Messages say "value" instead of repeating submitted data.
#[must_use]
pub fn instance_errors(validator: &Validator, instance: &Value) -> Vec<(String, String)> {
    validator
        .iter_errors(instance)
        .take(MAX_DIAGNOSTICS)
        .map(|error| {
            (
                error.instance_path().to_string(),
                error.masked().to_string(),
            )
        })
        .collect()
}

/// Compiles `schema`, or reports why it does not compile as `invalid_schema` diagnostics:
/// one per meta-schema error under `pointer`, otherwise the compile error at `pointer`.
pub fn compile_at(schema: &Value, pointer: &str) -> Result<Validator, Vec<Diagnostic>> {
    compile(schema).map_err(|message| {
        let errors = schema_errors(schema);
        if errors.is_empty() {
            return vec![Diagnostic::new("invalid_schema", pointer, message)];
        }
        errors
            .into_iter()
            .map(|(path, message)| {
                Diagnostic::new("invalid_schema", format!("{pointer}{path}"), message)
            })
            .collect()
    })
}
```

In `crates/domain/src/catalog.rs`, apply these replacements in order:

Replace:

```rust
use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    diagnostic::{Diagnostic, pointer_token},
```

With:

```rust
use std::collections::{BTreeMap, HashSet};

use jsonschema::Validator;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    diagnostic::{Diagnostic, MAX_DIAGNOSTICS, capped, pointer_token},
```

Replace:

```rust
pub const FILE_TEXT_MAX_BYTES: usize = 262_144;
```

With:

```rust
pub const FILE_TEXT_MAX_BYTES: usize = 262_144;
/// The most input ports, and the most output ports, one definition can have.
pub const MAX_PORTS: usize = 50;
```

Replace:

```rust
    diagnostics.extend(validate_spec(&definition.spec));
    diagnostics
}
```

With:

```rust
    diagnostics.extend(validate_spec(&definition.spec));
    capped(diagnostics)
}
```

Replace:

```rust
    for (side, ports) in [("inputs", &spec.inputs), ("outputs", &spec.outputs)] {
        let mut names = HashSet::new();
        for (index, port) in ports.iter().enumerate() {
            let pointer = format!("/{side}/{index}");
```

With:

```rust
    for (side, ports) in [("inputs", &spec.inputs), ("outputs", &spec.outputs)] {
        if ports.len() > MAX_PORTS {
            diagnostics.push(Diagnostic::new(
                "too_many_ports",
                format!("/{side}"),
                format!("A definition can have at most {MAX_PORTS} {side}."),
            ));
            continue;
        }
        let mut names = HashSet::new();
        for (index, port) in ports.iter().enumerate() {
            if diagnostics.len() > MAX_DIAGNOSTICS {
                break;
            }
            let pointer = format!("/{side}/{index}");
```

Replace:

```rust
    for (field, ui) in &spec.ui_schema {
        let pointer = format!("/uiSchema/{}", pointer_token(field));
```

With:

```rust
    for (field, ui) in &spec.ui_schema {
        if diagnostics.len() > MAX_DIAGNOSTICS {
            break;
        }
        let pointer = format!("/uiSchema/{}", pointer_token(field));
```

Replace:

```rust
) -> Vec<Diagnostic> {
    let Ok(validator) = compile(&spec.config_schema) else {
        return vec![Diagnostic::new(
            "invalid_config",
            pointer_prefix,
            "The node definition's config schema does not compile.",
        )];
    };
    let mut diagnostics: Vec<Diagnostic> = instance_errors(&validator, config)
```

With:

```rust
) -> Vec<Diagnostic> {
    let validator = compile(&spec.config_schema).ok();
    validate_compiled_config(spec, validator.as_ref(), config, pointer_prefix)
}

/// `validate_config` with the config schema already compiled, so a graph compiles each
/// definition's schema once. `None` means the schema does not compile.
#[must_use]
pub fn validate_compiled_config(
    spec: &NodeDefinitionSpec,
    validator: Option<&Validator>,
    config: &Value,
    pointer_prefix: &str,
) -> Vec<Diagnostic> {
    let Some(validator) = validator else {
        return vec![Diagnostic::new(
            "invalid_config",
            pointer_prefix,
            "The node definition's config schema does not compile.",
        )];
    };
    let mut diagnostics: Vec<Diagnostic> = instance_errors(validator, config)
```

In `crates/domain/src/publish.rs`, apply these replacements in order:

Replace:

```rust
use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    catalog::{BUILTIN_EXECUTORS, NodeDefinitionSpec, PortSpec, fits, validate_config},
    diagnostic::{Diagnostic, pointer_token},
```

With:

```rust
use std::collections::{HashMap, HashSet};

use jsonschema::Validator;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    catalog::{BUILTIN_EXECUTORS, NodeDefinitionSpec, PortSpec, fits, validate_compiled_config},
    diagnostic::{Diagnostic, MAX_DIAGNOSTICS, capped, pointer_token},
```

Replace:

```rust
/// Every reason `graph` cannot be published, sorted by (pointer, code). Empty means publishable.
/// An unknown definition or port skips the rules that depend on it, so nothing cascades.
```

With:

```rust
/// Every reason `graph` cannot be published, sorted by (pointer, code) and capped at
/// `MAX_DIAGNOSTICS`. Empty means publishable. An unknown definition or port skips the rules
/// that depend on it, so nothing cascades.
```

Replace:

```rust
    for (index, edge) in graph.edges.iter().enumerate() {
        let pointer = format!("/edges/{index}");
```

With:

```rust
    for (index, edge) in graph.edges.iter().enumerate() {
        if diagnostics.len() > MAX_DIAGNOSTICS {
            break;
        }
        let pointer = format!("/edges/{index}");
```

Replace:

```rust
    let mut node_ids = HashSet::new();
    for (index, node) in graph.nodes.iter().enumerate() {
        let pointer = format!("/nodes/{index}");
```

With:

```rust
    // Each definition's config schema, and each port schema a literal needs, compiles at most
    // once, however many nodes use it.
    let mut config_validators: HashMap<(&str, i32), Option<Validator>> = HashMap::new();
    let mut port_validators: HashMap<(&str, i32, &str), Option<Validator>> = HashMap::new();
    let mut node_ids = HashSet::new();
    for (index, node) in graph.nodes.iter().enumerate() {
        if diagnostics.len() > MAX_DIAGNOSTICS {
            break;
        }
        let pointer = format!("/nodes/{index}");
```

Replace:

```rust
        diagnostics.extend(validate_config(
            spec,
            &node.config,
            &format!("{pointer}/config"),
        ));

        for (name, binding) in &node.inputs {
            let at = format!("{pointer}/inputs/{}", pointer_token(name));
```

With:

```rust
        let (key, version) = (node.definition.key.as_str(), node.definition.version);
        let validator = config_validators
            .entry((key, version))
            .or_insert_with(|| compile(&spec.config_schema).ok());
        diagnostics.extend(validate_compiled_config(
            spec,
            validator.as_ref(),
            &node.config,
            &format!("{pointer}/config"),
        ));

        for (name, binding) in &node.inputs {
            if diagnostics.len() > MAX_DIAGNOSTICS {
                break;
            }
            let at = format!("{pointer}/inputs/{}", pointer_token(name));
```

Replace:

```rust
                InputBinding::Literal { value } => match compile(&port.schema) {
                    Ok(validator) => {
                        for (path, message) in instance_errors(&validator, value) {
                            let pointer = format!("{at}/value{path}");
                            diagnostics.push(Diagnostic::new("invalid_literal", pointer, message));
                        }
                    }
                    Err(_) => diagnostics.push(Diagnostic::new(
```

With:

```rust
                InputBinding::Literal { value } => match port_validators
                    .entry((key, version, port.name.as_str()))
                    .or_insert_with(|| compile(&port.schema).ok())
                {
                    Some(validator) => {
                        for (path, message) in instance_errors(validator, value) {
                            let pointer = format!("{at}/value{path}");
                            diagnostics.push(Diagnostic::new("invalid_literal", pointer, message));
                        }
                    }
                    None => diagnostics.push(Diagnostic::new(
```

Replace:

```rust
    diagnostics.sort_by(|a, b| (&a.pointer, &a.code).cmp(&(&b.pointer, &b.code)));
    diagnostics
}
```

With:

```rust
    diagnostics.sort_by(|a, b| (&a.pointer, &a.code).cmp(&(&b.pointer, &b.code)));
    capped(diagnostics)
}
```

- [ ] **Step 4: Verify the domain tests pass**

```text
cargo test -p corqty-domain --test catalog_rules --test publish_rules
```

Expected: `catalog_rules` 7 and `publish_rules` 10 tests pass.

- [ ] **Step 5: Validate off the async runtime and close the races**

`blocking` lives in `crates/api/src/catalog.rs` next to `Items`, which `workflows.rs` already imports from there.
The handlers still authorize first, so an outsider learns nothing from validation, and the blueprint metadata
checks stay on the runtime because they are cheap. Publish validates while it holds the row lock, as before; the
lock is what keeps the validated draft and the stored version the same.

`map_version_conflict` now matches on the constraint name, so one function maps the version race for both
tables. Any other database error still becomes `Internal` through `From<sqlx::Error>`.

In `crates/api/src/error.rs`:

Replace:

```rust
impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        Self::internal(error)
    }
}
```

With:

```rust
impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        // 22P05 and 22021: PostgreSQL text and jsonb cannot hold a NUL character.
        let nul = error
            .as_database_error()
            .and_then(|database| database.code())
            .is_some_and(|code| code == "22P05" || code == "22021");
        if nul {
            return Self::Validation("unsupported_character");
        }
        Self::internal(error)
    }
}
```

In `crates/api/src/catalog.rs`, apply these replacements in order:

Replace:

```rust
    let diagnostics = validate_new_definition(&definition);
```

With:

```rust
    let (definition, diagnostics) = blocking(definition, validate_new_definition).await?;
```

Replace:

```rust
    let definitions = catalog::definitions_map(&mut access.tx).await?;
    diagnostics.extend(
        validate_graph(&blueprint.graph, &definitions)
            .into_iter()
            .map(|diagnostic| Diagnostic {
                pointer: format!("/graph{}", diagnostic.pointer),
                ..diagnostic
            }),
    );
```

With:

```rust
    let definitions = catalog::definitions_map(&mut access.tx).await?;
    let (blueprint, graph_diagnostics) = blocking(blueprint, move |blueprint| {
        validate_graph(&blueprint.graph, &definitions)
    })
    .await?;
    diagnostics.extend(graph_diagnostics.into_iter().map(|diagnostic| Diagnostic {
        pointer: format!("/graph{}", diagnostic.pointer),
        ..diagnostic
    }));
```

Replace:

```rust
        &blueprint.graph,
    )
    .await?;
    access.commit().await?;
    Ok((StatusCode::CREATED, Json(record)))
}

/// Two requests computed the same next version; the later insert fails on the unique key.
fn map_version_conflict(error: sqlx::Error) -> ApiError {
    if error
        .as_database_error()
        .is_some_and(|database| database.constraint() == Some("node_definitions_key_version"))
    {
        ApiError::Conflict("node_definition_version_conflict")
    } else {
        error.into()
    }
}
```

With:

```rust
        &blueprint.graph,
    )
    .await
    .map_err(map_version_conflict)?;
    access.commit().await?;
    Ok((StatusCode::CREATED, Json(record)))
}

/// Runs `check(&input)` on a blocking thread, so a slow validation never stalls the async
/// workers, and hands `input` back with the result.
pub(crate) async fn blocking<T, R>(
    input: T,
    check: impl FnOnce(&T) -> R + Send + 'static,
) -> Result<(T, R), ApiError>
where
    T: Send + 'static,
    R: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let result = check(&input);
        (input, result)
    })
    .await
    .map_err(ApiError::internal)
}

/// Two requests computed the same next version; the later insert fails on the unique key.
fn map_version_conflict(error: sqlx::Error) -> ApiError {
    let code = match error
        .as_database_error()
        .and_then(|database| database.constraint())
    {
        Some("node_definitions_key_version") => "node_definition_version_conflict",
        Some("blueprints_key_version") => "blueprint_version_conflict",
        _ => return error.into(),
    };
    ApiError::Conflict(code)
}
```

In `crates/api/src/workflows.rs`, apply these replacements in order:

Replace:

```rust
    catalog::Items,
```

With:

```rust
    catalog::{Items, blocking},
```

Replace:

```rust
    let diagnostics = validate_graph(&body.graph, &definitions);
```

With:

```rust
    let (_, diagnostics) =
        blocking(body.graph, move |graph| validate_graph(graph, &definitions)).await?;
```

Replace:

```rust
    let diagnostics = validate_graph(&draft.graph, &definitions);
```

With:

```rust
    let (graph, diagnostics) = blocking(draft.graph, move |graph| {
        validate_graph(graph, &definitions)
    })
    .await?;
```

Replace:

```rust
        &draft.graph,
        &canonical_checksum(&draft.graph),
```

With:

```rust
        &graph,
        &canonical_checksum(&graph),
```

In `crates/persistence/src/catalog.rs`:

Replace:

```rust
/// Deprecates one of the organization's own definitions. System and foreign ids give None.
pub async fn deprecate_node_definition(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    id: Uuid,
) -> Result<Option<NodeDefinitionRecord>, sqlx::Error> {
    let record: Option<NodeDefinitionRecord> = sqlx::query_as(&format!(
        "UPDATE node_definitions
         SET deprecated_at = COALESCE(deprecated_at, now())
         WHERE id = $1 AND organization_id = $2
         RETURNING {DEFINITION}"
    ))
    .bind(id)
    .bind(organization_id)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(record) = &record {
        audit_definition(
            conn,
            organization_id,
            actor_user_id,
            "node_definition.deprecated",
            record,
        )
        .await?;
    }
    Ok(record)
}
```

With:

```rust
/// Deprecates one of the organization's own definitions. An already deprecated one comes
/// back unchanged and without a second audit row. System and foreign ids give None.
pub async fn deprecate_node_definition(
    conn: &mut PgConnection,
    organization_id: Uuid,
    actor_user_id: Uuid,
    id: Uuid,
) -> Result<Option<NodeDefinitionRecord>, sqlx::Error> {
    let deprecated: Option<NodeDefinitionRecord> = sqlx::query_as(&format!(
        "UPDATE node_definitions
         SET deprecated_at = now()
         WHERE id = $1 AND organization_id = $2 AND deprecated_at IS NULL
         RETURNING {DEFINITION}"
    ))
    .bind(id)
    .bind(organization_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(record) = deprecated else {
        return sqlx::query_as(&format!(
            "SELECT {DEFINITION} FROM node_definitions WHERE id = $1 AND organization_id = $2"
        ))
        .bind(id)
        .bind(organization_id)
        .fetch_optional(conn)
        .await;
    };
    audit_definition(
        conn,
        organization_id,
        actor_user_id,
        "node_definition.deprecated",
        &record,
    )
    .await?;
    Ok(Some(record))
}
```

In `crates/persistence/src/workflows.rs`, apply these replacements in order:

Replace:

```rust
/// Replaces the draft if it is still at `expected_revision` and returns the new revision.
/// None means the revision moved on or the workflow is not visible; nothing is written then.
```

With:

```rust
/// Replaces the draft if it is still at `expected_revision` and returns the new revision.
/// None means the revision moved on or the workflow is not visible; the caller then rolls
/// back. The workflow row is locked before the draft row, in `lock_workflow`'s order, so a
/// save and a publish of one workflow cannot deadlock.
```

Replace:

```rust
) -> Result<Option<i64>, sqlx::Error> {
    let revision: Option<i64> = sqlx::query_scalar(
```

With:

```rust
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query("UPDATE workflows SET updated_at = now() WHERE id = $1")
        .bind(workflow_id)
        .execute(&mut *conn)
        .await?;
    let revision: Option<i64> = sqlx::query_scalar(
```

Replace:

```rust
    let Some(revision) = revision else {
        return Ok(None);
    };
    sqlx::query("UPDATE workflows SET updated_at = now() WHERE id = $1")
        .bind(workflow_id)
        .execute(&mut *conn)
        .await?;
```

With:

```rust
    let Some(revision) = revision else {
        return Ok(None);
    };
```

- [ ] **Step 6: Record the limits in the spec**

In `docs/superpowers/specs/2026-07-28-corqty-workflow-orchestrator-design.md`:

Replace:

```markdown
- Graph and timeout limits are fixed constants in Milestone 2; deployment configuration is deferred to Milestone 5.
```

With:

```markdown
- Graph and timeout limits are fixed constants in Milestone 2; deployment configuration is deferred to Milestone 5.
- User JSON Schemas are self-contained (no `$ref`, `$dynamicRef`, or `$recursiveRef`), use Draft 2020-12 throughout, and are at most 16 KiB each. A node definition has at most 50 input ports and 50 output ports, and one validation reports at most 200 diagnostics.
```

- [ ] **Step 7: Verify GREEN**

```text
cargo test -p corqty-api --no-fail-fast --test catalog_http --test workflows_http
```

Expected: `catalog_http` 4 and `workflows_http` 4 tests pass.

```text
cargo test -p corqty-persistence --no-fail-fast --test workflow_catalog
```

Expected: all 6 tests pass. The three race tests passed in each of five consecutive runs.

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
`catalog_http` 4, `members_http` 3, `organizations_http` 1, `problems_http` 2, `publish_http` 2,
`workflows_http` 4. corqty-domain: `account_and_rbac` 6, `catalog_rules` 7, `graph_rules` 8, `publish_rules` 10.
corqty-persistence: `auth_store` 2, `catalog_guards` 3, `final_admin` 2, `migrations` 1, `tenant_isolation` 1,
`test_reset` 1, `workflow_catalog` 6. 75 tests in total.

- [ ] **Step 9: Commit**

```text
git add crates/domain crates/api crates/persistence docs/superpowers/specs/2026-07-28-corqty-workflow-orchestrator-design.md
git commit -m "fix: bound validation cost and close API races"
```

Expected: `git show --stat HEAD` lists exactly the 15 files above (489 insertions, 93 deletions).

## Task 21: Keep studio drafts in sync and recover from save failures

**Files:**

- Create: `apps/web/src/app/(app)/orgs/[organizationId]/error.tsx`
- Modify: `apps/web/src/auth/auth-context.tsx`
- Modify: `apps/web/src/auth/auth-gate.tsx`
- Modify: `apps/web/src/studio/graph-model.ts`
- Modify: `apps/web/src/studio/widgets.tsx`
- Modify: `apps/web/src/studio/studio-canvas.tsx`
- Modify: `apps/web/src/studio/inspector.tsx`
- Modify: `apps/web/src/studio/use-draft-autosave.ts`
- Modify: `apps/web/src/studio/studio.tsx`
- Modify: `apps/web/src/test/render.tsx`
- Test: `apps/web/src/studio/use-draft-autosave.test.ts`
- Test: `apps/web/src/studio/studio.test.tsx`
- Test: `apps/web/src/views/auth-views.test.tsx`

This task fixes the studio problems that the review of Tasks 1-19 confirmed.

The studio no longer reopens at an old revision. Every successful autosave, including the one sent when the
studio unmounts, writes `{revision, graph}` into the cached workflow (`setQueryData` on
`queryKeys.workflow(organizationId, workflowId)`). The studio also renders the editor only once the workflow
query `isFetchedAfterMount`, so re-entering the studio always starts from a fresh `GET`. A save from the
previous visit that lands after that fetch is the one remaining gap: the next edit then reports a conflict,
and Reload resolves it. That gap is marked with a `ponytail:` comment.

Save failures no longer drop edits silently:
- `flush()` retries an edit whose save failed for any reason other than a 409.
- It waits until no save is due or in flight. It loops while the save queue keeps changing, and it sends any
  edit that is still waiting for its timer.
- When a flush still cannot save, Validate and Publish say `Save the draft first: the last save failed.`
- The save-failure text has `role="alert"`.
- While an edit waits for its timer or a save is in flight, a `beforeunload` listener calls
  `preventDefault()`, so the browser asks before closing or reloading the tab.

Drafts are normalized on load. The PUT draft API accepts any JSON as the workflow input schema and as a node's
config (D13). The canvas now turns a non-object `workflowInputSchema` into `{"type":"object","properties":{}}`
and a non-object node `config` into `{}` (`normalizeGraph` in `graph-model.ts`, which now also owns
`isRecord`). The new `orgs/[organizationId]/error.tsx` boundary keeps the app shell on screen when a page
crashes and offers Retry, which calls `reset()`.

`AuthProvider` exposes `error`. It is set when `/auth/me` fails with anything but a 401 and cleared by a
success or a 401. `AuthGate` redirects to `/login` only when `!loading && !me && !error`. Otherwise it shows
`We could not check your session.` with a Retry button that calls `refresh()`.

The canvas and inspector split into two columns from `xl` (1280 px) instead of `lg`. Between 1024 and
1279 px, the inspector stacks below a wider canvas.

- [ ] **Step 1: Keep the test providers across rerenders**

The studio tests leave and re-enter the studio with `rerender`. The providers become a `wrapper`, so a
rerender keeps the same `QueryClient` and its cache.

Replace the contents of `apps/web/src/test/render.tsx` with:

```tsx
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { render } from "@testing-library/react"
import type { ReactElement } from "react"
import { vi } from "vitest"

import { AuthProvider } from "@/auth/auth-context"

// setup.ts mocks next/navigation: useRouter returns this router and
// usePathname reads window.location.
export const router = { push: vi.fn(), replace: vi.fn() }

// The providers wrap every rerender, so the query cache outlives a page.
export function renderWithProviders(ui: ReactElement, { route = "/" } = {}) {
  window.history.replaceState(null, "", route)
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  return render(ui, {
    wrapper: ({ children }) => (
      <QueryClientProvider client={queryClient}>
        <AuthProvider>{children}</AuthProvider>
      </QueryClientProvider>
    ),
  })
}
```

- [ ] **Step 2: Write the failing autosave tests**

The hook now needs a `QueryClient`, so `renderAutosave` seeds one with the loaded workflow. A flush now retries
a failed save, so the 413/422 test waits for the failure through the `beforeunload` signal instead of
flushing.

In `apps/web/src/studio/use-draft-autosave.test.ts`, apply these replacements in order:

Replace:

```ts
import { act, renderHook } from "@testing-library/react"
import { http, HttpResponse } from "msw/http"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import type { GraphSnapshot } from "@/api/types"
```

With:

```ts
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { act, cleanup, renderHook } from "@testing-library/react"
import { http, HttpResponse } from "msw/http"
import { createElement, type ReactNode } from "react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import { queryKeys } from "@/api/queries"
import type { GraphSnapshot, Workflow } from "@/api/types"
```

Replace:

```ts
const { draft } = workflow // revision 3
```

With:

```ts
const { draft } = workflow // revision 3
const workflowKey = queryKeys.workflow("org-1", "wf-2")
// The studio's query cache, holding the workflow as it was loaded.
let queryClient: QueryClient
```

Replace:

```ts
function renderAutosave(readOnly = false) {
  const { result } = renderHook(() =>
    useDraftAutosave("org-1", "wf-2", draft, readOnly),
  )
```

With:

```ts
function renderAutosave(readOnly = false) {
  queryClient = new QueryClient()
  queryClient.setQueryData(workflowKey, workflow)
  const { result } = renderHook(
    () => useDraftAutosave("org-1", "wf-2", draft, readOnly),
    {
      wrapper: ({ children }: { children: ReactNode }) =>
        createElement(QueryClientProvider, { client: queryClient }, children),
    },
  )
```

Replace:

```ts
async function settle(autosave: Autosave) {
  await act(() => autosave.current.flush())
}
```

With:

```ts
async function settle(autosave: Autosave) {
  await act(() => autosave.current.flush())
}

// True while the page asks the browser to confirm leaving: an edit waits for
// its timer, or a save is in flight.
function unloadBlocked() {
  return !window.dispatchEvent(new Event("beforeunload", { cancelable: true }))
}
```

Replace:

```ts
  const autosave = renderAutosave()
  act(() => autosave.current.edit(named("A")))
  await wait(AUTOSAVE_DELAY_MS)
  await settle(autosave)
  expect(autosave.current.status).toBe("error")
```

With:

```ts
  const autosave = renderAutosave()
  act(() => autosave.current.edit(named("A")))
  await wait(AUTOSAVE_DELAY_MS)
  // A flush would retry the save, so this only waits for it to fail.
  await act(() => vi.waitUntil(() => !unloadBlocked()))
  expect(autosave.current.status).toBe("error")
```

Replace:

```ts
  expect(await act(() => autosave.current.flush())).toEqual(draft)
  expect(saves).toEqual([])
})
```

With:

```ts
  expect(await act(() => autosave.current.flush())).toEqual(draft)
  expect(saves).toEqual([])
})

it("flush also waits for a save chained behind the one in flight", async () => {
  const answers: (() => void)[] = []
  const saves = recordSaves(async ({ revision }) => {
    await new Promise<void>((resolve) => answers.push(resolve))
    return HttpResponse.json({ revision: revision + 1 })
  })
  const autosave = renderAutosave()
  act(() => autosave.current.edit(named("A")))
  await wait(AUTOSAVE_DELAY_MS)
  await vi.waitUntil(() => saves.length === 1)

  // Publish flushes while A saves, and an edit meanwhile chains B behind A.
  const flushed = autosave.current.flush()
  act(() => autosave.current.edit(named("B")))
  await wait(AUTOSAVE_DELAY_MS)
  const result = await act(async () => {
    answers[0]()
    await vi.waitUntil(() => saves.length === 2)
    answers[1]()
    return flushed
  })
  expect(result).toEqual({ revision: 5, graph: named("B") })
})

it("each save updates the cached workflow, including the save on leaving", async () => {
  const autosave = renderAutosave()
  const cached = () => queryClient.getQueryData<Workflow>(workflowKey)
  act(() => autosave.current.edit(named("A")))
  await wait(AUTOSAVE_DELAY_MS)
  await settle(autosave)
  expect(cached()).toEqual({
    ...workflow,
    draft: { revision: 4, graph: named("A") },
  })

  // Unmounting, as leaving the studio does, saves the pending edit.
  act(() => autosave.current.edit(named("B")))
  cleanup()
  await vi.waitFor(() =>
    expect(cached()).toEqual({
      ...workflow,
      draft: { revision: 5, graph: named("B") },
    }),
  )
})

it("asks before unloading while an edit waits or saves", async () => {
  let answer = () => {}
  const saves = recordSaves(async ({ revision }) => {
    await new Promise<void>((resolve) => {
      answer = resolve
    })
    return HttpResponse.json({ revision: revision + 1 })
  })
  const autosave = renderAutosave()
  expect(unloadBlocked()).toBe(false)

  act(() => autosave.current.edit(named("A")))
  expect(unloadBlocked()).toBe(true)
  await wait(AUTOSAVE_DELAY_MS)
  await vi.waitUntil(() => saves.length === 1)
  expect(unloadBlocked()).toBe(true)

  answer()
  await settle(autosave)
  expect(unloadBlocked()).toBe(false)
})
```

- [ ] **Step 3: Write the failing studio tests**

`page()` mirrors the layouts around any page, so a test can leave the studio for another page and come back
while the cache stays.

In `apps/web/src/studio/studio.test.tsx`, apply these replacements in order:

Replace:

```tsx
import { http, HttpResponse } from "msw/http"
import { expect, it } from "vitest"

import type { Diagnostic, GraphSnapshot } from "@/api/types"
```

With:

```tsx
import { http, HttpResponse } from "msw/http"
import type { ReactNode } from "react"
import { expect, it } from "vitest"

import type {
  Diagnostic,
  GraphNode,
  GraphSnapshot,
  JsonSchema,
  Workflow,
} from "@/api/types"
```

Replace:

```tsx
// Mirrors the (app) and orgs/[organizationId] layouts around the studio page.
function renderStudio() {
  return renderWithProviders(
    <AuthGate>
      <AppShell organizationId="org-1">
        <Studio organizationId="org-1" workflowId="wf-2" />
      </AppShell>
    </AuthGate>,
    { route: "/orgs/org-1/workflows/wf-2" },
  )
}
```

With:

```tsx
// Mirrors the (app) and orgs/[organizationId] layouts around a page.
function page(children: ReactNode) {
  return (
    <AuthGate>
      <AppShell organizationId="org-1">{children}</AppShell>
    </AuthGate>
  )
}

const studioPage = page(<Studio organizationId="org-1" workflowId="wf-2" />)

function renderStudio() {
  return renderWithProviders(studioPage, {
    route: "/orgs/org-1/workflows/wf-2",
  })
}
```

Replace:

```tsx
  fireEvent.click(pane)
  expect(
    await within(inspector).findByLabelText("Input schema (JSON)"),
  ).toBeInTheDocument()
})
```

With:

```tsx
  fireEvent.click(pane)
  expect(
    await within(inspector).findByLabelText("Input schema (JSON)"),
  ).toBeInTheDocument()
})

it("opens a draft whose input schema and node config are not objects", async () => {
  // Draft saves accept any JSON in these two fields.
  const [vpc, ...rest] = infrastructure.graph.nodes
  withDraft({
    ...infrastructure.graph,
    workflowInputSchema: null as unknown as JsonSchema,
    nodes: [{ ...vpc, config: null as unknown as GraphNode["config"] }, ...rest],
  })
  const { container } = renderStudio()
  fireEvent.click(
    await screen.findByRole("group", { name: "Create VPC (simulated)" }),
  )
  const inspector = screen.getByRole("complementary", { name: "Inspector" })
  expect(await within(inspector).findByLabelText("Display name")).toHaveValue(
    "Create VPC (simulated)",
  )

  const pane = container.querySelector<HTMLElement>(".react-flow__pane")
  if (!pane) throw new Error("No pane")
  fireEvent.click(pane)
  expect(
    await within(inspector).findByLabelText("Input schema (JSON)"),
  ).toHaveValue(JSON.stringify({ type: "object", properties: {} }, null, 2))
})
```

Replace:

```tsx
  expect(screen.getByText("Saved")).toBeInTheDocument()
  expect(saves).toEqual([{ revision: 3, graph: renamed("Network") }])
})
```

With:

```tsx
  expect(screen.getByText("Saved")).toBeInTheDocument()
  expect(saves).toEqual([{ revision: 3, graph: renamed("Network") }])
})

it("re-entering the studio starts from the saved draft", async () => {
  // The API keeps what the studio saves.
  let current = workflow
  server.use(
    http.get(workflowUrl, () => HttpResponse.json(current)),
    http.put(`${workflowUrl}/draft`, async ({ request }) => {
      const { revision, graph } = (await request.json()) as Workflow["draft"]
      current = { ...current, draft: { revision: revision + 1, graph } }
      return HttpResponse.json({ revision: revision + 1 })
    }),
  )
  const { rerender } = renderStudio()
  await renameVpc("Network")
  expect(
    await screen.findByText("rev 4", {}, { timeout: 2000 }),
  ).toBeInTheDocument()

  rerender(page(<p>Workflow list</p>))
  rerender(studioPage)
  // The editor waits for a fresh draft instead of starting from the cache.
  expect(screen.getByText("Loading workflow…")).toBeInTheDocument()
  expect(
    await screen.findByRole("group", { name: "Network" }),
  ).toBeInTheDocument()
  expect(screen.getByText("rev 4")).toBeInTheDocument()
})
```

Replace:

```tsx
    ["validate", { graph: renamed("Network") }],
  ])
  expect(screen.getByText("rev 4")).toBeInTheDocument()
})
```

With:

```tsx
    ["validate", { graph: renamed("Network") }],
  ])
  expect(screen.getByText("rev 4")).toBeInTheDocument()
})

it("Validate retries a save that failed", async () => {
  const requests: unknown[] = []
  let failing = true
  server.use(
    http.put(`${workflowUrl}/draft`, async ({ request }) => {
      requests.push(["save", await request.json()])
      if (!failing) return HttpResponse.json({ revision: 4 })
      failing = false
      return new HttpResponse(null, { status: 502 })
    }),
    http.post(`${workflowUrl}/validate`, async ({ request }) => {
      requests.push(["validate", await request.json()])
      return HttpResponse.json({ valid: true, diagnostics: [] })
    }),
  )
  renderStudio()
  await renameVpc("Network")
  expect(
    await screen.findByRole("alert", {}, { timeout: 2000 }),
  ).toHaveTextContent("The draft could not be saved (unknown).")

  await userEvent.click(screen.getByRole("button", { name: "Validate" }))
  expect(await screen.findByText("No problems found.")).toBeInTheDocument()
  expect(requests).toEqual([
    ["save", { revision: 3, graph: renamed("Network") }],
    ["save", { revision: 3, graph: renamed("Network") }],
    ["validate", { graph: renamed("Network") }],
  ])
  expect(screen.getByText("Saved")).toBeInTheDocument()
  expect(screen.getByText("rev 4")).toBeInTheDocument()
})

it("Validate says so when the draft still cannot be saved", async () => {
  const requests: string[] = []
  server.use(
    http.put(`${workflowUrl}/draft`, () => {
      requests.push("save")
      return problem(413, "payload_too_large")
    }),
    http.post(`${workflowUrl}/validate`, () => {
      requests.push("validate")
      return HttpResponse.json({ valid: true, diagnostics: [] })
    }),
  )
  renderStudio()
  await renameVpc("Network")
  expect(
    await screen.findByRole("alert", {}, { timeout: 2000 }),
  ).toHaveTextContent("The draft could not be saved (payload_too_large).")

  await userEvent.click(screen.getByRole("button", { name: "Validate" }))
  expect(
    await screen.findByText("Save the draft first: the last save failed."),
  ).toBeInTheDocument()
  expect(requests).toEqual(["save", "save"])
})
```

- [ ] **Step 4: Write the failing session-check test**

Replace the contents of `apps/web/src/views/auth-views.test.tsx` with:

```tsx
import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { http, HttpResponse } from "msw/http"
import { expect, it } from "vitest"

import { AuthGate } from "@/auth/auth-gate"
import { renderWithProviders, router } from "@/test/render"
import { server } from "@/test/server"
import { AcceptInvitationView } from "@/views/accept-invitation-view"
import { FirstOrganizationView } from "@/views/first-organization-view"
import { LoginView } from "@/views/login-view"
import { RegisterView } from "@/views/register-view"

function signedOutOnce() {
  server.use(
    http.get(
      "/api/v1/auth/me",
      () => new HttpResponse(null, { status: 401 }),
      { once: true },
    ),
  )
}

it("sends an anonymous user to login and remembers the page", async () => {
  signedOutOnce()
  renderWithProviders(
    <AuthGate>
      <AcceptInvitationView />
    </AuthGate>,
    { route: "/invitations/accept#token=invite-once" },
  )
  await waitFor(() => expect(router.replace).toHaveBeenCalledWith("/login"))
  expect(sessionStorage.getItem("corqty.returnTo")).toBe(
    "/invitations/accept#token=invite-once",
  )
})

it("offers a retry instead of the login page when the session check fails", async () => {
  server.use(
    http.get(
      "/api/v1/auth/me",
      () => HttpResponse.json({ code: "internal" }, { status: 500 }),
      { once: true },
    ),
  )
  renderWithProviders(
    <AuthGate>
      <p>Signed in</p>
    </AuthGate>,
  )
  expect(
    await screen.findByText("We could not check your session."),
  ).toBeInTheDocument()
  expect(router.replace).not.toHaveBeenCalled()

  await userEvent.click(screen.getByRole("button", { name: "Retry" }))
  expect(await screen.findByText("Signed in")).toBeInTheDocument()
  expect(router.replace).not.toHaveBeenCalled()
})

it("submits registration and returns to the remembered page", async () => {
  sessionStorage.setItem(
    "corqty.returnTo",
    "/invitations/accept#token=invite-once",
  )
  signedOutOnce()
  let registered: unknown
  server.use(
    http.post("/api/v1/auth/register", async ({ request }) => {
      registered = await request.json()
      return HttpResponse.json({ ok: true }, { status: 201 })
    }),
  )
  renderWithProviders(<RegisterView />, { route: "/register" })
  await userEvent.type(screen.getByLabelText("Email"), "admin@example.com")
  await userEvent.type(
    screen.getByLabelText("Password"),
    "correct horse battery staple",
  )
  await userEvent.type(screen.getByLabelText("Organization"), "Acme")
  await userEvent.click(screen.getByRole("button", { name: "Create account" }))

  await waitFor(() =>
    expect(router.replace).toHaveBeenCalledWith(
      "/invitations/accept#token=invite-once",
    ),
  )
  expect(sessionStorage.getItem("corqty.returnTo")).toBeNull()
  expect(registered).toEqual({
    email: "admin@example.com",
    password: "correct horse battery staple",
    organizationName: "Acme",
  })
})

it("signs in and ignores a return path to another origin", async () => {
  sessionStorage.setItem("corqty.returnTo", "//evil.example/orgs")
  signedOutOnce()
  server.use(
    http.post("/api/v1/auth/login", () => HttpResponse.json({ ok: true })),
  )
  renderWithProviders(<LoginView />, { route: "/login" })
  await userEvent.type(screen.getByLabelText("Email"), "admin@example.com")
  await userEvent.type(
    screen.getByLabelText("Password"),
    "correct horse battery staple",
  )
  await userEvent.click(screen.getByRole("button", { name: "Sign in" }))

  await waitFor(() => expect(router.replace).toHaveBeenCalledWith("/"))
  expect(sessionStorage.getItem("corqty.returnTo")).toBeNull()
})

it("opens the first organization from the home page", async () => {
  renderWithProviders(
    <AuthGate>
      <FirstOrganizationView />
    </AuthGate>,
  )
  await waitFor(() =>
    expect(router.replace).toHaveBeenCalledWith("/orgs/org-1"),
  )
})
```

- [ ] **Step 5: Run the tests and verify RED**

Run:

```text
pnpm web:test
```

Expected: `Test Files  3 failed | 9 passed (12)` and `Tests  8 failed | 79 passed (87)`. All 79 tests that
existed before this task still pass, including with the new render wrapper. The failures are:
- `flush also waits for a save chained behind the one in flight` fails with `AssertionError: expected {
  revision: 4, graph: { …(5) } } to deeply equal { revision: 5, graph: { …(5) } }`.
- `each save updates the cached workflow, including the save on leaving` fails with `AssertionError: expected
  { id: 'wf-2', …(3) } to deeply equal { id: 'wf-2', …(3) }`. The cache still shows `"displayName": "Create
  VPC (simulated)"`.
- `asks before unloading while an edit waits or saves` fails with `AssertionError: expected false to be true`.
- `offers a retry instead of the login page when the session check fails` fails with
  `TestingLibraryElementError: Unable to find an element with the text: We could not check your session.`
- `opens a draft whose input schema and node config are not objects` fails with `TypeError: Cannot read
  properties of null (reading 'mode')` at `src/studio/inspector.tsx:240:29` (`value={node.config[name]}`).
- `re-entering the studio starts from the saved draft` fails with `Unable to find an element with the text:
  Loading workflow…`, because the editor opens at once from the cached revision 3.
- `Validate retries a save that failed` and `Validate says so when the draft still cannot be saved` both fail
  with `TestingLibraryElementError: Unable to find role="alert"`.

On Node 25 or newer, each worker may print `ExperimentalWarning: localStorage is not available because
--localstorage-file was not provided.`; this is harmless.

- [ ] **Step 6: Normalize drafts on load**

`isRecord` moves from `widgets.tsx` to `graph-model.ts`, so that the pure model can use it too.

In `apps/web/src/studio/graph-model.ts`, apply these replacements in order:

Replace:

```ts
export function isSimulated(key: string) {
  return key.startsWith("corqty.simulated")
}
```

With:

```ts
export function isSimulated(key: string) {
  return key.startsWith("corqty.simulated")
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
}
```

Replace:

```ts
export function toFlow(graph: GraphSnapshot, definitions: NodeDefinition[]) {
```

With:

```ts
// Draft saves accept any JSON as the input schema and as node config (D13);
// the studio edits them as objects.
export function normalizeGraph(graph: GraphSnapshot): GraphSnapshot {
  return {
    ...graph,
    workflowInputSchema: isRecord(graph.workflowInputSchema)
      ? graph.workflowInputSchema
      : { type: "object", properties: {} },
    nodes: graph.nodes.map((node) =>
      isRecord(node.config) ? node : { ...node, config: {} },
    ),
  }
}

export function toFlow(graph: GraphSnapshot, definitions: NodeDefinition[]) {
```

In `apps/web/src/studio/widgets.tsx`, apply these replacements in order:

Replace:

```tsx
import { Textarea } from "@/components/ui/textarea"
```

With:

```tsx
import { Textarea } from "@/components/ui/textarea"
import { isRecord } from "@/studio/graph-model"
```

Replace:

```tsx
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
}

function stringEnum(schema: unknown): string[] | undefined {
```

With:

```tsx
function stringEnum(schema: unknown): string[] | undefined {
```

The canvas normalizes the loaded draft before building the flow from it. Its first report to the autosave is
therefore the normalized graph, which becomes the saved baseline, so opening a draft does not trigger a save.

In `apps/web/src/studio/studio-canvas.tsx`, apply these replacements in order:

Replace:

```tsx
  newNodeFromDefinition,
  refusalMessage,
```

With:

```tsx
  newNodeFromDefinition,
  normalizeGraph,
  refusalMessage,
```

Replace:

```tsx
  const [initial] = useState(() => toFlow(graph, definitions))
  // XYFlow holds the nodes and edges; this holds the rest of the snapshot.
  const [snapshot, setSnapshot] = useState(graph)
```

With:

```tsx
  // XYFlow holds the nodes and edges; this holds the rest of the snapshot.
  const [snapshot, setSnapshot] = useState(() => normalizeGraph(graph))
  const [initial] = useState(() => toFlow(snapshot, definitions))
```

- [ ] **Step 7: Update the cache, retry failed saves and guard unloads in the autosave**

`createAutosave` takes an `onSaved` callback, which the hook wires to `setQueryData` on the workflow key. The
`busy()` test that drives the `saving` status also drives the `beforeunload` listener. The listener shares an
effect with the unmount flush.

In `apps/web/src/studio/use-draft-autosave.ts`, apply these replacements in order:

Replace:

```ts
import { useEffect, useState } from "react"

import { ApiProblem, api } from "@/api/client"
import type { GraphSnapshot } from "@/api/types"
```

With:

```ts
import { useQueryClient } from "@tanstack/react-query"
import { useEffect, useState } from "react"

import { ApiProblem, api } from "@/api/client"
import { queryKeys } from "@/api/queries"
import type { GraphSnapshot, Workflow } from "@/api/types"
```

Replace:

```ts
  report: (state: Report) => void,
) {
```

With:

```ts
  report: (state: Report) => void,
  onSaved: (draft: SavedDraft) => void,
) {
```

Replace:

```ts
  function update() {
    const busy = timer !== undefined || queued > 0
    report({
      status: halted ?? (busy ? "saving" : "saved"),
```

With:

```ts
  // An edit waits for its timer, or a save is in flight.
  function busy() {
    return timer !== undefined || queued > 0
  }

  function update() {
    report({
      status: halted ?? (busy() ? "saving" : "saved"),
```

Replace:

```ts
      revision = result.revision
      saved = sending
    } catch (error) {
```

With:

```ts
      revision = result.revision
      saved = sending
      onSaved({ revision, graph: saved.graph })
    } catch (error) {
```

Replace:

```ts
  return {
    edit(graph: GraphSnapshot, readOnly: boolean) {
```

With:

```ts
  return {
    busy,

    edit(graph: GraphSnapshot, readOnly: boolean) {
```

Replace:

```ts
    // Saves pending edits now and waits for every save. Resolves to the saved
    // draft, or null when it could not be saved.
    async flush(): Promise<SavedDraft | null> {
      if (timer !== undefined) {
        clearTimeout(timer)
        timer = undefined
        enqueue()
      }
      await queue
      return halted ? null : { revision, graph: saved.graph }
    },
```

With:

```ts
    // Saves pending edits now, retrying a save that failed, and waits until
    // no save is due or in flight. Resolves to the saved draft, or null when
    // it could not be saved.
    async flush(): Promise<SavedDraft | null> {
      if (halted === "error") {
        halted = undefined
        enqueue()
      }
      let current: Promise<void>
      do {
        if (timer !== undefined) {
          clearTimeout(timer)
          timer = undefined
          enqueue()
        }
        current = queue
        await current
      } while (current !== queue || timer !== undefined)
      return halted ? null : { revision, graph: saved.graph }
    },
```

Replace:

```ts
// Saves the draft 800 ms after the last edit. A 409 stops saving until the
// studio reloads the draft; any other failure stops it until the next edit.
export function useDraftAutosave(
  organizationId: string,
  workflowId: string,
  draft: SavedDraft,
  readOnly: boolean,
) {
  const [state, setState] = useState<Report>({
```

With:

```ts
// Saves the draft 800 ms after the last edit. A 409 stops saving until the
// studio reloads the draft; any other failure stops it until the next edit or
// flush. Each save also updates the cached workflow, so the studio reopens at
// the saved revision.
export function useDraftAutosave(
  organizationId: string,
  workflowId: string,
  draft: SavedDraft,
  readOnly: boolean,
) {
  const queryClient = useQueryClient()
  const [state, setState] = useState<Report>({
```

Replace:

```ts
      draft,
      setState,
    ),
  )
  // Leaving the studio saves pending edits at once instead of dropping them.
  useEffect(() => () => void autosave.flush(), [autosave])
```

With:

```ts
      draft,
      setState,
      (saved) =>
        queryClient.setQueryData<Workflow>(
          queryKeys.workflow(organizationId, workflowId),
          (workflow) => workflow && { ...workflow, draft: saved },
        ),
    ),
  )
  useEffect(() => {
    // Closing or reloading the page asks first while an edit is unsaved.
    function warn(event: BeforeUnloadEvent) {
      if (autosave.busy()) event.preventDefault()
    }
    window.addEventListener("beforeunload", warn)
    return () => {
      window.removeEventListener("beforeunload", warn)
      // Leaving the studio saves pending edits at once instead of dropping them.
      void autosave.flush()
    }
  }, [autosave])
```

- [ ] **Step 8: Start the studio from a fresh draft and report failed flushes**

In `apps/web/src/studio/studio.tsx`, apply these replacements in order:

Replace:

```tsx
    const saved = await autosave.flush()
    if (saved) await task(saved)
    setBusy(false)
```

With:

```tsx
    const saved = await autosave.flush()
    if (saved) await task(saved)
    else setNotice("Save the draft first: the last save failed.")
    setBusy(false)
```

Replace:

```tsx
          <span className="text-destructive">
            The draft could not be saved ({autosave.errorCode}). The next edit
```

With:

```tsx
          <span role="alert" className="text-destructive">
            The draft could not be saved ({autosave.errorCode}). The next edit
```

Replace:

```tsx
  // A failed background refetch keeps the loaded studio on screen.
  if (!workflow.data || !definitions.data || !blueprints.data) {
```

With:

```tsx
  // The editor starts from a draft fetched on this visit, not from the cache:
  // the draft may have been saved since. Once it has started, a failed
  // background refetch keeps it on screen.
  // ponytail: a save from the last visit that lands after this fetch makes the
  // next edit a conflict, which Reload resolves.
  if (
    !workflow.isFetchedAfterMount ||
    !workflow.data ||
    !definitions.data ||
    !blueprints.data
  ) {
```

- [ ] **Step 9: Split the canvas and the inspector from 1280 px**

In `apps/web/src/studio/studio-canvas.tsx`, apply this replacement:

Replace:

```tsx
    <div className="grid min-h-0 lg:grid-cols-[1fr_20rem]">
```

With:

```tsx
    <div className="grid min-h-0 xl:grid-cols-[1fr_20rem]">
```

In `apps/web/src/studio/inspector.tsx`, apply this replacement:

Replace:

```tsx
      className="overflow-y-auto border-t p-4 lg:border-t-0 lg:border-l"
```

With:

```tsx
      className="overflow-y-auto border-t p-4 xl:border-t-0 xl:border-l"
```

- [ ] **Step 10: Keep the app shell when an organization page crashes**

`error.tsx` wraps the organization pages but not their layout, so the sidebar stays. Next 16.4 passes both
`reset` and `retry`. `reset()` re-renders the page without refetching its server payload. That is enough here
because every page is a client view, and a remounted studio fetches a fresh draft anyway.

Create `apps/web/src/app/(app)/orgs/[organizationId]/error.tsx`:

```tsx
"use client"

import { Button } from "@/components/ui/button"

// A page that crashes leaves the app shell around it in place.
export default function OrganizationError({ reset }: { reset: () => void }) {
  return (
    <div className="grid justify-items-start gap-3 p-4 text-sm md:p-6">
      <p role="alert">Something went wrong on this page.</p>
      <Button variant="outline" size="sm" onClick={() => reset()}>
        Retry
      </Button>
    </div>
  )
}
```

- [ ] **Step 11: Tell a failed session check apart from signing out**

Replace the contents of `apps/web/src/auth/auth-context.tsx` with:

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

import { ApiProblem, api } from "@/api/client"
import type { Me } from "@/api/types"

interface AuthValue {
  me: Me | null
  loading: boolean
  // The last session check failed for a reason other than 401.
  error: boolean
  refresh: () => Promise<void>
}

const AuthContext = createContext<AuthValue | null>(null)

export function AuthProvider({ children }: { children: ReactNode }) {
  const [me, setMe] = useState<Me | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(false)
  const refresh = useCallback(async () => {
    try {
      setMe(await api<Me>("/auth/me"))
      setError(false)
    } catch (problem) {
      const signedOut = problem instanceof ApiProblem && problem.status === 401
      if (signedOut) setMe(null)
      setError(!signedOut)
    } finally {
      setLoading(false)
    }
  }, [])
  useEffect(() => {
    void refresh()
  }, [refresh])
  const value = useMemo(
    () => ({ me, loading, error, refresh }),
    [me, loading, error, refresh],
  )
  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth(): AuthValue {
  const value = useContext(AuthContext)
  if (!value) throw new Error("useAuth must be used inside AuthProvider")
  return value
}
```

Replace the contents of `apps/web/src/auth/auth-gate.tsx` with:

```tsx
"use client"

import { useRouter } from "next/navigation"
import { useEffect } from "react"
import type { ReactNode } from "react"

import { useAuth } from "@/auth/auth-context"
import { rememberReturnTo } from "@/auth/return-to"
import { Button } from "@/components/ui/button"

export function AuthGate({ children }: { children: ReactNode }) {
  const { me, loading, error, refresh } = useAuth()
  const router = useRouter()

  // Only a 401 means signed out; other failures leave the session unknown.
  useEffect(() => {
    if (loading || me || error) return
    const { pathname, search, hash } = window.location
    rememberReturnTo(`${pathname}${search}${hash}`)
    router.replace("/login")
  }, [loading, me, error, router])

  if (me) return children
  if (error) {
    return (
      <div className="grid justify-items-start gap-3 p-6 text-sm">
        <p role="alert">We could not check your session.</p>
        <Button variant="outline" size="sm" onClick={() => void refresh()}>
          Retry
        </Button>
      </div>
    )
  }
  return <div className="p-6 text-sm text-muted-foreground">Loading…</div>
}
```

- [ ] **Step 12: Verify GREEN**

Run:

```text
pnpm web:test
```

Expected: `Test Files  12 passed (12)` and `Tests  87 passed (87)` with no unhandled errors. That is
use-draft-autosave 10, studio 19 and auth-views 5, plus the 53 tests of the other nine files, unchanged.

- [ ] **Step 13: Run lint and build**

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

Expected: `✓ Compiled successfully`, `Finished TypeScript`, and the same route table as Task 18, with
`ƒ /orgs/[organizationId]/workflows/[workflowId]` still dynamic. It exits 0.

- [ ] **Step 14: Commit**

```text
git add apps/web
git commit -m "fix: keep studio drafts in sync and recover from save failures"
```
