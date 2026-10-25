"use client"

import { useWorkflow } from "@/api/queries"

// Placeholder until the canvas lands: it only proves the workflow loads.
export function Studio({
  organizationId,
  workflowId,
}: {
  organizationId: string
  workflowId: string
}) {
  const workflow = useWorkflow(organizationId, workflowId)

  return (
    <section className="flex flex-col gap-2 p-4 md:p-6">
      {workflow.isPending && (
        <p className="text-sm text-muted-foreground">Loading workflow…</p>
      )}
      {workflow.isError && (
        <p role="alert" className="text-sm text-destructive">
          The workflow could not be loaded.
        </p>
      )}
      {workflow.data && (
        <>
          <h1 className="text-2xl font-semibold tracking-tight">
            {workflow.data.name}
          </h1>
          <p className="text-sm text-muted-foreground">
            Draft revision {workflow.data.draft.revision}. The editor is not
            available yet.
          </p>
        </>
      )}
    </section>
  )
}
