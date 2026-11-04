"use client"

import "@xyflow/react/dist/style.css"

import {
  Background,
  Controls,
  ReactFlow,
  ReactFlowProvider,
  addEdge,
  useEdgesState,
  useNodesState,
  useReactFlow,
  type Connection,
  type IsValidConnection,
  type NodeTypes,
  type OnConnectEnd,
} from "@xyflow/react"
import { useEffect, useState, type DragEvent } from "react"

import type {
  Blueprint,
  GraphNode,
  GraphSnapshot,
  NodeDefinition,
} from "@/api/types"
import type { MappedDiagnostic } from "@/studio/diagnostics"
import { cn } from "@/lib/utils"
import {
  connectionError,
  fromFlow,
  insertBlueprint,
  newNodeFromDefinition,
  normalizeGraph,
  refusalMessage,
  toFlow,
  toFlowNode,
} from "@/studio/graph-model"
import { Inspector } from "@/studio/inspector"
import { BLUEPRINT_TYPE, DEFINITION_TYPE, Palette } from "@/studio/palette"
import { DiagnosticsContext, StudioNodeView } from "@/studio/studio-node"

const nodeTypes = { step: StudioNodeView } satisfies NodeTypes

type CanvasProps = {
  graph: GraphSnapshot
  definitions: NodeDefinition[]
  paletteDefinitions: NodeDefinition[]
  blueprints: Blueprint[]
  readOnly: boolean
  problems: MappedDiagnostic[]
  onChange: (graph: GraphSnapshot) => void
}

function Canvas({
  graph,
  definitions,
  paletteDefinitions,
  blueprints,
  readOnly,
  problems,
  onChange,
}: CanvasProps) {
  // XYFlow holds the nodes and edges; this holds the rest of the snapshot.
  const [snapshot, setSnapshot] = useState(() => normalizeGraph(graph))
  const [initial] = useState(() => toFlow(snapshot, definitions))
  const [nodes, setNodes, onNodesChange] = useNodesState(initial.nodes)
  const [edges, setEdges, onEdgesChange] = useEdgesState(initial.edges)
  const [announcement, setAnnouncement] = useState("")
  const { screenToFlowPosition } = useReactFlow()

  const current = () => fromFlow(snapshot, nodes, edges)
  // The studio saves what the canvas reports.
  useEffect(() => {
    onChange(fromFlow(snapshot, nodes, edges))
  }, [onChange, snapshot, nodes, edges])
  // The inspector edits a single selected node, otherwise the workflow inputs.
  const selected = nodes.filter((node) => node.selected)

  function updateNode(node: GraphNode) {
    setNodes((existing) =>
      existing.map((candidate) =>
        candidate.id === node.id
          ? {
              ...candidate,
              ariaLabel: node.displayName,
              data: { ...candidate.data, node },
            }
          : candidate,
      ),
    )
  }

  const isValidConnection: IsValidConnection = (connection) =>
    connectionError(current(), definitions, connection) === null

  function onConnect(connection: Connection) {
    setEdges((existing) =>
      addEdge({ ...connection, id: crypto.randomUUID() }, existing),
    )
  }

  // Explain why a connection dropped on a port was refused.
  const onConnectEnd: OnConnectEnd = (_, { fromHandle, toHandle }) => {
    if (!fromHandle || !toHandle || fromHandle.type === toHandle.type) return
    const [source, target] =
      fromHandle.type === "source"
        ? [fromHandle, toHandle]
        : [toHandle, fromHandle]
    const message = refusalMessage(current(), definitions, {
      source: source.nodeId,
      sourceHandle: source.id,
      target: target.nodeId,
      targetHandle: target.id,
    })
    setAnnouncement(message ?? "")
  }

  function onDragOver(event: DragEvent) {
    event.preventDefault()
    event.dataTransfer.dropEffect = "move"
  }

  function onDrop(event: DragEvent) {
    event.preventDefault()
    const position = screenToFlowPosition({
      x: event.clientX,
      y: event.clientY,
    })
    insert(
      event.dataTransfer.getData(DEFINITION_TYPE),
      event.dataTransfer.getData(BLUEPRINT_TYPE),
      position,
    )
  }

  function insert(
    definitionId: string,
    blueprintId: string,
    position: { x: number; y: number },
  ) {
    const definition = definitions.find(({ id }) => id === definitionId)
    if (definition) {
      const node = toFlowNode(
        newNodeFromDefinition(definition, position),
        definitions,
      )
      setNodes((existing) => [...existing, node])
      setAnnouncement(`Added ${definition.displayName} to the canvas.`)
      return
    }
    const blueprint = blueprints.find(({ id }) => id === blueprintId)
    if (blueprint) {
      const next = insertBlueprint(current(), blueprint, position)
      const flow = toFlow(next, definitions)
      setSnapshot(next)
      // insertBlueprint appends, so the nodes already on the canvas keep their
      // measured size and selection.
      setNodes([...nodes, ...flow.nodes.slice(nodes.length)])
      setEdges([...edges, ...flow.edges.slice(edges.length)])
      setAnnouncement(`Added ${blueprint.displayName} to the canvas.`)
    }
  }

  // Keep keyboard additions in the visible viewport, offset from one another.
  function addFromPalette(definitionId: string, blueprintId: string) {
    const { x, y, zoom } = snapshot.viewport
    const offset = 60 + (nodes.length % 6) * 28
    insert(definitionId, blueprintId, {
      x: (offset - x) / zoom,
      y: (offset - y) / zoom,
    })
  }

  return (
    <div className={cn("grid min-h-0", !readOnly && "grid-cols-[16rem_1fr]")}>
      {!readOnly && (
        <Palette
          definitions={paletteDefinitions}
          blueprints={blueprints}
          onAddDefinition={(id) => addFromPalette(id, "")}
          onAddBlueprint={(id) => addFromPalette("", id)}
        />
      )}
      <div className="grid min-h-0 xl:grid-cols-[1fr_20rem]">
        <div className="flex min-h-96 flex-col">
          <div className="min-h-0 flex-1">
            <DiagnosticsContext value={problems}>
              <ReactFlow
                nodes={nodes}
                edges={edges}
                nodeTypes={nodeTypes}
                onNodesChange={onNodesChange}
                onEdgesChange={onEdgesChange}
                onConnect={onConnect}
                onConnectEnd={onConnectEnd}
                isValidConnection={isValidConnection}
                onDragOver={readOnly ? undefined : onDragOver}
                onDrop={readOnly ? undefined : onDrop}
                onMoveEnd={(_, viewport) =>
                  setSnapshot((existing) => ({ ...existing, viewport }))
                }
                nodesDraggable={!readOnly}
                nodesConnectable={!readOnly}
                deleteKeyCode={readOnly ? null : ["Backspace", "Delete"]}
                defaultViewport={graph.viewport}
                colorMode="system"
              >
                <Background />
                <Controls showInteractive={false} />
              </ReactFlow>
            </DiagnosticsContext>
          </div>
          <p
            role="status"
            aria-live="polite"
            className="min-h-9 border-t px-4 py-2 text-sm text-muted-foreground"
          >
            {announcement}
          </p>
        </div>
        <Inspector
          graph={current()}
          definitions={definitions}
          nodeId={selected.length === 1 ? selected[0].id : null}
          readOnly={readOnly}
          onNodeChange={updateNode}
          onInputSchemaChange={(workflowInputSchema) =>
            setSnapshot((existing) => ({ ...existing, workflowInputSchema }))
          }
        />
      </div>
    </div>
  )
}

export function StudioCanvas(props: CanvasProps) {
  return (
    <ReactFlowProvider>
      <Canvas {...props} />
    </ReactFlowProvider>
  )
}
