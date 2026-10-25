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
