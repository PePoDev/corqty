"use client"

import dynamic from "next/dynamic"
import { useSyncExternalStore } from "react"

import { useBlueprints, useNodeDefinitions, useWorkflow } from "@/api/queries"
import { useCan } from "@/auth/use-can"
import { StatusBadge } from "@/components/status/status-badge"
import { cn } from "@/lib/utils"
import { Palette } from "@/studio/palette"

// XYFlow measures the DOM and follows the system color scheme, so the canvas
// renders only in the browser.
const StudioCanvas = dynamic(
  () => import("@/studio/studio-canvas").then((module) => module.StudioCanvas),
  {
    ssr: false,
    loading: () => (
      <p className="p-4 text-sm text-muted-foreground">Loading editor…</p>
    ),
  },
)

const wideScreen = "(min-width: 1024px)"

function subscribe(onChange: () => void) {
  const query = window.matchMedia(wideScreen)
  query.addEventListener("change", onChange)
  return () => query.removeEventListener("change", onChange)
}

// Full editing needs 1024 px; the server render assumes a narrow screen.
function useWideScreen() {
  return useSyncExternalStore(
    subscribe,
    () => window.matchMedia(wideScreen).matches,
    () => false,
  )
}

export function Studio({
  organizationId,
  workflowId,
}: {
  organizationId: string
  workflowId: string
}) {
  const workflow = useWorkflow(organizationId, workflowId)
  const definitions = useNodeDefinitions(organizationId)
  const blueprints = useBlueprints(organizationId)
  const canEdit = useCan(organizationId, "edit_workflows")
  const wide = useWideScreen()
  const readOnlyReason = !canEdit
    ? "Your role can view this workflow but not edit it."
    : !wide
      ? "Editing needs a screen at least 1024 pixels wide."
      : null

  if (workflow.isError || definitions.isError || blueprints.isError) {
    return (
      <p role="alert" className="p-4 text-sm text-destructive md:p-6">
        The workflow could not be loaded.
      </p>
    )
  }
  if (!workflow.data || !definitions.data || !blueprints.data) {
    return (
      <p className="p-4 text-sm text-muted-foreground md:p-6">
        Loading workflow…
      </p>
    )
  }

  return (
    <section className="flex h-dvh flex-col">
      <header className="flex flex-wrap items-center gap-3 border-b px-4 py-3">
        <h1 className="text-lg font-semibold tracking-tight">
          {workflow.data.name}
        </h1>
        {readOnlyReason && (
          <>
            <StatusBadge status="read-only" />
            <span className="text-sm text-muted-foreground">
              {readOnlyReason}
            </span>
          </>
        )}
      </header>
      <div
        className={cn(
          "grid min-h-0 flex-1",
          !readOnlyReason && "grid-cols-[16rem_1fr]",
        )}
      >
        {!readOnlyReason && (
          <Palette
            definitions={definitions.data.items}
            blueprints={blueprints.data.items}
          />
        )}
        <StudioCanvas
          graph={workflow.data.draft.graph}
          definitions={definitions.data.items}
          blueprints={blueprints.data.items}
          readOnly={readOnlyReason !== null}
        />
      </div>
    </section>
  )
}
