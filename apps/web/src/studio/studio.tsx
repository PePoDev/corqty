"use client"

import { useQueryClient } from "@tanstack/react-query"
import dynamic from "next/dynamic"
import Link from "next/link"
import { useId, useState, useSyncExternalStore } from "react"

import { ApiProblem } from "@/api/client"
import {
  queryKeys,
  useBlueprints,
  useNodeDefinitions,
  usePublishWorkflow,
  useSettings,
  useValidateWorkflow,
  useWorkflow,
} from "@/api/queries"
import type {
  Blueprint,
  Diagnostic,
  GraphSnapshot,
  NodeDefinition,
  Workflow,
} from "@/api/types"
import { useCan } from "@/auth/use-can"
import { StatusBadge } from "@/components/status/status-badge"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"
import { RunDialog } from "@/runs/run-dialog"
import { RunsDrawer } from "@/runs/runs-drawer"
import { mapDiagnostics, type MappedDiagnostic } from "@/studio/diagnostics"
import { Palette } from "@/studio/palette"
import { useDraftAutosave, type SavedDraft } from "@/studio/use-draft-autosave"

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

const noProblems: MappedDiagnostic[] = []

// Names a problem's place with the names in the graph that was checked.
function place(graph: GraphSnapshot, problem: MappedDiagnostic) {
  const node = graph.nodes.find(({ id }) => id === problem.nodeId)
  if (!node) return problem.pointer
  const port = problem.port?.split(":")[1]
  return port ? `${node.displayName} · ${port}` : node.displayName
}

function Editor({
  organizationId,
  workflow,
  definitions,
  blueprints,
  onReload,
}: {
  organizationId: string
  workflow: Workflow
  definitions: NodeDefinition[]
  blueprints: Blueprint[]
  onReload: () => void
}) {
  const canEdit = useCan(organizationId, "edit_workflows")
  const canPublish = useCan(organizationId, "publish_workflows")
  const canRun = useCan(organizationId, "run_workflows")
  const wide = useWideScreen()
  const readOnlyReason = !canEdit
    ? "Your role can view this workflow but not edit it."
    : !wide
      ? "Editing needs a screen at least 1024 pixels wide."
      : null
  const autosave = useDraftAutosave(
    organizationId,
    workflow.id,
    workflow.draft,
    readOnlyReason !== null,
  )
  const validate = useValidateWorkflow(organizationId, workflow.id)
  const publish = usePublishWorkflow(organizationId, workflow.id)
  const settings = useSettings(organizationId)
  // The shell preset (and any definition on the shell executor) is offered
  // only once an admin enables shell.
  const paletteDefinitions = settings.data?.shellEnabled
    ? definitions
    : definitions.filter(({ executor }) => executor.key !== "shell")
  const queryClient = useQueryClient()
  const [problems, setProblems] = useState<{
    graph: GraphSnapshot
    items: MappedDiagnostic[]
  } | null>(null)
  const [notice, setNotice] = useState("")
  const [busy, setBusy] = useState(false)
  const problemsId = useId()

  function show(graph: GraphSnapshot, diagnostics: Diagnostic[]) {
    const count = diagnostics.length
    setProblems(
      count > 0 ? { graph, items: mapDiagnostics(graph, diagnostics) } : null,
    )
    setNotice(
      count === 0
        ? "No problems found."
        : `${count} problem${count === 1 ? "" : "s"} found.`,
    )
  }

  async function validateDraft({ graph }: SavedDraft) {
    try {
      show(graph, (await validate.mutateAsync(graph)).diagnostics)
    } catch (error) {
      const code = error instanceof ApiProblem ? error.code : "network_error"
      setNotice(`Validation failed (${code}).`)
    }
  }

  async function publishDraft({ revision, graph }: SavedDraft) {
    try {
      const { version } = await publish.mutateAsync(revision)
      setProblems(null)
      setNotice(`Published v${version}.`)
    } catch (error) {
      const problem =
        error instanceof ApiProblem ? error : new ApiProblem(0, "network_error")
      if (problem.code === "graph_invalid") {
        show(graph, problem.diagnostics)
      } else if (problem.code === "draft_revision_conflict") {
        autosave.conflict()
      } else if (problem.code === "draft_already_published") {
        // That version is the latest one: read it from the workflow.
        const queryKey = queryKeys.workflow(organizationId, workflow.id)
        await queryClient.refetchQueries({ queryKey })
        const latest = queryClient.getQueryData<Workflow>(queryKey)
        setNotice(`Already published as v${latest?.publishedVersion}.`)
      } else {
        setNotice(`Publishing failed (${problem.code}).`)
      }
    }
  }

  // Both act on the saved draft, so pending edits are saved first.
  async function run(task: (saved: SavedDraft) => Promise<void>) {
    setBusy(true)
    setNotice("")
    const saved = await autosave.flush()
    if (saved) await task(saved)
    else setNotice("Save the draft first: the last save failed.")
    setBusy(false)
  }

  const conflict = autosave.status === "conflict"
  return (
    <section className="flex h-dvh flex-col">
      <header className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b px-4 py-3 text-sm">
        <nav aria-label="Breadcrumb">
          <ol className="flex items-center gap-2">
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
            <li aria-current="page">
              <h1 className="text-lg font-semibold tracking-tight">
                {workflow.name}
              </h1>
            </li>
          </ol>
        </nav>
        {readOnlyReason ? (
          <>
            <StatusBadge status="read-only" />
            <span className="text-muted-foreground">{readOnlyReason}</span>
          </>
        ) : (
          <StatusBadge status={autosave.status} />
        )}
        {autosave.status === "error" && (
          <span role="alert" className="text-destructive">
            The draft could not be saved ({autosave.errorCode}). The next edit
            tries again.
          </span>
        )}
        <span className="text-muted-foreground">rev {autosave.revision}</span>
        {workflow.publishedVersion !== null && (
          <span className="text-muted-foreground">
            v{workflow.publishedVersion} published
          </span>
        )}
        <span aria-live="polite">{notice}</span>
        {(canPublish || canRun) && (
          <div className="ml-auto flex items-center gap-2">
            {canPublish && (
              <>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={busy || conflict}
                  onClick={() => void run(validateDraft)}
                >
                  Validate
                </Button>
                <Button
                  size="sm"
                  disabled={busy || conflict}
                  onClick={() => void run(publishDraft)}
                >
                  Publish
                </Button>
              </>
            )}
            {canRun && (
              <RunDialog
                organizationId={organizationId}
                workflowId={workflow.id}
                version={workflow.publishedVersion}
              />
            )}
          </div>
        )}
      </header>
      {conflict && (
        <div
          role="alert"
          className="flex flex-wrap items-center gap-3 border-b bg-muted px-4 py-2 text-sm"
        >
          This draft was saved somewhere else. Reload it to keep editing;
          changes made here since the last save are discarded.
          <Button variant="outline" size="sm" onClick={onReload}>
            Reload
          </Button>
        </div>
      )}
      {problems && (
        <section
          aria-labelledby={problemsId}
          className="max-h-40 overflow-y-auto border-b px-4 py-2 text-sm"
        >
          <h2 id={problemsId} className="font-medium">
            Problems
          </h2>
          <ul aria-labelledby={problemsId} className="mt-1 grid gap-1">
            {problems.items.map((problem, index) => (
              <li key={index}>
                <span className="font-medium">
                  {place(problems.graph, problem)}
                </span>{" "}
                {problem.message}{" "}
                <span className="font-mono text-xs text-muted-foreground">
                  {problem.code}
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}
      <div
        className={cn(
          "grid min-h-0 flex-1",
          !readOnlyReason && "grid-cols-[16rem_1fr]",
        )}
      >
        {!readOnlyReason && (
          <Palette definitions={paletteDefinitions} blueprints={blueprints} />
        )}
        <StudioCanvas
          graph={workflow.draft.graph}
          definitions={definitions}
          blueprints={blueprints}
          readOnly={readOnlyReason !== null}
          problems={problems?.items ?? noProblems}
          onChange={autosave.edit}
        />
      </div>
      <RunsDrawer organizationId={organizationId} workflowId={workflow.id} />
    </section>
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
  // Reloading after a conflict starts the editor over from the latest draft.
  const [generation, setGeneration] = useState(0)

  // The editor starts from a draft fetched on this visit, not from the cache:
  // the draft may have been saved since. Once it has started, a failed
  // background refetch keeps it on screen.
  // ponytail: a save from the last visit that lands after this fetch makes the
  // next edit a conflict, which Reload resolves.
  if (
    !workflow.isFetchedAfterMount ||
    !workflow.data ||
    !definitions.data ||
    !blueprints.data
  ) {
    return workflow.isError || definitions.isError || blueprints.isError ? (
      <p role="alert" className="p-4 text-sm text-destructive md:p-6">
        The workflow could not be loaded.
      </p>
    ) : (
      <p className="p-4 text-sm text-muted-foreground md:p-6">
        Loading workflow…
      </p>
    )
  }

  async function reload() {
    const result = await workflow.refetch()
    if (result.isSuccess) setGeneration((current) => current + 1)
  }

  return (
    <Editor
      key={generation}
      organizationId={organizationId}
      workflow={workflow.data}
      definitions={definitions.data.items}
      blueprints={blueprints.data.items}
      onReload={() => void reload()}
    />
  )
}
