import { fireEvent, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { RunInputForm } from "@/runs/run-input-form"

it("builds the run inputs from the workflow input schema", async () => {
  const onSubmit = vi.fn()
  render(
    <RunInputForm
      schema={{
        type: "object",
        properties: {
          region: { type: "string", title: "Region" },
          size: { type: "string", enum: ["small", "large"], default: "small" },
          replicas: { type: "integer", description: "Leave empty for one." },
          dryRun: { type: "boolean" },
          tags: { type: "object" },
        },
        required: ["region", "dryRun"],
      }}
      pending={false}
      diagnostics={[
        { code: "invalid_value", pointer: "/region", message: "Too short." },
      ]}
      onSubmit={onSubmit}
    />,
  )
  expect(screen.getByText("Leave empty for one.")).toBeInTheDocument()
  await userEvent.type(screen.getByLabelText("Region"), "eu-west-1")
  await userEvent.selectOptions(screen.getByLabelText("size"), "large")
  await userEvent.type(screen.getByLabelText("replicas"), "3")
  await userEvent.clear(screen.getByLabelText("replicas"))
  await userEvent.click(screen.getByRole("button", { name: "Start run" }))

  // Optional fields left empty are omitted; required ones are always sent.
  expect(onSubmit).toHaveBeenCalledWith({
    region: "eu-west-1",
    size: "large",
    dryRun: false,
  })
  expect(screen.getByRole("alert")).toHaveTextContent("/region Too short.")
})

it("edits a schema without properties as one JSON object", async () => {
  const onSubmit = vi.fn()
  render(
    <RunInputForm
      schema={{ type: "object" }}
      pending={false}
      diagnostics={[]}
      onSubmit={onSubmit}
    />,
  )
  const json = screen.getByLabelText("Inputs (JSON)")
  expect(json).toHaveValue("{}")
  fireEvent.change(json, { target: { value: '{"payload": {"a": 1}}' } })
  await userEvent.click(screen.getByRole("button", { name: "Start run" }))
  expect(onSubmit).toHaveBeenCalledWith({ payload: { a: 1 } })
  expect(screen.queryByRole("alert")).not.toBeInTheDocument()
})

it("disables the form while the run starts", () => {
  render(
    <RunInputForm
      schema={{ type: "object", properties: { region: { type: "string" } } }}
      pending
      diagnostics={[]}
      onSubmit={vi.fn()}
    />,
  )
  expect(screen.getByLabelText("region")).toBeDisabled()
  expect(screen.getByRole("button", { name: "Starting…" })).toBeDisabled()
})
