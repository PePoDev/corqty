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
