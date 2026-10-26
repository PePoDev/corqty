"use client"

import { useId, useState } from "react"

import { ApiProblem } from "@/api/client"
import { useSetShellEnabled, useSettings } from "@/api/queries"
import { useCan } from "@/auth/use-can"
import { ShellWarning } from "@/components/shell-warning"
import { Label } from "@/components/ui/label"

export function SettingsView({ organizationId }: { organizationId: string }) {
  const canEnableShell = useCan(organizationId, "enable_shell_executor")
  const settings = useSettings(organizationId)
  const setShellEnabled = useSetShellEnabled(organizationId)
  const [error, setError] = useState("")
  const headingId = useId()
  const toggleId = useId()

  async function change(enabled: boolean) {
    setError("")
    try {
      await setShellEnabled.mutateAsync(enabled)
    } catch (problem) {
      const code =
        problem instanceof ApiProblem ? problem.code : "network_error"
      setError(`The shell executor could not be changed (${code}).`)
    }
  }

  return (
    <section className="flex flex-col gap-5 p-4 md:p-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Settings</h1>
        <p className="text-sm text-muted-foreground">
          Options for the whole organization.
        </p>
      </div>
      <section aria-labelledby={headingId} className="grid max-w-2xl gap-3">
        <h2 id={headingId} className="text-lg font-semibold">
          Shell executor
        </h2>
        <p className="text-sm text-muted-foreground">
          Lets workflow nodes run shell commands on the workers. Only an admin
          can change it; runs that already exist are not affected.
        </p>
        <ShellWarning />
        {settings.isPending && (
          <p className="text-sm text-muted-foreground">Loading settings…</p>
        )}
        {settings.isError && (
          <p role="alert" className="text-sm text-destructive">
            The settings could not be loaded.
          </p>
        )}
        {settings.data &&
          (canEnableShell ? (
            <div className="flex items-center gap-2">
              <input
                id={toggleId}
                type="checkbox"
                className="size-4 accent-primary"
                checked={settings.data.shellEnabled}
                disabled={setShellEnabled.isPending}
                onChange={(event) => void change(event.target.checked)}
              />
              <Label htmlFor={toggleId}>Enable the shell executor</Label>
            </div>
          ) : (
            <p className="text-sm">
              The shell executor is{" "}
              {settings.data.shellEnabled ? "enabled" : "disabled"}.
            </p>
          ))}
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
      </section>
    </section>
  )
}
