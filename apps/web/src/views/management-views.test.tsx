import { fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { http, HttpResponse } from "msw/http"
import type { ReactElement } from "react"
import { expect, it } from "vitest"

import type {
  RuntimePlugin,
  SecretMetadata,
  ServiceToken,
  Trigger,
} from "@/api/types"
import { AuthGate } from "@/auth/auth-gate"
import { AppShell } from "@/layout/app-shell"
import { meAs, workflows } from "@/test/handlers"
import { renderWithProviders } from "@/test/render"
import { server } from "@/test/server"
import { PluginsView } from "@/views/plugins-view"
import { SecretsView } from "@/views/secrets-view"
import { ServiceTokensView } from "@/views/service-tokens-view"
import { TriggersView } from "@/views/triggers-view"

const org = "/api/v1/orgs/org-1"
const now = "2026-11-04T00:00:00Z"
const secret: SecretMetadata = {
  id: "secret-1",
  name: "Deployment key",
  createdAt: now,
  updatedAt: now,
}
const plugin: RuntimePlugin = {
  id: "plugin-1",
  key: "sample",
  version: "1.0.0",
  hostContract: "1",
  checksum: "a".repeat(64),
  enabled: true,
  approved: false,
  createdAt: now,
  manifest: {
    key: "sample",
    version: "1.0.0",
    hostContract: "1",
    capabilities: [
      { type: "log" },
      { type: "network", host: "api.example.com", port: 443 },
    ],
  },
}

function renderPage(view: ReactElement, route: string) {
  return renderWithProviders(
    <AuthGate>
      <AppShell organizationId="org-1">{view}</AppShell>
    </AuthGate>,
    { route: `/orgs/org-1/${route}` },
  )
}

it("creates and replaces a secret without keeping plaintext in the form", async () => {
  const writes: unknown[] = []
  let items: SecretMetadata[] = []
  server.use(
    http.get(`${org}/secrets`, () => HttpResponse.json({ items })),
    http.post(`${org}/secrets`, async ({ request }) => {
      writes.push(await request.json())
      items = [secret]
      return HttpResponse.json(secret, { status: 201 })
    }),
    http.put(`${org}/secrets/${secret.id}`, async ({ request }) => {
      writes.push(await request.json())
      return HttpResponse.json(secret)
    }),
  )
  renderPage(<SecretsView organizationId="org-1" />, "secrets")
  await userEvent.type(await screen.findByLabelText("Name"), secret.name)
  await userEvent.type(screen.getByLabelText("Value"), "private-first-value")
  await userEvent.click(screen.getByRole("button", { name: "Create secret" }))
  expect(await screen.findByText("Secret created.")).toBeInTheDocument()
  expect(screen.getByLabelText("Value")).toHaveValue("")
  expect(screen.queryByText("private-first-value")).not.toBeInTheDocument()
  await userEvent.click(screen.getByRole("button", { name: "Replace value" }))
  expect(screen.getByLabelText("New value")).toHaveValue("")
  await userEvent.type(
    screen.getByLabelText("New value"),
    "private-replacement",
  )
  await userEvent.click(screen.getByRole("button", { name: "Replace secret" }))
  expect(
    await screen.findByText(
      "Secret updated. Future executions use the new value.",
    ),
  ).toBeInTheDocument()
  expect(writes).toEqual([
    { name: secret.name, value: "private-first-value" },
    { name: secret.name, value: "private-replacement" },
  ])
})

it("explains secret deletion protection and retains the item", async () => {
  server.use(
    http.get(`${org}/secrets`, () => HttpResponse.json({ items: [secret] })),
    http.delete(`${org}/secrets/${secret.id}`, () =>
      HttpResponse.json({ code: "secret_in_use" }, { status: 409 }),
    ),
  )
  renderPage(<SecretsView organizationId="org-1" />, "secrets")
  await userEvent.click(await screen.findByRole("button", { name: "Delete" }))
  await userEvent.click(screen.getByRole("button", { name: "Confirm delete" }))
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "referenced by a draft",
  )
  expect(screen.getByRole("heading", { name: secret.name })).toBeInTheDocument()
})

it("editors see secret names and protection limits without write actions", async () => {
  server.use(
    http.get("/api/v1/auth/me", () => HttpResponse.json(meAs("editor"))),
    http.get(`${org}/secrets`, () => HttpResponse.json({ items: [secret] })),
  )
  renderPage(<SecretsView organizationId="org-1" />, "secrets")
  expect(await screen.findByText(secret.name)).toBeInTheDocument()
  expect(
    screen.queryByRole("button", { name: "Create secret" }),
  ).not.toBeInTheDocument()
  expect(
    screen.queryByRole("button", { name: "Replace value" }),
  ).not.toBeInTheDocument()
  expect(screen.getByText(/Encoded, transformed/)).toBeInTheDocument()
})

it("requires reviewing capabilities before approving the exact component checksum", async () => {
  let approval: unknown
  let approved = false
  server.use(
    http.get(`${org}/plugins`, () =>
      HttpResponse.json({ items: [{ ...plugin, approved }] }),
    ),
    http.put(`${org}/plugins/${plugin.id}/approval`, async ({ request }) => {
      approval = await request.json()
      approved = true
      return HttpResponse.json({ ...plugin, approved })
    }),
  )
  renderPage(<PluginsView organizationId="org-1" />, "plugins")
  expect(
    await screen.findByText("Network: api.example.com:443"),
  ).toBeInTheDocument()
  await userEvent.click(screen.getByRole("button", { name: "Review approval" }))
  const confirm = screen.getByRole("button", { name: "Confirm approval" })
  expect(confirm).toBeDisabled()
  await userEvent.click(screen.getByRole("checkbox"))
  await userEvent.click(confirm)
  expect(await screen.findByText("sample@1.0.0 approved.")).toBeInTheDocument()
  expect(approval).toEqual({
    checksum: plugin.checksum,
    capabilities: plugin.manifest.capabilities,
  })
  expect(
    screen.queryByRole("button", { name: "Review approval" }),
  ).not.toBeInTheDocument()
})

it("uploads component bytes and manifest together, rejecting core modules", async () => {
  let installed: unknown
  server.use(
    http.post(`${org}/plugins`, async ({ request }) => {
      installed = await request.json()
      return HttpResponse.json(plugin, { status: 201 })
    }),
  )
  renderPage(<PluginsView organizationId="org-1" />, "plugins")
  const fileInput = await screen.findByLabelText(
    "WebAssembly Component (.wasm)",
  )
  await userEvent.upload(
    fileInput,
    new File([new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0])], "core.wasm", {
      type: "application/wasm",
    }),
  )
  expect((fileInput as HTMLInputElement).files).toHaveLength(1)
  expect(fileInput.closest("form")).toBeValid()
  await userEvent.click(screen.getByRole("button", { name: "Install plugin" }))
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "not a WebAssembly Component",
  )
  expect(installed).toBeUndefined()
  const bytes = new Uint8Array([0, 97, 115, 109, 13, 0, 1, 0])
  await userEvent.upload(
    fileInput,
    new File([bytes], "sample.wasm", { type: "application/wasm" }),
  )
  await userEvent.click(screen.getByRole("button", { name: "Install plugin" }))
  expect(await screen.findByText(/Plugin installed/)).toBeInTheDocument()
  expect(installed).toEqual({
    manifest: { ...plugin.manifest, capabilities: [{ type: "log" }] },
    componentBase64: "AGFzbQ0AAQA=",
  })
})

it("shows a token only once and revokes it after confirmation", async () => {
  let items: ServiceToken[] = []
  const token: ServiceToken = {
    id: "token-1",
    name: "CI deploy",
    scope: "workflow:run",
    createdAt: now,
    revokedAt: null,
  }
  let revoked = false
  server.use(
    http.get(`${org}/service-tokens`, () => HttpResponse.json({ items })),
    http.post(`${org}/service-tokens`, () => {
      items = [token]
      return HttpResponse.json(
        { ...token, token: "once-only-bearer" },
        { status: 201 },
      )
    }),
    http.delete(`${org}/service-tokens/${token.id}`, () => {
      revoked = true
      items = [{ ...token, revokedAt: now }]
      return new HttpResponse(null, { status: 204 })
    }),
  )
  renderPage(<ServiceTokensView organizationId="org-1" />, "service-tokens")
  await userEvent.type(await screen.findByLabelText("Token name"), "CI deploy")
  await userEvent.click(
    screen.getByRole("button", { name: "Create service token" }),
  )
  expect(await screen.findByText("once-only-bearer")).toBeInTheDocument()
  await userEvent.click(screen.getByRole("button", { name: "I saved it" }))
  expect(screen.queryByText("once-only-bearer")).not.toBeInTheDocument()
  await userEvent.click(screen.getByRole("button", { name: "Revoke" }))
  expect(revoked).toBe(false)
  await userEvent.click(screen.getByRole("button", { name: "Confirm revoke" }))
  expect(await screen.findByText("Revoked")).toBeInTheDocument()
  expect(revoked).toBe(true)
})

it("creates an editor's cron trigger with an exact version and timezone", async () => {
  let created: unknown
  server.use(
    http.get("/api/v1/auth/me", () => HttpResponse.json(meAs("editor"))),
    http.post(`${org}/triggers`, async ({ request }) => {
      created = await request.json()
      return HttpResponse.json(
        {
          ...(created as object),
          id: "trigger-1",
          createdAt: now,
          updatedAt: now,
          nextScheduledAt: now,
        },
        { status: 201 },
      )
    }),
  )
  renderPage(<TriggersView organizationId="org-1" />, "triggers")
  await userEvent.type(await screen.findByLabelText("Name"), "Hourly sync")
  await userEvent.selectOptions(
    screen.getByLabelText("Published workflow"),
    workflows[0].id,
  )
  fireEvent.change(screen.getByLabelText("Timezone"), {
    target: { value: "Asia/Bangkok" },
  })
  await userEvent.click(screen.getByRole("button", { name: "Create trigger" }))
  expect(
    await screen.findByText("Trigger created and pinned to v1."),
  ).toBeInTheDocument()
  expect(created).toEqual({
    name: "Hourly sync",
    workflowId: workflows[0].id,
    version: 1,
    kind: "cron",
    cron: "0 * * * *",
    timezone: "Asia/Bangkok",
    input: {},
    enabled: true,
    overlapPolicy: "skip",
  })
})

it("rejects malformed trigger input and reveals a new webhook secret once", async () => {
  let calls = 0
  server.use(
    http.post(`${org}/triggers`, async ({ request }) => {
      calls++
      return HttpResponse.json(
        {
          ...((await request.json()) as object),
          id: "trigger-1",
          createdAt: now,
          updatedAt: now,
          nextScheduledAt: null,
          webhookSecret: "webhook-once",
          webhookPath: "/hooks/v1/trigger-1",
        },
        { status: 201 },
      )
    }),
  )
  renderPage(<TriggersView organizationId="org-1" />, "triggers")
  await userEvent.type(await screen.findByLabelText("Name"), "Deploy hook")
  await userEvent.selectOptions(
    screen.getByLabelText("Published workflow"),
    workflows[0].id,
  )
  await userEvent.selectOptions(
    screen.getByLabelText("Trigger type"),
    "webhook",
  )
  fireEvent.change(screen.getByLabelText("Workflow input (JSON)"), {
    target: { value: "[]" },
  })
  await userEvent.click(screen.getByRole("button", { name: "Create trigger" }))
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "must be a JSON object",
  )
  expect(calls).toBe(0)
  fireEvent.change(screen.getByLabelText("Workflow input (JSON)"), {
    target: { value: "{}" },
  })
  await userEvent.click(screen.getByRole("button", { name: "Create trigger" }))
  expect(await screen.findByText("webhook-once")).toBeInTheDocument()
  expect(screen.getByText("/hooks/v1/trigger-1")).toBeInTheDocument()
  await userEvent.click(screen.getByRole("button", { name: "I saved it" }))
  expect(screen.queryByText("webhook-once")).not.toBeInTheDocument()
})

it("viewers can inspect pinned triggers but cannot change them or see admin navigation", async () => {
  const trigger: Trigger = {
    id: "trigger-1",
    name: "Hourly sync",
    workflowId: workflows[0].id,
    version: 1,
    kind: "cron",
    cron: "0 * * * *",
    timezone: "UTC",
    overlapPolicy: "skip",
    input: {},
    enabled: true,
    createdAt: now,
    updatedAt: now,
    nextScheduledAt: now,
  }
  server.use(
    http.get("/api/v1/auth/me", () => HttpResponse.json(meAs("viewer"))),
    http.get(`${org}/triggers`, () => HttpResponse.json({ items: [trigger] })),
  )
  renderPage(<TriggersView organizationId="org-1" />, "triggers")
  expect(await screen.findByText("Hourly sync")).toBeInTheDocument()
  expect(
    screen.queryByRole("button", { name: "Create trigger" }),
  ).not.toBeInTheDocument()
  expect(screen.queryByRole("button", { name: "Edit" })).not.toBeInTheDocument()
  const navigation = screen.getByRole("navigation", { name: "Organization" })
  expect(
    within(navigation).queryByRole("link", { name: "Secrets" }),
  ).not.toBeInTheDocument()
  expect(
    within(navigation).queryByRole("link", { name: "Service tokens" }),
  ).not.toBeInTheDocument()
  expect(
    within(navigation).queryByRole("link", { name: "Runtime plugins" }),
  ).not.toBeInTheDocument()
})

it.each([
  [
    "plugins",
    <PluginsView key="plugins" organizationId="org-1" />,
    "Only admins can manage runtime plugins.",
  ],
  [
    "service-tokens",
    <ServiceTokensView key="tokens" organizationId="org-1" />,
    "Only admins can manage service tokens.",
  ],
  [
    "secrets",
    <SecretsView key="secrets" organizationId="org-1" />,
    "Your role cannot view or manage secrets.",
  ],
] as const)(
  "denies a viewer's direct visit to %s",
  async (route, view, message) => {
    let reads = 0
    server.use(
      http.get("/api/v1/auth/me", () => HttpResponse.json(meAs("viewer"))),
      http.get(`${org}/${route}`, () => {
        reads++
        return HttpResponse.json({ items: [] })
      }),
    )
    renderPage(view, route)
    expect(await screen.findByText(message)).toBeInTheDocument()
    await waitFor(() =>
      expect(screen.queryByText("Loading…")).not.toBeInTheDocument(),
    )
    expect(reads).toBe(0)
  },
)
