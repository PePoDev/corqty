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

beforeAll(() => server.listen({ onUnhandledFrame: "error" }))
afterEach(() => {
  cleanup()
  server.resetHandlers()
  sessionStorage.clear()
  vi.clearAllMocks()
})
afterAll(() => server.close())
