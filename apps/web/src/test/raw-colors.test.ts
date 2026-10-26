import { readdirSync, readFileSync } from "node:fs"
import { join, resolve } from "node:path"
import { expect, it } from "vitest"

const src = resolve(import.meta.dirname, "..")
// D17: status badges keep fixed hues and shadcn primitives ship their own;
// everything else uses the semantic tokens from globals.css.
const exempt = ["components/status/", "components/ui/"]
const rawColor =
  /\b(?:bg|text|border|ring|fill|stroke)-(?:red|green|amber|yellow|blue|emerald|orange|rose)-\d{2,3}\b|#[\da-f]{3,8}\b|\b(?:rgba?|hsla?|oklch)\(/gi

function rawColors(path: string) {
  return readFileSync(join(src, path), "utf8").match(rawColor) ?? []
}

it("only status components use raw palette colors", () => {
  const offenders = readdirSync(src, { recursive: true, encoding: "utf8" })
    .map((path) => path.replaceAll("\\", "/"))
    .filter(
      (path) =>
        /\.tsx?$/.test(path) &&
        !exempt.some((folder) => path.startsWith(folder)),
    )
    .flatMap((path) => rawColors(path).map((color) => `${path}: ${color}`))
  expect(offenders).toEqual([])
  // The pattern does catch raw colors where they are allowed.
  expect(rawColors("components/status/status-badge.tsx")).not.toEqual([])
})
