import { http, HttpResponse } from "msw/http"

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
  publishedVersion: 1,
  draft: { revision: 3, graph: blueprints[1].graph },
}

// A signed-in admin of org-1 who reads data and saves drafts; tests override
// other writes and failures.
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
  http.put(
    "/api/v1/orgs/:organizationId/workflows/:workflowId/draft",
    async ({ request }) => {
      const { revision } = (await request.json()) as { revision: number }
      return HttpResponse.json({ revision: revision + 1 })
    },
  ),
  http.get("/api/v1/orgs/:organizationId/node-definitions", () =>
    HttpResponse.json({ items: nodeDefinitions }),
  ),
  http.get("/api/v1/orgs/:organizationId/blueprints", () =>
    HttpResponse.json({ items: blueprints }),
  ),
]
