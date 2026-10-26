import {
  createEvent,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { http, HttpResponse } from "msw/http"
import { sse } from "msw/sse"
import type { ReactNode } from "react"
import { expect, it, vi } from "vitest"

import type {
  Diagnostic,
  GraphNode,
  GraphSnapshot,
  JsonSchema,
  RunEvent,
  Workflow,
} from "@/api/types"
import { AuthGate } from "@/auth/auth-gate"
import { AppShell } from "@/layout/app-shell"
import { Studio } from "@/studio/studio"
// Loaded up front so next/dynamic resolves within the findBy timeouts even
// on a cold start.
import "@/studio/studio-canvas"
import {
  blueprints,
  meAs,
  nodeDefinitions,
  runs,
  workflow,
} from "@/test/handlers"
import { renderWithProviders, router } from "@/test/render"
import { server } from "@/test/server"

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/
const mockTask = nodeDefinitions[2] // corqty.mock.task
const infrastructure = blueprints[1] // corqty.simulated-infrastructure
const workflowUrl = "/api/v1/orgs/:organizationId/workflows/:workflowId"

// Mirrors the (app) and orgs/[organizationId] layouts around a page.
function page(children: ReactNode) {
  return (
    <AuthGate>
      <AppShell organizationId="org-1">{children}</AppShell>
    </AuthGate>
  )
}

const studioPage = page(<Studio organizationId="org-1" workflowId="wf-2" />)

function renderStudio() {
  return renderWithProviders(studioPage, {
    route: "/orgs/org-1/workflows/wf-2",
  })
}

function withDraft(graph: GraphSnapshot) {
  server.use(
    http.get(workflowUrl, () =>
      HttpResponse.json({ ...workflow, draft: { revision: 1, graph } }),
    ),
  )
}

function problem(status: number, code: string, diagnostics?: Diagnostic[]) {
  return HttpResponse.json(
    { type: "about:blank", title: "Problem", status, code, diagnostics },
    { status },
  )
}

// The draft with its VPC node renamed.
function renamed(displayName: string): GraphSnapshot {
  const [vpc, ...rest] = infrastructure.graph.nodes
  return { ...infrastructure.graph, nodes: [{ ...vpc, displayName }, ...rest] }
}

async function renameVpc(displayName: string) {
  fireEvent.click(
    await screen.findByRole("group", { name: "Create VPC (simulated)" }),
  )
  const name = await screen.findByLabelText("Display name")
  await userEvent.clear(name)
  await userEvent.type(name, displayName)
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
  const breadcrumb = screen.getByRole("navigation", { name: "Breadcrumb" })
  expect(
    within(breadcrumb).getByRole("link", { name: "Workflows" }),
  ).toHaveAttribute("href", "/orgs/org-1/workflows")
  expect(screen.getByText("Saved")).toBeInTheDocument()
  expect(screen.getByText("rev 3")).toBeInTheDocument()
  expect(screen.getByText("v1 published")).toBeInTheDocument()
  expect(screen.getByRole("button", { name: "Validate" })).toBeEnabled()
  expect(screen.getByRole("button", { name: "Publish" })).toBeEnabled()
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

it("the palette offers the shell preset only while shell is enabled", async () => {
  let shellEnabled = false
  let reads = 0
  server.use(
    http.get("/api/v1/orgs/:organizationId/settings", () => {
      reads += 1
      return HttpResponse.json({ shellEnabled })
    }),
  )
  renderStudio()
  const definitions = await screen.findByRole("list", {
    name: "Node definitions",
  })
  await waitFor(() => expect(reads).toBe(1))
  expect(within(definitions).getByText("Mock task")).toBeInTheDocument()
  expect(
    within(definitions).queryByText("Shell command"),
  ).not.toBeInTheDocument()

  // An admin enables shell elsewhere; coming back to the tab refetches.
  shellEnabled = true
  window.dispatchEvent(new Event("visibilitychange"))
  expect(await within(definitions).findByText("Shell command")).toBeVisible()
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

it("opens a draft whose input schema and node config are not objects", async () => {
  // Draft saves accept any JSON in these two fields.
  const [vpc, ...rest] = infrastructure.graph.nodes
  withDraft({
    ...infrastructure.graph,
    workflowInputSchema: null as unknown as JsonSchema,
    nodes: [{ ...vpc, config: null as unknown as GraphNode["config"] }, ...rest],
  })
  const { container } = renderStudio()
  fireEvent.click(
    await screen.findByRole("group", { name: "Create VPC (simulated)" }),
  )
  const inspector = screen.getByRole("complementary", { name: "Inspector" })
  expect(await within(inspector).findByLabelText("Display name")).toHaveValue(
    "Create VPC (simulated)",
  )

  const pane = container.querySelector<HTMLElement>(".react-flow__pane")
  if (!pane) throw new Error("No pane")
  fireEvent.click(pane)
  expect(
    await within(inspector).findByLabelText("Input schema (JSON)"),
  ).toHaveValue(JSON.stringify({ type: "object", properties: {} }, null, 2))
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

it("saves the draft once editing pauses", async () => {
  const saves: unknown[] = []
  server.use(
    http.put(`${workflowUrl}/draft`, async ({ request }) => {
      saves.push(await request.json())
      return HttpResponse.json({ revision: 4 })
    }),
  )
  renderStudio()
  await renameVpc("Network")
  expect(screen.getByText("Saving…")).toBeInTheDocument()
  expect(
    await screen.findByText("rev 4", {}, { timeout: 2000 }),
  ).toBeInTheDocument()
  expect(screen.getByText("Saved")).toBeInTheDocument()
  expect(saves).toEqual([{ revision: 3, graph: renamed("Network") }])
})

it("re-entering the studio starts from the saved draft", async () => {
  // The API keeps what the studio saves.
  let current = workflow
  server.use(
    http.get(workflowUrl, () => HttpResponse.json(current)),
    http.put(`${workflowUrl}/draft`, async ({ request }) => {
      const { revision, graph } = (await request.json()) as Workflow["draft"]
      current = { ...current, draft: { revision: revision + 1, graph } }
      return HttpResponse.json({ revision: revision + 1 })
    }),
  )
  const { rerender } = renderStudio()
  await renameVpc("Network")
  expect(
    await screen.findByText("rev 4", {}, { timeout: 2000 }),
  ).toBeInTheDocument()

  rerender(page(<p>Workflow list</p>))
  rerender(studioPage)
  // The editor waits for a fresh draft instead of starting from the cache.
  expect(screen.getByText("Loading workflow…")).toBeInTheDocument()
  expect(
    await screen.findByRole("group", { name: "Network" }),
  ).toBeInTheDocument()
  expect(screen.getByText("rev 4")).toBeInTheDocument()
})

it("a conflict stops saving until Reload loads the latest draft", async () => {
  server.use(
    http.put(`${workflowUrl}/draft`, () =>
      problem(409, "draft_revision_conflict"),
    ),
  )
  renderStudio()
  await renameVpc("Network")
  const banner = await screen.findByRole("alert", {}, { timeout: 2000 })
  expect(banner).toHaveTextContent("This draft was saved somewhere else.")
  expect(screen.getByText("Conflict")).toBeInTheDocument()
  expect(screen.getByRole("button", { name: "Publish" })).toBeDisabled()

  server.use(
    http.get(workflowUrl, () =>
      HttpResponse.json({
        ...workflow,
        draft: { revision: 7, graph: renamed("Remote network") },
      }),
    ),
  )
  await userEvent.click(within(banner).getByRole("button", { name: "Reload" }))
  expect(
    await screen.findByRole("group", { name: "Remote network" }),
  ).toBeInTheDocument()
  expect(screen.getByText("rev 7")).toBeInTheDocument()
  expect(screen.getByText("Saved")).toBeInTheDocument()
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
})

it("Validate saves pending edits first and checks the saved graph", async () => {
  const requests: unknown[] = []
  server.use(
    http.put(`${workflowUrl}/draft`, async ({ request }) => {
      requests.push(["save", await request.json()])
      return HttpResponse.json({ revision: 4 })
    }),
    http.post(`${workflowUrl}/validate`, async ({ request }) => {
      requests.push(["validate", await request.json()])
      return HttpResponse.json({ valid: true, diagnostics: [] })
    }),
  )
  renderStudio()
  await renameVpc("Network")
  await userEvent.click(screen.getByRole("button", { name: "Validate" }))
  expect(await screen.findByText("No problems found.")).toBeInTheDocument()
  expect(requests).toEqual([
    ["save", { revision: 3, graph: renamed("Network") }],
    ["validate", { graph: renamed("Network") }],
  ])
  expect(screen.getByText("rev 4")).toBeInTheDocument()
})

it("Validate retries a save that failed", async () => {
  const requests: unknown[] = []
  let failing = true
  server.use(
    http.put(`${workflowUrl}/draft`, async ({ request }) => {
      requests.push(["save", await request.json()])
      if (!failing) return HttpResponse.json({ revision: 4 })
      failing = false
      return new HttpResponse(null, { status: 502 })
    }),
    http.post(`${workflowUrl}/validate`, async ({ request }) => {
      requests.push(["validate", await request.json()])
      return HttpResponse.json({ valid: true, diagnostics: [] })
    }),
  )
  renderStudio()
  await renameVpc("Network")
  expect(
    await screen.findByRole("alert", {}, { timeout: 2000 }),
  ).toHaveTextContent("The draft could not be saved (unknown).")

  await userEvent.click(screen.getByRole("button", { name: "Validate" }))
  expect(await screen.findByText("No problems found.")).toBeInTheDocument()
  expect(requests).toEqual([
    ["save", { revision: 3, graph: renamed("Network") }],
    ["save", { revision: 3, graph: renamed("Network") }],
    ["validate", { graph: renamed("Network") }],
  ])
  expect(screen.getByText("Saved")).toBeInTheDocument()
  expect(screen.getByText("rev 4")).toBeInTheDocument()
})

it("Validate says so when the draft still cannot be saved", async () => {
  const requests: string[] = []
  server.use(
    http.put(`${workflowUrl}/draft`, () => {
      requests.push("save")
      return problem(413, "payload_too_large")
    }),
    http.post(`${workflowUrl}/validate`, () => {
      requests.push("validate")
      return HttpResponse.json({ valid: true, diagnostics: [] })
    }),
  )
  renderStudio()
  await renameVpc("Network")
  expect(
    await screen.findByRole("alert", {}, { timeout: 2000 }),
  ).toHaveTextContent("The draft could not be saved (payload_too_large).")

  await userEvent.click(screen.getByRole("button", { name: "Validate" }))
  expect(
    await screen.findByText("Save the draft first: the last save failed."),
  ).toBeInTheDocument()
  expect(requests).toEqual(["save", "save"])
})

it("publishing an invalid draft lists the problems and marks the node and port", async () => {
  withDraft({ ...infrastructure.graph, edges: [] })
  const message = "This input is required: connect an edge or add a binding."
  const published: unknown[] = []
  server.use(
    http.post(`${workflowUrl}/publish`, async ({ request }) => {
      published.push(await request.json())
      return problem(422, "graph_invalid", [
        {
          code: "missing_required_input",
          pointer: "/nodes/1/inputs/vpcId",
          message,
        },
        {
          code: "missing_required_input",
          pointer: "/nodes/2/inputs/vpcId",
          message,
        },
      ])
    }),
  )
  renderStudio()
  const rds = await screen.findByRole("group", {
    name: "Create RDS (simulated)",
  })
  await userEvent.click(screen.getByRole("button", { name: "Publish" }))
  const list = await screen.findByRole("list", { name: "Problems" })
  expect(
    within(list)
      .getAllByRole("listitem")
      .map((item) => item.textContent),
  ).toEqual([
    `Create RDS (simulated) · vpcId ${message} missing_required_input`,
    `Deploy service (simulated) · vpcId ${message} missing_required_input`,
  ])
  expect(screen.getByText("2 problems found.")).toBeInTheDocument()
  expect(published).toEqual([{ revision: 1 }])

  expect(rds.firstElementChild).toHaveAttribute("data-invalid")
  const port = handle("rds", "in:vpcId").parentElement
  expect(port).toHaveAttribute("data-invalid")
  expect(
    within(port as HTMLElement).getByRole("img", { name: "Has a problem" }),
  ).toBeInTheDocument()
  const vpc = screen.getByRole("group", { name: "Create VPC (simulated)" })
  expect(vpc.firstElementChild).not.toHaveAttribute("data-invalid")
})

it("publishing reports the new version, then that the draft already has one", async () => {
  let publishedVersion = 1
  server.use(
    http.get(workflowUrl, () =>
      HttpResponse.json({ ...workflow, publishedVersion }),
    ),
    http.post(`${workflowUrl}/publish`, () => {
      if (publishedVersion === 2) return problem(409, "draft_already_published")
      publishedVersion = 2
      return HttpResponse.json(
        {
          id: "version-2",
          version: 2,
          checksum: "0".repeat(64),
          draftRevision: 3,
          publishedAt: "2026-10-26T00:00:00Z",
        },
        { status: 201 },
      )
    }),
  )
  renderStudio()
  await screen.findByRole("group", { name: "Create VPC (simulated)" })
  expect(screen.getByText("v1 published")).toBeInTheDocument()

  await userEvent.click(screen.getByRole("button", { name: "Publish" }))
  expect(await screen.findByText("Published v2.")).toBeInTheDocument()
  expect(screen.getByText("v2 published")).toBeInTheDocument()

  await userEvent.click(screen.getByRole("button", { name: "Publish" }))
  expect(
    await screen.findByText("Already published as v2."),
  ).toBeInTheDocument()
})

it.each([
  { role: "runner" as const, canRun: true },
  { role: "viewer" as const, canRun: false },
])(
  "a $role cannot validate or publish, and runs only with run_workflows",
  async ({ role, canRun }) => {
    server.use(http.get("/api/v1/auth/me", () => HttpResponse.json(meAs(role))))
    renderStudio()
    await screen.findByRole("group", { name: "Create VPC (simulated)" })
    expect(screen.getByText("rev 3")).toBeInTheDocument()
    expect(
      screen.queryByRole("button", { name: "Validate" }),
    ).not.toBeInTheDocument()
    expect(
      screen.queryByRole("button", { name: "Publish" }),
    ).not.toBeInTheDocument()
    expect(Boolean(screen.queryByRole("button", { name: "Run" }))).toBe(canRun)
  },
)

async function openRunDialog() {
  await userEvent.click(await screen.findByRole("button", { name: "Run" }))
  return screen.findByRole("dialog", { name: "Run workflow" })
}

it("Run starts the published version with its inputs and opens the run", async () => {
  const started: unknown[] = []
  server.use(
    // The published version asks for other inputs than the draft.
    http.get(`${workflowUrl}/versions/:version`, ({ params }) =>
      HttpResponse.json({
        version: Number(params.version),
        graph: {
          ...infrastructure.graph,
          workflowInputSchema: {
            type: "object",
            properties: {
              region: { type: "string", title: "Region", default: "us-east-1" },
              replicas: { type: "integer", title: "Replicas" },
            },
            required: ["region"],
          },
        },
      }),
    ),
    http.post(`${workflowUrl}/runs`, async ({ request }) => {
      started.push(await request.json())
      return HttpResponse.json(
        { ...runs[0], id: "run-3", status: "queued" },
        { status: 201 },
      )
    }),
  )
  renderStudio()
  const dialog = await openRunDialog()
  expect(
    within(dialog).getByText("Starts published version 1 with these inputs."),
  ).toBeInTheDocument()
  const region = await within(dialog).findByLabelText("Region")
  expect(region).toHaveValue("us-east-1")
  expect(within(dialog).getByLabelText("Replicas")).toHaveValue(null)
  await userEvent.clear(region)
  await userEvent.type(region, "eu-west-1")
  await userEvent.click(
    within(dialog).getByRole("button", { name: "Start run" }),
  )

  await waitFor(() =>
    expect(router.push).toHaveBeenCalledWith("/orgs/org-1/runs/run-3"),
  )
  expect(started).toEqual([{ input: { region: "eu-west-1" }, version: 1 }])
})

it("Run shows why a run could not start", async () => {
  server.use(
    http.post(`${workflowUrl}/runs`, () =>
      problem(422, "run_input_invalid", [
        {
          code: "invalid_value",
          pointer: "/region",
          message: "The value is shorter than 1 character.",
        },
      ]),
    ),
  )
  renderStudio()
  const dialog = await openRunDialog()
  const start = await within(dialog).findByRole("button", { name: "Start run" })
  await userEvent.click(start)
  expect(await within(dialog).findByRole("alert")).toHaveTextContent(
    "/region The value is shorter than 1 character.",
  )

  server.use(
    http.post(`${workflowUrl}/runs`, () =>
      problem(409, "workflow_not_published"),
    ),
  )
  await userEvent.click(start)
  await waitFor(() =>
    expect(within(dialog).getByRole("alert")).toHaveTextContent(
      "The run could not be started (workflow_not_published).",
    ),
  )
  expect(router.push).not.toHaveBeenCalled()
})

it("Run waits until the workflow is published", async () => {
  server.use(
    http.get(workflowUrl, () =>
      HttpResponse.json({ ...workflow, publishedVersion: null }),
    ),
  )
  renderStudio()
  const run = await screen.findByRole("button", { name: "Run" })
  expect(run).toBeDisabled()
  expect(run).toHaveAccessibleDescription(
    "Publish the workflow before running it.",
  )
})

it("the runs drawer opens on the recent runs with links to each run", async () => {
  renderStudio()
  const toggle = await screen.findByRole("button", { name: "Runs" })
  const panel = document.getElementById(
    toggle.getAttribute("aria-controls") ?? "",
  )
  if (!panel) throw new Error("No runs panel")
  expect(toggle).toHaveAttribute("aria-expanded", "false")
  expect(panel).not.toBeVisible()

  await userEvent.click(toggle)
  expect(toggle).toHaveAttribute("aria-expanded", "true")
  expect(panel).toBeVisible()
  expect(
    within(panel).getByRole("tab", { name: "Recent runs" }),
  ).toHaveAttribute("aria-selected", "true")
  const rows = (await within(panel).findAllByRole("row")).slice(1)
  expect(
    rows.map((row) => within(row).getByRole("link").getAttribute("href")),
  ).toEqual(["/orgs/org-1/runs/run-2", "/orgs/org-1/runs/run-1"])
  expect(within(rows[0]).getByText("Running")).toBeInTheDocument()
  expect(within(rows[1]).getByText("Succeeded")).toBeInTheDocument()
  expect(within(rows[1]).getByText("v1")).toBeInTheDocument()

  await userEvent.click(toggle)
  expect(panel).not.toBeVisible()
})

it("the read-only studio on a narrow screen still lists the recent runs", async () => {
  Object.assign(window, { innerWidth: 800 })
  renderStudio()
  expect(
    await screen.findByText(
      "Editing needs a screen at least 1024 pixels wide.",
    ),
  ).toBeInTheDocument()
  await userEvent.click(screen.getByRole("button", { name: "Runs" }))
  const drawer = screen.getByRole("region", { name: "Runs" })
  const links = await within(drawer).findAllByRole("link")
  expect(links.map((link) => link.getAttribute("href"))).toEqual([
    "/orgs/org-1/runs/run-2",
    "/orgs/org-1/runs/run-1",
  ])
})

it("the Logs tab streams the newest run's log while the drawer is open", async () => {
  const streamed: string[] = []
  server.use(
    sse<{ message: RunEvent }>(
      "/api/v1/orgs/:organizationId/runs/:runId/events",
      ({ client, params }) => {
        streamed.push(String(params.runId))
        client.send({
          id: "4",
          data: {
            seq: 4,
            type: "node.log",
            nodeId: "vpc",
            stream: "stdout",
            text: "creating the VPC",
          },
        })
      },
    ),
  )
  const close = vi.spyOn(EventSource.prototype, "close")
  renderStudio()
  const toggle = await screen.findByRole("button", { name: "Runs" })
  await userEvent.click(toggle)
  await userEvent.click(screen.getByRole("tab", { name: "Logs" }))
  const log = await screen.findByRole("log", { name: "Logs" })
  expect(await within(log).findByText("creating the VPC")).toBeInTheDocument()
  expect(streamed).toEqual(["run-2"])
  expect(close).not.toHaveBeenCalled()

  await userEvent.click(toggle)
  expect(close).toHaveBeenCalledTimes(1)
  close.mockRestore()
})
