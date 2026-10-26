"use client"

import { useLayoutEffect, useRef } from "react"

import type { RunEvent } from "@/api/types"
import { EVENT_LIMIT } from "@/runs/use-run-events"

type LogEvent = Extract<RunEvent, { type: "node.log" | "node.log_truncated" }>

// The log lines of one node, or of every node when `nodeId` is null. The
// cache holds at most EVENT_LIMIT events, so the list stays bounded, and rows
// off screen skip layout and paint.
export function LogView({
  events,
  nodeId,
}: {
  events: RunEvent[]
  nodeId: string | null
}) {
  const lines = events.filter(
    (event): event is LogEvent =>
      (event.type === "node.log" || event.type === "node.log_truncated") &&
      (nodeId === null || event.nodeId === nodeId),
  )
  const viewport = useRef<HTMLDivElement>(null)
  // Follow new lines only while the reader stays at the bottom.
  const pinned = useRef(true)
  const last = lines.at(-1)?.seq
  useLayoutEffect(() => {
    const element = viewport.current
    if (element && pinned.current) element.scrollTop = element.scrollHeight
  }, [last])
  return (
    <div
      ref={viewport}
      role="log"
      aria-label="Logs"
      // A flood of lines would drown a screen reader; states are announced
      // elsewhere.
      aria-live="off"
      tabIndex={0}
      onScroll={({ currentTarget: element }) => {
        pinned.current =
          element.scrollHeight - element.scrollTop - element.clientHeight < 24
      }}
      className="h-56 overflow-y-auto rounded-md bg-muted/40 p-2 font-mono text-xs"
    >
      {events.length >= EVENT_LIMIT && (
        <p className="text-muted-foreground">
          Showing the newest {EVENT_LIMIT.toLocaleString("en-US")} events.
        </p>
      )}
      {lines.length === 0 ? (
        <p className="text-muted-foreground">No log output yet.</p>
      ) : (
        <ol>
          {lines.map((line) => (
            <li
              key={line.seq}
              data-stream={line.type === "node.log" ? line.stream : undefined}
              className="break-all whitespace-pre-wrap [contain-intrinsic-size:auto_1rem] [content-visibility:auto] data-[stream=stderr]:text-destructive data-[stream=system]:text-muted-foreground"
            >
              {nodeId === null && (
                <span className="text-muted-foreground">{line.nodeId} </span>
              )}
              {line.type === "node.log" ? (
                line.text
              ) : (
                <em className="text-muted-foreground">
                  Log truncated at 10 MiB
                </em>
              )}
            </li>
          ))}
        </ol>
      )}
    </div>
  )
}
