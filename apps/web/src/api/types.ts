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
