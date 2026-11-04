"use client"

import { useState, type ReactNode } from "react"

import { ApiProblem } from "@/api/client"
import { Button } from "@/components/ui/button"

export function problemMessage(error: unknown, action: string) {
  const code = error instanceof ApiProblem ? error.code : "network_error"
  if (code === "secret_in_use")
    return "This secret is referenced by a draft, published workflow, or enabled trigger. Remove those references before deleting it."
  if (code === "command_exec_unavailable")
    return "Host command execution is disabled for this deployment. The operator must opt into trusted execution before these capabilities can be approved."
  return `${action} failed (${code}).`
}

export function ManagementPage({
  title,
  description,
  children,
}: {
  title: string
  description: string
  children: ReactNode
}) {
  return (
    <section className="mx-auto flex max-w-6xl flex-col gap-6 p-4 md:p-6">
      <header className="grid gap-1">
        <h1 className="text-2xl font-semibold tracking-tight">{title}</h1>
        <p className="max-w-3xl text-sm text-muted-foreground">{description}</p>
      </header>
      {children}
    </section>
  )
}

export function Notice({ error, status }: { error?: string; status?: string }) {
  return (
    <>
      {error && (
        <p
          role="alert"
          className="rounded-lg border p-3 text-sm text-destructive"
        >
          {error}
        </p>
      )}
      {status && (
        <p role="status" className="text-sm">
          {status}
        </p>
      )}
    </>
  )
}

export function OnceCredential({
  label,
  value,
  onDismiss,
}: {
  label: string
  value: string
  onDismiss: () => void
}) {
  const [copied, setCopied] = useState(false)
  const [error, setError] = useState("")
  async function copy() {
    try {
      await navigator.clipboard.writeText(value)
      setCopied(true)
      setError("")
    } catch {
      setError("Copy is unavailable. Select and copy the value below.")
    }
  }
  return (
    <section
      aria-label={label}
      className="grid gap-3 rounded-xl border bg-muted/40 p-4"
    >
      <h2 className="font-medium">Save your {label.toLowerCase()}</h2>
      <p className="text-sm text-muted-foreground">
        Shown only once. Store it securely before dismissing this message.
      </p>
      <code className="break-all rounded-md border bg-background p-3 text-sm select-all">
        {value}
      </code>
      <div className="flex flex-wrap gap-2">
        <Button type="button" variant="outline" onClick={() => void copy()}>
          {copied ? "Copied" : `Copy ${label.toLowerCase()}`}
        </Button>
        <Button type="button" variant="ghost" onClick={onDismiss}>
          I saved it
        </Button>
      </div>
      <Notice error={error} />
    </section>
  )
}
