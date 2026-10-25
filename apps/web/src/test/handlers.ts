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
