"use client"

import "@xyflow/react/dist/style.css"

import {
  Background,
  Controls,
  ReactFlow,
  type NodeProps,
  type NodeTypes,
  type OnSelectionChangeFunc,
} from "@xyflow/react"
import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
} from "react"

import type { GraphSnapshot, NodeDefinition, NodeRun } from "@/api/types"
import { StatusBadge } from "@/components/status/status-badge"
import { toFlow, type StudioNode } from "@/studio/graph-model"
import { StudioNodeView } from "@/studio/studio-node"

// Node runs by node id. Nodes read them from context, so a state change
// re-renders only the overlays: the node objects, their measured sizes and
// the viewport stay as they are.
const NodeRunsContext = createContext<ReadonlyMap<string, NodeRun>>(new Map())

function RunNodeView(props: NodeProps<StudioNode>) {
  const nodeRun = useContext(NodeRunsContext).get(props.id)
  return (
    <div
      data-selected={props.selected || undefined}
      className="relative rounded-lg data-selected:ring-2 data-selected:ring-ring"
    >
      {nodeRun && (
        <span className="absolute -top-3 right-2 rounded-4xl bg-background">
          <StatusBadge status={nodeRun.state} />
        </span>
      )}
      <StudioNodeView {...props} />
    </div>
  )
}

const nodeTypes = { step: RunNodeView } satisfies NodeTypes

// The run's pinned graph, read once: key it by run. XYFlow keeps the nodes, so
// they stay selectable by pointer and keyboard; the one selected node, or
// null, goes to onNodeSelect.
export function RunGraph({
  graph,
  definitions,
  nodeRuns,
  onNodeSelect,
}: {
  graph: GraphSnapshot
  definitions: NodeDefinition[]
  nodeRuns: NodeRun[]
  onNodeSelect: (nodeId: string | null) => void
}) {
  const [flow] = useState(() => toFlow(graph, definitions))
  const byNode = useMemo(
    () => new Map(nodeRuns.map((nodeRun) => [nodeRun.nodeId, nodeRun])),
    [nodeRuns],
  )
  const onSelectionChange: OnSelectionChangeFunc = useCallback(
    ({ nodes }) => onNodeSelect(nodes.length === 1 ? nodes[0].id : null),
    [onNodeSelect],
  )
  return (
    <NodeRunsContext value={byNode}>
      <ReactFlow
        defaultNodes={flow.nodes}
        defaultEdges={flow.edges}
        nodeTypes={nodeTypes}
        onSelectionChange={onSelectionChange}
        nodesDraggable={false}
        nodesConnectable={false}
        edgesFocusable={false}
        deleteKeyCode={null}
        defaultViewport={graph.viewport}
        colorMode="system"
      >
        <Background />
        <Controls showInteractive={false} />
      </ReactFlow>
    </NodeRunsContext>
  )
}
