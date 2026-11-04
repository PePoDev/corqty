import { fireEvent, render, screen } from "@testing-library/react"
import { expect, it } from "vitest"

import type { RunEvent } from "@/api/types"
import { LogView } from "@/runs/log-view"
import { EVENT_LIMIT } from "@/runs/use-run-events"

const line = (
  seq: number,
  nodeId = "vpc",
  stream: "stdout" | "stderr" | "system" = "stdout",
): RunEvent => ({ seq, type: "node.log", nodeId, stream, text: `line ${seq}` })

const rows = () =>
  screen.getAllByRole("listitem").map((item) => item.textContent)

// jsdom has no layout: give the log a fixed geometry.
function geometry(element: HTMLElement, scrollHeight: number) {
  Object.defineProperties(element, {
    scrollHeight: { configurable: true, value: scrollHeight },
    clientHeight: { configurable: true, value: 100 },
  })
}

it("follows new lines until the reader scrolls up", () => {
  const { rerender } = render(<LogView events={[line(1)]} nodeId="vpc" />)
  const log = screen.getByRole("log", { name: "Logs" })
  geometry(log, 500)
  rerender(<LogView events={[line(1), line(2)]} nodeId="vpc" />)
  expect(log.scrollTop).toBe(500)

  log.scrollTop = 100
  fireEvent.scroll(log)
  geometry(log, 900)
  rerender(<LogView events={[line(1), line(2), line(3)]} nodeId="vpc" />)
  expect(log.scrollTop).toBe(100)
  expect(rows()).toEqual(["line 1", "line 2", "line 3"])
})

it("shows one node's lines and marks a truncated log", () => {
  const events: RunEvent[] = [
    line(1, "vpc"),
    { seq: 2, type: "node.state", nodeId: "rds", state: "running", attempt: 1 },
    line(3, "rds", "stderr"),
    { seq: 4, type: "node.log_truncated", nodeId: "rds" },
  ]
  const { rerender } = render(<LogView events={events} nodeId={null} />)
  // Every node's lines, each with its node.
  expect(rows()).toEqual([
    "vpc line 1",
    "rds line 3",
    "rds Log truncated at 10 MiB",
  ])

  rerender(<LogView events={events} nodeId="rds" />)
  expect(rows()).toEqual(["line 3", "Log truncated at 10 MiB"])
  expect(screen.getByText("line 3")).toHaveAttribute("data-stream", "stderr")

  rerender(<LogView events={events} nodeId="deploy" />)
  expect(screen.getByText("No log output yet.")).toBeInTheDocument()
})

it("says when older events were dropped", () => {
  const events = Array.from({ length: EVENT_LIMIT }, (_, index) =>
    line(index + 1),
  )
  render(<LogView events={events} nodeId="deploy" />)
  expect(
    screen.getByText("Showing the newest 2,000 events."),
  ).toBeInTheDocument()
})
