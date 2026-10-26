"use client"

import { ChevronDownIcon, ChevronUpIcon } from "lucide-react"
import Link from "next/link"
import { useId, useState } from "react"

import { useRuns } from "@/api/queries"
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

// The studio's bottom drawer. The closed panel is only hidden, so it keeps its
// tab and scroll position; it is not a Sheet, which would cover the canvas.
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
          </TabsList>
          <TabsContent value="runs">
            <RecentRuns
              organizationId={organizationId}
              workflowId={workflowId}
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
