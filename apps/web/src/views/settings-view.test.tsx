import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { http, HttpResponse } from "msw/http"
import { expect, it } from "vitest"

import { AuthGate } from "@/auth/auth-gate"
import { AppShell } from "@/layout/app-shell"
import { meAs } from "@/test/handlers"
import { renderWithProviders } from "@/test/render"
import { server } from "@/test/server"
import { SettingsView } from "@/views/settings-view"

const settingsUrl = "/api/v1/orgs/:organizationId/settings"

// Mirrors the (app) and orgs/[organizationId] layouts around the page.
function renderSettings() {
  return renderWithProviders(
    <AuthGate>
      <AppShell organizationId="org-1">
        <SettingsView organizationId="org-1" />
      </AppShell>
    </AuthGate>,
    { route: "/orgs/org-1/settings" },
  )
}

it("an admin turns the shell executor on and off below the trusted-user warning", async () => {
  let shellEnabled = false
  const changes: unknown[] = []
  server.use(
    http.get(settingsUrl, () =>
      HttpResponse.json({ shellAvailable: true, shellEnabled }),
    ),
    http.put(`${settingsUrl}/shell`, async ({ request }) => {
      const change = (await request.json()) as { enabled: boolean }
      changes.push(change)
      shellEnabled = change.enabled
      return HttpResponse.json({ shellAvailable: true, shellEnabled })
    }),
  )
  renderSettings()
  const toggle = await screen.findByRole("checkbox", {
    name: "Enable the shell executor",
  })
  expect(screen.getByRole("link", { name: "Settings" })).toHaveAttribute(
    "aria-current",
    "page",
  )
  expect(screen.getByRole("alert")).toHaveTextContent(
    "The shell executor is not a security boundary",
  )
  expect(toggle).not.toBeChecked()

  await userEvent.click(toggle)
  await waitFor(() => expect(toggle).toBeChecked())
  await waitFor(() => expect(toggle).toBeEnabled())
  await userEvent.click(toggle)
  await waitFor(() => expect(toggle).not.toBeChecked())
  expect(changes).toEqual([{ enabled: true }, { enabled: false }])
})

it("an editor sees whether shell is enabled but cannot change it", async () => {
  server.use(
    http.get("/api/v1/auth/me", () => HttpResponse.json(meAs("editor"))),
    http.get(settingsUrl, () =>
      HttpResponse.json({ shellAvailable: true, shellEnabled: true }),
    ),
  )
  renderSettings()
  expect(
    await screen.findByText("The shell executor is enabled."),
  ).toBeInTheDocument()
  expect(screen.getByRole("alert")).toHaveTextContent("Trusted users only")
  expect(screen.queryByRole("checkbox")).not.toBeInTheDocument()
})

it("explains that the deployment turned the shell executor off", async () => {
  server.use(
    http.get(settingsUrl, () =>
      HttpResponse.json({ shellAvailable: false, shellEnabled: true }),
    ),
  )
  renderSettings()
  expect(
    await screen.findByText(
      "The shell executor is turned off for this deployment (CORQTY_SHELL_EXECUTOR).",
    ),
  ).toBeInTheDocument()
  expect(screen.queryByRole("checkbox")).not.toBeInTheDocument()
  expect(
    screen.queryByText("The shell executor is enabled."),
  ).not.toBeInTheDocument()
})

it("says why the shell executor could not be changed", async () => {
  server.use(
    http.put(`${settingsUrl}/shell`, () =>
      HttpResponse.json({ code: "forbidden" }, { status: 403 }),
    ),
  )
  renderSettings()
  const toggle = await screen.findByRole("checkbox", {
    name: "Enable the shell executor",
  })
  await userEvent.click(toggle)
  expect(
    await screen.findByText(
      "The shell executor could not be changed (forbidden).",
    ),
  ).toBeInTheDocument()
  expect(toggle).not.toBeChecked()
})
