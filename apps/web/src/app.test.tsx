import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { MemoryRouter } from "react-router-dom"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { App } from "@/app"

describe("application authentication", () => {
  beforeEach(() => {
    vi.restoreAllMocks()
  })

  it("shows the login page for an anonymous protected route", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(null, { status: 401 }),
    )
    render(
      <MemoryRouter initialEntries={["/orgs/acme"]}>
        <App />
      </MemoryRouter>,
    )
    expect(
      await screen.findByRole("heading", { name: "Sign in to Corqty" }),
    ).toBeInTheDocument()
  })

  it("submits registration and enters the organization shell", async () => {
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(new Response(null, { status: 401 }))
      .mockResolvedValueOnce(Response.json({ ok: true }, { status: 201 }))
      .mockResolvedValueOnce(
        Response.json({
          user: { id: "user-1", email: "admin@example.com" },
          organizations: [
            {
              organizationId: "org-1",
              name: "Acme",
              slug: "acme",
              role: "admin",
            },
          ],
        }),
      )
    render(
      <MemoryRouter initialEntries={["/register"]}>
        <App />
      </MemoryRouter>,
    )
    await userEvent.type(screen.getByLabelText("Email"), "admin@example.com")
    await userEvent.type(
      screen.getByLabelText("Password"),
      "correct horse battery staple",
    )
    await userEvent.type(screen.getByLabelText("Organization"), "Acme")
    await userEvent.click(screen.getByRole("button", { name: "Create account" }))

    expect(await screen.findByText("Acme")).toBeInTheDocument()
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/auth/register",
      expect.objectContaining({ method: "POST" }),
    )
  })
})
