import { Handle, Position, type NodeProps } from "@xyflow/react"

import type { PortSpec } from "@/api/types"
import { StatusBadge } from "@/components/status/status-badge"
import { isSimulated, type StudioNode } from "@/studio/graph-model"

function Port({
  port,
  side,
  isConnectable,
}: {
  port: PortSpec
  side: "in" | "out"
  isConnectable: boolean
}) {
  return (
    <li className="relative px-3 py-1">
      <Handle
        id={`${side}:${port.name}`}
        type={side === "in" ? "target" : "source"}
        position={side === "in" ? Position.Left : Position.Right}
        isConnectable={isConnectable}
      />
      {port.name}
      <span className="block text-muted-foreground">{port.dataType}</span>
    </li>
  )
}

export function StudioNodeView({ data, isConnectable }: NodeProps<StudioNode>) {
  const { node, definition } = data
  return (
    <div className="min-w-52 rounded-lg border bg-card text-sm text-card-foreground shadow-sm">
      <div className="flex items-center justify-between gap-2 border-b px-3 py-2">
        <span className="font-medium">{node.displayName}</span>
        {isSimulated(node.definition.key) && <StatusBadge status="simulated" />}
      </div>
      {definition ? (
        <div className="grid grid-cols-2 gap-4 py-2 text-xs">
          <ul>
            {definition.inputs.map((port) => (
              <Port
                key={port.name}
                port={port}
                side="in"
                isConnectable={isConnectable}
              />
            ))}
          </ul>
          <ul className="text-right">
            {definition.outputs.map((port) => (
              <Port
                key={port.name}
                port={port}
                side="out"
                isConnectable={isConnectable}
              />
            ))}
          </ul>
        </div>
      ) : (
        <p className="px-3 py-2 text-xs text-destructive">
          Unknown definition {node.definition.key} v{node.definition.version}
        </p>
      )}
    </div>
  )
}
