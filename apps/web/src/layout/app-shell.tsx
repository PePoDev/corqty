"use client"

import Link from "next/link"
import { usePathname, useRouter } from "next/navigation"
import type { ReactNode } from "react"

import { api } from "@/api/client"
import { useAuth } from "@/auth/auth-context"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Separator } from "@/components/ui/separator"

const links = [
  { label: "Overview", suffix: "" },
  { label: "Members", suffix: "/members" },
  { label: "Audit log", suffix: "/audit" },
]

export function AppShell({
  organizationId,
  children,
}: {
  organizationId: string
  children: ReactNode
}) {
  const { me, refresh } = useAuth()
  const router = useRouter()
  const pathname = usePathname()
  const membership = me?.organizations.find(
    (organization) => organization.organizationId === organizationId,
  )
  if (!me || !membership) {
    return <p className="p-6">Organization not found.</p>
  }

  async function logout() {
    await api("/auth/logout", { method: "POST" })
    // With `me` cleared, the auth gate sends the browser to /login.
    await refresh()
  }

  return (
    <div className="min-h-screen bg-background md:grid md:grid-cols-[220px_1fr]">
      <a
        href="#main-content"
        className="sr-only z-50 rounded-md bg-background px-3 py-2 text-sm shadow focus:not-sr-only focus:fixed focus:top-3 focus:left-3"
      >
        Skip to content
      </a>
      <aside className="flex flex-wrap items-center border-b bg-sidebar p-3 text-sidebar-foreground md:min-h-screen md:flex-nowrap md:flex-col md:items-stretch md:border-r md:border-b-0">
        <div className="flex items-center gap-2 px-2 py-3">
          <span className="grid size-7 place-items-center rounded-md bg-foreground text-xs font-bold text-background">
            C
          </span>
          <span className="font-semibold tracking-tight">Corqty</span>
        </div>
        <select
          aria-label="Organization"
          className="ml-auto h-9 max-w-48 rounded-md border bg-background px-2 text-sm md:ml-0 md:max-w-none"
          value={membership.organizationId}
          onChange={(event) => router.push(`/orgs/${event.target.value}`)}
        >
          {me.organizations.map((organization) => (
            <option
              key={organization.organizationId}
              value={organization.organizationId}
            >
              {organization.name}
            </option>
          ))}
        </select>
        <Separator className="mx-3 hidden md:my-3 md:block" />
        <nav
          aria-label="Organization"
          className="order-3 mt-3 flex basis-full gap-1 overflow-x-auto md:order-none md:mt-0 md:flex-col"
        >
          {links.map((link) => {
            const href = `/orgs/${membership.organizationId}${link.suffix}`
            const active = link.suffix
              ? pathname.startsWith(href)
              : pathname === href
            return (
              <Link
                key={link.label}
                href={href}
                aria-current={active ? "page" : undefined}
                className={`rounded-md px-3 py-2 text-sm ${
                  active
                    ? "bg-sidebar-accent font-medium"
                    : "text-muted-foreground"
                }`}
              >
                {link.label}
              </Link>
            )
          })}
        </nav>
        <div className="ml-2 flex items-center gap-2 md:mt-auto md:ml-0 md:flex-col md:items-stretch md:gap-2 md:border-t md:pt-3">
          <span className="hidden truncate px-2 text-xs text-muted-foreground md:block">
            {me.user.email}
          </span>
          <div className="flex items-center justify-between gap-2 px-2">
            <Badge variant="secondary">{membership.role}</Badge>
            <Button variant="ghost" size="sm" onClick={() => void logout()}>
              Log out
            </Button>
          </div>
        </div>
      </aside>
      <main id="main-content" className="min-w-0">
        {children}
      </main>
    </div>
  )
}
