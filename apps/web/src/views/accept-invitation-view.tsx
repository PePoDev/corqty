"use client"

import { useRouter } from "next/navigation"
import { useEffect, useRef, useState } from "react"

import { api } from "@/api/client"
import { useAuth } from "@/auth/auth-context"

interface AcceptedInvitation {
  organizationId: string
  role: string
}

export function AcceptInvitationView() {
  const router = useRouter()
  const { refresh } = useAuth()
  const [error, setError] = useState("")
  const started = useRef(false)

  useEffect(() => {
    if (started.current) return
    started.current = true
    // The token travels in the fragment, so it never reaches a server log.
    const token = new URLSearchParams(window.location.hash.slice(1)).get(
      "token",
    )
    if (!token) {
      setError("This invitation is invalid or expired.")
      return
    }
    void api<AcceptedInvitation>("/invitations/accept", {
      method: "POST",
      body: JSON.stringify({ token }),
    })
      .then(async (accepted) => {
        await refresh()
        router.replace(`/orgs/${accepted.organizationId}`)
      })
      .catch(() => setError("This invitation is invalid or expired."))
  }, [refresh, router])

  return (
    <main className="grid min-h-screen place-items-center p-6">
      <p className="text-sm text-muted-foreground">
        {error || "Accepting invitation…"}
      </p>
    </main>
  )
}
