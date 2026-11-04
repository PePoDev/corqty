"use client"

import { ChevronDownIcon, ChevronUpIcon } from "lucide-react"
import dynamic from "next/dynamic"
import Link from "next/link"
import { useRouter } from "next/navigation"
import { useId, useState } from "react"

import { ApiProblem } from "@/api/client"
import {
  useCancelRun,
  useNodeDefinitions,
  useRerunRun,
  useRun,
} from "@/api/queries"
import type { Run, RunEvent } from "@/api/types"
import { useCan } from "@/auth/use-can"
import { StatusBadge } from "@/components/status/status-badge"
import { Button } from "@/components/ui/button"
import { LogView } from "@/runs/log-view"
import {
  canStream,
  isTerminal,
  useRunEventLog,
  useRunEvents,
} from "@/runs/use-run-events"

// XYFlow measures the DOM, so the graph renders only in the browser.
const RunGraph = dynamic(
  () => import("@/runs/run-graph").then((module) => module.RunGraph),
  {
    ssr: false,
    loading: () => (
      <p className="p-4 text-sm text-muted-foreground">Loading graph…</p>
    ),
  },
)

function RunHeader({
  organizationId,
  run,
  connection,
}: {
  organizationId: string
  run: Run
  connection: string
}) {
  const canRun = useCan(organizationId, "run_workflows")
  const router = useRouter()
  const cancel = useCancelRun(organizationId)
  const rerun = useRerunRun(organizationId)
  const [notice, setNotice] = useState("")
  const terminal = isTerminal(run.status)

  async function act(action: string, task: () => Promise<void>) {
    setNotice("")
    try {
      await task()
    } catch (error) {
      const code = error instanceof ApiProblem ? error.code : "network_error"
      setNotice(`${action} failed (${code}).`)
    }
  }

  return (
    <header className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b px-4 py-3 text-sm">
      <nav aria-label="Breadcrumb">
        <ol className="flex flex-wrap items-center gap-2">
          <li>
            <Link
              href={`/orgs/${organizationId}/workflows`}
              className="text-muted-foreground hover:text-foreground"
            >
              Workflows
            </Link>
          </li>
          <li aria-hidden="true" className="text-muted-foreground">
            /
          </li>
          <li>
            <Link
              href={`/orgs/${organizationId}/workflows/${run.workflowId}`}
              className="text-muted-foreground hover:text-foreground"
            >
              {run.workflowName}
            </Link>
          </li>
          <li aria-hidden="true" className="text-muted-foreground">
            /
          </li>
          <li aria-current="page">
            <h1 className="text-lg font-semibold tracking-tight">
              Run {new Date(run.createdAt).toLocaleString()}
            </h1>
          </li>
        </ol>
      </nav>
      <span role="status" aria-label="Run status">
        <StatusBadge status={run.status} />
      </span>
      <span className="text-muted-foreground">v{run.version}</span>
      {run.cancelRequested && !terminal && (
        <span className="text-muted-foreground">Cancel requested</span>
      )}
      <span aria-live="polite">{notice}</span>
      <span role="status" className="text-muted-foreground">
        {connection}
      </span>
      {canRun && (
        <div className="ml-auto flex items-center gap-2">
          {!terminal && (
            <Button
              variant="outline"
              size="sm"
              disabled={cancel.isPending || run.cancelRequested}
              onClick={() =>
                void act("Cancel", async () => {
                  await cancel.mutateAsync(run.id)
                })
              }
            >
              Cancel
            </Button>
          )}
          <Button
            variant="secondary"
            size="sm"
            disabled={rerun.isPending || rerun.isSuccess}
            onClick={() =>
              void act("Rerun", async () => {
                const next = await rerun.mutateAsync(run.id)
                router.push(`/orgs/${organizationId}/runs/${next.id}`)
              })
            }
          >
            Rerun
          </Button>
        </div>
      )}
    </header>
  )
}

function Json({ label, value }: { label: string; value: unknown }) {
  return (
    <figure className="grid gap-1">
      <figcaption className="font-medium">{label}</figcaption>
      {value === null ? (
        <p className="text-muted-foreground">None</p>
      ) : (
        <pre className="overflow-x-auto rounded-md bg-muted/40 p-2 font-mono text-xs">
          {JSON.stringify(value, null, 2)}
        </pre>
      )}
    </figure>
  )
}

function NodeDetails({ run, nodeId }: { run: Run; nodeId: string | null }) {
  const node = run.graph.nodes.find(({ id }) => id === nodeId)
  const nodeRun = run.nodes.find((candidate) => candidate.nodeId === nodeId)
  if (!node || !nodeRun) {
    return (
      <p className="text-muted-foreground">
        Select a node to see its details and only its log lines.
      </p>
    )
  }
  return (
    <div className="grid gap-3">
      <h2 className="font-medium">{node.displayName}</h2>
      <dl className="grid grid-cols-[auto_1fr] items-center gap-x-3 gap-y-1">
        <dt className="text-muted-foreground">State</dt>
        <dd>
          <StatusBadge status={nodeRun.state} />
        </dd>
        <dt className="text-muted-foreground">Attempt</dt>
        <dd>
          {nodeRun.attempt} of {nodeRun.maxAttempts}
        </dd>
      </dl>
      {(nodeRun.state === "queued" || nodeRun.state === "retry_wait") &&
        nodeRun.queueReason === "waiting_for_compatible_worker" && (
          <p>Waiting for a worker that runs {nodeRun.executor}.</p>
        )}
      {nodeRun.error && (
        <p className="text-destructive">
          {nodeRun.error.message}{" "}
          <span className="font-mono text-xs">{nodeRun.error.code}</span>
        </p>
      )}
      <Json label="Input" value={nodeRun.input} />
      <Json label="Output" value={nodeRun.output} />
    </div>
  )
}

// The run page's bottom drawer starts open; closed, it is only hidden.
function LogDrawer({
  events,
  nodeId,
}: {
  events: RunEvent[]
  nodeId: string | null
}) {
  const [open, setOpen] = useState(true)
  const panelId = useId()
  return (
    <section aria-label="Logs" className="border-t">
      <Button
        variant="ghost"
        size="sm"
        className="m-1"
        aria-expanded={open}
        aria-controls={panelId}
        onClick={() => setOpen(!open)}
      >
        {open ? <ChevronDownIcon /> : <ChevronUpIcon />}
        Logs
      </Button>
      <div id={panelId} hidden={!open} className="px-4 pb-4">
        <LogView events={events} nodeId={nodeId} />
      </div>
    </section>
  )
}

// Follows one run live: overlays on its pinned graph, the selected node's
// details, and its log. Unlike editing, this works on any screen width.
export function RunView({
  organizationId,
  runId,
}: {
  organizationId: string
  runId: string
}) {
  const run = useRun(organizationId, runId)
  const definitions = useNodeDefinitions(organizationId)
  const streaming = canStream(run)
  useRunEvents(organizationId, runId, streaming)
  const events = useRunEventLog(organizationId, runId)
  const [selected, setSelected] = useState<string | null>(null)

  if (!run.data || !definitions.data) {
    return run.isError || definitions.isError ? (
      <p role="alert" className="p-4 text-sm text-destructive md:p-6">
        The run could not be loaded.
      </p>
    ) : (
      <p className="p-4 text-sm text-muted-foreground md:p-6">Loading run…</p>
    )
  }

  // A failed read leaves the last known run on screen; say why it may be
  // behind.
  const connection = !run.isError
    ? ""
    : streaming
      ? "Reconnecting…"
      : "Live updates stopped."

  return (
    <section>
      <RunHeader
        organizationId={organizationId}
        run={run.data}
        connection={connection}
      />
      <div className="grid lg:grid-cols-[1fr_20rem]">
        <div className="h-112">
          <RunGraph
            graph={run.data.graph}
            definitions={definitions.data.items}
            nodeRuns={run.data.nodes}
            onNodeSelect={setSelected}
          />
        </div>
        <aside
          aria-label="Node details"
          className="border-t p-4 text-sm lg:max-h-112 lg:overflow-y-auto lg:border-t-0 lg:border-l"
        >
          <NodeDetails run={run.data} nodeId={selected} />
        </aside>
      </div>
      <LogDrawer events={events.data?.events ?? []} nodeId={selected} />
    </section>
  )
}
