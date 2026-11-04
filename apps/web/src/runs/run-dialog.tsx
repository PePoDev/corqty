"use client"

import { useRouter } from "next/navigation"
import { useId, useState } from "react"

import { ApiProblem } from "@/api/client"
import { useStartRun, useWorkflowVersion } from "@/api/queries"
import type { Diagnostic } from "@/api/types"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"
import { RunInputForm } from "@/runs/run-input-form"

// Runs start from the published version, so Run waits for one.
export function RunDialog({
  organizationId,
  workflowId,
  version,
}: {
  organizationId: string
  workflowId: string
  version: number | null
}) {
  const reasonId = useId()
  if (version === null) {
    return (
      <>
        <span id={reasonId} className="text-muted-foreground">
          Publish the workflow before running it.
        </span>
        <Button
          size="sm"
          variant="secondary"
          disabled
          aria-describedby={reasonId}
        >
          Run
        </Button>
      </>
    )
  }
  return (
    <Dialog>
      <DialogTrigger render={<Button size="sm" variant="secondary" />}>
        Run
      </DialogTrigger>
      <DialogContent className="sm:max-w-md">
        <RunForm
          organizationId={organizationId}
          workflowId={workflowId}
          version={version}
        />
      </DialogContent>
    </Dialog>
  )
}

// Mounted only while the dialog is open, so it starts from the defaults each
// time. The form stays disabled after a start until the run page opens.
function RunForm({
  organizationId,
  workflowId,
  version,
}: {
  organizationId: string
  workflowId: string
  version: number
}) {
  const router = useRouter()
  const published = useWorkflowVersion(organizationId, workflowId, version)
  const startRun = useStartRun(organizationId, workflowId)
  const [diagnostics, setDiagnostics] = useState<Diagnostic[]>([])

  async function start(input: Record<string, unknown>) {
    setDiagnostics([])
    try {
      const run = await startRun.mutateAsync({ input, version })
      router.push(`/orgs/${organizationId}/runs/${run.id}`)
    } catch (error) {
      const problem =
        error instanceof ApiProblem ? error : new ApiProblem(0, "network_error")
      setDiagnostics(
        problem.diagnostics.length > 0
          ? problem.diagnostics
          : [
              {
                code: problem.code,
                pointer: "",
                message: `The run could not be started (${problem.code}).`,
              },
            ],
      )
    }
  }

  return (
    <>
      <DialogHeader>
        <DialogTitle>Run workflow</DialogTitle>
        <DialogDescription>
          Starts published version {version} with these inputs.
        </DialogDescription>
      </DialogHeader>
      {published.data ? (
        <RunInputForm
          schema={published.data.graph.workflowInputSchema}
          pending={startRun.isPending || startRun.isSuccess}
          diagnostics={diagnostics}
          onSubmit={(input) => void start(input)}
        />
      ) : published.isError ? (
        <p role="alert" className="text-destructive">
          The published version could not be loaded.
        </p>
      ) : (
        <p className="text-muted-foreground">Loading inputs…</p>
      )}
    </>
  )
}
