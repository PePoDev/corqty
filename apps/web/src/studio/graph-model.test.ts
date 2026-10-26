import { readFileSync } from "node:fs"
import { resolve } from "node:path"
import { expect, it } from "vitest"

import type {
  GraphNode,
  GraphSnapshot,
  NodeDefinition,
  PortSpec,
} from "@/api/types"
import {
  compatible,
  connectionError,
  fromFlow,
  hasCycle,
  insertBlueprint,
  latestDefinitions,
  newNodeFromDefinition,
  refusalMessage,
  toFlow,
  wouldCreateCycle,
} from "@/studio/graph-model"
import { blueprints, nodeDefinitions } from "@/test/handlers"

// The Rust domain tests read the same file, so both sides share one rule set.
const rules: {
  compatibility: { source: string; target: string; compatible: boolean }[]
  cycles: { name: string; edges: [string, string][]; hasCycle: boolean }[]
} = JSON.parse(
  readFileSync(
    resolve(
      import.meta.dirname,
      "../../../../crates/domain/tests/fixtures/graph-rules.json",
    ),
    "utf8",
  ),
)

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/
const infrastructure = blueprints[1] // corqty.simulated-infrastructure

function definition(key: string) {
  const found = nodeDefinitions.find((candidate) => candidate.key === key)
  if (!found) throw new Error(`No seeded definition ${key}`)
  return found
}

function node(
  id: string,
  from: NodeDefinition,
  inputs: GraphNode["inputs"] = {},
): GraphNode {
  return { ...newNodeFromDefinition(from, { x: 0, y: 0 }), id, inputs }
}

function port(name: string, dataType: string): PortSpec {
  return { name, dataType, schema: {}, required: false, description: "" }
}

function connect(
  source: string,
  output: string,
  target: string,
  input: string,
) {
  return {
    source,
    sourceHandle: `out:${output}`,
    target,
    targetHandle: `in:${input}`,
  }
}

it("follows the shared compatibility and cycle rules", () => {
  expect(rules.compatibility.length).toBeGreaterThan(0)
  for (const rule of rules.compatibility) {
    expect(
      compatible(rule.source, rule.target),
      `${rule.source} -> ${rule.target}`,
    ).toBe(rule.compatible)
  }
  expect(rules.cycles.length).toBeGreaterThan(0)
  for (const rule of rules.cycles) {
    expect(hasCycle(rule.edges), rule.name).toBe(rule.hasCycle)
  }
  const chain: [string, string][] = [
    ["a", "b"],
    ["b", "c"],
  ]
  expect(wouldCreateCycle(chain, ["c", "a"])).toBe(true)
  expect(wouldCreateCycle(chain, ["a", "c"])).toBe(false)
})

it("names the reason a connection is refused", () => {
  const mock = definition("corqty.mock.task")
  // Organization definitions with numeric ports, for integer -> number.
  const counter = {
    ...mock,
    id: "acme-counter",
    key: "acme.counter",
    inputs: [port("limit", "core.integer")],
    outputs: [port("count", "core.integer")],
  }
  const average = {
    ...mock,
    id: "acme-average",
    key: "acme.average",
    inputs: [port("value", "core.number")],
    outputs: [port("mean", "core.number")],
  }
  const definitions = [...nodeDefinitions, counter, average]
  // vpc (region bound) feeds rds and deploy; a feeds b.
  const graph: GraphSnapshot = {
    ...infrastructure.graph,
    nodes: [
      ...infrastructure.graph.nodes,
      node("a", mock),
      node("b", mock),
      node("summary", definition("corqty.mock.produce-summary")),
      node("counter", counter),
      node("average", average),
    ],
    edges: [
      ...infrastructure.graph.edges,
      {
        id: "a-b",
        source: { nodeId: "a", port: "result" },
        target: { nodeId: "b", port: "input" },
      },
    ],
  }
  const check = (connection: ReturnType<typeof connect>) =>
    connectionError(graph, definitions, connection)

  expect(check(connect("a", "result", "a", "input"))).toBe("self")
  expect(check(connect("vpc", "vpcId", "rds", "vpcId"))).toBe("occupied")
  expect(check(connect("summary", "summary", "vpc", "region"))).toBe("bound")
  expect(check(connect("rds", "endpoint", "summary", "normalized"))).toBe(
    "incompatible",
  )
  expect(check(connect("average", "mean", "counter", "limit"))).toBe(
    "incompatible",
  )
  expect(check(connect("b", "result", "a", "input"))).toBe("cycle")
  expect(check(connect("counter", "count", "average", "value"))).toBeNull()
  expect(check(connect("rds", "endpoint", "a", "input"))).toBeNull()

  const message = (connection: ReturnType<typeof connect>) =>
    refusalMessage(graph, definitions, connection)
  expect(message(connect("rds", "endpoint", "summary", "normalized"))).toBe(
    "Incompatible types: core.string -> core.object",
  )
  expect(message(connect("b", "result", "a", "input"))).toBe(
    "This connection would create a cycle",
  )
  expect(message(connect("rds", "endpoint", "a", "input"))).toBeNull()
})

it("offers the highest non-deprecated version of each key", () => {
  const mock = definition("corqty.mock.task")
  const versions = [
    { ...mock, id: "old-1", key: "acme.old", deprecated: true },
    { ...mock, id: "thing-1", key: "acme.thing" },
    { ...mock, id: "thing-2", key: "acme.thing", version: 2 },
    { ...mock, id: "thing-3", key: "acme.thing", version: 3, deprecated: true },
    mock,
  ]
  expect(latestDefinitions(versions).map(({ id }) => id)).toEqual([
    "thing-2",
    mock.id,
  ])
})

it("creates a node from a definition with a fresh id and its defaults", () => {
  const vpc = definition("corqty.simulated.create-vpc")
  const first = newNodeFromDefinition(vpc, { x: 40, y: 80 })
  expect(first).toEqual({
    id: expect.stringMatching(uuid),
    definition: { key: "corqty.simulated.create-vpc", version: 1 },
    displayName: "Create VPC (simulated)",
    position: { x: 40, y: 80 },
    config: {
      mode: "fixed",
      output: { vpcId: "vpc-0simulated" },
      delayMs: 500,
      failureMessage: null,
    },
    inputs: {},
    retry: { maxAttempts: 1 },
    timeoutSeconds: 900,
  })
  expect(first.config.output).not.toBe(vpc.defaults.output)
  expect(newNodeFromDefinition(vpc, { x: 0, y: 0 }).id).not.toBe(first.id)
})

it("inserts a blueprint at the drop point with fresh ids and merged inputs", () => {
  const existing = node("existing", definition("corqty.mock.task"))
  const graph: GraphSnapshot = {
    schemaVersion: 1,
    workflowInputSchema: {
      type: "object",
      properties: {
        region: { type: "string", default: "eu-west-1" },
        size: { type: "integer" },
      },
      required: ["size"],
    },
    nodes: [existing],
    edges: [],
    viewport: { x: 10, y: 20, zoom: 2 },
  }

  const result = insertBlueprint(graph, infrastructure, { x: 100, y: 50 })

  const [vpc, rds, deploy] = result.nodes.slice(1)
  expect(result.nodes[0]).toBe(existing)
  expect([vpc, rds, deploy].map(({ position }) => position)).toEqual([
    { x: 100, y: 50 },
    { x: 400, y: -50 },
    { x: 400, y: 150 },
  ])
  const ids = [vpc.id, rds.id, deploy.id, ...result.edges.map(({ id }) => id)]
  expect(new Set(ids).size).toBe(5)
  for (const id of ids) expect(id).toMatch(uuid)
  expect({ ...vpc, id: "vpc", position: { x: 0, y: 0 } }).toEqual(
    infrastructure.graph.nodes[0],
  )
  expect(vpc.config).not.toBe(infrastructure.graph.nodes[0].config)
  expect(result.edges.map(({ source, target }) => [source, target])).toEqual([
    [
      { nodeId: vpc.id, port: "vpcId" },
      { nodeId: rds.id, port: "vpcId" },
    ],
    [
      { nodeId: vpc.id, port: "vpcId" },
      { nodeId: deploy.id, port: "vpcId" },
    ],
  ])
  expect(result.workflowInputSchema).toEqual({
    type: "object",
    properties: {
      region: { type: "string", default: "eu-west-1" },
      size: { type: "integer" },
    },
    required: ["size", "region"],
  })
  expect(result.viewport).toEqual(graph.viewport)
  expect(graph.nodes).toEqual([existing])
})

it("maps a graph onto the canvas and positions back into the graph", () => {
  const { graph } = infrastructure
  const flow = toFlow(graph, nodeDefinitions)
  expect(flow.nodes[0]).toMatchObject({
    id: "vpc",
    type: "step",
    position: { x: 0, y: 0 },
    data: {
      node: graph.nodes[0],
      definition: definition("corqty.simulated.create-vpc"),
    },
  })
  expect(flow.edges[0]).toEqual({
    id: "vpc-rds",
    source: "vpc",
    sourceHandle: "out:vpcId",
    target: "rds",
    targetHandle: "in:vpcId",
  })

  const moved = flow.nodes.map((flowNode) => ({
    ...flowNode,
    position: { x: flowNode.position.x + 5, y: flowNode.position.y - 5 },
  }))

  expect(fromFlow(graph, moved, flow.edges)).toEqual({
    ...graph,
    nodes: graph.nodes.map((graphNode) => ({
      ...graphNode,
      position: { x: graphNode.position.x + 5, y: graphNode.position.y - 5 },
    })),
  })
})
