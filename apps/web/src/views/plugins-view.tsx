"use client"

import { useState, type FormEvent } from "react"
import Link from "next/link"

import {
  useApprovePlugin,
  useInstallPlugin,
  useManagementList,
  useSetPluginEnabled,
} from "@/api/management"
import type {
  PluginCapability,
  PluginManifest,
  RuntimePlugin,
} from "@/api/types"
import { useCan } from "@/auth/use-can"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import {
  ManagementPage,
  Notice,
  problemMessage,
} from "@/views/management-shared"

const maxComponentBytes = 25 * 1024 * 1024
const initialManifest = JSON.stringify(
  {
    key: "sample",
    version: "1.0.0",
    hostContract: "1",
    capabilities: [{ type: "log" }],
  },
  null,
  2,
)

function capabilityLabel(capability: PluginCapability) {
  if (capability.type === "network")
    return `Network: ${capability.host}:${capability.port}`
  if (capability.type === "command-exec")
    return `Host commands: ${capability.executables.join(", ")}`
  return {
    log: "Write logs",
    "secret-read": "Read secrets bound to this node",
    "scratch-fs": "Use an ephemeral scratch directory",
  }[capability.type]
}

async function componentBase64(file: File) {
  if (file.size > maxComponentBytes || file.size < 8)
    throw new Error(
      "Choose a precompiled WebAssembly Component of at most 25 MiB.",
    )
  const bytes = new Uint8Array(await file.arrayBuffer())
  const header = [0, 97, 115, 109, 13, 0, 1, 0]
  if (!header.every((byte, index) => bytes[index] === byte))
    throw new Error(
      "This is not a WebAssembly Component. Upload a component .wasm file, not source or a core module.",
    )
  let binary = ""
  for (let offset = 0; offset < bytes.length; offset += 8192)
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 8192))
  return btoa(binary)
}

export function PluginsView({ organizationId }: { organizationId: string }) {
  const canManage = useCan(organizationId, "manage_plugins")
  const plugins = useManagementList<RuntimePlugin>(
    organizationId,
    "plugins",
    canManage,
  )
  const install = useInstallPlugin(organizationId)
  const approve = useApprovePlugin(organizationId)
  const setEnabled = useSetPluginEnabled(organizationId)
  const [manifest, setManifest] = useState(initialManifest)
  const [file, setFile] = useState<File | null>(null)
  const [fileKey, setFileKey] = useState(0)
  const [error, setError] = useState("")
  const [status, setStatus] = useState("")
  const [reviewing, setReviewing] = useState<string | null>(null)
  const [accepted, setAccepted] = useState(false)
  const [uploading, setUploading] = useState(false)

  async function upload(event: FormEvent) {
    event.preventDefault()
    setError("")
    setStatus("")
    setUploading(true)
    try {
      if (!file) {
        setError("Choose a component file.")
        return
      }
      let parsed: PluginManifest
      try {
        parsed = JSON.parse(manifest) as PluginManifest
      } catch {
        setError("The manifest must be valid JSON.")
        return
      }
      if (
        !parsed ||
        !Array.isArray(parsed.capabilities) ||
        !parsed.key ||
        !parsed.version ||
        parsed.hostContract !== "1"
      ) {
        setError(
          'The manifest needs key, version, hostContract "1", and a capabilities array.',
        )
        return
      }
      let component: string
      try {
        component = await componentBase64(file)
      } catch (error) {
        setError((error as Error).message)
        return
      }
      await install.mutateAsync({
        manifest: parsed,
        componentBase64: component,
      })
      setFile(null)
      setFileKey((key) => key + 1)
      setStatus(
        "Plugin installed. Review its requested capabilities before use.",
      )
    } catch (error) {
      setError(problemMessage(error, "Installing plugin"))
    } finally {
      install.reset()
      setUploading(false)
    }
  }
  async function approveVersion(plugin: RuntimePlugin) {
    setError("")
    setStatus("")
    try {
      await approve.mutateAsync(plugin)
      setReviewing(null)
      setAccepted(false)
      setStatus(`${plugin.key}@${plugin.version} approved.`)
    } catch (error) {
      setError(problemMessage(error, "Approving plugin"))
    }
  }
  async function toggle(plugin: RuntimePlugin) {
    setError("")
    setStatus("")
    try {
      await setEnabled.mutateAsync({ id: plugin.id, enabled: !plugin.enabled })
      setStatus(`Plugin ${plugin.enabled ? "disabled" : "enabled"}.`)
    } catch (error) {
      setError(problemMessage(error, "Changing plugin"))
    }
  }

  return (
    <ManagementPage
      title="Runtime plugins"
      description="Install precompiled WebAssembly Components and approve their exact capabilities for this organization. Every version has its own checksum and approval."
    >
      {!canManage ? (
        <p>Only admins can manage runtime plugins.</p>
      ) : (
        <>
          <Notice
            error={
              error || (plugins.isError ? "Plugins could not be loaded." : "")
            }
            status={status}
          />
          <form
            className="grid max-w-2xl gap-3 rounded-xl border p-4"
            onSubmit={(event) => void upload(event)}
          >
            <h2 className="font-medium">Install a component</h2>
            <Label htmlFor="plugin-file">WebAssembly Component (.wasm)</Label>
            <Input
              key={fileKey}
              id="plugin-file"
              type="file"
              accept=".wasm,application/wasm"
              onChange={(event) => setFile(event.target.files?.[0] ?? null)}
            />
            <p className="text-xs text-muted-foreground">
              Maximum 25 MiB. Get the manifest and compiled component from the
              plugin author.
            </p>
            <Label htmlFor="plugin-manifest">Manifest (JSON)</Label>
            <Textarea
              id="plugin-manifest"
              value={manifest}
              onChange={(event) => setManifest(event.target.value)}
              className="min-h-48 font-mono text-xs"
              required
              spellCheck={false}
            />
            <Button
              type="submit"
              className="justify-self-start"
              disabled={uploading}
            >
              {uploading ? "Installing…" : "Install plugin"}
            </Button>
          </form>
          {plugins.isPending ? (
            <p>Loading plugins…</p>
          ) : plugins.data?.items.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              No runtime plugins installed. Built-in mock and shell executors
              appear in the catalog.
            </p>
          ) : (
            <ul className="grid gap-4">
              {plugins.data?.items.map((plugin) => (
                <li
                  key={plugin.id}
                  className="grid gap-3 rounded-xl border p-4"
                >
                  <header className="flex flex-wrap items-center gap-2">
                    <h2 className="font-medium">
                      {plugin.key}@{plugin.version}
                    </h2>
                    <Badge variant="secondary">
                      {plugin.enabled ? "Enabled" : "Disabled"}
                    </Badge>
                    <Badge variant="outline">
                      {plugin.approved ? "Approved" : "Needs approval"}
                    </Badge>
                  </header>
                  <p className="break-all font-mono text-xs text-muted-foreground">
                    SHA-256 {plugin.checksum}
                  </p>
                  {plugin.enabled && plugin.approved && (
                    <Link
                      className="text-sm underline underline-offset-4"
                      href={`/orgs/${organizationId}/catalog?executor=${encodeURIComponent(plugin.key)}&executorVersion=${encodeURIComponent(plugin.version)}`}
                    >
                      Create a node definition for this plugin
                    </Link>
                  )}
                  <ul className="grid gap-1 text-sm">
                    {plugin.manifest.capabilities.map((capability, index) => (
                      <li key={index}>{capabilityLabel(capability)}</li>
                    ))}
                  </ul>
                  {plugin.manifest.capabilities.some(
                    (capability) => capability.type === "command-exec",
                  ) && (
                    <p className="rounded-lg border p-3 text-sm">
                      Host commands run as the shared worker user. Approve them
                      only in a trusted deployment.
                    </p>
                  )}
                  {reviewing === plugin.id ? (
                    <div className="grid gap-3 rounded-lg border bg-muted/40 p-3">
                      <label className="flex items-start gap-2 text-sm">
                        <input
                          className="mt-1"
                          type="checkbox"
                          checked={accepted}
                          onChange={(event) =>
                            setAccepted(event.target.checked)
                          }
                        />
                        I approve the capabilities above for this organization,
                        exact version, manifest, and checksum.
                      </label>
                      <div className="flex gap-2">
                        <Button
                          disabled={!accepted || approve.isPending}
                          onClick={() => void approveVersion(plugin)}
                        >
                          Confirm approval
                        </Button>
                        <Button
                          variant="ghost"
                          onClick={() => setReviewing(null)}
                        >
                          Cancel
                        </Button>
                      </div>
                    </div>
                  ) : (
                    <div className="flex gap-2">
                      {!plugin.approved && (
                        <Button
                          variant="outline"
                          onClick={() => {
                            setReviewing(plugin.id)
                            setAccepted(false)
                          }}
                        >
                          Review approval
                        </Button>
                      )}
                      <Button
                        variant="ghost"
                        disabled={setEnabled.isPending}
                        onClick={() => void toggle(plugin)}
                      >
                        {plugin.enabled ? "Disable" : "Enable"}
                      </Button>
                    </div>
                  )}
                </li>
              ))}
            </ul>
          )}
          <p className="text-xs text-muted-foreground">
            Disabling prevents new publishes and runs. Existing runs continue
            using their pinned plugin version.
          </p>
        </>
      )}
    </ManagementPage>
  )
}
