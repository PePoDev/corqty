import type { Diagnostic, GraphSnapshot } from "@/api/types"

// A diagnostic placed on the canvas. `port` is a handle id: `in:<port>` or
// `out:<port>`.
export type MappedDiagnostic = Diagnostic & {
  nodeId?: string
  port?: string
  edgeId?: string
}

// RFC 6901 escapes `~` as `~0` and `/` as `~1`.
function unescape(token: string) {
  return token.replaceAll("~1", "/").replaceAll("~0", "~")
}

// Pointers index into the graph that was sent, so they resolve against it:
// the canvas may have changed since. Problems with the graph as a whole stay
// unplaced.
export function mapDiagnostics(
  graph: GraphSnapshot,
  diagnostics: Diagnostic[],
): MappedDiagnostic[] {
  return diagnostics.map((diagnostic) => {
    const [, list, index, field, name] = diagnostic.pointer.split("/")
    if (list === "nodes") {
      const node = graph.nodes[Number(index)]
      if (!node) return diagnostic
      return field === "inputs" && name !== undefined
        ? { ...diagnostic, nodeId: node.id, port: `in:${unescape(name)}` }
        : { ...diagnostic, nodeId: node.id }
    }
    if (list === "edges") {
      const edge = graph.edges[Number(index)]
      if (!edge) return diagnostic
      if (field === "id") return { ...diagnostic, edgeId: edge.id }
      // An edge problem shows on the end it names, else on the input it feeds.
      const [end, port] =
        field === "source"
          ? [edge.source, `out:${edge.source.port}`]
          : [edge.target, `in:${edge.target.port}`]
      return { ...diagnostic, edgeId: edge.id, nodeId: end.nodeId, port }
    }
    return diagnostic
  })
}
