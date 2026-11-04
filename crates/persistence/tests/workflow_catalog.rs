use std::pin::pin;

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
use sqlx::{PgPool, Postgres, Transaction};
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
