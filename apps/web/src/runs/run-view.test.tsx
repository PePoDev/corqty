import { fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { http, HttpResponse } from "msw/http"
import { sse } from "msw/sse"
import { expect, it } from "vitest"

import type { Run, RunEvent } from "@/api/types"
import { AuthGate } from "@/auth/auth-gate"
import { AppShell } from "@/layout/app-shell"
// Loaded up front so next/dynamic resolves within the findBy timeouts.
import "@/runs/run-graph"
import { RunView } from "@/runs/run-view"
import { meAs, run, runs } from "@/test/handlers"
import { renderWithProviders, router } from "@/test/render"
import { server } from "@/test/server"

const runUrl = "/api/v1/orgs/:organizationId/runs/:runId"

function renderRun() {
  return renderWithProviders(
    <AuthGate>
      <AppShell organizationId="org-1">
        <RunView organizationId="org-1" runId="run-2" />
      </AppShell>
    </AuthGate>,
    { route: "/orgs/org-1/runs/run-2" },
  )
}

function withRun(changes: Partial<Run>) {
  server.use(http.get(runUrl, () => HttpResponse.json({ ...run, ...changes })))
}

function signedInAs(role: "runner" | "viewer") {
  server.use(http.get("/api/v1/auth/me", () => HttpResponse.json(meAs(role))))
}

// Each connection gets a function that sends it events.
function stream(...initial: RunEvent[]) {
  const connections: ((event: RunEvent) => void)[] = []
  server.use(
    sse<{ message: RunEvent }>(`${runUrl}/events`, ({ client }) => {
      const send = (event: RunEvent) =>
        client.send({ id: String(event.seq), data: event })
      initial.forEach(send)
      connections.push(send)
    }),
  )
  return connections
}

const log = (seq: number, nodeId: string): RunEvent => ({
  seq,
  type: "node.log",
  nodeId,
  stream: "stdout",
  text: `line ${seq}`,
})

const nodeOf = (id: string) => screen.findByTestId(`rf__node-${id}`)

it("overlays node-run states on the pinned graph as events arrive", async () => {
  // The run shows the graph it was started from, not the current draft.
  withRun({
    graph: {
      ...run.graph,
      nodes: run.graph.nodes.map((node) =>
        node.id === "vpc" ? { ...node, displayName: "VPC as published" } : node,
      ),
    },
  })
  const connections = stream()
  renderRun()
  const vpc = await nodeOf("vpc")
  expect(within(vpc).getByText("VPC as published")).toBeInTheDocument()
  expect(within(vpc).getByText("Running")).toBeInTheDocument()
  expect(within(await nodeOf("rds")).getByText("Blocked")).toBeInTheDocument()
  await waitFor(() => expect(connections).toHaveLength(1))

  connections[0]({
    seq: 4,
    type: "node.state",
    nodeId: "vpc",
    state: "succeeded",
    attempt: 1,
    output: { vpcId: "vpc-0simulated" },
  })
  connections[0]({
    seq: 5,
    type: "node.state",
    nodeId: "rds",
    state: "queued",
    attempt: 0,
  })
  await waitFor(() =>
    expect(within(vpc).getByText("Succeeded")).toBeInTheDocument(),
  )
  expect(
    within(screen.getByTestId("rf__node-rds")).getByText("Queued"),
  ).toBeInTheDocument()
  // The overlay changed in place: the node was not rebuilt.
  expect(screen.getByTestId("rf__node-vpc")).toBe(vpc)

  connections[0]({ seq: 9, type: "run.state", state: "succeeded" })
  const status = screen.getByRole("status", { name: "Run status" })
  await waitFor(() => expect(status).toHaveTextContent("Succeeded"))
  expect(
    screen.queryByRole("button", { name: "Cancel" }),
  ).not.toBeInTheDocument()
})

it("shows the selected node's details and only its log", async () => {
  withRun({
    nodes: [
      {
        ...run.nodes[0],
        state: "succeeded",
        output: { vpcId: "vpc-0simulated" },
      },
      {
        ...run.nodes[1],
        state: "failed",
        attempt: 2,
        maxAttempts: 2,
        input: { vpcId: "vpc-0simulated" },
        error: {
          code: "mock_failure",
          message: "The database did not start.",
          retryable: false,
        },
      },
      {
        ...run.nodes[2],
        state: "queued",
        queueReason: "waiting_for_compatible_worker",
      },
    ],
  })
  stream(log(4, "vpc"), log(5, "rds"))
  renderRun()
  const details = await screen.findByRole("complementary", {
    name: "Node details",
  })
  expect(details).toHaveTextContent("Select a node to see its details")
  const logs = screen.getByRole("log", { name: "Logs" })
  expect(await within(logs).findByText("line 5")).toBeInTheDocument()
  expect(within(logs).getByText("line 4")).toBeInTheDocument()

  fireEvent.click(
    await screen.findByRole("group", { name: "Create RDS (simulated)" }),
  )
  expect(
    within(details).getByRole("heading", { name: "Create RDS (simulated)" }),
  ).toBeInTheDocument()
  expect(within(details).getByText("Failed")).toBeInTheDocument()
  expect(within(details).getByText("2 of 2")).toBeInTheDocument()
  expect(details).toHaveTextContent("The database did not start.")
  expect(details).toHaveTextContent("mock_failure")
  const figure = (caption: string) =>
    within(details).getByText(caption).closest("figure")
  expect(figure("Input")).toHaveTextContent('{ "vpcId": "vpc-0simulated" }')
  expect(figure("Output")).toHaveTextContent("None")
  expect(within(logs).getByText("line 5")).toBeInTheDocument()
  expect(within(logs).queryByText("line 4")).not.toBeInTheDocument()

  fireEvent.click(
    screen.getByRole("group", { name: "Deploy service (simulated)" }),
  )
  expect(details).toHaveTextContent(
    "Waiting for a worker that runs mock@1.0.0.",
  )

  // The log drawer folds away.
  const toggle = screen.getByRole("button", { name: "Logs" })
  expect(toggle).toHaveAttribute("aria-expanded", "true")
  await userEvent.click(toggle)
  expect(toggle).toHaveAttribute("aria-expanded", "false")
  expect(logs).not.toBeVisible()
})

it("a runner cancels the run and then reruns it", async () => {
  signedInAs("runner")
  const requests: string[] = []
  let cancelRequested = false
  server.use(
    http.get(runUrl, () => HttpResponse.json({ ...run, cancelRequested })),
    http.post(`${runUrl}/cancel`, ({ params }) => {
      requests.push(`cancel ${params.runId}`)
      cancelRequested = true
      return HttpResponse.json({ ...runs[0], cancelRequested })
    }),
    http.post(`${runUrl}/rerun`, async ({ params, request }) => {
      requests.push(
        `rerun ${params.runId} ${JSON.stringify(await request.json())}`,
      )
      return HttpResponse.json(
        { ...run, id: "run-3", status: "queued", lastEventSeq: 1 },
        { status: 201 },
      )
    }),
  )
  stream()
  renderRun()
  await userEvent.click(await screen.findByRole("button", { name: "Cancel" }))
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled(),
  )
  expect(screen.getByText("Cancel requested")).toBeInTheDocument()

  await userEvent.click(screen.getByRole("button", { name: "Rerun" }))
  await waitFor(() =>
    expect(router.push).toHaveBeenCalledWith("/orgs/org-1/runs/run-3"),
  )
  expect(requests).toEqual(["cancel run-2", "rerun run-2 {}"])
})

it("Cancel says why it failed", async () => {
  server.use(
    http.post(`${runUrl}/cancel`, () =>
      HttpResponse.json({ status: 409, code: "run_finished" }, { status: 409 }),
    ),
  )
  stream()
  renderRun()
  await userEvent.click(await screen.findByRole("button", { name: "Cancel" }))
  expect(
    await screen.findByText("Cancel failed (run_finished)."),
  ).toBeInTheDocument()
})

it("a viewer follows the run without Cancel or Rerun", async () => {
  signedInAs("viewer")
  stream()
  renderRun()
  expect(await nodeOf("vpc")).toBeInTheDocument()
  expect(
    screen.queryByRole("button", { name: "Cancel" }),
  ).not.toBeInTheDocument()
  expect(
    screen.queryByRole("button", { name: "Rerun" }),
  ).not.toBeInTheDocument()
})

it("on a narrow screen a runner still sees the graph, details, log and Cancel", async () => {
  Object.assign(window, { innerWidth: 800 })
  signedInAs("runner")
  stream(log(4, "vpc"))
  renderRun()
  expect(await nodeOf("vpc")).toBeVisible()
  expect(
    screen.getByRole("complementary", { name: "Node details" }),
  ).toBeVisible()
  expect(await screen.findByText("line 4")).toBeVisible()
  expect(screen.getByRole("button", { name: "Cancel" })).toBeVisible()
  expect(
    screen.queryByText("Editing needs a screen at least 1024 pixels wide."),
  ).not.toBeInTheDocument()
})
