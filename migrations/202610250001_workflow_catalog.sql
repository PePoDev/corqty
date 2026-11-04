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
