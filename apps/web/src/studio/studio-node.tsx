import { Handle, Position, type NodeProps } from "@xyflow/react"
import { createContext, useContext } from "react"

import type { PortSpec } from "@/api/types"
import { StatusBadge } from "@/components/status/status-badge"
import type { MappedDiagnostic } from "@/studio/diagnostics"
import { isSimulated, type StudioNode } from "@/studio/graph-model"

// The problems found by the last Validate or Publish.
export const DiagnosticsContext = createContext<MappedDiagnostic[]>([])

function Port({
  port,
  side,
  isConnectable,
  invalid,
}: {
  port: PortSpec
  side: "in" | "out"
  isConnectable: boolean
  invalid: boolean
}) {
  return (
    <li data-invalid={invalid || undefined} className="relative px-3 py-1">
      <Handle
        id={`${side}:${port.name}`}
        type={side === "in" ? "target" : "source"}
        position={side === "in" ? Position.Left : Position.Right}
        isConnectable={isConnectable}
      />
      {port.name}
      {invalid && (
        <span
          role="img"
          aria-label="Has a problem"
          className="ml-1 inline-block size-2 rounded-full bg-destructive"
        />
      )}
      <span className="block text-muted-foreground">{port.dataType}</span>
    </li>
  )
}

export function StudioNodeView({
  id,
  data,
  isConnectable,
}: NodeProps<StudioNode>) {
  const { node, definition } = data
  const problems = useContext(DiagnosticsContext).filter(
    (problem) => problem.nodeId === id,
  )
  const invalid = (handle: string) =>
    problems.some((problem) => problem.port === handle)
  return (
    <div
      data-invalid={problems.length > 0 || undefined}
      className="min-w-52 rounded-lg border bg-card text-sm text-card-foreground shadow-sm data-invalid:border-destructive data-invalid:ring-2 data-invalid:ring-destructive/40"
    >
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
                invalid={invalid(`in:${port.name}`)}
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
                invalid={invalid(`out:${port.name}`)}
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
