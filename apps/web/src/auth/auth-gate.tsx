"use client"

import { useRouter } from "next/navigation"
import { useEffect } from "react"
import type { ReactNode } from "react"

import { useAuth } from "@/auth/auth-context"
import { rememberReturnTo } from "@/auth/return-to"

export function AuthGate({ children }: { children: ReactNode }) {
  const { me, loading } = useAuth()
  const router = useRouter()

  useEffect(() => {
    if (loading || me) return
    const { pathname, search, hash } = window.location
    rememberReturnTo(`${pathname}${search}${hash}`)
    router.replace("/login")
  }, [loading, me, router])

  if (!me) {
    return <div className="p-6 text-sm text-muted-foreground">Loading…</div>
  }
  return children
}
