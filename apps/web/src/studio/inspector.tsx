"use client"

import { useId, useState } from "react"

import type {
  GraphNode,
  GraphSnapshot,
  InputBinding,
  JsonSchema,
  NodeDefinition,
  PortSpec,
} from "@/api/types"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { findDefinition } from "@/studio/graph-model"
import {
  blankValue,
  selectClassName,
  widgetFor,
  widgets,
} from "@/studio/widgets"

const bindingKinds = {
  none: "Not bound",
  literal: "Literal value",
  workflowInput: "Workflow input",
  secret: "Secret",
}

type BindingKind = keyof typeof bindingKinds

function text(value: unknown) {
  return typeof value === "string" ? value : undefined
}

// Writes only whole numbers in range, so the draft always deserializes.
function WholeNumber({
  label,
  min,
  max,
  value,
  onChange,
}: {
  label: string
  min: number
  max: number
  value: number
  onChange: (value: number) => void
}) {
  const id = useId()
  const [entered, setEntered] = useState(String(value))
  const valid = (candidate: string) =>
    /^\d+$/.test(candidate) &&
    Number(candidate) >= min &&
    Number(candidate) <= max
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      <Input
        id={id}
        type="number"
        min={min}
        max={max}
        step={1}
        value={entered}
        aria-invalid={!valid(entered) || undefined}
        onChange={({ target }) => {
          setEntered(target.value)
          if (valid(target.value)) onChange(Number(target.value))
        }}
      />
      {!valid(entered) && (
        <p role="alert" className="text-xs text-destructive">
          Enter a whole number from {min} to {max}.
        </p>
      )}
    </div>
  )
}

function Binding({
  graph,
  node,
  port,
  onChange,
}: {
  graph: GraphSnapshot
  node: GraphNode
  port: PortSpec
  onChange: (node: GraphNode) => void
}) {
  const kindId = useId()
  const pointerId = useId()
  const binding = node.inputs[port.name]
  const [kind, setKind] = useState<BindingKind>(binding?.kind ?? "none")
  const edge = graph.edges.find(
    ({ target }) => target.nodeId === node.id && target.port === port.name,
  )
  const source = graph.nodes.find(({ id }) => id === edge?.source.nodeId)
  const pointer = binding?.kind === "workflowInput" ? binding.pointer : ""
  // ponytail: the picker offers top-level workflow inputs only.
  const pointers = Object.keys(graph.workflowInputSchema.properties ?? {}).map(
    (name) => `/${name.replaceAll("~", "~0").replaceAll("/", "~1")}`,
  )
  if (pointer && !pointers.includes(pointer)) pointers.push(pointer)
  const Literal = widgets[widgetFor(port.schema)]

  // null leaves the port unbound.
  function bind(next: InputBinding | null) {
    const inputs = Object.fromEntries(
      Object.entries(node.inputs).filter(([name]) => name !== port.name),
    )
    onChange({
      ...node,
      inputs: next ? { ...inputs, [port.name]: next } : inputs,
    })
  }

  function choose(next: BindingKind) {
    setKind(next)
    // A workflow input or a secret binds once one is chosen.
    bind(
      next === "literal"
        ? { kind: "literal", value: blankValue(port.schema) }
        : null,
    )
  }

  return (
    <fieldset className="grid gap-2 rounded-lg border p-3">
      <legend className="px-1 text-sm font-medium">
        {port.name}{" "}
        <span className="font-mono text-xs font-normal text-muted-foreground">
          {port.dataType}
          {port.required && ", required"}
        </span>
      </legend>
      {edge ? (
        <p className="text-sm text-muted-foreground">
          Connected from {source?.displayName ?? edge.source.nodeId}.
          {edge.source.port}
        </p>
      ) : (
        <>
          <Label htmlFor={kindId}>Binding</Label>
          <select
            id={kindId}
            className={selectClassName}
            value={kind}
            onChange={(event) => choose(event.target.value as BindingKind)}
          >
            {Object.entries(bindingKinds).map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
          {binding?.kind === "literal" && (
            <Literal
              label="Value"
              schema={port.schema}
              value={binding.value}
              onChange={(value) => bind({ kind: "literal", value })}
            />
          )}
          {kind === "workflowInput" && (
            <>
              <Label htmlFor={pointerId}>Workflow input</Label>
              <select
                id={pointerId}
                className={selectClassName}
                value={pointer}
                onChange={(event) =>
                  bind({ kind: "workflowInput", pointer: event.target.value })
                }
              >
                <option value="" disabled>
                  Choose an input…
                </option>
                {pointers.map((option) => (
                  <option key={option}>{option}</option>
                ))}
              </select>
            </>
          )}
          {kind === "secret" && (
            <widgets.secretReference
              label="Secret id"
              schema={{}}
              value={binding?.kind === "secret" ? binding : null}
              onChange={(value) => bind(value as InputBinding | null)}
            />
          )}
        </>
      )}
    </fieldset>
  )
}

function NodeSettings({
  graph,
  node,
  definition,
  onChange,
}: {
  graph: GraphSnapshot
  node: GraphNode
  definition: NodeDefinition | undefined
  onChange: (node: GraphNode) => void
}) {
  const properties = Object.entries(definition?.configSchema.properties ?? {})
  return (
    <>
      <header className="grid gap-0.5">
        <h2 className="font-semibold">{node.displayName}</h2>
        <p className="font-mono text-xs text-muted-foreground">
          {node.definition.key} v{node.definition.version}
        </p>
      </header>
      <widgets.text
        label="Display name"
        schema={{ type: "string" }}
        value={node.displayName}
        onChange={(value) => onChange({ ...node, displayName: String(value) })}
      />
      {properties.length > 0 && (
        <section className="grid gap-3">
          <h3 className="text-xs font-medium text-muted-foreground">
            Configuration
          </h3>
          {properties.map(([name, schema]) => {
            const field = definition?.uiSchema[name]
            const Widget = widgets[widgetFor(schema, field)]
            return (
              <Widget
                key={name}
                label={field?.label ?? text(schema.title) ?? name}
                help={field?.help ?? text(schema.description)}
                schema={schema}
                value={node.config[name]}
                onChange={(value) =>
                  onChange({
                    ...node,
                    config: { ...node.config, [name]: value },
                  })
                }
              />
            )
          })}
        </section>
      )}
      {definition && definition.inputs.length > 0 && (
        <section className="grid gap-3">
          <h3 className="text-xs font-medium text-muted-foreground">Inputs</h3>
          {definition.inputs.map((port) => (
            <Binding
              key={port.name}
              graph={graph}
              node={node}
              port={port}
              onChange={onChange}
            />
          ))}
        </section>
      )}
      <section className="grid gap-3">
        <h3 className="text-xs font-medium text-muted-foreground">Execution</h3>
        <WholeNumber
          label="Max attempts"
          min={1}
          max={5}
          value={node.retry.maxAttempts}
          onChange={(maxAttempts) =>
            onChange({ ...node, retry: { maxAttempts } })
          }
        />
        <WholeNumber
          label="Timeout (seconds)"
          min={1}
          max={14_400}
          value={node.timeoutSeconds}
          onChange={(timeoutSeconds) => onChange({ ...node, timeoutSeconds })}
        />
      </section>
    </>
  )
}

export function Inspector({
  graph,
  definitions,
  nodeId,
  readOnly,
  onNodeChange,
  onInputSchemaChange,
}: {
  graph: GraphSnapshot
  definitions: NodeDefinition[]
  nodeId: string | null
  readOnly: boolean
  onNodeChange: (node: GraphNode) => void
  onInputSchemaChange: (schema: JsonSchema) => void
}) {
  const node = graph.nodes.find(({ id }) => id === nodeId)
  return (
    <aside
      aria-label="Inspector"
      className="overflow-y-auto border-t p-4 lg:border-t-0 lg:border-l"
    >
      {/* A disabled fieldset disables every control inside it. */}
      <fieldset disabled={readOnly} className="grid gap-5">
        {node ? (
          <NodeSettings
            key={node.id}
            graph={graph}
            node={node}
            definition={findDefinition(definitions, node.definition)}
            onChange={onNodeChange}
          />
        ) : (
          <>
            <h2 className="font-semibold">Workflow inputs</h2>
            <widgets.code
              label="Input schema (JSON)"
              help="Node inputs can bind to its top-level properties. Select a node to edit it."
              schema={{ type: "object" }}
              value={graph.workflowInputSchema}
              onChange={(value) => onInputSchemaChange(value as JsonSchema)}
            />
          </>
        )}
      </fieldset>
    </aside>
  )
}
