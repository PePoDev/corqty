"use client"

import { useRouter } from "next/navigation"
import { useEffect } from "react"
import type { ReactNode } from "react"

import { useAuth } from "@/auth/auth-context"
import { rememberReturnTo } from "@/auth/return-to"
import { Button } from "@/components/ui/button"

export function AuthGate({ children }: { children: ReactNode }) {
  const { me, loading, error, refresh } = useAuth()
  const router = useRouter()

  // Only a 401 means signed out; other failures leave the session unknown.
  useEffect(() => {
    if (loading || me || error) return
    const { pathname, search, hash } = window.location
    rememberReturnTo(`${pathname}${search}${hash}`)
    router.replace("/login")
  }, [loading, me, error, router])

  if (me) return children
  if (error) {
    return (
      <div className="grid justify-items-start gap-3 p-6 text-sm">
        <p role="alert">We could not check your session.</p>
        <Button variant="outline" size="sm" onClick={() => void refresh()}>
          Retry
        </Button>
      </div>
    )
  }
  return <div className="p-6 text-sm text-muted-foreground">Loading…</div>
}
