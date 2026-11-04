"use client"

import { createContext, useContext, type ReactNode } from "react"

import { useSecrets } from "@/api/management"
import type { SecretMetadata } from "@/api/types"

export type SecretCatalog = {
  items: SecretMetadata[]
  loading?: boolean
  error?: boolean
}
export const SecretCatalogContext = createContext<SecretCatalog>({ items: [] })
export const useSecretCatalog = () => useContext(SecretCatalogContext)

export function SecretCatalogProvider({
  organizationId,
  enabled,
  children,
}: {
  organizationId: string
  enabled: boolean
  children: ReactNode
}) {
  const secrets = useSecrets(organizationId, enabled)
  return (
    <SecretCatalogContext.Provider
      value={{
        items: secrets.data?.items ?? [],
        loading: enabled && secrets.isPending,
        error: secrets.isError,
      }}
    >
      {children}
    </SecretCatalogContext.Provider>
  )
}
