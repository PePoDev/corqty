import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { http, HttpResponse } from "msw/http"
import { describe, expect, it } from "vitest"

import { App } from "@/app"
import { renderWithProviders } from "@/test/render"
import { server } from "@/test/server"

describe("application authentication", () => {
  it("shows the login page for an anonymous protected route", async () => {
    server.use(
      http.get("/api/v1/auth/me", () => new HttpResponse(null, { status: 401 })),
    )
    renderWithProviders(<App />, { route: "/orgs/acme" })
    expect(
      await screen.findByRole("heading", { name: "Sign in to Corqty" }),
    ).toBeInTheDocument()
  })

  it("submits registration and enters the organization shell", async () => {
    let registered: unknown
    server.use(
      http.get(
        "/api/v1/auth/me",
        () => new HttpResponse(null, { status: 401 }),
        { once: true },
      ),
      http.post("/api/v1/auth/register", async ({ request }) => {
        registered = await request.json()
        return HttpResponse.json({ ok: true }, { status: 201 })
      }),
    )
    renderWithProviders(<App />, { route: "/register" })
    await userEvent.type(screen.getByLabelText("Email"), "admin@example.com")
    await userEvent.type(
      screen.getByLabelText("Password"),
      "correct horse battery staple",
    )
    await userEvent.type(screen.getByLabelText("Organization"), "Acme")
    await userEvent.click(screen.getByRole("button", { name: "Create account" }))

    expect(await screen.findByText("Acme")).toBeInTheDocument()
    expect(registered).toEqual({
      email: "admin@example.com",
      password: "correct horse battery staple",
      organizationName: "Acme",
    })
  })
})
