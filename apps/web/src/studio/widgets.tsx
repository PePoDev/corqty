"use client"

import { XIcon } from "lucide-react"
import { useId, useState, type ReactNode } from "react"

import type { JsonSchema, UiField } from "@/api/types"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { isRecord } from "@/studio/graph-model"

// Mirrors corqty_domain::catalog::FILE_TEXT_MAX_BYTES.
export const FILE_TEXT_MAX_BYTES = 262_144

// Native selects, styled like Input.
export const selectClassName =
  "h-8 w-full min-w-0 rounded-lg border border-input bg-transparent px-2.5 py-1 text-base outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50 md:text-sm dark:bg-input/30"

const checkboxClassName = "size-4 accent-primary"

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

export type WidgetProps = {
  label: string
  help?: string | null
  schema: JsonSchema
  value: unknown
  onChange: (value: unknown) => void
}

function stringEnum(schema: unknown): string[] | undefined {
  const values = isRecord(schema) ? schema.enum : undefined
  return Array.isArray(values) &&
    values.every((value) => typeof value === "string")
    ? values
    : undefined
}

function Help({ children }: { children: ReactNode }) {
  return <p className="text-xs text-muted-foreground">{children}</p>
}

function Problem({ children }: { children: ReactNode }) {
  return (
    <p role="alert" className="text-xs text-destructive">
      {children}
    </p>
  )
}

function Labelled({
  id,
  label,
  help,
  children,
}: {
  id: string
  label: string
  help?: string | null
  children: ReactNode
}) {
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      {children}
      {help && <Help>{help}</Help>}
    </div>
  )
}

function TextWidget({ label, help, value, onChange }: WidgetProps) {
  const id = useId()
  return (
    <Labelled id={id} label={label} help={help}>
      <Input
        id={id}
        value={typeof value === "string" ? value : ""}
        onChange={(event) => onChange(event.target.value)}
      />
    </Labelled>
  )
}

function TextareaWidget({ label, help, value, onChange }: WidgetProps) {
  const id = useId()
  return (
    <Labelled id={id} label={label} help={help}>
      <Textarea
        id={id}
        className="max-h-64"
        value={typeof value === "string" ? value : ""}
        onChange={(event) => onChange(event.target.value)}
      />
    </Labelled>
  )
}

// An empty field writes null; integer schemas get whole numbers.
function NumberWidget({ label, help, schema, value, onChange }: WidgetProps) {
  const id = useId()
  const integer = schema.type === "integer"
  return (
    <Labelled id={id} label={label} help={help}>
      <Input
        id={id}
        type="number"
        step={integer ? 1 : "any"}
        value={typeof value === "number" ? value : ""}
        onChange={({ target }) => {
          const number = Number(target.value)
          onChange(
            target.value === "" ? null : integer ? Math.trunc(number) : number,
          )
        }}
      />
    </Labelled>
  )
}

function BooleanWidget({ label, help, value, onChange }: WidgetProps) {
  const id = useId()
  return (
    <div className="grid gap-1.5">
      <div className="flex items-center gap-2">
        <input
          id={id}
          type="checkbox"
          className={checkboxClassName}
          checked={value === true}
          onChange={(event) => onChange(event.target.checked)}
        />
        <Label htmlFor={id}>{label}</Label>
      </div>
      {help && <Help>{help}</Help>}
    </div>
  )
}

function SelectWidget({ label, help, schema, value, onChange }: WidgetProps) {
  const id = useId()
  return (
    <Labelled id={id} label={label} help={help}>
      <select
        id={id}
        className={selectClassName}
        value={typeof value === "string" ? value : ""}
        onChange={(event) => onChange(event.target.value)}
      >
        <option value="" disabled>
          Choose…
        </option>
        {stringEnum(schema)?.map((option) => (
          <option key={option}>{option}</option>
        ))}
      </select>
    </Labelled>
  )
}

function MultiSelectWidget({
  label,
  help,
  schema,
  value,
  onChange,
}: WidgetProps) {
  const options = stringEnum(schema.items) ?? []
  const selected = Array.isArray(value) ? value : []
  return (
    <fieldset className="grid gap-1.5">
      <legend className="mb-1.5 text-sm font-medium">{label}</legend>
      {options.map((option) => (
        <label key={option} className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            className={checkboxClassName}
            checked={selected.includes(option)}
            onChange={(event) =>
              onChange(
                options.filter((candidate) =>
                  candidate === option
                    ? event.target.checked
                    : selected.includes(candidate),
                ),
              )
            }
          />
          {option}
        </label>
      ))}
      {help && <Help>{help}</Help>}
    </fieldset>
  )
}

// ponytail: a UUID field until secrets exist. The text is local: only this
// widget changes the value while it is shown.
function SecretReferenceWidget({ label, help, value, onChange }: WidgetProps) {
  const id = useId()
  const [text, setText] = useState(
    isRecord(value) && typeof value.secretId === "string" ? value.secretId : "",
  )
  const invalid = text !== "" && !uuid.test(text)
  return (
    <Labelled id={id} label={label} help={help}>
      <Input
        id={id}
        value={text}
        aria-invalid={invalid || undefined}
        onChange={({ target }) => {
          setText(target.value)
          if (target.value === "") onChange(null)
          else if (uuid.test(target.value)) {
            onChange({ kind: "secret", secretId: target.value })
          }
        }}
      />
      {invalid && <Problem>Enter the secret id as a UUID.</Problem>}
      <Help>Secret picker arrives with secrets.</Help>
    </Labelled>
  )
}

function json(value: unknown) {
  return value === undefined ? "" : JSON.stringify(value, null, 2)
}

// Object fields accept only a JSON object.
function parse(text: string, schema: JsonSchema) {
  try {
    const value: unknown = JSON.parse(text)
    return schema.type !== "object" || isRecord(value) ? { value } : undefined
  } catch {
    return undefined
  }
}

function CodeWidget({ label, help, schema, value, onChange }: WidgetProps) {
  const id = useId()
  const [text, setText] = useState(() => json(value))
  const [shown, setShown] = useState(value)
  // Follow changes made elsewhere, such as a blueprint adding workflow inputs.
  if (value !== shown) {
    setShown(value)
    setText(json(value))
  }
  const invalid = text !== json(value) && !parse(text, schema)
  return (
    <Labelled id={id} label={label} help={help}>
      <Textarea
        id={id}
        className="max-h-64 font-mono text-xs"
        spellCheck={false}
        value={text}
        aria-invalid={invalid || undefined}
        onChange={({ target }) => {
          setText(target.value)
          const parsed = parse(target.value, schema)
          if (parsed) {
            setShown(parsed.value)
            onChange(parsed.value)
          }
        }}
      />
      {invalid && (
        <Problem>
          This is not valid JSON for this field. The last valid value is kept.
        </Problem>
      )}
    </Labelled>
  )
}

// The rows are local: only this widget changes the value while it is shown.
function KeyValueWidget({ label, help, value, onChange }: WidgetProps) {
  const [rows, setRows] = useState(() =>
    Object.entries(isRecord(value) ? value : {}).map(
      ([key, entry]): [string, string] => [key, String(entry)],
    ),
  )
  function update(next: [string, string][]) {
    setRows(next)
    onChange(Object.fromEntries(next.filter(([key]) => key !== "")))
  }
  return (
    <fieldset className="grid gap-2">
      <legend className="mb-1.5 text-sm font-medium">{label}</legend>
      {rows.map(([key, entry], index) => (
        // Rows have no identity besides their position.
        <div key={index} className="grid grid-cols-[1fr_1fr_auto] gap-2">
          <Input
            aria-label={`Key ${index + 1}`}
            value={key}
            onChange={({ target }) =>
              update(
                rows.map((row, other) =>
                  other === index ? [target.value, entry] : row,
                ),
              )
            }
          />
          <Input
            aria-label={`Value ${index + 1}`}
            value={entry}
            onChange={({ target }) =>
              update(
                rows.map((row, other) =>
                  other === index ? [key, target.value] : row,
                ),
              )
            }
          />
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label={`Remove entry ${index + 1}`}
            onClick={() => update(rows.filter((_, other) => other !== index))}
          >
            <XIcon />
          </Button>
        </div>
      ))}
      <Button
        type="button"
        variant="outline"
        size="sm"
        className="justify-self-start"
        onClick={() => setRows([...rows, ["", ""]])}
      >
        Add entry
      </Button>
      {help && <Help>{help}</Help>}
    </fieldset>
  )
}

function FileTextWidget({ label, help, value, onChange }: WidgetProps) {
  const id = useId()
  const [tooLarge, setTooLarge] = useState(false)
  async function load(file: File | undefined) {
    if (!file) return
    setTooLarge(file.size > FILE_TEXT_MAX_BYTES)
    if (file.size <= FILE_TEXT_MAX_BYTES) onChange(await file.text())
  }
  return (
    <Labelled id={id} label={label} help={help}>
      <Textarea
        id={id}
        className="max-h-64 font-mono text-xs"
        spellCheck={false}
        value={typeof value === "string" ? value : ""}
        onChange={(event) => onChange(event.target.value)}
      />
      <Input
        type="file"
        aria-label={`Import ${label} from a file`}
        onChange={(event) => void load(event.target.files?.[0])}
      />
      {tooLarge && <Problem>Choose a file of at most 262,144 bytes.</Problem>}
    </Labelled>
  )
}

// One editor per uiSchema widget name (corqty_domain::catalog::WIDGETS), each
// writing its value contract.
export const widgets = {
  text: TextWidget,
  textarea: TextareaWidget,
  number: NumberWidget,
  boolean: BooleanWidget,
  select: SelectWidget,
  multiSelect: MultiSelectWidget,
  secretReference: SecretReferenceWidget,
  code: CodeWidget,
  keyValue: KeyValueWidget,
  fileText: FileTextWidget,
} satisfies Record<string, (props: WidgetProps) => ReactNode>

export type WidgetName = keyof typeof widgets

// The uiSchema widget, or else the default for the schema.
export function widgetFor(schema: JsonSchema, field?: UiField): WidgetName {
  if (field && Object.hasOwn(widgets, field.widget)) {
    return field.widget as WidgetName
  }
  switch (schema.type) {
    case "string":
      return stringEnum(schema) ? "select" : "text"
    case "integer":
    case "number":
      return "number"
    case "boolean":
      return "boolean"
    case "array":
      return stringEnum(schema.items) ? "multiSelect" : "code"
    default:
      return "code"
  }
}

const blanks: Partial<Record<WidgetName, unknown>> = {
  text: "",
  select: "",
  number: 0,
  boolean: false,
  multiSelect: [],
}

// A new literal starts from the schema's default or its widget's empty value.
export function blankValue(schema: JsonSchema): unknown {
  return structuredClone(schema.default ?? blanks[widgetFor(schema)] ?? null)
}
