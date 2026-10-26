"use client"

import { skipToken, useQuery, useQueryClient } from "@tanstack/react-query"
import { useEffect } from "react"

import { ApiProblem } from "@/api/client"
import { queryKeys } from "@/api/queries"
import type { Run, RunEvent, RunStatus } from "@/api/types"

export type RunEventLog = { lastSeq: number; events: RunEvent[] }

// ponytail: the newest 2,000 events of a run stay in memory; older ones stay
// on the server.
export const EVENT_LIMIT = 2000

export function isTerminal(status: RunStatus) {
  return status === "succeeded" || status === "failed" || status === "canceled"
}

// Written only by useRunEvents.
export function useRunEventLog(organizationId: string, runId: string) {
  return useQuery<RunEventLog>({
    queryKey: queryKeys.runEvents(organizationId, runId),
    queryFn: skipToken,
  })
}

// Drops events it already has and keeps the newest EVENT_LIMIT.
export function mergeEvents(
  log: RunEventLog | undefined,
  incoming: RunEvent[],
): RunEventLog {
  let lastSeq = log?.lastSeq ?? 0
  const fresh = incoming.filter((event) => {
    if (event.seq <= lastSeq) return false
    lastSeq = event.seq
    return true
  })
  if (fresh.length === 0 && log) return log
  return {
    lastSeq,
    events: [...(log?.events ?? []), ...fresh].slice(-EVENT_LIMIT),
  }
}

// Events the snapshot already reflects (seq <= lastEventSeq) change nothing.
// A node.state carries only the keys it changes, so the rest follows the
// server's rules: a success has no error (a canceled retry keeps its last
// one), and a queue reason holds only while the node waits.
export function applyEvents(run: Run, events: RunEvent[]): Run {
  const fresh = events.filter((event) => event.seq > run.lastEventSeq)
  if (fresh.length === 0) return run
  let { status, nodes } = run
  for (const event of fresh) {
    if (event.type === "run.state") status = event.state
    if (event.type === "node.state") {
      nodes = nodes.map((node) =>
        node.nodeId === event.nodeId
          ? {
              ...node,
              state: event.state,
              attempt: event.attempt,
              error:
                event.state === "succeeded"
                  ? null
                  : (event.error ?? node.error),
              input: event.input ?? node.input,
              output: event.output ?? node.output,
              queueReason:
                event.state === "queued" || event.state === "retry_wait"
                  ? node.queueReason
                  : null,
            }
          : node,
      )
    }
  }
  return { ...run, status, nodes, lastEventSeq: fresh.at(-1)!.seq }
}

// Whether to stream a loaded run. A failed read (an outage) keeps the cached
// run and the stream retrying; a 401, 403 or 404 ends it for good.
export function canStream(run: { data?: Run; error: Error | null }) {
  return (
    run.data !== undefined &&
    !(
      run.error instanceof ApiProblem &&
      [401, 403, 404].includes(run.error.status)
    )
  )
}

// Streams a run's events into the query cache while `enabled`; pass
// `canStream(run)`. It resumes after the newest event in the cache, or
// replays from the start.
export function useRunEvents(
  organizationId: string,
  runId: string,
  enabled: boolean,
) {
  const queryClient = useQueryClient()
  useEffect(() => {
    if (!enabled) return
    const runKey = queryKeys.run(organizationId, runId)
    const eventsKey = queryKeys.runEvents(organizationId, runId)
    const lastSeq = () =>
      queryClient.getQueryData<RunEventLog>(eventsKey)?.lastSeq ?? 0
    // Nothing is left when the run is terminal and every event it reports
    // has arrived.
    const done = () => {
      const run = queryClient.getQueryData<Run>(runKey)
      return (
        run !== undefined &&
        isTerminal(run.status) &&
        lastSeq() >= run.lastEventSeq
      )
    }
    let source: EventSource | undefined
    let retryTimer: ReturnType<typeof setTimeout> | undefined
    let flushTimer: ReturnType<typeof setTimeout> | undefined
    let pending: RunEvent[] = []
    let failures = 0
    let stopped = false

    // One cache write per 100 ms keeps a log flood from rendering per line.
    function flush() {
      clearTimeout(flushTimer)
      flushTimer = undefined
      const batch = pending
      pending = []
      if (batch.length === 0) return
      queryClient.setQueryData<RunEventLog>(eventsKey, (log) =>
        mergeEvents(log, batch),
      )
      queryClient.setQueryData<Run>(runKey, (run) =>
        run ? applyEvents(run, batch) : run,
      )
    }

    function connect() {
      if (stopped || done()) return
      const current = new EventSource(
        `/api/v1/orgs/${organizationId}/runs/${runId}/events?after=${lastSeq()}`,
      )
      source = current
      current.onopen = () => {
        failures = 0
        // A run read that failed while the stream was down is tried again.
        if (queryClient.getQueryState(runKey)?.status === "error") {
          void queryClient.invalidateQueries({ queryKey: runKey, exact: true })
        }
      }
      current.onmessage = (message) => {
        const event = JSON.parse(message.data) as RunEvent
        pending.push(event)
        flushTimer ??= setTimeout(flush, 100)
        // The server ends the stream after the terminal event; closing first
        // stops the browser from reconnecting.
        if (event.type === "run.state" && isTerminal(event.state)) {
          current.close()
          flush()
          // The final read brings what no event carries, like finish times.
          void queryClient.invalidateQueries({ queryKey: runKey, exact: true })
        }
      }
      current.onerror = () => {
        // CONNECTING: the browser retries by itself and sends Last-Event-ID.
        if (current.readyState !== EventSource.CLOSED) return
        // CLOSED: a non-200 (204 included) that the browser never retries.
        // The status is hidden, so read the run first: a 401, 403 or 404
        // turns `enabled` off, and a finished run ends in done().
        flush()
        void queryClient
          .invalidateQueries({ queryKey: runKey, exact: true })
          .then(() => {
            if (stopped) return
            const delay = Math.min(1000 * 2 ** failures++, 30_000)
            retryTimer = setTimeout(connect, delay)
          })
      }
    }

    connect()
    return () => {
      stopped = true
      source?.close()
      clearTimeout(retryTimer)
      flush()
    }
  }, [enabled, organizationId, queryClient, runId])
}
