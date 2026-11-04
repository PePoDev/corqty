import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useState } from "react"
import { expect, it, vi } from "vitest"

import type { GraphNode, GraphSnapshot, JsonSchema } from "@/api/types"
import { newNodeFromDefinition } from "@/studio/graph-model"
import { Inspector } from "@/studio/inspector"
import { blueprints, nodeDefinitions, shellCommand } from "@/test/handlers"

const infrastructure = blueprints[1].graph // vpc -> rds, vpc -> deploy
const secretId = "0190f5c2-7d1e-7a00-8000-0000000000aa"

// Holds the graph like the canvas does and records what the inspector writes.
function renderInspector(
  nodeId: string | null,
  { graph = infrastructure, readOnly = false } = {},
) {
  const nodeChanges = vi.fn<(node: GraphNode) => void>()
  const schemaChanges = vi.fn<(schema: JsonSchema) => void>()
  function Harness() {
    const [current, setCurrent] = useState<GraphSnapshot>(graph)
    return (
      <Inspector
        graph={current}
        definitions={nodeDefinitions}
        nodeId={nodeId}
        readOnly={readOnly}
        onNodeChange={(node) => {
          nodeChanges(node)
          setCurrent((previous) => ({
            ...previous,
            nodes: previous.nodes.map((candidate) =>
              candidate.id === node.id ? node : candidate,
            ),
          }))
        }}
        onInputSchemaChange={(schema) => {
          schemaChanges(schema)
          setCurrent((previous) => ({
            ...previous,
            workflowInputSchema: schema,
          }))
        }}
      />
    )
  }
  render(<Harness />)
  return {
    nodeChanges,
    schemaChanges,
    lastNode: () => nodeChanges.mock.lastCall?.[0],
  }
}

// Each input port is a group named after the port.
const port = (name: string) =>
  screen.getByRole("group", { name: new RegExp(`^${name} `) })

it("edits the node's name and its configuration through the uiSchema widgets", async () => {
  const { lastNode } = renderInspector("vpc")
  expect(
    screen.getByRole("heading", { name: "Create VPC (simulated)" }),
  ).toBeInTheDocument()
  const name = screen.getByLabelText("Display name")
  await userEvent.clear(name)
  await userEvent.type(name, "Network")
  expect(lastNode()?.displayName).toBe("Network")
  expect(screen.getByRole("heading", { name: "Network" })).toBeInTheDocument()

  expect(screen.getByLabelText("Mode")).toHaveValue("fixed")
  expect(screen.getByLabelText("Fixed output")).toHaveValue(
    '{\n  "vpcId": "vpc-0simulated"\n}',
  )
  expect(screen.getByLabelText("Delay (ms)")).toHaveValue(500)
  await userEvent.selectOptions(screen.getByLabelText("Mode"), "echo")
  await userEvent.type(screen.getByLabelText("Fail with message"), "boom")
  expect(lastNode()?.config).toEqual({
    mode: "echo",
    output: { vpcId: "vpc-0simulated" },
    delayMs: 500,
    failureMessage: "boom",
  })
})

it("warns before a shell node is configured", () => {
  const shell = newNodeFromDefinition(shellCommand, { x: 0, y: 0 })
  const graph = { ...infrastructure, nodes: [shell], edges: [] }
  renderInspector(shell.id, { graph })
  expect(screen.getByRole("alert")).toHaveTextContent("Trusted users only")
  expect(screen.getByLabelText("Command")).toHaveValue(
    `jq '{result: .}' "$CORQTY_INPUT" > "$CORQTY_OUTPUT"`,
  )
})

it("shows where a connected input comes from instead of an editor", () => {
  renderInspector("rds")
  const vpcId = port("vpcId")
  expect(vpcId).toHaveTextContent("Connected from Create VPC (simulated).vpcId")
  expect(within(vpcId).queryByRole("combobox")).not.toBeInTheDocument()
})

it("binds an input to a workflow input, a literal or a secret", async () => {
  const graph = {
    ...infrastructure,
    workflowInputSchema: {
      type: "object",
      properties: { region: { type: "string" }, "a/b": { type: "string" } },
    },
  }
  const { lastNode } = renderInspector("vpc", { graph })
  const region = port("region")
  const binding = within(region).getByLabelText("Binding")
  expect(binding).toHaveValue("workflowInput")
  const picker = within(region).getByLabelText("Workflow input")
  expect(picker).toHaveValue("/region")
  expect(
    within(picker)
      .getAllByRole("option")
      .map((option) => option.textContent),
  ).toEqual(["Choose an input…", "/region", "/a~1b"])
  await userEvent.selectOptions(picker, "/a~1b")
  expect(lastNode()?.inputs).toEqual({
    region: { kind: "workflowInput", pointer: "/a~1b" },
  })

  await userEvent.selectOptions(binding, "Literal value")
  expect(lastNode()?.inputs).toEqual({ region: { kind: "literal", value: "" } })
  await userEvent.type(within(region).getByLabelText("Value"), "eu-west-1")
  expect(lastNode()?.inputs).toEqual({
    region: { kind: "literal", value: "eu-west-1" },
  })

  await userEvent.selectOptions(binding, "Secret")
  expect(lastNode()?.inputs).toEqual({})
  expect(
    within(region).getByText("Secret picker arrives with secrets."),
  ).toBeInTheDocument()
  const secret = within(region).getByLabelText("Secret id")
  await userEvent.type(secret, "not-a-uuid")
  expect(within(region).getByRole("alert")).toHaveTextContent(
    "Enter the secret id as a UUID.",
  )
  expect(lastNode()?.inputs).toEqual({})
  await userEvent.clear(secret)
  await userEvent.paste(secretId)
  expect(lastNode()?.inputs).toEqual({ region: { kind: "secret", secretId } })

  await userEvent.selectOptions(binding, "Not bound")
  expect(lastNode()?.inputs).toEqual({})
})

it("retry and timeout take whole numbers within their bounds", async () => {
  const { nodeChanges, lastNode } = renderInspector("vpc")
  const attempts = screen.getByLabelText("Max attempts")
  expect(attempts).toHaveValue(1)
  await userEvent.clear(attempts)
  await userEvent.type(attempts, "0")
  expect(screen.getByRole("alert")).toHaveTextContent(
    "Enter a whole number from 1 to 5.",
  )
  await userEvent.type(attempts, "{Backspace}6")
  expect(screen.getByRole("alert")).toBeInTheDocument()
  expect(nodeChanges).not.toHaveBeenCalled()
  await userEvent.type(attempts, "{Backspace}5")
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
  expect(lastNode()?.retry).toEqual({ maxAttempts: 5 })

  const timeout = screen.getByLabelText("Timeout (seconds)")
  expect(timeout).toHaveValue(900)
  await userEvent.clear(timeout)
  await userEvent.type(timeout, "14401")
  expect(screen.getByRole("alert")).toHaveTextContent(
    "Enter a whole number from 1 to 14400.",
  )
  expect(lastNode()?.timeoutSeconds).toBe(1440)
  await userEvent.type(timeout, "{Backspace}0")
  expect(lastNode()?.timeoutSeconds).toBe(14_400)
  expect(lastNode()?.retry).toEqual({ maxAttempts: 5 })
})

it("without a selected node it edits the workflow input schema as JSON", async () => {
  const { schemaChanges } = renderInspector(null)
  expect(
    screen.getByRole("heading", { name: "Workflow inputs" }),
  ).toBeInTheDocument()
  const editor = screen.getByLabelText("Input schema (JSON)")
  expect(editor).toHaveValue(
    JSON.stringify(infrastructure.workflowInputSchema, null, 2),
  )
  await userEvent.clear(editor)
  await userEvent.paste("null")
  expect(screen.getByRole("alert")).toBeInTheDocument()
  expect(schemaChanges).not.toHaveBeenCalled()
  await userEvent.clear(editor)
  await userEvent.paste('{"type": "object", "properties": {}}')
  expect(schemaChanges).toHaveBeenLastCalledWith({
    type: "object",
    properties: {},
  })
})

it.each(["vpc", null])(
  "read-only disables every control (node %s)",
  (nodeId) => {
    renderInspector(nodeId, { readOnly: true })
    const controls = screen
      .getByRole("complementary", { name: "Inspector" })
      .querySelectorAll("input, select, textarea, button")
    expect(controls.length).toBeGreaterThan(0)
    for (const control of controls) {
      expect(control).toBeDisabled()
    }
  },
)
