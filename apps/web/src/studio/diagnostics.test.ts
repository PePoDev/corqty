import { expect, it } from "vitest"

import { mapDiagnostics } from "@/studio/diagnostics"
import { blueprints } from "@/test/handlers"

// Nodes vpc, rds and deploy; edges vpc-rds and vpc-deploy.
const graph = blueprints[1].graph

it.each([
  ["/nodes/1", { nodeId: "rds" }],
  ["/nodes/0/config/delayMs", { nodeId: "vpc" }],
  ["/nodes/2/inputs/vpcId", { nodeId: "deploy", port: "in:vpcId" }],
  ["/nodes/0/inputs/a~1b~0c/value/0", { nodeId: "vpc", port: "in:a/b~c" }],
  ["/edges/1", { edgeId: "vpc-deploy", nodeId: "deploy", port: "in:vpcId" }],
  ["/edges/0/source", { edgeId: "vpc-rds", nodeId: "vpc", port: "out:vpcId" }],
  ["/edges/0/target", { edgeId: "vpc-rds", nodeId: "rds", port: "in:vpcId" }],
  ["/edges/1/id", { edgeId: "vpc-deploy" }],
  ["/nodes/3/id", {}],
  ["/edges/2", {}],
  ["/nodes", {}],
  ["/workflowInputSchema/properties/region", {}],
])("places %s on the graph that was sent", (pointer, placement) => {
  const diagnostic = { code: "some_code", pointer, message: "A message." }
  expect(mapDiagnostics(graph, [diagnostic])).toEqual([
    { ...diagnostic, ...placement },
  ])
})
