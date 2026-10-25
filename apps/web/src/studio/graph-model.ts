import type { Edge, Node } from "@xyflow/react"

import type {
  Blueprint,
  GraphNode,
  GraphSnapshot,
  NodeDefinition,
} from "@/api/types"

// A canvas node carries its graph node; XYFlow owns the position while editing.
export type StudioNode = Node<
  { node: GraphNode; definition: NodeDefinition | undefined },
  "step"
>

// Node handles are named `in:<port>` and `out:<port>`.
export type PortConnection = {
  source: string
  sourceHandle?: string | null
  target: string
  targetHandle?: string | null
}

export type ConnectionError =
  | "self"
  | "occupied"
  | "bound"
  | "incompatible"
  | "cycle"

export function isSimulated(key: string) {
  return key.startsWith("corqty.simulated")
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
}

function portName(handle: string | null | undefined) {
  return handle?.split(":")[1] ?? ""
}

export function findDefinition(
  definitions: NodeDefinition[],
  { key, version }: GraphNode["definition"],
) {
  return definitions.find(
    (definition) => definition.key === key && definition.version === version,
  )
}

export function toFlowNode(
  node: GraphNode,
  definitions: NodeDefinition[],
): StudioNode {
  return {
    id: node.id,
    type: "step",
    position: node.position,
    ariaLabel: node.displayName,
    data: { node, definition: findDefinition(definitions, node.definition) },
  }
}

// Draft saves accept any JSON as the input schema and as node config (D13);
// the studio edits them as objects.
export function normalizeGraph(graph: GraphSnapshot): GraphSnapshot {
  return {
    ...graph,
    workflowInputSchema: isRecord(graph.workflowInputSchema)
      ? graph.workflowInputSchema
      : { type: "object", properties: {} },
    nodes: graph.nodes.map((node) =>
      isRecord(node.config) ? node : { ...node, config: {} },
    ),
  }
}

export function toFlow(graph: GraphSnapshot, definitions: NodeDefinition[]) {
  return {
    nodes: graph.nodes.map((node) => toFlowNode(node, definitions)),
    edges: graph.edges.map(
      (edge): Edge => ({
        id: edge.id,
        source: edge.source.nodeId,
        sourceHandle: `out:${edge.source.port}`,
        target: edge.target.nodeId,
        targetHandle: `in:${edge.target.port}`,
      }),
    ),
  }
}

// Nodes and edges come from the canvas, everything else from `graph`.
export function fromFlow(
  graph: GraphSnapshot,
  nodes: StudioNode[],
  edges: Edge[],
): GraphSnapshot {
  return {
    ...graph,
    nodes: nodes.map(({ position, data }) => ({
      ...data.node,
      position: { x: position.x, y: position.y },
    })),
    edges: edges.map((edge) => ({
      id: edge.id,
      source: { nodeId: edge.source, port: portName(edge.sourceHandle) },
      target: { nodeId: edge.target, port: portName(edge.targetHandle) },
    })),
  }
}

// Mirrors corqty_domain::graph::compatible.
export function compatible(source: string, target: string) {
  return (
    source === target ||
    (source === "core.integer" && target === "core.number") ||
    target === "core.any"
  )
}

// Kahn's algorithm: nodes on a cycle never run out of incoming edges.
export function hasCycle(edges: [string, string][]) {
  const incoming = new Map<string, number>()
  const outgoing = new Map<string, string[]>()
  for (const [source, target] of edges) {
    incoming.set(source, incoming.get(source) ?? 0)
    incoming.set(target, (incoming.get(target) ?? 0) + 1)
    outgoing.set(source, [...(outgoing.get(source) ?? []), target])
  }
  const ready = [...incoming.keys()].filter((id) => incoming.get(id) === 0)
  // `ready` grows while it is walked.
  for (const id of ready) {
    for (const target of outgoing.get(id) ?? []) {
      const count = (incoming.get(target) ?? 0) - 1
      incoming.set(target, count)
      if (count === 0) ready.push(target)
    }
  }
  return ready.length < incoming.size
}

export function wouldCreateCycle(
  edges: [string, string][],
  candidate: [string, string],
) {
  return hasCycle([...edges, candidate])
}

// The data type behind an `in:<port>` or `out:<port>` handle.
export function portType(
  graph: GraphSnapshot,
  definitions: NodeDefinition[],
  nodeId: string,
  handle: string | null | undefined,
) {
  const node = graph.nodes.find((candidate) => candidate.id === nodeId)
  const definition = node && findDefinition(definitions, node.definition)
  const ports = handle?.startsWith("in:")
    ? definition?.inputs
    : definition?.outputs
  return ports?.find((port) => port.name === portName(handle))?.dataType
}

// Checked in this order; null means the connection may become an edge.
export function connectionError(
  graph: GraphSnapshot,
  definitions: NodeDefinition[],
  connection: PortConnection,
): ConnectionError | null {
  const { source, sourceHandle, target, targetHandle } = connection
  const port = portName(targetHandle)
  if (source === target) return "self"
  const occupied = graph.edges.some(
    (edge) => edge.target.nodeId === target && edge.target.port === port,
  )
  if (occupied) return "occupied"
  const targetNode = graph.nodes.find((node) => node.id === target)
  if (targetNode && Object.hasOwn(targetNode.inputs, port)) return "bound"
  const sourceType = portType(graph, definitions, source, sourceHandle)
  const targetType = portType(graph, definitions, target, targetHandle)
  if (!sourceType || !targetType || !compatible(sourceType, targetType)) {
    return "incompatible"
  }
  const edges = graph.edges.map(
    (edge): [string, string] => [edge.source.nodeId, edge.target.nodeId],
  )
  return wouldCreateCycle(edges, [source, target]) ? "cycle" : null
}

const refusals = {
  self: "A node cannot connect to itself",
  occupied: "This input already has a connection",
  bound: "This input already has a bound value",
  cycle: "This connection would create a cycle",
}

export function refusalMessage(
  graph: GraphSnapshot,
  definitions: NodeDefinition[],
  connection: PortConnection,
) {
  const error = connectionError(graph, definitions, connection)
  if (error !== "incompatible") return error && refusals[error]
  const { source, sourceHandle, target, targetHandle } = connection
  const sourceType = portType(graph, definitions, source, sourceHandle)
  const targetType = portType(graph, definitions, target, targetHandle)
  return `Incompatible types: ${sourceType} -> ${targetType}`
}

// The palette offers the highest non-deprecated version of each key.
export function latestDefinitions(definitions: NodeDefinition[]) {
  const latest = new Map<string, NodeDefinition>()
  for (const definition of definitions) {
    const current = latest.get(definition.key)
    if (
      !definition.deprecated &&
      (!current || definition.version > current.version)
    ) {
      latest.set(definition.key, definition)
    }
  }
  return [...latest.values()]
}

export function newNodeFromDefinition(
  definition: NodeDefinition,
  position: GraphNode["position"],
): GraphNode {
  return {
    id: crypto.randomUUID(),
    definition: { key: definition.key, version: definition.version },
    displayName: definition.displayName,
    position,
    config: structuredClone(definition.defaults),
    inputs: {},
    retry: { maxAttempts: 1 },
    timeoutSeconds: 900,
  }
}

// D14: the blueprint's origin lands on the drop point, every node and edge gets
// a fresh id, and its workflow inputs join the graph's (existing entries win).
export function insertBlueprint(
  graph: GraphSnapshot,
  blueprint: Blueprint,
  position: GraphNode["position"],
): GraphSnapshot {
  const ids = new Map(
    blueprint.graph.nodes.map((node) => [node.id, crypto.randomUUID()]),
  )
  const freshId = (id: string) => ids.get(id) ?? id
  const schema = graph.workflowInputSchema
  const added = blueprint.graph.workflowInputSchema
  const required = [
    ...new Set([...(schema.required ?? []), ...(added.required ?? [])]),
  ]
  return {
    ...graph,
    workflowInputSchema: {
      ...schema,
      properties: { ...added.properties, ...schema.properties },
      ...(required.length > 0 && { required }),
    },
    nodes: [
      ...graph.nodes,
      ...blueprint.graph.nodes.map((node) => ({
        ...structuredClone(node),
        id: freshId(node.id),
        position: {
          x: position.x + node.position.x,
          y: position.y + node.position.y,
        },
      })),
    ],
    edges: [
      ...graph.edges,
      ...blueprint.graph.edges.map((edge) => ({
        id: crypto.randomUUID(),
        source: { ...edge.source, nodeId: freshId(edge.source.nodeId) },
        target: { ...edge.target, nodeId: freshId(edge.target.nodeId) },
      })),
    ],
  }
}
