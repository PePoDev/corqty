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
