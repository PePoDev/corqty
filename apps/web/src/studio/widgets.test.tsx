import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useState } from "react"
import { expect, it, vi } from "vitest"

import type { JsonSchema } from "@/api/types"
import { widgetFor, widgets, type WidgetName } from "@/studio/widgets"

const secretId = "0190f5c2-7d1e-7a00-8000-0000000000aa"

// Holds the value like the inspector does and records every write.
function renderWidget(name: WidgetName, schema: JsonSchema, initial?: unknown) {
  const onChange = vi.fn()
  const Widget = widgets[name]
  function Harness() {
    const [value, setValue] = useState(initial)
    return (
      <Widget
        label="Field"
        schema={schema}
        value={value}
        onChange={(next) => {
          setValue(next)
          onChange(next)
        }}
      />
    )
  }
  render(<Harness />)
  return onChange
}

const field = () => screen.getByLabelText("Field")

it("registers exactly the catalog's ten widgets", () => {
  expect(Object.keys(widgets)).toEqual([
    "text",
    "textarea",
    "number",
    "boolean",
    "select",
    "multiSelect",
    "secretReference",
    "code",
    "keyValue",
    "fileText",
  ])
})

it.each<{
  widget: WidgetName
  schema: JsonSchema
  initial?: unknown
  edit: () => Promise<void>
  expected: unknown
}>([
  {
    widget: "text",
    schema: { type: "string" },
    edit: () => userEvent.type(field(), "us-east-1"),
    expected: "us-east-1",
  },
  {
    widget: "textarea",
    schema: { type: "string" },
    edit: () => userEvent.type(field(), "one{Enter}two"),
    expected: "one\ntwo",
  },
  {
    widget: "number",
    schema: { type: "integer" },
    edit: () => userEvent.type(field(), "42"),
    expected: 42,
  },
  {
    widget: "boolean",
    schema: { type: "boolean" },
    initial: false,
    edit: () => userEvent.click(field()),
    expected: true,
  },
  {
    widget: "select",
    schema: { type: "string", enum: ["fixed", "echo"] },
    initial: "fixed",
    edit: () => userEvent.selectOptions(field(), "echo"),
    expected: "echo",
  },
  {
    widget: "multiSelect",
    schema: { type: "array", items: { type: "string", enum: ["a", "b", "c"] } },
    edit: async () => {
      expect(screen.getByRole("group", { name: "Field" })).toBeInTheDocument()
      await userEvent.click(screen.getByLabelText("c"))
      await userEvent.click(screen.getByLabelText("a"))
    },
    expected: ["a", "c"],
  },
  {
    widget: "secretReference",
    schema: { type: ["object", "null"] },
    initial: null,
    edit: async () => {
      await userEvent.click(field())
      await userEvent.paste(secretId)
    },
    expected: { kind: "secret", secretId },
  },
  {
    widget: "code",
    schema: { type: "object" },
    initial: {},
    edit: async () => {
      await userEvent.clear(field())
      await userEvent.paste('{"ok": true}')
    },
    expected: { ok: true },
  },
  {
    widget: "keyValue",
    schema: { type: "object" },
    edit: async () => {
      expect(screen.getByRole("group", { name: "Field" })).toBeInTheDocument()
      await userEvent.click(screen.getByRole("button", { name: "Add entry" }))
      await userEvent.type(screen.getByLabelText("Key 1"), "region")
      await userEvent.type(screen.getByLabelText("Value 1"), "us-east-1")
    },
    expected: { region: "us-east-1" },
  },
  {
    widget: "fileText",
    schema: { type: "string" },
    edit: () =>
      userEvent.upload(
        screen.getByLabelText("Import Field from a file"),
        new File(["#!/bin/sh\necho ok\n"], "run.sh"),
      ),
    expected: "#!/bin/sh\necho ok\n",
  },
])(
  "$widget writes its value contract",
  async ({ widget, schema, initial, edit, expected }) => {
    const onChange = renderWidget(widget, schema, initial)
    await edit()
    await waitFor(() => expect(onChange).toHaveBeenLastCalledWith(expected))
  },
)

it("keyValue removes an entry", async () => {
  const onChange = renderWidget(
    "keyValue",
    { type: "object" },
    { region: "us-east-1", tier: "gold" },
  )
  expect(screen.getByLabelText("Key 2")).toHaveValue("tier")
  await userEvent.click(screen.getByRole("button", { name: "Remove entry 1" }))
  expect(onChange).toHaveBeenLastCalledWith({ tier: "gold" })
})

it("code keeps the last valid value and explains why", async () => {
  const onChange = renderWidget("code", { type: "object" }, { ok: true })
  expect(field()).toHaveValue('{\n  "ok": true\n}')
  await userEvent.type(field(), "{{")
  expect(screen.getByRole("alert")).toHaveTextContent(
    "This is not valid JSON for this field. The last valid value is kept.",
  )
  // An object field accepts only a JSON object.
  await userEvent.clear(field())
  await userEvent.paste("[1]")
  expect(screen.getByRole("alert")).toBeInTheDocument()
  expect(onChange).not.toHaveBeenCalled()
})

it("fileText rejects a file over 262,144 bytes", async () => {
  const onChange = renderWidget("fileText", { type: "string" }, "kept")
  const importer = screen.getByLabelText("Import Field from a file")
  await userEvent.upload(importer, new File(["x".repeat(262_145)], "big.txt"))
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Choose a file of at most 262,144 bytes.",
  )
  expect(onChange).not.toHaveBeenCalled()
  expect(field()).toHaveValue("kept")

  const largest = "x".repeat(262_144)
  await userEvent.upload(importer, new File([largest], "largest.txt"))
  await waitFor(() => expect(onChange).toHaveBeenCalledOnce())
  expect(onChange.mock.lastCall?.[0]).toHaveLength(262_144)
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
})

it("widgetFor uses the uiSchema widget or the default for the schema", () => {
  expect(widgetFor({ type: "string" }, { widget: "textarea" })).toBe("textarea")
  const defaults: [JsonSchema, WidgetName][] = [
    [{ type: "string", enum: ["fixed", "echo"] }, "select"],
    [{ type: "string" }, "text"],
    [{ type: "integer" }, "number"],
    [{ type: "number" }, "number"],
    [{ type: "boolean" }, "boolean"],
    [{ type: "array", items: { type: "string", enum: ["a"] } }, "multiSelect"],
    [{ type: "array", items: { type: "string" } }, "code"],
    [{ type: "object" }, "code"],
    [{ type: ["string", "null"] }, "code"],
    [{}, "code"],
  ]
  for (const [schema, widget] of defaults) {
    expect(widgetFor(schema)).toBe(widget)
  }
})
