import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { http, HttpResponse } from "msw/http"
import type { ReactElement } from "react"
import { expect, it, vi } from "vitest"

import { AuthGate } from "@/auth/auth-gate"
import { AppShell } from "@/layout/app-shell"
import { meAs } from "@/test/handlers"
import { renderWithProviders, router } from "@/test/render"
import { server } from "@/test/server"
import { AcceptInvitationView } from "@/views/accept-invitation-view"
import { AuditView } from "@/views/audit-view"
import { MembersView } from "@/views/members-view"

// Mirrors the (app) and orgs/[organizationId] layouts around a page.
function renderInShell(view: ReactElement, route: string) {
  return renderWithProviders(
    <AuthGate>
      <AppShell organizationId="org-1">{view}</AppShell>
    </AuthGate>,
    { route },
  )
}

it("admin can create an invitation and sees the copyable token", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined)
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  })
  let invited: unknown
  server.use(
    http.post(
      "/api/v1/orgs/:organizationId/invitations",
      async ({ request }) => {
        invited = await request.json()
        return HttpResponse.json(
          { token: "invite-once", expiresAt: "2026-08-04T00:00:00Z" },
          { status: 201 },
        )
      },
    ),
  )
  renderInShell(<MembersView organizationId="org-1" />, "/orgs/org-1/members")
  await userEvent.click(
    await screen.findByRole("button", { name: "Invite member" }),
  )
  await userEvent.type(screen.getByLabelText("Email"), "editor@example.com")
  await userEvent.click(
    screen.getByRole("button", { name: "Create invitation" }),
  )
  expect(await screen.findByText("invite-once")).toBeInTheDocument()
  expect(invited).toEqual({ email: "editor@example.com", role: "viewer" })
  await userEvent.click(screen.getByRole("button", { name: "Copy link" }))
  expect(writeText).toHaveBeenCalledWith("invite-once")
})

it("a viewer sees no invite form or member actions", async () => {
  server.use(http.get("/api/v1/auth/me", () => HttpResponse.json(meAs("viewer"))))
  renderInShell(<MembersView organizationId="org-1" />, "/orgs/org-1/members")
  expect(
    await screen.findByRole("cell", { name: "admin@example.com" }),
  ).toBeInTheDocument()
  expect(
    screen.queryByRole("button", { name: "Invite member" }),
  ).not.toBeInTheDocument()
  expect(
    screen.queryByRole("button", { name: "Remove" }),
  ).not.toBeInTheDocument()
  expect(
    screen.queryByRole("combobox", { name: "Role for admin@example.com" }),
  ).not.toBeInTheDocument()
})

it("shows an error when the members cannot be loaded", async () => {
  server.use(
    http.get("/api/v1/orgs/:organizationId/members", () =>
      HttpResponse.json({ code: "internal" }, { status: 500 }),
    ),
  )
  renderInShell(<MembersView organizationId="org-1" />, "/orgs/org-1/members")
  expect(
    await screen.findByText("The members could not be loaded."),
  ).toBeInTheDocument()
})

it("renders administrative audit events", async () => {
  renderInShell(<AuditView organizationId="org-1" />, "/orgs/org-1/audit")
  expect(await screen.findByText("invitation.created")).toBeInTheDocument()
  expect(screen.getByRole("link", { name: "Audit log" })).toHaveAttribute(
    "aria-current",
    "page",
  )
})

it("accepts a fragment invitation and opens its organization", async () => {
  let accepted: unknown
  server.use(
    http.post("/api/v1/invitations/accept", async ({ request }) => {
      accepted = await request.json()
      return HttpResponse.json({ organizationId: "org-1", role: "editor" })
    }),
  )
  renderWithProviders(
    <AuthGate>
      <AcceptInvitationView />
    </AuthGate>,
    { route: "/invitations/accept#token=invite-once" },
  )
  await waitFor(() =>
    expect(router.replace).toHaveBeenCalledWith("/orgs/org-1"),
  )
  expect(accepted).toEqual({ token: "invite-once" })
})
