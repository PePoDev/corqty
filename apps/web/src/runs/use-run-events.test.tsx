import { screen, waitFor, within } from "@testing-library/react"
import { http, HttpResponse } from "msw/http"
import { sse } from "msw/sse"
import { expect, it } from "vitest"

import { useRun } from "@/api/queries"
import type { RunEvent } from "@/api/types"
import {
  EVENT_LIMIT,
  mergeEvents,
  useRunEventLog,
  useRunEvents,
} from "@/runs/use-run-events"
import { run } from "@/test/handlers"
import { renderWithProviders } from "@/test/render"
import { server } from "@/test/server"

const runUrl = "/api/v1/orgs/:organizationId/runs/:runId"
const eventsUrl = `${runUrl}/events`

const log = (seq: number): RunEvent => ({
  seq,
  type: "node.log",
  nodeId: "vpc",
  stream: "stdout",
  text: `line ${seq}`,
})
const vpcSucceeded: RunEvent = {
  seq: 5,
  type: "node.state",
  nodeId: "vpc",
  state: "succeeded",
  attempt: 1,
  output: { vpcId: "vpc-0simulated" },
}
const succeeded: RunEvent = { seq: 9, type: "run.state", state: "succeeded" }

function RunProbe() {
  const query = useRun("org-1", "run-2")
  useRunEvents("org-1", "run-2", query.isSuccess)
  const events = useRunEventLog("org-1", "run-2")
  return (
    <>
      <p>Run {query.data?.status}</p>
      <ul aria-label="Nodes">
        {query.data?.nodes.map((node) => (
          <li key={node.nodeId}>
            {node.nodeId} {node.state}
          </li>
        ))}
      </ul>
      <ol aria-label="Events">
        {events.data?.events.map((event) => (
          <li key={event.seq}>{event.seq}</li>
        ))}
      </ol>
    </>
  )
}

const shownSeqs = () =>
  within(screen.getByRole("list", { name: "Events" }))
    .queryAllByRole("listitem")
    .map((item) => item.textContent)

it("applies each streamed event once, and never behind the snapshot", async () => {
  const afters: (string | null)[] = []
  server.use(
    sse<{ message: RunEvent }>(eventsUrl, ({ client, request }) => {
      afters.push(new URL(request.url).searchParams.get("after"))
      // Replayed from the start: the snapshot already reflects events 1-3.
      client.send({
        id: "1",
        data: { seq: 1, type: "run.state", state: "queued" },
      })
      client.send({ id: "4", data: log(4) })
      client.send({ id: "5", data: vpcSucceeded })
      client.send({ id: "5", data: vpcSucceeded })
      client.send({ id: "6", data: log(6) })
    }),
  )
  renderWithProviders(<RunProbe />)

  await waitFor(() => expect(shownSeqs()).toEqual(["1", "4", "5", "6"]))
  expect(screen.getByText("vpc succeeded")).toBeInTheDocument()
  expect(screen.getByText("Run running")).toBeInTheDocument()
  expect(afters).toEqual(["0"])
})

it("resumes with Last-Event-ID when the stream drops", async () => {
  const reconnects: { lastEventId: string | null; after: string | null }[] = []
  let connections = 0
  server.use(
    sse<{ message: RunEvent }>(eventsUrl, ({ client, request }) => {
      connections += 1
      if (connections === 1) {
        client.send({ retry: 10 })
        client.send({ id: "4", data: log(4) })
        client.send({ id: "5", data: vpcSucceeded })
        // Ending the body makes the browser reconnect after `retry` ms.
        client.close()
        return
      }
      reconnects.push({
        lastEventId: request.headers.get("last-event-id"),
        after: new URL(request.url).searchParams.get("after"),
      })
      client.send({ id: "5", data: vpcSucceeded })
      client.send({ id: "9", data: succeeded })
    }),
  )
  renderWithProviders(<RunProbe />)

  await waitFor(() => expect(screen.getByText("Run succeeded")).toBeVisible())
  expect(shownSeqs()).toEqual(["4", "5", "9"])
  // The URL keeps the first `after`: the server prefers the header.
  expect(reconnects).toEqual([{ lastEventId: "5", after: "0" }])
})

it("reconnects with ?after= once the browser gives up", async () => {
  const afters: (string | null)[] = []
  let connections = 0
  let runReads = 0
  server.use(
    http.get(runUrl, () => {
      runReads += 1
      return HttpResponse.json(run)
    }),
    http.get(eventsUrl, () => {
      connections += 1
      // The browser's own retry fails for good (CLOSED).
      if (connections === 2) {
        return HttpResponse.json({ code: "unavailable" }, { status: 503 })
      }
    }),
    sse<{ message: RunEvent }>(eventsUrl, ({ client, request }) => {
      afters.push(new URL(request.url).searchParams.get("after"))
      if (afters.length === 1) {
        client.send({ retry: 10 })
        client.send({ id: "4", data: log(4) })
        client.send({ id: "5", data: vpcSucceeded })
        client.close()
        return
      }
      client.send({ id: "9", data: succeeded })
    }),
  )
  renderWithProviders(<RunProbe />)

  await waitFor(() => expect(screen.getByText("Run succeeded")).toBeVisible(), {
    timeout: 3000,
  })
  expect(afters).toEqual(["0", "5"])
  expect(shownSeqs()).toEqual(["4", "5", "9"])
  // The run is read again before the new connection.
  expect(runReads).toBe(2)
})

it("closes the stream on the terminal run state", async () => {
  let connections = 0
  server.use(
    sse<{ message: RunEvent }>(eventsUrl, ({ client }) => {
      connections += 1
      client.send({ retry: 10 })
      client.send({ id: "9", data: succeeded })
      // A browser that kept the stream would reconnect after 10 ms.
      client.close()
    }),
  )
  renderWithProviders(<RunProbe />)

  await waitFor(() => expect(screen.getByText("Run succeeded")).toBeVisible())
  await new Promise((resolve) => setTimeout(resolve, 100))
  expect(connections).toBe(1)
})

it("keeps the newest events and drops ones it has", () => {
  const many = Array.from({ length: EVENT_LIMIT + 1 }, (_, index) =>
    log(index + 1),
  )
  const merged = mergeEvents(undefined, many)
  expect(merged.events).toHaveLength(EVENT_LIMIT)
  expect(merged.events[0].seq).toBe(2)
  expect(merged.lastSeq).toBe(EVENT_LIMIT + 1)
  expect(mergeEvents(merged, [log(5)])).toBe(merged)
})
