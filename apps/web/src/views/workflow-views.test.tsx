import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { http, HttpResponse } from "msw/http"
import type { ReactElement } from "react"
import { expect, it } from "vitest"

import { AuthGate } from "@/auth/auth-gate"
import { AppShell } from "@/layout/app-shell"
import { meAs, nodeDefinitions } from "@/test/handlers"
import { renderWithProviders, router } from "@/test/render"
import { server } from "@/test/server"
import { CatalogView } from "@/views/catalog-view"
import { WorkflowsView } from "@/views/workflows-view"

// Mirrors the (app) and orgs/[organizationId] layouts around a page.
function renderInShell(view: ReactElement, route: string) {
  return renderWithProviders(
    <AuthGate>
      <AppShell organizationId="org-1">{view}</AppShell>
    </AuthGate>,
    { route },
  )
}

function signedInAs(role: "editor" | "viewer") {
  server.use(http.get("/api/v1/auth/me", () => HttpResponse.json(meAs(role))))
}

it("lists workflows in the order the API returns them", async () => {
  renderInShell(
    <WorkflowsView organizationId="org-1" />,
    "/orgs/org-1/workflows",
  )
  expect(
    await screen.findByRole("link", { name: "Provision staging" }),
  ).toHaveAttribute("href", "/orgs/org-1/workflows/wf-2")
  const rows = within(screen.getByRole("table")).getAllByRole("row").slice(1)
  expect(rows.map((row) => within(row).getByRole("link").textContent)).toEqual(
    ["Provision staging", "Nightly report"],
  )
  expect(within(rows[0]).getByText("v1")).toBeInTheDocument()
  expect(within(rows[1]).getByText("Not published")).toBeInTheDocument()
  expect(screen.getByRole("link", { name: "Workflows" })).toHaveAttribute(
    "aria-current",
    "page",
  )
})

it("a viewer sees the workflows but cannot create one", async () => {
  signedInAs("viewer")
  renderInShell(
    <WorkflowsView organizationId="org-1" />,
    "/orgs/org-1/workflows",
  )
  expect(
    await screen.findByRole("link", { name: "Provision staging" }),
  ).toBeInTheDocument()
  expect(
    screen.queryByRole("button", { name: "New workflow" }),
  ).not.toBeInTheDocument()
})

it("an editor creates a workflow from a blueprint and opens it", async () => {
  signedInAs("editor")
  let created: unknown
  server.use(
    http.post("/api/v1/orgs/:organizationId/workflows", async ({ request }) => {
      created = await request.json()
      return HttpResponse.json(
        { id: "wf-new", name: "Staging", draft: { revision: 1 } },
        { status: 201 },
      )
    }),
  )
  renderInShell(
    <WorkflowsView organizationId="org-1" />,
    "/orgs/org-1/workflows",
  )
  await userEvent.click(
    await screen.findByRole("button", { name: "New workflow" }),
  )
  await userEvent.type(screen.getByLabelText("Name"), "Staging")
  const simulated = await screen.findByRole("radio", {
    name: /^Simulated infrastructure/,
  })
  const simulatedOption = simulated.closest("label") as HTMLElement
  expect(within(simulatedOption).getByText("Simulated")).toBeInTheDocument()
  expect(
    within(simulatedOption).getByText(/^Simulated: every step uses the mock/),
  ).toBeInTheDocument()
  const dataOption = screen
    .getByRole("radio", { name: /^Data processing/ })
    .closest("label") as HTMLElement
  expect(within(dataOption).queryByText("Simulated")).not.toBeInTheDocument()
  await userEvent.click(simulated)
  await userEvent.click(screen.getByRole("button", { name: "Create workflow" }))
  await waitFor(() =>
    expect(router.push).toHaveBeenCalledWith("/orgs/org-1/workflows/wf-new"),
  )
  expect(created).toEqual({
    name: "Staging",
    blueprintId: "00000000-0000-7000-8000-000000000101",
  })
})

it("the catalog shows the latest definitions and marks simulated entries", async () => {
  server.use(
    http.get("/api/v1/orgs/:organizationId/node-definitions", () =>
      HttpResponse.json({
        items: [
          {
            id: "acme-thing-1",
            key: "acme.thing",
            version: 1,
            displayName: "Old thing",
            description: "",
            category: "Acme",
            deprecated: true,
          },
          {
            id: "acme-thing-2",
            key: "acme.thing",
            version: 2,
            displayName: "Thing",
            description: "Does the thing.",
            category: "Acme",
            deprecated: false,
          },
          ...nodeDefinitions,
        ],
      }),
    ),
  )
  renderInShell(<CatalogView organizationId="org-1" />, "/orgs/org-1/catalog")
  const definitions = await screen.findByRole("table", {
    name: "Node definitions",
  })
  const thing = await within(definitions).findByRole("row", {
    name: /acme\.thing/,
  })
  expect(within(definitions).getAllByRole("row")).toHaveLength(1 + 8)
  expect(within(thing).getByText("Thing")).toBeInTheDocument()
  expect(within(thing).getByText("v2")).toBeInTheDocument()
  expect(within(thing).getByText("v1 deprecated")).toHaveClass(
    "text-muted-foreground",
  )
  expect(within(thing).queryByText("Old thing")).not.toBeInTheDocument()
  for (const definition of nodeDefinitions) {
    const row = within(definitions).getByRole("row", {
      name: new RegExp(definition.key.replaceAll(".", "\\.")),
    })
    expect(within(row).queryAllByText("Simulated")).toHaveLength(
      definition.key.startsWith("corqty.simulated") ? 1 : 0,
    )
  }
  const blueprints = screen.getByRole("table", { name: "Blueprints" })
  const simulated = await within(blueprints).findByRole("row", {
    name: /Simulated infrastructure/,
  })
  expect(within(simulated).getByText("Simulated")).toBeInTheDocument()
  expect(
    within(
      within(blueprints).getByRole("row", { name: /Data processing/ }),
    ).queryByText("Simulated"),
  ).not.toBeInTheDocument()
  expect(screen.getByRole("link", { name: "Catalog" })).toHaveAttribute(
    "aria-current",
    "page",
  )
})
