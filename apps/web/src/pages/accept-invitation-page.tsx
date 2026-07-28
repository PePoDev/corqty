import { useEffect, useRef, useState } from "react"
import { useLocation, useNavigate } from "react-router-dom"

import { api } from "@/api/client"
import { useAuth } from "@/auth/auth-context"

interface AcceptedInvitation {
  organizationId: string
  role: string
}

export function AcceptInvitationPage() {
  const location = useLocation()
  const navigate = useNavigate()
  const { refresh } = useAuth()
  const [error, setError] = useState("")
  const started = useRef(false)

  useEffect(() => {
    if (started.current) return
    started.current = true
    const token = new URLSearchParams(location.hash.slice(1)).get("token")
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
        navigate(`/orgs/${accepted.organizationId}`, { replace: true })
      })
      .catch(() => setError("This invitation is invalid or expired."))
  }, [location.hash, navigate, refresh])

  return (
    <main className="grid min-h-screen place-items-center p-6">
      <p className="text-sm text-muted-foreground">
        {error || "Accepting invitation…"}
      </p>
    </main>
  )
}
