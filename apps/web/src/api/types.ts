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

export interface OrganizationSettings {
  // False while the deployment keeps shell off (CORQTY_SHELL_EXECUTOR).
  shellAvailable: boolean
  shellEnabled: boolean
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

export interface WorkflowVersion extends VersionSummary {
  graph: GraphSnapshot
}

// Run payloads list every field; a missing value is null.
export type RunStatus =
  | "queued"
  | "running"
  | "succeeded"
  | "failed"
  | "canceled"

export type NodeRunState =
  | "blocked"
  | "queued"
  | "running"
  | "retry_wait"
  | "succeeded"
  | "failed"
  | "canceled"
  | "skipped"

export interface RunSummary {
  id: string
  workflowId: string
  version: number
  status: RunStatus
  triggerSource: string
  createdAt: string
  startedAt: string | null
  finishedAt: string | null
  cancelRequested: boolean
}

export interface NodeRun {
  nodeId: string
  state: NodeRunState
  attempt: number
  maxAttempts: number
  executor: string
  queueReason: "waiting_for_compatible_worker" | null
  input: Record<string, unknown> | null
  output: Record<string, unknown> | null
  error: { code: string; message: string; retryable: boolean } | null
  startedAt: string | null
  finishedAt: string | null
}

export type Run = RunSummary & {
  workflowName: string
  input: Record<string, unknown>
  lastEventSeq: number
  graph: GraphSnapshot
  nodes: NodeRun[]
}

// Event payloads omit absent keys: a node.state carries `error` when the node
// failed or waits to retry, `output` when it succeeded, and `input` once a
// finished attempt resolved it.
export type RunEvent =
  | { seq: number; type: "run.state"; state: RunStatus }
  | {
      seq: number
      type: "node.state"
      nodeId: string
      state: NodeRunState
      attempt: number
      error?: NonNullable<NodeRun["error"]>
      input?: Record<string, unknown>
      output?: Record<string, unknown>
    }
  | {
      seq: number
      type: "node.log"
      nodeId: string
      stream: "stdout" | "stderr" | "system"
      text: string
    }
  | { seq: number; type: "node.log_truncated"; nodeId: string }
