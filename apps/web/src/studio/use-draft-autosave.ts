import { useEffect, useState } from "react"

import { ApiProblem, api } from "@/api/client"
import type { GraphSnapshot } from "@/api/types"

export const AUTOSAVE_DELAY_MS = 800

export type SaveStatus = "saved" | "saving" | "conflict" | "error"

export type SavedDraft = { revision: number; graph: GraphSnapshot }

type Report = { status: SaveStatus; revision: number; errorCode: string }

function createAutosave(
  path: string,
  draft: SavedDraft,
  report: (state: Report) => void,
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

  function update() {
    const busy = timer !== undefined || queued > 0
    report({
      status: halted ?? (busy ? "saving" : "saved"),
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

    // Saves pending edits now and waits for every save. Resolves to the saved
    // draft, or null when it could not be saved.
    async flush(): Promise<SavedDraft | null> {
      if (timer !== undefined) {
        clearTimeout(timer)
        timer = undefined
        enqueue()
      }
      await queue
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
// studio reloads the draft; any other failure stops it until the next edit.
export function useDraftAutosave(
  organizationId: string,
  workflowId: string,
  draft: SavedDraft,
  readOnly: boolean,
) {
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
    ),
  )
  // Leaving the studio saves pending edits at once instead of dropping them.
  useEffect(() => () => void autosave.flush(), [autosave])
  return {
    ...state,
    edit: (graph: GraphSnapshot) => autosave.edit(graph, readOnly),
    flush: autosave.flush,
    conflict: autosave.conflict,
  }
}
