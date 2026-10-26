import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"

import { api } from "@/api/client"
import type {
  AuditEvent,
  Blueprint,
  Diagnostic,
  GraphSnapshot,
  Invitation,
  Member,
  NodeDefinition,
  Role,
  Run,
  RunSummary,
  VersionSummary,
  Workflow,
  WorkflowSummary,
  WorkflowVersion,
} from "@/api/types"

export const queryKeys = {
  organization: (organizationId: string) => ["orgs", organizationId] as const,
  members: (organizationId: string) =>
    ["orgs", organizationId, "members"] as const,
  auditEvents: (organizationId: string) =>
    ["orgs", organizationId, "audit-events"] as const,
  workflows: (organizationId: string) =>
    ["orgs", organizationId, "workflows"] as const,
  workflow: (organizationId: string, workflowId: string) =>
    ["orgs", organizationId, "workflows", workflowId] as const,
  workflowVersion: (
    organizationId: string,
    workflowId: string,
    version: number,
  ) =>
    [
      "orgs",
      organizationId,
      "workflows",
      workflowId,
      "versions",
      version,
    ] as const,
  runs: (organizationId: string, workflowId: string) =>
    ["orgs", organizationId, "workflows", workflowId, "runs"] as const,
  run: (organizationId: string, runId: string) =>
    ["orgs", organizationId, "runs", runId] as const,
  runEvents: (organizationId: string, runId: string) =>
    ["orgs", organizationId, "runs", runId, "events"] as const,
  nodeDefinitions: (organizationId: string) =>
    ["orgs", organizationId, "node-definitions"] as const,
  blueprints: (organizationId: string) =>
    ["orgs", organizationId, "blueprints"] as const,
}

export function useMembers(organizationId: string) {
  return useQuery({
    queryKey: queryKeys.members(organizationId),
    queryFn: ({ signal }) =>
      api<Member[]>(`/orgs/${organizationId}/members`, { signal }),
  })
}

export function useAuditEvents(organizationId: string) {
  return useQuery({
    queryKey: queryKeys.auditEvents(organizationId),
    queryFn: ({ signal }) =>
      api<{ items: AuditEvent[] }>(
        `/orgs/${organizationId}/audit-events?limit=50`,
        { signal },
      ),
  })
}

// Writes also append audit events: refresh the whole organization.
function useOrganizationMutation<Variables, Result>(
  organizationId: string,
  mutationFn: (variables: Variables) => Promise<Result>,
) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn,
    onSuccess: () =>
      queryClient.invalidateQueries({
        queryKey: queryKeys.organization(organizationId),
      }),
  })
}

export function useCreateInvitation(organizationId: string) {
  return useOrganizationMutation(
    organizationId,
    (body: { email: string; role: Role }) =>
      api<Invitation>(`/orgs/${organizationId}/invitations`, {
        method: "POST",
        body: JSON.stringify(body),
      }),
  )
}

export function useChangeMemberRole(organizationId: string) {
  return useOrganizationMutation(
    organizationId,
    ({ userId, role }: { userId: string; role: Role }) =>
      api<void>(`/orgs/${organizationId}/members/${userId}`, {
        method: "PATCH",
        body: JSON.stringify({ role }),
      }),
  )
}

export function useRemoveMember(organizationId: string) {
  return useOrganizationMutation(organizationId, (userId: string) =>
    api<void>(`/orgs/${organizationId}/members/${userId}`, {
      method: "DELETE",
    }),
  )
}

export function useWorkflows(organizationId: string) {
  return useQuery({
    queryKey: queryKeys.workflows(organizationId),
    queryFn: ({ signal }) =>
      api<{ items: WorkflowSummary[] }>(`/orgs/${organizationId}/workflows`, {
        signal,
      }),
  })
}

export function useWorkflow(organizationId: string, workflowId: string) {
  return useQuery({
    queryKey: queryKeys.workflow(organizationId, workflowId),
    queryFn: ({ signal }) =>
      api<Workflow>(`/orgs/${organizationId}/workflows/${workflowId}`, {
        signal,
      }),
  })
}

export function useNodeDefinitions(organizationId: string) {
  return useQuery({
    queryKey: queryKeys.nodeDefinitions(organizationId),
    queryFn: ({ signal }) =>
      api<{ items: NodeDefinition[] }>(
        `/orgs/${organizationId}/node-definitions`,
        { signal },
      ),
  })
}

export function useBlueprints(organizationId: string) {
  return useQuery({
    queryKey: queryKeys.blueprints(organizationId),
    queryFn: ({ signal }) =>
      api<{ items: Blueprint[] }>(`/orgs/${organizationId}/blueprints`, {
        signal,
      }),
  })
}

export function useCreateWorkflow(organizationId: string) {
  return useOrganizationMutation(
    organizationId,
    (body: { name: string; blueprintId?: string }) =>
      api<Workflow>(`/orgs/${organizationId}/workflows`, {
        method: "POST",
        body: JSON.stringify(body),
      }),
  )
}

// Validation writes nothing, so nothing needs a refresh.
export function useValidateWorkflow(
  organizationId: string,
  workflowId: string,
) {
  return useMutation({
    mutationFn: (graph: GraphSnapshot) =>
      api<{ diagnostics: Diagnostic[] }>(
        `/orgs/${organizationId}/workflows/${workflowId}/validate`,
        { method: "POST", body: JSON.stringify({ graph }) },
      ),
  })
}

export function usePublishWorkflow(organizationId: string, workflowId: string) {
  return useOrganizationMutation(organizationId, (revision: number) =>
    api<VersionSummary>(
      `/orgs/${organizationId}/workflows/${workflowId}/publish`,
      { method: "POST", body: JSON.stringify({ revision }) },
    ),
  )
}

export function useWorkflowVersion(
  organizationId: string,
  workflowId: string,
  version: number,
) {
  return useQuery({
    queryKey: queryKeys.workflowVersion(organizationId, workflowId, version),
    queryFn: ({ signal }) =>
      api<WorkflowVersion>(
        `/orgs/${organizationId}/workflows/${workflowId}/versions/${version}`,
        { signal },
      ),
  })
}

export function useRuns(organizationId: string, workflowId: string) {
  return useQuery({
    queryKey: queryKeys.runs(organizationId, workflowId),
    queryFn: ({ signal }) =>
      api<{ items: RunSummary[] }>(
        `/orgs/${organizationId}/workflows/${workflowId}/runs`,
        { signal },
      ),
  })
}

// Runs the given published version, not the draft.
export function useStartRun(organizationId: string, workflowId: string) {
  return useOrganizationMutation(
    organizationId,
    (body: { input: Record<string, unknown>; version: number }) =>
      api<Run>(`/orgs/${organizationId}/workflows/${workflowId}/runs`, {
        method: "POST",
        body: JSON.stringify(body),
      }),
  )
}

// The run stream also writes this run (runs/use-run-events.ts). A refetch
// never moves it back behind events already applied.
export function useRun(organizationId: string, runId: string) {
  return useQuery({
    queryKey: queryKeys.run(organizationId, runId),
    queryFn: async ({ signal, client, queryKey }) => {
      const fresh = await api<Run>(`/orgs/${organizationId}/runs/${runId}`, {
        signal,
      })
      const cached = client.getQueryData<Run>(queryKey)
      return cached && cached.lastEventSeq > fresh.lastEventSeq ? cached : fresh
    },
  })
}

// The refresh that follows brings the run's cancelRequested flag.
export function useCancelRun(organizationId: string) {
  return useOrganizationMutation(organizationId, (runId: string) =>
    api<RunSummary>(`/orgs/${organizationId}/runs/${runId}/cancel`, {
      method: "POST",
    }),
  )
}

// A rerun starts the source run's version with its input.
export function useRerunRun(organizationId: string) {
  return useOrganizationMutation(organizationId, (runId: string) =>
    api<Run>(`/orgs/${organizationId}/runs/${runId}/rerun`, {
      method: "POST",
      body: JSON.stringify({}),
    }),
  )
}
