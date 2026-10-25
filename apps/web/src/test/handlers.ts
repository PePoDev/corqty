import { http, HttpResponse } from "msw/http"

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
