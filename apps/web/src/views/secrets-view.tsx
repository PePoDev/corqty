"use client"

import { useState, type FormEvent } from "react"

import { useDeleteResource, useSaveSecret, useSecrets } from "@/api/management"
import type { SecretMetadata } from "@/api/types"
import { useCan } from "@/auth/use-can"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  ManagementPage,
  Notice,
  problemMessage,
} from "@/views/management-shared"

export const secretProtectionNote =
  "Exact bound secret values are redacted from stored inputs and logs; outputs containing them fail. Encoded, transformed, generated values and short fragments may not be detected."

export function SecretsView({ organizationId }: { organizationId: string }) {
  const canManage = useCan(organizationId, "manage_secrets")
  const canView = useCan(organizationId, "edit_workflows")
  const secrets = useSecrets(organizationId, canView)
  const save = useSaveSecret(organizationId)
  const remove = useDeleteResource(organizationId, "secrets")
  const [editing, setEditing] = useState<SecretMetadata | null>(null)
  const [name, setName] = useState("")
  const [value, setValue] = useState("")
  const [error, setError] = useState("")
  const [status, setStatus] = useState("")
  const [deleting, setDeleting] = useState<string | null>(null)

  function reset() {
    setEditing(null)
    setName("")
    setValue("")
    save.reset()
  }
  async function submit(event: FormEvent) {
    event.preventDefault()
    setError("")
    setStatus("")
    try {
      await save.mutateAsync({ id: editing?.id, name, value })
      setStatus(
        editing
          ? "Secret updated. Future executions use the new value."
          : "Secret created.",
      )
      reset()
    } catch (error) {
      setError(problemMessage(error, "Saving secret"))
    } finally {
      setValue("")
      save.reset()
    }
  }
  async function destroy(id: string) {
    setError("")
    setStatus("")
    try {
      await remove.mutateAsync(id)
      setDeleting(null)
      setStatus("Secret deleted.")
      if (editing?.id === id) reset()
    } catch (error) {
      setError(problemMessage(error, "Deleting secret"))
    }
  }

  return (
    <ManagementPage
      title="Secrets"
      description="Keep credentials encrypted and reference them by name in your workflows. Values can be replaced, but can never be read back."
    >
      {!canView ? (
        <p>Your role cannot view or manage secrets.</p>
      ) : (
        <>
          <p className="rounded-lg border bg-muted/40 p-3 text-sm text-muted-foreground">
            {secretProtectionNote}
          </p>
          <Notice
            error={
              error || (secrets.isError ? "Secrets could not be loaded." : "")
            }
            status={status}
          />
          {canManage && (
            <form
              onSubmit={(event) => void submit(event)}
              className="grid max-w-xl gap-3 rounded-xl border p-4"
            >
              <h2 className="font-medium">
                {editing ? `Replace ${editing.name}` : "Create secret"}
              </h2>
              <Label htmlFor="secret-name">Name</Label>
              <Input
                id="secret-name"
                required
                maxLength={100}
                value={name}
                onChange={(event) => setName(event.target.value)}
                autoComplete="off"
              />
              <Label htmlFor="secret-value">
                {editing ? "New value" : "Value"}
              </Label>
              <Input
                id="secret-value"
                required
                type="password"
                maxLength={65_536}
                value={value}
                onChange={(event) => setValue(event.target.value)}
                autoComplete="new-password"
              />
              <div className="flex gap-2">
                <Button type="submit" disabled={save.isPending}>
                  {editing ? "Replace secret" : "Create secret"}
                </Button>
                {editing && (
                  <Button type="button" variant="ghost" onClick={reset}>
                    Cancel
                  </Button>
                )}
              </div>
            </form>
          )}
          {secrets.isPending ? (
            <p>Loading secrets…</p>
          ) : secrets.data?.items.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              No secrets yet. An admin can create one here.
            </p>
          ) : (
            <ul className="divide-y rounded-xl border">
              {secrets.data?.items.map((secret) => (
                <li
                  key={secret.id}
                  className="flex flex-wrap items-center gap-3 p-4"
                >
                  <div className="min-w-0 flex-1">
                    <h2 className="font-medium">{secret.name}</h2>
                    <p className="text-xs text-muted-foreground">
                      Updated {new Date(secret.updatedAt).toLocaleString()}
                    </p>
                  </div>
                  {canManage &&
                    (deleting === secret.id ? (
                      <>
                        <span className="text-sm">Delete {secret.name}?</span>
                        <Button
                          variant="destructive"
                          disabled={remove.isPending}
                          onClick={() => void destroy(secret.id)}
                        >
                          Confirm delete
                        </Button>
                        <Button
                          variant="ghost"
                          onClick={() => setDeleting(null)}
                        >
                          Keep secret
                        </Button>
                      </>
                    ) : (
                      <>
                        <Button
                          variant="outline"
                          onClick={() => {
                            setEditing(secret)
                            setName(secret.name)
                            setValue("")
                            setError("")
                          }}
                        >
                          Replace value
                        </Button>
                        <Button
                          variant="ghost"
                          onClick={() => setDeleting(secret.id)}
                        >
                          Delete
                        </Button>
                      </>
                    ))}
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </ManagementPage>
  )
}
