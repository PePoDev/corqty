"use client"

import { ChevronDownIcon, ChevronUpIcon } from "lucide-react"
import Link from "next/link"
import { useId, useState } from "react"

import { useRun, useRuns } from "@/api/queries"
import { StatusBadge } from "@/components/status/status-badge"
import { Button } from "@/components/ui/button"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { LogView } from "@/runs/log-view"
import { useRunEventLog, useRunEvents } from "@/runs/use-run-events"

// The studio's bottom drawer. The closed panel is only hidden, so it keeps its
// tab and scroll position; it is not a Sheet, which would cover the canvas.
// An inactive tab's panel unmounts, so the Logs tab streams only while the
// drawer is open on it.
export function RunsDrawer({
  organizationId,
  workflowId,
}: {
  organizationId: string
  workflowId: string
}) {
  const [open, setOpen] = useState(false)
  const panelId = useId()
  return (
    <section aria-label="Runs" className="border-t">
      <Button
        variant="ghost"
        size="sm"
        className="m-1"
        aria-expanded={open}
        aria-controls={panelId}
        onClick={() => setOpen(!open)}
      >
        {open ? <ChevronDownIcon /> : <ChevronUpIcon />}
        Runs
      </Button>
      <div id={panelId} hidden={!open} className="h-72 overflow-y-auto px-4">
        <Tabs defaultValue="runs">
          <TabsList>
            <TabsTrigger value="runs">Recent runs</TabsTrigger>
            <TabsTrigger value="logs">Logs</TabsTrigger>
          </TabsList>
          <TabsContent value="runs">
            <RecentRuns
              organizationId={organizationId}
              workflowId={workflowId}
            />
          </TabsContent>
          <TabsContent value="logs">
            <NewestRunLogs
              organizationId={organizationId}
              workflowId={workflowId}
              streaming={open}
            />
          </TabsContent>
        </Tabs>
      </div>
    </section>
  )
}

// ponytail: the list refreshes on focus and after a run starts; the run page
// follows a run live.
function RecentRuns({
  organizationId,
  workflowId,
}: {
  organizationId: string
  workflowId: string
}) {
  const runs = useRuns(organizationId, workflowId)
  if (runs.isPending) {
    return <p className="text-muted-foreground">Loading runs…</p>
  }
  if (runs.isError) {
    return (
      <p role="alert" className="text-destructive">
        The runs could not be loaded.
      </p>
    )
  }
  if (runs.data.items.length === 0) {
    return <p className="text-muted-foreground">No runs yet.</p>
  }
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Created</TableHead>
          <TableHead>Version</TableHead>
          <TableHead>Status</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {runs.data.items.map((run) => (
          <TableRow key={run.id}>
            <TableCell>
              <Link
                href={`/orgs/${organizationId}/runs/${run.id}`}
                className="font-medium underline-offset-4 hover:underline"
              >
                {new Date(run.createdAt).toLocaleString()}
              </Link>
            </TableCell>
            <TableCell>v{run.version}</TableCell>
            <TableCell>
              <StatusBadge status={run.status} />
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  )
}

// The log of the workflow's newest run, every node's lines.
function NewestRunLogs({
  organizationId,
  workflowId,
  streaming,
}: {
  organizationId: string
  workflowId: string
  streaming: boolean
}) {
  const runs = useRuns(organizationId, workflowId)
  const newest = runs.data?.items[0]
  if (!newest) {
    return (
      <p className="text-muted-foreground">
        {runs.isPending
          ? "Loading runs…"
          : runs.isError
            ? "The runs could not be loaded."
            : "No runs yet."}
      </p>
    )
  }
  return (
    <RunLogs
      organizationId={organizationId}
      runId={newest.id}
      streaming={streaming}
    />
  )
}

function RunLogs({
  organizationId,
  runId,
  streaming,
}: {
  organizationId: string
  runId: string
  streaming: boolean
}) {
  const run = useRun(organizationId, runId)
  useRunEvents(organizationId, runId, streaming && run.isSuccess)
  const events = useRunEventLog(organizationId, runId)
  return <LogView events={events.data?.events ?? []} nodeId={null} />
}
