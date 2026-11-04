"use client"

import { useState, type FormEvent } from "react"

import { api, ApiProblem } from "@/api/client"
import { useManagementWrite } from "@/api/management"
import type { Diagnostic, NodeDefinition } from "@/api/types"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { isRecord } from "@/studio/graph-model"
import { Notice, problemMessage } from "@/views/management-shared"

const fields = [
  ["inputs", "Input ports", "[]"],
  ["outputs", "Output ports", "[]"],
  ["configSchema", "Configuration schema", '{"type":"object","properties":{}}'],
  ["uiSchema", "Configuration widgets", "{}"],
  ["defaults", "Default configuration", "{}"],
] as const

export function CatalogEditor({
  organizationId,
  initialExecutor,
}: {
  organizationId: string
  initialExecutor?: { key: string; version: string }
}) {
  const [open, setOpen] = useState(!!initialExecutor)
  const [name, setName] = useState("")
  const [key, setKey] = useState("")
  const [description, setDescription] = useState("")
  const [category, setCategory] = useState("Custom")
  const [executor, setExecutor] = useState(initialExecutor?.key ?? "mock")
  const [version, setVersion] = useState(initialExecutor?.version ?? "1.0.0")
  const [jsonFields, setJsonFields] = useState<Record<string, string>>(
    Object.fromEntries(fields.map(([key, , value]) => [key, value])),
  )
  const [error, setError] = useState("")
  const [status, setStatus] = useState("")
  const [diagnostics, setDiagnostics] = useState<Diagnostic[]>([])
  const save = useManagementWrite(
    organizationId,
    (body: Record<string, unknown>) =>
      api<NodeDefinition>(`/orgs/${organizationId}/node-definitions`, {
        method: "POST",
        body: JSON.stringify(body),
      }),
  )

  async function submit(event: FormEvent) {
    event.preventDefault()
    setError("")
    setStatus("")
    setDiagnostics([])
    const parsed: Record<string, unknown> = {}
    for (const [field, label] of fields) {
      try {
        parsed[field] = JSON.parse(jsonFields[field])
      } catch {
        setError(`${label} must be valid JSON.`)
        return
      }
      if (
        ["inputs", "outputs"].includes(field)
          ? !Array.isArray(parsed[field])
          : !isRecord(parsed[field])
      ) {
        setError(
          `${label} must be a JSON ${["inputs", "outputs"].includes(field) ? "array" : "object"}.`,
        )
        return
      }
    }
    try {
      const definition = await save.mutateAsync({
        key,
        displayName: name,
        description,
        category,
        executor: { key: executor, version },
        ...parsed,
      })
      setStatus(
        `Created ${definition.displayName} v${definition.version}. It is available in the workflow palette.`,
      )
      setOpen(false)
    } catch (error) {
      setError(problemMessage(error, "Creating definition"))
      if (error instanceof ApiProblem) setDiagnostics(error.diagnostics)
    }
  }

  return (
    <section className="grid gap-3">
      <Notice error={error} status={status} />
      {diagnostics.length > 0 && (
        <ul aria-label="Definition problems" className="grid gap-1 text-sm">
          {diagnostics.map((problem, index) => (
            <li key={index}>
              <code>{problem.pointer}</code>: {problem.message}
            </li>
          ))}
        </ul>
      )}
      {!open ? (
        <Button
          variant="outline"
          className="justify-self-start"
          onClick={() => setOpen(true)}
        >
          New node definition
        </Button>
      ) : (
        <form
          className="grid max-w-3xl gap-4 rounded-xl border p-4"
          onSubmit={(event) => void submit(event)}
        >
          <h2 className="font-medium">Create a node definition</h2>
          <p className="text-sm text-muted-foreground">
            Define reusable typed inputs and outputs for an exact executor
            version. Reusing a key creates its next immutable definition
            version.
          </p>
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="grid gap-2">
              <Label htmlFor="definition-name">Display name</Label>
              <Input
                id="definition-name"
                required
                maxLength={100}
                value={name}
                onChange={(event) => setName(event.target.value)}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="definition-key">Definition key</Label>
              <Input
                id="definition-key"
                required
                maxLength={100}
                pattern="[a-z0-9]+([.-][a-z0-9]+)*"
                placeholder="my-team.sync"
                value={key}
                onChange={(event) => setKey(event.target.value)}
              />
            </div>
          </div>
          <div className="grid gap-2">
            <Label htmlFor="definition-description">Description</Label>
            <Input
              id="definition-description"
              maxLength={1000}
              value={description}
              onChange={(event) => setDescription(event.target.value)}
            />
          </div>
          <div className="grid gap-4 sm:grid-cols-3">
            <div className="grid gap-2">
              <Label htmlFor="definition-category">Category</Label>
              <Input
                id="definition-category"
                required
                maxLength={60}
                value={category}
                onChange={(event) => setCategory(event.target.value)}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="definition-executor">Executor key</Label>
              <Input
                id="definition-executor"
                required
                value={executor}
                onChange={(event) => setExecutor(event.target.value)}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="definition-version">Executor version</Label>
              <Input
                id="definition-version"
                required
                value={version}
                onChange={(event) => setVersion(event.target.value)}
              />
            </div>
          </div>
          <p className="text-xs text-muted-foreground">
            Use mock, shell, or an installed and approved runtime plugin.
            Executor versions are exact, such as 1.0.0.
          </p>
          <details className="grid gap-3" open={!!initialExecutor}>
            <summary className="cursor-pointer text-sm font-medium">
              Ports and configuration (JSON)
            </summary>
            <p className="my-3 text-xs text-muted-foreground">
              Ports are arrays of objects with name, dataType, schema, required,
              and description. Example:{" "}
              {`[{"name":"result","dataType":"core.string","schema":{"type":"string"},"required":true}]`}
            </p>
            <div className="grid gap-4">
              {fields.map(([field, label]) => (
                <div key={field} className="grid gap-2">
                  <Label htmlFor={`definition-${field}`}>{label}</Label>
                  <Textarea
                    id={`definition-${field}`}
                    value={jsonFields[field]}
                    onChange={(event) =>
                      setJsonFields({
                        ...jsonFields,
                        [field]: event.target.value,
                      })
                    }
                    required
                    spellCheck={false}
                    className="min-h-20 font-mono text-xs"
                  />
                </div>
              ))}
            </div>
          </details>
          <div className="flex gap-2">
            <Button type="submit" disabled={save.isPending}>
              Create definition
            </Button>
            <Button
              type="button"
              variant="ghost"
              onClick={() => setOpen(false)}
            >
              Cancel
            </Button>
          </div>
        </form>
      )}
    </section>
  )
}
