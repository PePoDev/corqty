import { useQueryClient } from "@tanstack/react-query"
import { useEffect, useState } from "react"

import { ApiProblem, api } from "@/api/client"
import { queryKeys } from "@/api/queries"
import type { GraphSnapshot, Workflow } from "@/api/types"

export const AUTOSAVE_DELAY_MS = 800

export type SaveStatus = "saved" | "saving" | "conflict" | "error"

export type SavedDraft = { revision: number; graph: GraphSnapshot }

type Report = { status: SaveStatus; revision: number; errorCode: string }

function createAutosave(
  path: string,
  draft: SavedDraft,
  report: (state: Report) => void,
  onSaved: (draft: SavedDraft) => void,
) {
  let { revision } = draft
  // What the server holds at `revision`, and the latest edit.
  let saved = { graph: draft.graph, json: JSON.stringify(draft.graph) }
  let latest: typeof saved | undefined
  let timer: ReturnType<typeof setTimeout> | undefined
  let queue = Promise.resolve()
  let queued = 0
  let halted: "conflict" | "error" | undefined
  let errorCode = ""

  // An edit waits for its timer, or a save is in flight.
  function busy() {
    return timer !== undefined || queued > 0
  }

  function update() {
    report({
      status: halted ?? (busy() ? "saving" : "saved"),
      revision,
      errorCode,
    })
  }

  async function save() {
    const sending = latest
    try {
      if (halted || !sending || sending.json === saved.json) return
      const result = await api<{ revision: number }>(path, {
        method: "PUT",
        body: JSON.stringify({ revision, graph: sending.graph }),
      })
      revision = result.revision
      saved = sending
      onSaved({ revision, graph: saved.graph })
    } catch (error) {
      clearTimeout(timer)
      timer = undefined
      const problem = error instanceof ApiProblem ? error : undefined
      halted = problem?.status === 409 ? "conflict" : "error"
      errorCode = problem?.code ?? "network_error"
    } finally {
      queued -= 1
      update()
    }
  }

  // Each save waits for the one before it, so one request is in flight.
  function enqueue() {
    queued += 1
    queue = queue.then(save)
    update()
  }

  return {
    busy,

    edit(graph: GraphSnapshot, readOnly: boolean) {
      const json = JSON.stringify(graph)
      // The canvas reports its graph after every render. The first report is
      // the loaded draft as the canvas holds it; later ones may change nothing.
      if (!latest) {
        latest = { graph, json }
        saved = latest
        return
      }
      if (json === latest.json) return
      latest = { graph, json }
      if (readOnly || halted === "conflict") return
      halted = undefined
      const idle = timer === undefined
      clearTimeout(timer)
      timer = setTimeout(() => {
        timer = undefined
        enqueue()
      }, AUTOSAVE_DELAY_MS)
      // Later edits in a burst leave the status as it is.
      if (idle) update()
    },

    // Saves pending edits now, retrying a save that failed, and waits until
    // no save is due or in flight. Resolves to the saved draft, or null when
    // it could not be saved.
    async flush(): Promise<SavedDraft | null> {
      if (halted === "error") {
        halted = undefined
        enqueue()
      }
      let current: Promise<void>
      do {
        if (timer !== undefined) {
          clearTimeout(timer)
          timer = undefined
          enqueue()
        }
        current = queue
        await current
      } while (current !== queue || timer !== undefined)
      return halted ? null : { revision, graph: saved.graph }
    },

    conflict() {
      clearTimeout(timer)
      timer = undefined
      halted = "conflict"
      update()
    },
  }
}

// Saves the draft 800 ms after the last edit. A 409 stops saving until the
// studio reloads the draft; any other failure stops it until the next edit or
// flush. Each save also updates the cached workflow, so the studio reopens at
// the saved revision.
export function useDraftAutosave(
  organizationId: string,
  workflowId: string,
  draft: SavedDraft,
  readOnly: boolean,
) {
  const queryClient = useQueryClient()
  const [state, setState] = useState<Report>({
    status: "saved",
    revision: draft.revision,
    errorCode: "",
  })
  const [autosave] = useState(() =>
    createAutosave(
      `/orgs/${organizationId}/workflows/${workflowId}/draft`,
      draft,
      setState,
      (saved) =>
        queryClient.setQueryData<Workflow>(
          queryKeys.workflow(organizationId, workflowId),
          (workflow) => workflow && { ...workflow, draft: saved },
        ),
    ),
  )
  useEffect(() => {
    // Closing or reloading the page asks first while an edit is unsaved.
    function warn(event: BeforeUnloadEvent) {
      if (autosave.busy()) event.preventDefault()
    }
    window.addEventListener("beforeunload", warn)
    return () => {
      window.removeEventListener("beforeunload", warn)
      // Leaving the studio saves pending edits at once instead of dropping them.
      void autosave.flush()
    }
  }, [autosave])
  return {
    ...state,
    edit: (graph: GraphSnapshot) => autosave.edit(graph, readOnly),
    flush: autosave.flush,
    conflict: autosave.conflict,
  }
}
