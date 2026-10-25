import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"

import { api } from "@/api/client"
import type { AuditEvent, Invitation, Member, Role } from "@/api/types"

export const queryKeys = {
  organization: (organizationId: string) => ["orgs", organizationId] as const,
  members: (organizationId: string) =>
    ["orgs", organizationId, "members"] as const,
  auditEvents: (organizationId: string) =>
    ["orgs", organizationId, "audit-events"] as const,
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

// Member writes also append audit events: refresh the whole organization.
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
