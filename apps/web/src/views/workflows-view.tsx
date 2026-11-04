"use client"

import Link from "next/link"
import { useRouter } from "next/navigation"
import { useState } from "react"
import type { FormEvent } from "react"

import { ApiProblem } from "@/api/client"
import { useBlueprints, useCreateWorkflow, useWorkflows } from "@/api/queries"
import { useCan } from "@/auth/use-can"
import { StatusBadge } from "@/components/status/status-badge"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Field, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { isSimulated } from "@/studio/graph-model"

export function WorkflowsView({ organizationId }: { organizationId: string }) {
  const canEdit = useCan(organizationId, "edit_workflows")
  const workflows = useWorkflows(organizationId)
  const [creating, setCreating] = useState(false)

  return (
    <section className="flex flex-col gap-5 p-4 md:p-6">
      <header className="flex flex-col items-start justify-between gap-3 sm:flex-row sm:items-center">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Workflows</h1>
          <p className="text-sm text-muted-foreground">
            Drafts and published versions in this organization.
          </p>
        </div>
        {canEdit && (
          <Button onClick={() => setCreating(true)}>New workflow</Button>
        )}
      </header>
      {workflows.isPending && (
        <p className="text-sm text-muted-foreground">Loading workflows…</p>
      )}
      {workflows.isError && (
        <p role="alert" className="text-sm text-destructive">
          The workflows could not be loaded.
        </p>
      )}
      {workflows.data?.items.length === 0 && (
        <p className="text-sm text-muted-foreground">No workflows yet.</p>
      )}
      {canEdit && (
        <Dialog open={creating} onOpenChange={setCreating}>
          <DialogContent className="sm:max-w-md">
            <NewWorkflowForm organizationId={organizationId} />
          </DialogContent>
        </Dialog>
      )}
      <div className="overflow-x-auto rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Name</TableHead>
              <TableHead>Draft</TableHead>
              <TableHead>Published</TableHead>
              <TableHead>Updated</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {workflows.data?.items.map((workflow) => (
              <TableRow key={workflow.id}>
                <TableCell>
                  <Link
                    href={`/orgs/${organizationId}/workflows/${workflow.id}`}
                    className="font-medium underline-offset-4 hover:underline"
                  >
                    {workflow.name}
                  </Link>
                </TableCell>
                <TableCell>rev {workflow.draftRevision}</TableCell>
                <TableCell>
                  {workflow.publishedVersion === null ? (
                    <span className="text-muted-foreground">Not published</span>
                  ) : (
                    `v${workflow.publishedVersion}`
                  )}
                </TableCell>
                <TableCell>
                  {new Date(workflow.updatedAt).toLocaleString()}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </section>
  )
}

// Mounted only while the dialog is open, so it starts empty each time.
function NewWorkflowForm({ organizationId }: { organizationId: string }) {
  const router = useRouter()
  const blueprints = useBlueprints(organizationId)
  const createWorkflow = useCreateWorkflow(organizationId)
  const [name, setName] = useState("")
  const [blueprintId, setBlueprintId] = useState("")
  const [error, setError] = useState("")
  const options = [
    {
      id: "",
      key: "",
      displayName: "Empty workflow",
      description: "Start from a blank canvas.",
    },
    ...(blueprints.data?.items ?? []),
  ]

  async function create(event: FormEvent) {
    event.preventDefault()
    setError("")
    try {
      const workflow = await createWorkflow.mutateAsync({
        name,
        blueprintId: blueprintId || undefined,
      })
      router.push(`/orgs/${organizationId}/workflows/${workflow.id}`)
    } catch (problem) {
      setError(
        problem instanceof ApiProblem && problem.code === "invalid_workflow_name"
          ? "Enter a name of 1 to 100 characters."
          : "The workflow could not be created.",
      )
    }
  }

  return (
    <>
      <DialogHeader>
        <DialogTitle>New workflow</DialogTitle>
        <DialogDescription>
          Start from a blank canvas or copy a blueprint.
        </DialogDescription>
      </DialogHeader>
      <form className="grid gap-4" onSubmit={create}>
        <Field>
          <FieldLabel htmlFor="workflow-name">Name</FieldLabel>
          <Input
            id="workflow-name"
            required
            maxLength={100}
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
        </Field>
        <fieldset className="grid gap-2">
          <legend className="mb-2 text-sm font-medium">Start from</legend>
          {blueprints.isPending && (
            <p className="text-sm text-muted-foreground">
              Loading blueprints…
            </p>
          )}
          {blueprints.isError && (
            <p role="alert" className="text-sm text-destructive">
              The blueprints could not be loaded.
            </p>
          )}
          {options.map((option) => (
            <label
              key={option.id}
              className="flex gap-3 rounded-md border p-3 text-sm has-checked:border-ring has-checked:bg-muted/50"
            >
              <input
                type="radio"
                name="blueprint"
                className="mt-0.5"
                checked={blueprintId === option.id}
                onChange={() => setBlueprintId(option.id)}
              />
              <span className="grid gap-1">
                <span className="flex items-center gap-2 font-medium">
                  {option.displayName}
                  {isSimulated(option.key) && (
                    <StatusBadge status="simulated" />
                  )}
                </span>
                <span className="text-muted-foreground">
                  {option.description}
                </span>
              </span>
            </label>
          ))}
        </fieldset>
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
        <Button
          className="sm:justify-self-end"
          type="submit"
          disabled={createWorkflow.isPending}
        >
          Create workflow
        </Button>
      </form>
    </>
  )
}
