import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { MemoryRouter } from "react-router-dom"
import { beforeEach, expect, it, vi } from "vitest"

import { App } from "@/app"
import { AuditPage } from "@/pages/audit-page"
import { MembersPage } from "@/pages/members-page"

beforeEach(() => {
  vi.restoreAllMocks()
})

it("admin can create an invitation and sees the copyable token", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined)
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  })
  vi.spyOn(globalThis, "fetch")
    .mockResolvedValueOnce(
      Response.json([
        { userId: "u1", email: "admin@example.com", role: "admin" },
      ]),
    )
    .mockResolvedValueOnce(
      Response.json(
        { token: "invite-once", expiresAt: "2026-08-04T00:00:00Z" },
        { status: 201 },
      ),
    )
  render(
    <MemoryRouter>
      <MembersPage
        organizationId="org-1"
        currentRole="admin"
        currentUserId="u1"
      />
    </MemoryRouter>,
  )
  await screen.findByText("admin@example.com")
  await userEvent.click(screen.getByRole("button", { name: "Invite member" }))
  await userEvent.type(screen.getByLabelText("Email"), "editor@example.com")
  await userEvent.click(
    screen.getByRole("button", { name: "Create invitation" }),
  )
  expect(await screen.findByText("invite-once")).toBeInTheDocument()
  await userEvent.click(screen.getByRole("button", { name: "Copy link" }))
  expect(writeText).toHaveBeenCalledWith("invite-once")
})

it("viewer does not see member mutation actions", async () => {
  vi.spyOn(globalThis, "fetch").mockResolvedValue(
    Response.json([
      { userId: "u1", email: "viewer@example.com", role: "viewer" },
    ]),
  )
  render(
    <MemoryRouter>
      <MembersPage
        organizationId="org-1"
        currentRole="viewer"
        currentUserId="u1"
      />
    </MemoryRouter>,
  )
  await screen.findByText("viewer@example.com")
  expect(
    screen.queryByRole("button", { name: "Invite member" }),
  ).not.toBeInTheDocument()
})

it("renders administrative audit events", async () => {
  vi.spyOn(globalThis, "fetch").mockResolvedValue(
    Response.json({
      items: [
        {
          id: 1,
          actorUserId: "u1",
          action: "invitation.created",
          resourceType: "invitation",
          resourceId: "i1",
          metadata: { role: "viewer" },
          occurredAt: "2026-07-28T00:00:00Z",
        },
      ],
    }),
  )
  render(
    <MemoryRouter>
      <AuditPage organizationId="org-1" />
    </MemoryRouter>,
  )
  expect(await screen.findByText("invitation.created")).toBeInTheDocument()
})

it("accepts a fragment invitation and navigates to its organization", async () => {
  const me = {
    user: { id: "user-1", email: "editor@example.com" },
    organizations: [
      {
        organizationId: "org-1",
        name: "Acme",
        slug: "acme",
        role: "editor",
      },
    ],
  }
  const fetchMock = vi
    .spyOn(globalThis, "fetch")
    .mockResolvedValueOnce(Response.json(me))
    .mockResolvedValueOnce(
      Response.json({ organizationId: "org-1", role: "editor" }),
    )
    .mockResolvedValueOnce(Response.json(me))
  render(
    <MemoryRouter initialEntries={["/invitations/accept#token=invite-once"]}>
      <App />
    </MemoryRouter>,
  )
  await waitFor(() => {
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/invitations/accept",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ token: "invite-once" }),
      }),
    )
  })
  expect(
    await screen.findByRole("heading", { name: "Organization ready" }),
  ).toBeInTheDocument()
})
