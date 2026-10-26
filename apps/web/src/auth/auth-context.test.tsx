import { act, renderHook, waitFor } from "@testing-library/react"
import { http, HttpResponse } from "msw/http"
import { expect, it } from "vitest"

import { AuthProvider, useAuth } from "@/auth/auth-context"
import { server } from "@/test/server"

it("refresh keeps the session on a server error and clears it on 401", async () => {
  const { result } = renderHook(() => useAuth(), { wrapper: AuthProvider })
  await waitFor(() => {
    expect(result.current.me?.user.email).toBe("admin@example.com")
  })

  server.use(
    http.get("/api/v1/auth/me", () =>
      HttpResponse.json({ code: "internal" }, { status: 500 }),
    ),
  )
  await act(() => result.current.refresh())
  expect(result.current.me?.user.email).toBe("admin@example.com")

  server.use(
    http.get("/api/v1/auth/me", () => new HttpResponse(null, { status: 401 })),
  )
  await act(() => result.current.refresh())
  expect(result.current.me).toBeNull()
})
