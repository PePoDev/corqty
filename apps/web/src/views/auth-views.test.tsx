import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { http, HttpResponse } from "msw/http"
import { expect, it } from "vitest"

import { AuthGate } from "@/auth/auth-gate"
import { renderWithProviders, router } from "@/test/render"
import { server } from "@/test/server"
import { AcceptInvitationView } from "@/views/accept-invitation-view"
import { FirstOrganizationView } from "@/views/first-organization-view"
import { LoginView } from "@/views/login-view"
import { RegisterView } from "@/views/register-view"

function signedOutOnce() {
  server.use(
    http.get(
      "/api/v1/auth/me",
      () => new HttpResponse(null, { status: 401 }),
      { once: true },
    ),
  )
}

it("sends an anonymous user to login and remembers the page", async () => {
  signedOutOnce()
  renderWithProviders(
    <AuthGate>
      <AcceptInvitationView />
    </AuthGate>,
    { route: "/invitations/accept#token=invite-once" },
  )
  await waitFor(() => expect(router.replace).toHaveBeenCalledWith("/login"))
  expect(sessionStorage.getItem("corqty.returnTo")).toBe(
    "/invitations/accept#token=invite-once",
  )
})

it("offers a retry instead of the login page when the session check fails", async () => {
  server.use(
    http.get(
      "/api/v1/auth/me",
      () => HttpResponse.json({ code: "internal" }, { status: 500 }),
      { once: true },
    ),
  )
  renderWithProviders(
    <AuthGate>
      <p>Signed in</p>
    </AuthGate>,
  )
  expect(
    await screen.findByText("We could not check your session."),
  ).toBeInTheDocument()
  expect(router.replace).not.toHaveBeenCalled()

  await userEvent.click(screen.getByRole("button", { name: "Retry" }))
  expect(await screen.findByText("Signed in")).toBeInTheDocument()
  expect(router.replace).not.toHaveBeenCalled()
})

it("submits registration and returns to the remembered page", async () => {
  sessionStorage.setItem(
    "corqty.returnTo",
    "/invitations/accept#token=invite-once",
  )
  signedOutOnce()
  let registered: unknown
  server.use(
    http.post("/api/v1/auth/register", async ({ request }) => {
      registered = await request.json()
      return HttpResponse.json({ ok: true }, { status: 201 })
    }),
  )
  renderWithProviders(<RegisterView />, { route: "/register" })
  await userEvent.type(screen.getByLabelText("Email"), "admin@example.com")
  await userEvent.type(
    screen.getByLabelText("Password"),
    "correct horse battery staple",
  )
  await userEvent.type(screen.getByLabelText("Organization"), "Acme")
  await userEvent.click(screen.getByRole("button", { name: "Create account" }))

  await waitFor(() =>
    expect(router.replace).toHaveBeenCalledWith(
      "/invitations/accept#token=invite-once",
    ),
  )
  expect(sessionStorage.getItem("corqty.returnTo")).toBeNull()
  expect(registered).toEqual({
    email: "admin@example.com",
    password: "correct horse battery staple",
    organizationName: "Acme",
  })
})

it("signs in and ignores a return path to another origin", async () => {
  sessionStorage.setItem("corqty.returnTo", "//evil.example/orgs")
  signedOutOnce()
  server.use(
    http.post("/api/v1/auth/login", () => HttpResponse.json({ ok: true })),
  )
  renderWithProviders(<LoginView />, { route: "/login" })
  await userEvent.type(screen.getByLabelText("Email"), "admin@example.com")
  await userEvent.type(
    screen.getByLabelText("Password"),
    "correct horse battery staple",
  )
  await userEvent.click(screen.getByRole("button", { name: "Sign in" }))

  await waitFor(() => expect(router.replace).toHaveBeenCalledWith("/"))
  expect(sessionStorage.getItem("corqty.returnTo")).toBeNull()
})

it("opens the first organization from the home page", async () => {
  renderWithProviders(
    <AuthGate>
      <FirstOrganizationView />
    </AuthGate>,
  )
  await waitFor(() =>
    expect(router.replace).toHaveBeenCalledWith("/orgs/org-1"),
  )
})
