"use client"

import { useBlueprints, useNodeDefinitions } from "@/api/queries"
import type { NodeDefinition } from "@/api/types"
import { StatusBadge } from "@/components/status/status-badge"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { isSimulated } from "@/studio/graph-model"

// The API orders definitions by key, then version.
function versionsByKey(definitions: NodeDefinition[]) {
  const groups = new Map<string, NodeDefinition[]>()
  for (const definition of definitions) {
    groups.set(definition.key, [
      ...(groups.get(definition.key) ?? []),
      definition,
    ])
  }
  return [...groups.values()]
}

export function CatalogView({ organizationId }: { organizationId: string }) {
  const definitions = useNodeDefinitions(organizationId)
  const blueprints = useBlueprints(organizationId)

  return (
    <section className="flex flex-col gap-5 p-4 md:p-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Catalog</h1>
        <p className="text-sm text-muted-foreground">
          Node definitions and blueprints available to this organization.
        </p>
      </div>
      {(definitions.isPending || blueprints.isPending) && (
        <p className="text-sm text-muted-foreground">Loading catalog…</p>
      )}
      {(definitions.isError || blueprints.isError) && (
        <p role="alert" className="text-sm text-destructive">
          The catalog could not be loaded.
        </p>
      )}
      <h2 id="catalog-definitions" className="text-lg font-semibold">
        Node definitions
      </h2>
      <div className="overflow-x-auto rounded-md border">
        <Table aria-labelledby="catalog-definitions">
          <TableHeader>
            <TableRow>
              <TableHead>Name</TableHead>
              <TableHead>Key</TableHead>
              <TableHead>Version</TableHead>
              <TableHead>Category</TableHead>
              <TableHead>Description</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {versionsByKey(definitions.data?.items ?? []).map((versions) => {
              const current = versions.findLast(
                (version) => !version.deprecated,
              )
              const shown = current ?? versions[versions.length - 1]
              return (
                <TableRow key={shown.key}>
                  <TableCell>
                    <span className="flex items-center gap-2 font-medium">
                      {shown.displayName}
                      {isSimulated(shown.key) && (
                        <StatusBadge status="simulated" />
                      )}
                    </span>
                  </TableCell>
                  <TableCell className="font-mono text-xs">
                    {shown.key}
                  </TableCell>
                  <TableCell>
                    {current && <span>v{current.version}</span>}
                    {versions
                      .filter((version) => version.deprecated)
                      .map((version) => (
                        <span
                          key={version.id}
                          className="block text-xs text-muted-foreground"
                        >
                          v{version.version} deprecated
                        </span>
                      ))}
                  </TableCell>
                  <TableCell>{shown.category}</TableCell>
                  <TableCell className="min-w-64 whitespace-normal text-muted-foreground">
                    {shown.description}
                  </TableCell>
                </TableRow>
              )
            })}
          </TableBody>
        </Table>
      </div>
      <h2 id="catalog-blueprints" className="text-lg font-semibold">
        Blueprints
      </h2>
      <div className="overflow-x-auto rounded-md border">
        <Table aria-labelledby="catalog-blueprints">
          <TableHeader>
            <TableRow>
              <TableHead>Name</TableHead>
              <TableHead>Key</TableHead>
              <TableHead>Description</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {blueprints.data?.items.map((blueprint) => (
              <TableRow key={blueprint.id}>
                <TableCell>
                  <span className="flex items-center gap-2 font-medium">
                    {blueprint.displayName}
                    {isSimulated(blueprint.key) && (
                      <StatusBadge status="simulated" />
                    )}
                  </span>
                </TableCell>
                <TableCell className="font-mono text-xs">
                  {blueprint.key}
                </TableCell>
                <TableCell className="min-w-64 whitespace-normal text-muted-foreground">
                  {blueprint.description}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </section>
  )
}
