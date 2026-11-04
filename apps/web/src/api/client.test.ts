import { http, HttpResponse } from "msw/http"
import { expect, it } from "vitest"

import { api } from "@/api/client"
import { server } from "@/test/server"

it("sets the JSON content type only for string bodies without one", async () => {
  const contentTypes: (string | null)[] = []
  server.use(
    http.post("/api/v1/echo", ({ request }) => {
      contentTypes.push(request.headers.get("content-type"))
      return new HttpResponse(null, { status: 204 })
    }),
  )
  await api("/echo", { method: "POST", body: "{}" })
  await api("/echo", {
    method: "POST",
    body: "plain",
    headers: { "content-type": "text/plain" },
  })
  await api("/echo", { method: "POST", body: new Uint8Array([1]) })
  expect(contentTypes).toEqual(["application/json", "text/plain", null])
})

it("keeps problem diagnostics and defaults them to an empty list", async () => {
  const diagnostics = [
    { code: "cycle", pointer: "/edges/0", message: "Edge closes a cycle." },
  ]
  server.use(
    http.post("/api/v1/echo", () =>
      HttpResponse.json(
        { code: "graph_invalid", diagnostics },
        { status: 422 },
      ),
    ),
    http.get("/api/v1/echo", () => new HttpResponse(null, { status: 500 })),
  )
  await expect(
    api("/echo", { method: "POST", body: "{}" }),
  ).rejects.toMatchObject({ status: 422, code: "graph_invalid", diagnostics })
  await expect(api("/echo")).rejects.toMatchObject({
    status: 500,
    code: "unknown",
    diagnostics: [],
  })
})
