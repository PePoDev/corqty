import {
  createEvent,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { http, HttpResponse } from "msw/http"
import { expect, it } from "vitest"

import type { GraphSnapshot } from "@/api/types"
import { AuthGate } from "@/auth/auth-gate"
import { AppShell } from "@/layout/app-shell"
import { Studio } from "@/studio/studio"
// Loaded up front so next/dynamic resolves within the findBy timeouts even
// on a cold start.
import "@/studio/studio-canvas"
import { blueprints, meAs, nodeDefinitions, workflow } from "@/test/handlers"
import { renderWithProviders } from "@/test/render"
import { server } from "@/test/server"

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/
const mockTask = nodeDefinitions[2] // corqty.mock.task
const infrastructure = blueprints[1] // corqty.simulated-infrastructure

// Mirrors the (app) and orgs/[organizationId] layouts around the studio page.
function renderStudio() {
  return renderWithProviders(
    <AuthGate>
      <AppShell organizationId="org-1">
        <Studio organizationId="org-1" workflowId="wf-2" />
      </AppShell>
    </AuthGate>,
    { route: "/orgs/org-1/workflows/wf-2" },
  )
}

function withDraft(graph: GraphSnapshot) {
  server.use(
    http.get("/api/v1/orgs/:organizationId/workflows/:workflowId", () =>
      HttpResponse.json({ ...workflow, draft: { revision: 1, graph } }),
    ),
  )
}

async function findCanvas() {
  return screen.findByTestId("rf__wrapper")
}

// jsdom has no DataTransfer or DragEvent: hand XYFlow what the palette would
// put on a drag, on a plain event that carries the pointer position.
function drop(
  pane: HTMLElement,
  format: string,
  id: string,
  [clientX, clientY] = [40, 60],
) {
  const dataTransfer = {
    getData: (requested: string) => (requested === format ? id : ""),
    dropEffect: "none",
  }
  fireEvent.dragOver(pane, { dataTransfer })
  const event = createEvent.drop(pane, { dataTransfer })
  Object.assign(event, { clientX, clientY })
  fireEvent(pane, event)
}

function handle(nodeId: string, handleId: string) {
  const found = document.querySelector<HTMLElement>(
    `[data-nodeid="${nodeId}"][data-handleid="${handleId}"]`,
  )
  if (!found) throw new Error(`No handle ${nodeId} ${handleId}`)
  return found
}

// jsdom has no layout either, so XYFlow's hit test is pointed at the target
// handle while the pointer moves next to the target node at `at`.
function dragConnection(
  [sourceNode, sourceHandle]: [string, string],
  [targetNode, targetHandle]: [string, string],
  [clientX, clientY]: [number, number],
) {
  const target = handle(targetNode, targetHandle)
  document.elementFromPoint = () => target
  fireEvent.mouseDown(handle(sourceNode, sourceHandle), { button: 0 })
  fireEvent.mouseMove(document, { clientX, clientY })
  fireEvent.mouseUp(document, { clientX, clientY })
}

it("shows the draft with typed ports on an editable canvas", async () => {
  const { container } = renderStudio()
  const vpc = await screen.findByRole("group", {
    name: "Create VPC (simulated)",
  })
  expect(
    screen.getByRole("heading", { name: "Provision staging" }),
  ).toBeInTheDocument()
  expect(within(vpc).getByText("Simulated")).toBeInTheDocument()
  expect(vpc).toHaveClass("draggable")
  const output = handle("vpc", "out:vpcId")
  expect(output).toHaveClass("source", "connectable")
  expect(output.parentElement).toHaveTextContent(/^vpcIdaws\.vpc-id$/)
  expect(handle("rds", "in:vpcId")).toHaveClass("target", "connectable")
  await waitFor(() =>
    expect(container.querySelectorAll(".react-flow__edge")).toHaveLength(2),
  )
  expect(screen.queryByText("Read-only")).not.toBeInTheDocument()
})

it("the palette lists the latest definitions and blueprints and filters them", async () => {
  server.use(
    http.get("/api/v1/orgs/:organizationId/node-definitions", () =>
      HttpResponse.json({
        items: [
          {
            ...mockTask,
            id: "acme-old",
            key: "acme.old",
            displayName: "Old thing",
            deprecated: true,
          },
          ...nodeDefinitions,
          {
            ...mockTask,
            id: "mock-task-2",
            version: 2,
            displayName: "Mock task v2",
            deprecated: true,
          },
        ],
      }),
    ),
  )
  renderStudio()
  const definitions = await screen.findByRole("list", {
    name: "Node definitions",
  })
  const blueprintList = screen.getByRole("list", { name: "Blueprints" })
  const items = (list: HTMLElement) => within(list).queryAllByRole("listitem")
  expect(items(definitions)).toHaveLength(6)
  expect(within(definitions).getByText("Mock task")).toBeInTheDocument()
  expect(
    within(definitions).queryByText("Mock task v2"),
  ).not.toBeInTheDocument()
  expect(within(definitions).queryByText("Old thing")).not.toBeInTheDocument()
  expect(items(blueprintList)).toHaveLength(2)

  const filter = screen.getByRole("searchbox", { name: "Filter the palette" })
  await userEvent.type(filter, "VPC")
  expect(items(definitions)).toHaveLength(1)
  expect(
    within(definitions).getByText("Create VPC (simulated)"),
  ).toBeInTheDocument()
  expect(items(blueprintList)).toHaveLength(0)

  await userEvent.clear(filter)
  await userEvent.type(filter, "create-rds")
  expect(items(definitions)).toHaveLength(1)
  expect(
    within(definitions).getByText("corqty.simulated.create-rds"),
  ).toBeInTheDocument()

  await userEvent.clear(filter)
  await userEvent.type(filter, "data")
  expect(items(definitions)).toHaveLength(0)
  expect(items(blueprintList)).toHaveLength(1)
  expect(within(blueprintList).getByText("Data processing")).toBeInTheDocument()
})

it("dropping a definition adds a node with a fresh id at the drop point", async () => {
  withDraft({ ...infrastructure.graph, nodes: [], edges: [] })
  renderStudio()
  drop(await findCanvas(), "application/x-corqty-definition", mockTask.id)
  const node = await screen.findByRole("group", { name: "Mock task" })
  expect(node.dataset.id).toMatch(uuid)
  expect(node.style.transform).toBe("translate(40px,60px)")
})

it("dropping a blueprint inserts its nodes and edges with fresh ids", async () => {
  const { container } = renderStudio()
  await screen.findByRole("group", { name: "Create VPC (simulated)" })
  drop(
    await findCanvas(),
    "application/x-corqty-blueprint",
    infrastructure.id,
    [600, 0],
  )
  await waitFor(() =>
    expect(
      screen.getAllByRole("group", { name: "Create VPC (simulated)" }),
    ).toHaveLength(2),
  )
  const ids = [
    ...container.querySelectorAll<HTMLElement>(".react-flow__node"),
  ].map((node) => node.dataset.id)
  expect(ids.slice(0, 3)).toEqual(["vpc", "rds", "deploy"])
  expect(ids.slice(3)).toEqual([
    expect.stringMatching(uuid),
    expect.stringMatching(uuid),
    expect.stringMatching(uuid),
  ])
  await waitFor(() =>
    expect(container.querySelectorAll(".react-flow__edge")).toHaveLength(4),
  )
  expect(
    screen.getByRole("group", { name: `Edge from ${ids[3]} to ${ids[4]}` }),
  ).toBeInTheDocument()
})

it("connects compatible ports and announces refused connections", async () => {
  withDraft({ ...infrastructure.graph, edges: [] })
  const { container } = renderStudio()
  const deploy = await screen.findByRole("group", {
    name: "Deploy service (simulated)",
  })
  await waitFor(() => expect(deploy).toBeVisible())
  const status = screen.getByRole("status")

  dragConnection(["rds", "out:endpoint"], ["deploy", "in:vpcId"], [300, 100])
  expect(status).toHaveTextContent(
    "Incompatible types: core.string -> aws.vpc-id",
  )
  expect(container.querySelectorAll(".react-flow__edge")).toHaveLength(0)

  dragConnection(["vpc", "out:vpcId"], ["deploy", "in:vpcId"], [300, 100])
  await waitFor(() =>
    expect(container.querySelectorAll(".react-flow__edge")).toHaveLength(1),
  )
  expect(status).toBeEmptyDOMElement()
})

it("the inspector edits the selected node, or the workflow inputs when none is", async () => {
  const { container } = renderStudio()
  const vpc = await screen.findByRole("group", {
    name: "Create VPC (simulated)",
  })
  const inspector = screen.getByRole("complementary", { name: "Inspector" })
  expect(within(inspector).getByLabelText("Input schema (JSON)")).toHaveValue(
    JSON.stringify(infrastructure.graph.workflowInputSchema, null, 2),
  )

  fireEvent.click(vpc)
  const name = await within(inspector).findByLabelText("Display name")
  expect(name).toHaveValue("Create VPC (simulated)")
  // Backspace edits the field; it does not delete the selected node.
  await userEvent.clear(name)
  await userEvent.type(name, "Networks{Backspace}")
  expect(screen.getByRole("group", { name: "Network" })).toBeInTheDocument()
  expect(container.querySelectorAll(".react-flow__node")).toHaveLength(3)

  fireEvent.click(screen.getByRole("group", { name: "Create RDS (simulated)" }))
  expect(
    await within(inspector).findByText("Connected from Network.vpcId"),
  ).toBeInTheDocument()

  const pane = container.querySelector<HTMLElement>(".react-flow__pane")
  if (!pane) throw new Error("No pane")
  fireEvent.click(pane)
  expect(
    await within(inspector).findByLabelText("Input schema (JSON)"),
  ).toBeInTheDocument()
})

it.each([
  {
    role: "viewer" as const,
    width: 1280,
    reason: "Your role can view this workflow but not edit it.",
  },
  {
    role: "admin" as const,
    width: 800,
    reason: "Editing needs a screen at least 1024 pixels wide.",
  },
])(
  "the canvas is read-only for $role at $width px",
  async ({ role, width, reason }) => {
    server.use(
      http.get("/api/v1/auth/me", () => HttpResponse.json(meAs(role))),
    )
    Object.assign(window, { innerWidth: width })
    const { container } = renderStudio()
    const vpc = await screen.findByRole("group", {
      name: "Create VPC (simulated)",
    })
    expect(screen.getByText("Read-only")).toBeInTheDocument()
    expect(screen.getByText(reason)).toBeInTheDocument()
    expect(
      screen.queryByRole("complementary", { name: "Palette" }),
    ).not.toBeInTheDocument()
    expect(vpc).not.toHaveClass("draggable")
    expect(handle("vpc", "out:vpcId")).not.toHaveClass("connectable")
    drop(await findCanvas(), "application/x-corqty-definition", mockTask.id)
    expect(container.querySelectorAll(".react-flow__node")).toHaveLength(3)

    // The inspector shows the selected node but cannot change it.
    fireEvent.click(vpc)
    expect(await screen.findByLabelText("Display name")).toBeDisabled()
    expect(screen.getByLabelText("Max attempts")).toBeDisabled()
  },
)
