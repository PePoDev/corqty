"use client"

import { useId, useState } from "react"

import type { Blueprint, NodeDefinition } from "@/api/types"
import { StatusBadge } from "@/components/status/status-badge"
import { Input } from "@/components/ui/input"
import { isSimulated, latestDefinitions } from "@/studio/graph-model"

export const DEFINITION_TYPE = "application/x-corqty-definition"
export const BLUEPRINT_TYPE = "application/x-corqty-blueprint"

type Item = { id: string; key: string; displayName: string }

// ponytail: mouse drag and drop only; a keyboard "add" action can follow.
function PaletteList({
  title,
  format,
  items,
}: {
  title: string
  format: string
  items: Item[]
}) {
  const headingId = useId()
  return (
    <section className="grid gap-2">
      <h2 id={headingId} className="text-xs font-medium text-muted-foreground">
        {title}
      </h2>
      <ul aria-labelledby={headingId} className="grid gap-1.5">
        {items.map((item) => (
          <li
            key={item.id}
            draggable
            onDragStart={(event) => {
              event.dataTransfer.setData(format, item.id)
              event.dataTransfer.effectAllowed = "move"
            }}
            className="cursor-grab rounded-md border bg-card px-3 py-2 text-sm"
          >
            <span className="flex items-center gap-2 font-medium">
              {item.displayName}
              {isSimulated(item.key) && <StatusBadge status="simulated" />}
            </span>
            <span className="block truncate font-mono text-xs text-muted-foreground">
              {item.key}
            </span>
          </li>
        ))}
      </ul>
      {items.length === 0 && (
        <p className="text-sm text-muted-foreground">No matches.</p>
      )}
    </section>
  )
}

export function Palette({
  definitions,
  blueprints,
}: {
  definitions: NodeDefinition[]
  blueprints: Blueprint[]
}) {
  const [filter, setFilter] = useState("")
  const query = filter.trim().toLowerCase()
  const matches = (item: Item) =>
    item.displayName.toLowerCase().includes(query) ||
    item.key.toLowerCase().includes(query)

  return (
    <aside
      aria-label="Palette"
      className="flex min-h-0 flex-col gap-4 overflow-y-auto border-r p-3"
    >
      <Input
        type="search"
        aria-label="Filter the palette"
        placeholder="Filter by name or key"
        value={filter}
        onChange={(event) => setFilter(event.target.value)}
      />
      <PaletteList
        title="Node definitions"
        format={DEFINITION_TYPE}
        items={latestDefinitions(definitions).filter(matches)}
      />
      <PaletteList
        title="Blueprints"
        format={BLUEPRINT_TYPE}
        items={blueprints.filter(matches)}
      />
    </aside>
  )
}
