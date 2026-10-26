"use client"

import { useState, type FormEvent } from "react"

import type { Diagnostic, JsonSchema } from "@/api/types"
import { Button } from "@/components/ui/button"
import { isRecord } from "@/studio/graph-model"
import { blankValue, widgetFor, widgets } from "@/studio/widgets"

function text(value: unknown) {
  return typeof value === "string" ? value : undefined
}

function filled(value: unknown) {
  return value !== null && value !== undefined && value !== ""
}

// One widget per top-level property of the workflow input schema; any other
// schema is edited as one JSON object. The API validates the inputs.
export function RunInputForm({
  schema,
  pending,
  diagnostics,
  onSubmit,
}: {
  schema: JsonSchema
  pending: boolean
  diagnostics: Diagnostic[]
  onSubmit: (inputs: Record<string, unknown>) => void
}) {
  const properties = Object.entries(
    isRecord(schema.properties) ? schema.properties : {},
  )
  const required = new Set(schema.required ?? [])
  // Required fields and fields with a default start filled.
  const [inputs, setInputs] = useState<Record<string, unknown>>(() =>
    Object.fromEntries(
      properties
        .filter(
          ([name, property]) =>
            required.has(name) || property.default !== undefined,
        )
        .map(([name, property]) => [name, blankValue(property)]),
    ),
  )

  // An optional field left empty is omitted rather than sent as null.
  function submit(event: FormEvent) {
    event.preventDefault()
    onSubmit(
      properties.length === 0
        ? inputs
        : Object.fromEntries(
            Object.entries(inputs).filter(
              ([name, value]) => required.has(name) || filled(value),
            ),
          ),
    )
  }

  return (
    <form onSubmit={submit} aria-label="Run inputs">
      <fieldset disabled={pending} className="grid gap-4">
        {properties.length > 0 ? (
          properties.map(([name, property]) => {
            const Widget = widgets[widgetFor(property)]
            return (
              <Widget
                key={name}
                label={text(property.title) ?? name}
                help={text(property.description)}
                schema={property}
                value={inputs[name]}
                onChange={(value) =>
                  setInputs((current) => ({ ...current, [name]: value }))
                }
              />
            )
          })
        ) : (
          <widgets.code
            label="Inputs (JSON)"
            schema={{ type: "object" }}
            value={inputs}
            onChange={(value) => setInputs(value as Record<string, unknown>)}
          />
        )}
        {diagnostics.length > 0 && (
          <ul role="alert" className="grid gap-1 text-sm text-destructive">
            {diagnostics.map((diagnostic) => (
              <li key={`${diagnostic.pointer}:${diagnostic.code}`}>
                {diagnostic.pointer && (
                  <span className="font-mono">{diagnostic.pointer} </span>
                )}
                {diagnostic.message}
              </li>
            ))}
          </ul>
        )}
        <Button type="submit" className="justify-self-start">
          {pending ? "Starting…" : "Start run"}
        </Button>
      </fieldset>
    </form>
  )
}
