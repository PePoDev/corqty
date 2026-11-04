import type { Capability } from "@/api/types"
import { useAuth } from "@/auth/auth-context"

export function useCan(organizationId: string, capability: Capability) {
  const { me } = useAuth()
  return (
    me?.organizations
      .find((organization) => organization.organizationId === organizationId)
      ?.capabilities.includes(capability) ?? false
  )
}
