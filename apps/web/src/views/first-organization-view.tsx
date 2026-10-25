"use client"

import { useRouter } from "next/navigation"
import { useEffect } from "react"

import { useAuth } from "@/auth/auth-context"

export function FirstOrganizationView() {
  const { me } = useAuth()
  const router = useRouter()
  const organizationId = me?.organizations[0]?.organizationId

  useEffect(() => {
    if (organizationId) router.replace(`/orgs/${organizationId}`)
  }, [organizationId, router])

  if (organizationId) return null
  return (
    <p className="p-6 text-sm text-muted-foreground">
      No organization memberships.
    </p>
  )
}
