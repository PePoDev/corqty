"use client"

import Link from "next/link"
import { useState, type FormEvent } from "react"

import {
  useDeleteResource,
  useManagementList,
  useSaveTrigger,
} from "@/api/management"
import { useWorkflows } from "@/api/queries"
import type { Trigger, TriggerInput } from "@/api/types"
import { useCan } from "@/auth/use-can"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { isRecord } from "@/studio/graph-model"
import { selectClassName } from "@/studio/widgets"
import {
  ManagementPage,
  Notice,
  OnceCredential,
  problemMessage,
} from "@/views/management-shared"

const blank: TriggerInput = {
  name: "",
  workflowId: "",
  version: 1,
  kind: "cron",
  input: {},
  enabled: true,
  cron: "0 * * * *",
  timezone: "UTC",
  overlapPolicy: "skip",
}

export function TriggersView({ organizationId }: { organizationId: string }) {
  const canManage = useCan(organizationId, "manage_triggers")
  const triggers = useManagementList<Trigger>(organizationId, "triggers")
  const workflows = useWorkflows(organizationId)
  const save = useSaveTrigger(organizationId)
  const remove = useDeleteResource(organizationId, "triggers")
  const [form, setForm] = useState(blank)
  const [inputText, setInputText] = useState("{}")
  const [editing, setEditing] = useState<string | undefined>()
  const [secret, setSecret] = useState<{
    value: string
    path: string
    id: string
  } | null>(null)
  const [error, setError] = useState("")
  const [status, setStatus] = useState("")
  const [deleting, setDeleting] = useState<string | null>(null)
  const published =
    workflows.data?.items.filter(
      (workflow) => workflow.publishedVersion !== null,
    ) ?? []

  function reset() {
    setForm(blank)
    setInputText("{}")
    setEditing(undefined)
    save.reset()
  }
  function edit(trigger: Trigger) {
    setEditing(trigger.id)
    setForm(trigger)
    setInputText(JSON.stringify(trigger.input, null, 2))
    setError("")
    setStatus("")
  }
  function body(trigger: TriggerInput): TriggerInput {
    return {
      name: trigger.name,
      workflowId: trigger.workflowId,
      version: trigger.version,
      kind: trigger.kind,
      input: trigger.input,
      enabled: trigger.enabled,
      cron: trigger.kind === "cron" ? trigger.cron : null,
      timezone: trigger.kind === "cron" ? trigger.timezone : null,
      overlapPolicy: trigger.overlapPolicy,
    }
  }
  async function submit(event: FormEvent) {
    event.preventDefault()
    setError("")
    setStatus("")
    let input: unknown
    try {
      input = JSON.parse(inputText)
    } catch {
      setError("Workflow input must be a valid JSON object.")
      return
    }
    if (!isRecord(input)) {
      setError("Workflow input must be a JSON object.")
      return
    }
    try {
      const result = await save.mutateAsync({
        ...body({ ...form, input }),
        id: editing,
      })
      if (result.webhookSecret)
        setSecret({
          value: result.webhookSecret,
          path: result.webhookPath ?? `/hooks/v1/${result.id}`,
          id: result.id,
        })
      setStatus(
        `Trigger ${editing ? "updated" : "created"} and pinned to v${form.version}.`,
      )
      reset()
    } catch (error) {
      setError(problemMessage(error, "Saving trigger"))
    } finally {
      save.reset()
    }
  }
  async function toggle(trigger: Trigger) {
    setError("")
    setStatus("")
    try {
      await save.mutateAsync({
        ...body(trigger),
        enabled: !trigger.enabled,
        id: trigger.id,
      })
      setStatus(`Trigger ${trigger.enabled ? "disabled" : "enabled"}.`)
    } catch (error) {
      setError(problemMessage(error, "Changing trigger"))
    } finally {
      save.reset()
    }
  }
  async function destroy(id: string) {
    setError("")
    setStatus("")
    try {
      await remove.mutateAsync(id)
      setDeleting(null)
      if (secret?.id === id) setSecret(null)
      if (editing === id) reset()
      setStatus("Trigger deleted.")
    } catch (error) {
      setError(problemMessage(error, "Deleting trigger"))
    }
  }

  return (
    <ManagementPage
      title="Triggers"
      description="Start published workflows on a schedule or from a signed webhook. Triggers stay pinned to the exact version you select when newer versions are published."
    >
      <Notice
        error={
          error || (triggers.isError ? "Triggers could not be loaded." : "")
        }
        status={status}
      />
      {secret && (
        <>
          <OnceCredential
            label="Webhook secret"
            value={secret.value}
            onDismiss={() => setSecret(null)}
          />
          <p className="break-all text-sm">
            Webhook path: <code>{secret.path}</code>
          </p>
        </>
      )}
      {canManage && (
        <form
          className="grid max-w-2xl gap-4 rounded-xl border p-4"
          onSubmit={(event) => void submit(event)}
        >
          <h2 className="font-medium">
            {editing ? "Edit trigger" : "Create trigger"}
          </h2>
          <div className="grid gap-2">
            <Label htmlFor="trigger-name">Name</Label>
            <Input
              id="trigger-name"
              value={form.name}
              onChange={(event) =>
                setForm({ ...form, name: event.target.value })
              }
              required
              maxLength={100}
            />
          </div>
          <div className="grid gap-4 sm:grid-cols-[1fr_8rem]">
            <div className="grid gap-2">
              <Label htmlFor="trigger-workflow">Published workflow</Label>
              <select
                className={selectClassName}
                id="trigger-workflow"
                value={form.workflowId}
                required
                onChange={(event) =>
                  setForm({
                    ...form,
                    workflowId: event.target.value,
                    version:
                      published.find(
                        (workflow) => workflow.id === event.target.value,
                      )?.publishedVersion ?? 1,
                  })
                }
              >
                <option value="">Choose a workflow…</option>
                {published.map((workflow) => (
                  <option key={workflow.id} value={workflow.id}>
                    {workflow.name}
                  </option>
                ))}
              </select>
            </div>
            <div className="grid gap-2">
              <Label htmlFor="trigger-version">Pinned version</Label>
              <Input
                id="trigger-version"
                type="number"
                required
                min={1}
                step={1}
                max={
                  published.find((workflow) => workflow.id === form.workflowId)
                    ?.publishedVersion ?? undefined
                }
                value={form.version}
                onChange={(event) =>
                  setForm({ ...form, version: Number(event.target.value) })
                }
              />
            </div>
          </div>
          {workflows.isError && (
            <p role="alert" className="text-sm text-destructive">
              Workflows could not be loaded.
            </p>
          )}
          {workflows.isSuccess && published.length === 0 && (
            <p className="text-sm text-muted-foreground">
              Publish a workflow before creating a trigger.
            </p>
          )}
          <div className="grid gap-2">
            <Label htmlFor="trigger-kind">Trigger type</Label>
            <select
              id="trigger-kind"
              className={selectClassName}
              value={form.kind}
              disabled={!!editing}
              onChange={(event) =>
                setForm({
                  ...form,
                  kind: event.target.value as TriggerInput["kind"],
                })
              }
            >
              <option value="cron">Cron schedule</option>
              <option value="webhook">Signed webhook</option>
            </select>
          </div>
          {form.kind === "cron" && (
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="grid gap-2">
                <Label htmlFor="trigger-cron">Cron expression</Label>
                <Input
                  id="trigger-cron"
                  required
                  value={form.cron ?? ""}
                  onChange={(event) =>
                    setForm({ ...form, cron: event.target.value })
                  }
                  placeholder="0 * * * *"
                />
                <p className="text-xs text-muted-foreground">
                  Five fields: minute, hour, day of month, month, day of week.
                </p>
              </div>
              <div className="grid gap-2">
                <Label htmlFor="trigger-timezone">Timezone</Label>
                <Input
                  id="trigger-timezone"
                  required
                  value={form.timezone ?? ""}
                  onChange={(event) =>
                    setForm({ ...form, timezone: event.target.value })
                  }
                  placeholder="Asia/Bangkok"
                />
                <p className="text-xs text-muted-foreground">
                  Use an IANA timezone, such as UTC or Asia/Bangkok.
                </p>
              </div>
            </div>
          )}
          <div className="grid gap-2">
            <Label htmlFor="trigger-input">Workflow input (JSON)</Label>
            <Textarea
              id="trigger-input"
              required
              value={inputText}
              onChange={(event) => setInputText(event.target.value)}
              className="min-h-28 font-mono text-xs"
              spellCheck={false}
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="trigger-overlap">
              When a previous run is still active
            </Label>
            <select
              id="trigger-overlap"
              className={selectClassName}
              value={form.overlapPolicy}
              onChange={(event) =>
                setForm({
                  ...form,
                  overlapPolicy: event.target
                    .value as TriggerInput["overlapPolicy"],
                })
              }
            >
              <option value="skip">Skip this occurrence</option>
              <option value="allow">Allow overlapping runs</option>
            </select>
          </div>
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={form.enabled}
              onChange={(event) =>
                setForm({ ...form, enabled: event.target.checked })
              }
            />
            Enabled
          </label>
          <div className="flex gap-2">
            <Button
              type="submit"
              disabled={save.isPending || published.length === 0 || !!secret}
            >
              {editing ? "Save trigger" : "Create trigger"}
            </Button>
            {editing && (
              <Button type="button" variant="ghost" onClick={reset}>
                Cancel edit
              </Button>
            )}
          </div>
        </form>
      )}
      {triggers.isPending ? (
        <p>Loading triggers…</p>
      ) : triggers.data?.items.length === 0 ? (
        <p className="text-sm text-muted-foreground">No triggers yet.</p>
      ) : (
        <ul className="grid gap-4">
          {triggers.data?.items.map((trigger) => (
            <li key={trigger.id} className="grid gap-3 rounded-xl border p-4">
              <header className="flex flex-wrap items-center gap-2">
                <h2 className="font-medium">{trigger.name}</h2>
                <Badge variant="secondary">{trigger.kind}</Badge>
                <Badge variant="outline">
                  {trigger.enabled ? "Enabled" : "Disabled"}
                </Badge>
              </header>
              <p className="text-sm">
                <Link
                  className="underline underline-offset-4"
                  href={`/orgs/${organizationId}/workflows/${trigger.workflowId}`}
                >
                  {workflows.data?.items.find(
                    (workflow) => workflow.id === trigger.workflowId,
                  )?.name ?? "Workflow"}
                </Link>{" "}
                · pinned v{trigger.version}
              </p>
              {trigger.kind === "cron" ? (
                <p className="text-sm text-muted-foreground">
                  <code>{trigger.cron}</code> · {trigger.timezone}
                  {trigger.nextScheduledAt && (
                    <>
                      {" "}
                      · Next{" "}
                      {new Date(trigger.nextScheduledAt).toLocaleString()}
                    </>
                  )}
                </p>
              ) : (
                <p className="break-all font-mono text-xs text-muted-foreground">
                  /hooks/v1/{trigger.id}
                </p>
              )}
              {canManage && (
                <div className="flex flex-wrap items-center gap-2">
                  {deleting === trigger.id ? (
                    <>
                      <span className="text-sm">Delete {trigger.name}?</span>
                      <Button
                        variant="destructive"
                        disabled={remove.isPending}
                        onClick={() => void destroy(trigger.id)}
                      >
                        Confirm delete
                      </Button>
                      <Button variant="ghost" onClick={() => setDeleting(null)}>
                        Keep trigger
                      </Button>
                    </>
                  ) : (
                    <>
                      <Button variant="outline" onClick={() => edit(trigger)}>
                        Edit
                      </Button>
                      <Button
                        variant="ghost"
                        disabled={save.isPending}
                        onClick={() => void toggle(trigger)}
                      >
                        {trigger.enabled ? "Disable" : "Enable"}
                      </Button>
                      <Button
                        variant="ghost"
                        onClick={() => setDeleting(trigger.id)}
                      >
                        Delete
                      </Button>
                    </>
                  )}
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
      <details className="rounded-xl border p-4">
        <summary className="cursor-pointer text-sm font-medium">
          Webhook authentication and delivery
        </summary>
        <div className="mt-3 grid gap-2 text-sm text-muted-foreground">
          <p>
            Send a JSON request body with X-Corqty-Timestamp (Unix seconds),
            X-Corqty-Signature, and a unique Idempotency-Key.
          </p>
          <p>
            The signature is lowercase hexadecimal HMAC-SHA256 of the timestamp,
            a period, and the exact raw request body, using your webhook secret.
            Timestamps must be within five minutes; bodies are limited to 1 MiB.
            Repeated keys are deduplicated for 24 hours.
          </p>
        </div>
      </details>
    </ManagementPage>
  )
}
