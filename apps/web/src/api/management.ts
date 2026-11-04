import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"

import { api } from "@/api/client"
import { queryKeys } from "@/api/queries"
import type {
  PluginManifest,
  RuntimePlugin,
  SecretMetadata,
  ServiceToken,
  Trigger,
  TriggerInput,
} from "@/api/types"

export function useManagementList<T>(
  organizationId: string,
  resource: string,
  enabled = true,
) {
  return useQuery({
    queryKey: [...queryKeys.organization(organizationId), resource],
    queryFn: ({ signal }) =>
      api<{ items: T[] }>(`/orgs/${organizationId}/${resource}`, { signal }),
    enabled,
  })
}

// Keep one-time credentials out of the query cache. Mutation responses are
// reset by the caller immediately after copying them to short-lived state.
export function useManagementWrite<T, V>(
  organizationId: string,
  write: (value: V) => Promise<T>,
) {
  const client = useQueryClient()
  return useMutation({
    gcTime: 0,
    mutationFn: write,
    onSuccess: () =>
      client.invalidateQueries({
        queryKey: queryKeys.organization(organizationId),
      }),
  })
}

export function useSecrets(organizationId: string, enabled = true) {
  return useManagementList<SecretMetadata>(organizationId, "secrets", enabled)
}

export function useSaveSecret(organizationId: string) {
  return useManagementWrite(
    organizationId,
    ({ id, name, value }: { id?: string; name: string; value: string }) =>
      api<SecretMetadata>(
        `/orgs/${organizationId}/secrets${id ? `/${id}` : ""}`,
        {
          method: id ? "PUT" : "POST",
          body: JSON.stringify({ name, value }),
        },
      ),
  )
}

export function useDeleteResource(organizationId: string, resource: string) {
  return useManagementWrite(organizationId, (id: string) =>
    api<void>(`/orgs/${organizationId}/${resource}/${id}`, {
      method: "DELETE",
    }),
  )
}

export function useInstallPlugin(organizationId: string) {
  return useManagementWrite(
    organizationId,
    (body: { manifest: PluginManifest; componentBase64: string }) =>
      api<RuntimePlugin>(`/orgs/${organizationId}/plugins`, {
        method: "POST",
        body: JSON.stringify(body),
      }),
  )
}

export function useApprovePlugin(organizationId: string) {
  return useManagementWrite(organizationId, (plugin: RuntimePlugin) =>
    api<RuntimePlugin>(
      `/orgs/${organizationId}/plugins/${plugin.id}/approval`,
      {
        method: "PUT",
        body: JSON.stringify({
          checksum: plugin.checksum,
          capabilities: plugin.manifest.capabilities,
        }),
      },
    ),
  )
}

export function useSetPluginEnabled(organizationId: string) {
  return useManagementWrite(
    organizationId,
    ({ id, enabled }: { id: string; enabled: boolean }) =>
      api<RuntimePlugin>(`/orgs/${organizationId}/plugins/${id}/enabled`, {
        method: "PUT",
        body: JSON.stringify({ enabled }),
      }),
  )
}

export function useCreateServiceToken(organizationId: string) {
  return useManagementWrite(organizationId, (name: string) =>
    api<ServiceToken & { token: string }>(
      `/orgs/${organizationId}/service-tokens`,
      { method: "POST", body: JSON.stringify({ name }) },
    ),
  )
}

export function useSaveTrigger(organizationId: string) {
  return useManagementWrite(
    organizationId,
    ({ id, ...body }: TriggerInput & { id?: string }) =>
      api<Trigger & { webhookSecret?: string; webhookPath?: string }>(
        `/orgs/${organizationId}/triggers${id ? `/${id}` : ""}`,
        {
          method: id ? "PUT" : "POST",
          body: JSON.stringify(body),
        },
      ),
  )
}
