import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { act, cleanup, renderHook } from "@testing-library/react"
import { http, HttpResponse } from "msw/http"
import { createElement, type ReactNode } from "react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import { queryKeys } from "@/api/queries"
import type { GraphSnapshot, Workflow } from "@/api/types"
import {
  AUTOSAVE_DELAY_MS,
  useDraftAutosave,
} from "@/studio/use-draft-autosave"
import { workflow } from "@/test/handlers"
import { server } from "@/test/server"

type Save = { revision: number; graph: GraphSnapshot }

const { draft } = workflow // revision 3
const workflowKey = queryKeys.workflow("org-1", "wf-2")
// The studio's query cache, holding the workflow as it was loaded.
let queryClient: QueryClient

function named(displayName: string): GraphSnapshot {
  const [first, ...rest] = draft.graph.nodes
  return { ...draft.graph, nodes: [{ ...first, displayName }, ...rest] }
}

// Records every save and answers it with `respond`, by default with the next
// revision.
function recordSaves(
  respond: (save: Save) => Response | Promise<Response> = ({ revision }) =>
    HttpResponse.json({ revision: revision + 1 }),
) {
  const saves: Save[] = []
  server.use(
    http.put("/api/v1/orgs/org-1/workflows/wf-2/draft", async ({ request }) => {
      const save = (await request.json()) as Save
      saves.push(save)
      return respond(save)
    }),
  )
  return saves
}

function problem(status: number, code: string) {
  return HttpResponse.json({ status, code }, { status })
}

// The canvas reports its graph once on mount: that is the loaded draft.
function renderAutosave(readOnly = false) {
  queryClient = new QueryClient()
  queryClient.setQueryData(workflowKey, workflow)
  const { result } = renderHook(
    () => useDraftAutosave("org-1", "wf-2", draft, readOnly),
    {
      wrapper: ({ children }: { children: ReactNode }) =>
        createElement(QueryClientProvider, { client: queryClient }, children),
    },
  )
  act(() => result.current.edit(draft.graph))
  return result
}

type Autosave = ReturnType<typeof renderAutosave>

async function wait(ms: number) {
  await act(() => vi.advanceTimersByTimeAsync(ms))
}

// Waits for the saves already started, so React sees their updates in act.
// (Testing Library's waitFor needs a real setTimeout.)
async function settle(autosave: Autosave) {
  await act(() => autosave.current.flush())
}

// True while the page asks the browser to confirm leaving: an edit waits for
// its timer, or a save is in flight.
function unloadBlocked() {
  return !window.dispatchEvent(new Event("beforeunload", { cancelable: true }))
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] })
})

afterEach(() => {
  vi.useRealTimers()
})

it("saves once, 800 ms after the last edit", async () => {
  const saves = recordSaves()
  const autosave = renderAutosave()
  act(() => autosave.current.edit(named("A")))
  await wait(500)
  act(() => autosave.current.edit(named("B")))
  expect(autosave.current.status).toBe("saving")
  await wait(AUTOSAVE_DELAY_MS - 1)
  expect(saves).toEqual([])

  await wait(1)
  await settle(autosave)
  expect(autosave.current.status).toBe("saved")
  expect(saves).toEqual([{ revision: 3, graph: named("B") }])
  expect(autosave.current.revision).toBe(4)
})

it("sends one save at a time", async () => {
  let answer = () => {}
  const saves = recordSaves(async ({ revision }) => {
    await new Promise<void>((resolve) => {
      answer = resolve
    })
    return HttpResponse.json({ revision: revision + 1 })
  })
  const autosave = renderAutosave()
  act(() => autosave.current.edit(named("A")))
  await wait(AUTOSAVE_DELAY_MS)
  await vi.waitUntil(() => saves.length === 1)

  act(() => autosave.current.edit(named("B")))
  await wait(AUTOSAVE_DELAY_MS)
  expect(saves).toHaveLength(1)
  await act(async () => {
    answer()
    await vi.waitUntil(() => saves.length === 2)
    answer()
    await autosave.current.flush()
  })
  expect(autosave.current.status).toBe("saved")
  expect(saves).toEqual([
    { revision: 3, graph: named("A") },
    { revision: 4, graph: named("B") },
  ])
  expect(autosave.current.revision).toBe(5)
})

it("a conflict stops saving for good", async () => {
  const saves = recordSaves(() => problem(409, "draft_revision_conflict"))
  const autosave = renderAutosave()
  act(() => autosave.current.edit(named("A")))
  await wait(AUTOSAVE_DELAY_MS)
  await settle(autosave)
  expect(autosave.current.status).toBe("conflict")

  act(() => autosave.current.edit(named("B")))
  await wait(AUTOSAVE_DELAY_MS)
  expect(autosave.current.status).toBe("conflict")
  expect(await act(() => autosave.current.flush())).toBeNull()
  expect(saves).toHaveLength(1)
})

it.each([
  [413, "payload_too_large"],
  [422, "graph_invalid"],
])("a %i stops saving until the next edit", async (status, code) => {
  let failing = true
  const saves = recordSaves(({ revision }) =>
    failing
      ? problem(status, code)
      : HttpResponse.json({ revision: revision + 1 }),
  )
  const autosave = renderAutosave()
  act(() => autosave.current.edit(named("A")))
  await wait(AUTOSAVE_DELAY_MS)
  // A flush would retry the save, so this only waits for it to fail.
  await act(() => vi.waitUntil(() => !unloadBlocked()))
  expect(autosave.current.status).toBe("error")
  expect(autosave.current.errorCode).toBe(code)
  await wait(AUTOSAVE_DELAY_MS * 2)
  expect(saves).toHaveLength(1)

  failing = false
  act(() => autosave.current.edit(named("B")))
  await wait(AUTOSAVE_DELAY_MS)
  await settle(autosave)
  expect(autosave.current.status).toBe("saved")
  expect(saves).toEqual([
    { revision: 3, graph: named("A") },
    { revision: 3, graph: named("B") },
  ])
})

it("flush saves pending edits at once and returns the saved draft", async () => {
  const saves = recordSaves()
  const autosave = renderAutosave()
  act(() => autosave.current.edit(named("A")))
  expect(await act(() => autosave.current.flush())).toEqual({
    revision: 4,
    graph: named("A"),
  })
  expect(saves).toEqual([{ revision: 3, graph: named("A") }])
  await wait(AUTOSAVE_DELAY_MS)
  expect(saves).toHaveLength(1)
})

it("read-only mode never saves", async () => {
  const saves = recordSaves()
  const autosave = renderAutosave(true)
  act(() => autosave.current.edit(named("A")))
  await wait(AUTOSAVE_DELAY_MS)
  expect(autosave.current.status).toBe("saved")
  expect(await act(() => autosave.current.flush())).toEqual(draft)
  expect(saves).toEqual([])
})

it("flush also waits for a save chained behind the one in flight", async () => {
  const answers: (() => void)[] = []
  const saves = recordSaves(async ({ revision }) => {
    await new Promise<void>((resolve) => answers.push(resolve))
    return HttpResponse.json({ revision: revision + 1 })
  })
  const autosave = renderAutosave()
  act(() => autosave.current.edit(named("A")))
  await wait(AUTOSAVE_DELAY_MS)
  await vi.waitUntil(() => saves.length === 1)

  // Publish flushes while A saves, and an edit meanwhile chains B behind A.
  const flushed = autosave.current.flush()
  act(() => autosave.current.edit(named("B")))
  await wait(AUTOSAVE_DELAY_MS)
  const result = await act(async () => {
    answers[0]()
    await vi.waitUntil(() => saves.length === 2)
    answers[1]()
    return flushed
  })
  expect(result).toEqual({ revision: 5, graph: named("B") })
})

it("each save updates the cached workflow, including the save on leaving", async () => {
  const autosave = renderAutosave()
  const cached = () => queryClient.getQueryData<Workflow>(workflowKey)
  act(() => autosave.current.edit(named("A")))
  await wait(AUTOSAVE_DELAY_MS)
  await settle(autosave)
  expect(cached()).toEqual({
    ...workflow,
    draft: { revision: 4, graph: named("A") },
  })

  // Unmounting, as leaving the studio does, saves the pending edit.
  act(() => autosave.current.edit(named("B")))
  cleanup()
  await vi.waitFor(() =>
    expect(cached()).toEqual({
      ...workflow,
      draft: { revision: 5, graph: named("B") },
    }),
  )
})

it("asks before unloading while an edit waits or saves", async () => {
  let answer = () => {}
  const saves = recordSaves(async ({ revision }) => {
    await new Promise<void>((resolve) => {
      answer = resolve
    })
    return HttpResponse.json({ revision: revision + 1 })
  })
  const autosave = renderAutosave()
  expect(unloadBlocked()).toBe(false)

  act(() => autosave.current.edit(named("A")))
  expect(unloadBlocked()).toBe(true)
  await wait(AUTOSAVE_DELAY_MS)
  await vi.waitUntil(() => saves.length === 1)
  expect(unloadBlocked()).toBe(true)

  answer()
  await settle(autosave)
  expect(unloadBlocked()).toBe(false)
})
