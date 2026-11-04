"use client"

import { useState, type FormEvent } from "react"

import {
  useCreateServiceToken,
  useDeleteResource,
  useManagementList,
} from "@/api/management"
import type { ServiceToken } from "@/api/types"
import { useCan } from "@/auth/use-can"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  ManagementPage,
  Notice,
  OnceCredential,
  problemMessage,
} from "@/views/management-shared"

export function ServiceTokensView({
  organizationId,
}: {
  organizationId: string
}) {
  const canManage = useCan(organizationId, "manage_service_tokens")
  const tokens = useManagementList<ServiceToken>(
    organizationId,
    "service-tokens",
    canManage,
  )
  const create = useCreateServiceToken(organizationId)
  const revoke = useDeleteResource(organizationId, "service-tokens")
  const [name, setName] = useState("")
  const [token, setToken] = useState("")
  const [tokenId, setTokenId] = useState("")
  const [error, setError] = useState("")
  const [status, setStatus] = useState("")
  const [revoking, setRevoking] = useState<string | null>(null)

  async function submit(event: FormEvent) {
    event.preventDefault()
    setError("")
    setStatus("")
    try {
      const result = await create.mutateAsync(name)
      setToken(result.token)
      setTokenId(result.id)
      setName("")
    } catch (error) {
      setError(problemMessage(error, "Creating token"))
    } finally {
      create.reset()
    }
  }
  async function destroy(id: string) {
    setError("")
    setStatus("")
    try {
      await revoke.mutateAsync(id)
      setRevoking(null)
      if (tokenId === id) setToken("")
      setStatus("Service token revoked.")
    } catch (error) {
      setError(problemMessage(error, "Revoking token"))
    }
  }

  return (
    <ManagementPage
      title="Service tokens"
      description="Let automation start workflow runs in this organization. Tokens have only the workflow:run scope; only their hashes are stored."
    >
      {!canManage ? (
        <p>Only admins can manage service tokens.</p>
      ) : (
        <>
          <Notice
            error={
              error ||
              (tokens.isError ? "Service tokens could not be loaded." : "")
            }
            status={status}
          />
          {token && (
            <OnceCredential
              label="Service token"
              value={token}
              onDismiss={() => setToken("")}
            />
          )}
          <form
            className="grid max-w-xl gap-3 rounded-xl border p-4"
            onSubmit={(event) => void submit(event)}
          >
            <Label htmlFor="token-name">Token name</Label>
            <Input
              id="token-name"
              required
              maxLength={100}
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder="Production deployment"
            />
            <Button
              className="justify-self-start"
              type="submit"
              disabled={create.isPending || !!token}
            >
              Create service token
            </Button>
          </form>
          {tokens.isPending ? (
            <p>Loading service tokens…</p>
          ) : tokens.data?.items.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              No service tokens yet.
            </p>
          ) : (
            <ul className="divide-y rounded-xl border">
              {tokens.data?.items.map((item) => (
                <li
                  key={item.id}
                  className="flex flex-wrap items-center gap-3 p-4"
                >
                  <div className="flex-1">
                    <h2 className="font-medium">{item.name}</h2>
                    <p className="text-xs text-muted-foreground">
                      workflow:run · Created{" "}
                      {new Date(item.createdAt).toLocaleDateString()}
                    </p>
                  </div>
                  {item.revokedAt ? (
                    <Badge variant="outline">Revoked</Badge>
                  ) : revoking === item.id ? (
                    <>
                      <span className="text-sm">Revoke {item.name}?</span>
                      <Button
                        variant="destructive"
                        disabled={revoke.isPending}
                        onClick={() => void destroy(item.id)}
                      >
                        Confirm revoke
                      </Button>
                      <Button variant="ghost" onClick={() => setRevoking(null)}>
                        Keep token
                      </Button>
                    </>
                  ) : (
                    <Button
                      variant="outline"
                      onClick={() => setRevoking(item.id)}
                    >
                      Revoke
                    </Button>
                  )}
                </li>
              ))}
            </ul>
          )}
          <section className="grid gap-2 rounded-xl border p-4">
            <h2 className="font-medium">Start a run from your automation</h2>
            <p className="text-sm text-muted-foreground">
              Send an Authorization: Bearer header and a unique Idempotency-Key.
              Retrying the same request with that key returns the original run
              for 24 hours. A changed request with the same key returns a
              conflict.
            </p>
            <pre className="overflow-x-auto rounded-md bg-muted p-3 text-xs">{`POST /api/v1/orgs/${organizationId}/workflows/{workflowId}/runs\nAuthorization: Bearer <your-token>\nIdempotency-Key: <unique-request-id>\nContent-Type: application/json\n\n{"version": 1, "input": {}}`}</pre>
          </section>
        </>
      )}
    </ManagementPage>
  )
}
