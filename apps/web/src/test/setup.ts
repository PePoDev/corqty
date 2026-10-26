// First: msw/sse handlers need a global EventSource when they are created.
import "@/test/event-source"
import "@testing-library/jest-dom/vitest"
import { cleanup } from "@testing-library/react"
import { afterAll, afterEach, beforeAll, vi } from "vitest"

import { server } from "@/test/server"

vi.mock("next/navigation", async () => {
  const { router } = await import("@/test/render")
  return {
    usePathname: () => window.location.pathname,
    useRouter: () => router,
  }
})

// XYFlow measures nodes with layout APIs that jsdom lacks. Elements report
// the size of their inline style (1 px otherwise), and the observer reports it.
class ResizeObserverStub {
  callback: ResizeObserverCallback
  constructor(callback: ResizeObserverCallback) {
    this.callback = callback
  }
  observe(target: HTMLElement) {
    setTimeout(() => {
      const { offsetWidth: width, offsetHeight: height } = target
      const contentRect = { x: 0, y: 0, top: 0, left: 0, width, height }
      this.callback(
        [{ target, contentRect } as unknown as ResizeObserverEntry],
        this as unknown as ResizeObserver,
      )
    })
  }
  unobserve() {}
  disconnect() {}
}

class DOMMatrixReadOnlyStub {
  m22: number
  constructor(transform: string) {
    const scale = /scale\(([\d.]+)\)/.exec(transform)?.[1]
    this.m22 = scale === undefined ? 1 : Number(scale)
  }
}

Object.assign(globalThis, {
  ResizeObserver: ResizeObserverStub,
  DOMMatrixReadOnly: DOMMatrixReadOnlyStub,
})
Object.defineProperties(HTMLElement.prototype, {
  offsetHeight: {
    get(this: HTMLElement) {
      return parseFloat(this.style.height) || 1
    },
  },
  offsetWidth: {
    get(this: HTMLElement) {
      return parseFloat(this.style.width) || 1
    },
  },
})
Object.assign(SVGElement.prototype, {
  getBBox: () => ({ x: 0, y: 0, width: 0, height: 0 }),
})

// jsdom has no matchMedia either. min-width queries compare with
// window.innerWidth (1024 in jsdom), which tests may change; nothing else
// matches.
Object.assign(window, {
  matchMedia: (query: string) => ({
    media: query,
    matches:
      window.innerWidth >=
      Number(/\(min-width: (\d+)px\)/.exec(query)?.[1] ?? Infinity),
    addEventListener() {},
    removeEventListener() {},
  }),
})

beforeAll(() => server.listen({ onUnhandledFrame: "error" }))
afterEach(() => {
  cleanup()
  server.resetHandlers()
  sessionStorage.clear()
  vi.clearAllMocks()
  Object.assign(window, { innerWidth: 1024 })
})
afterAll(() => server.close())
