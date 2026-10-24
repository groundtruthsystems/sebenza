import type { GraphDoc, GraphEdge, GraphNode } from "../model";
import { flowchartFromMermaid, flowchartToMermaid } from "./flowchart";

/**
 * Use case diagrams. Mermaid has no use case type, so these are written as a
 * flowchart whose meaning stays legible as text: actors carry an `«actor»`
 * prefix, use cases are stadiums, the system boundary is a subgraph, and
 * include/extend are labelled dotted arrows. A `%% sebenza:kind usecase`
 * comment lets the editor reopen it as a use case diagram.
 */

export const USECASE_MARKER = "usecase";
const ACTOR_PREFIX = "«actor» ";

const TO_FLOW_SHAPE: Record<string, string> = {
  actor: "process",
  usecase: "terminal",
  boundary: "subgraph",
};

export function usecaseToMermaid(doc: GraphDoc): string {
  const nodes: GraphNode[] = doc.nodes.map((n) => ({
    ...n,
    shape: TO_FLOW_SHAPE[n.shape] ?? "terminal",
    label: n.shape === "actor" ? ACTOR_PREFIX + n.label : n.label,
  }));
  const edges: GraphEdge[] = doc.edges.map((e) => {
    switch (e.rel) {
      case "include":
        return { ...e, rel: "dotted", label: "«include»" };
      case "extend":
        return { ...e, rel: "dotted", label: "«extend»" };
      case "directed":
        return { ...e, rel: "arrow" };
      default:
        return { ...e, rel: "open" };
    }
  });
  return flowchartToMermaid({ ...doc, nodes, edges }, USECASE_MARKER);
}

export function usecaseFromMermaid(src: string): GraphDoc | null {
  const parsed = flowchartFromMermaid(src);
  if (!parsed || parsed.meta.kind !== USECASE_MARKER) return null;
  const flow = parsed.doc;
  const nodes: GraphNode[] = [];
  for (const n of flow.nodes) {
    if (n.shape === "process" && n.label.startsWith(ACTOR_PREFIX)) {
      nodes.push({ ...n, shape: "actor", label: n.label.slice(ACTOR_PREFIX.length) });
    } else if (n.shape === "terminal") {
      nodes.push({ ...n, shape: "usecase" });
    } else if (n.shape === "subgraph") {
      nodes.push({ ...n, shape: "boundary" });
    } else {
      return null;
    }
  }
  const edges: GraphEdge[] = [];
  for (const e of flow.edges) {
    if (e.rel === "open") edges.push({ ...e, rel: "association" });
    else if (e.rel === "arrow") edges.push({ ...e, rel: "directed" });
    else if (e.rel === "dotted" && e.label === "«include»") edges.push({ ...e, rel: "include", label: "" });
    else if (e.rel === "dotted" && e.label === "«extend»") edges.push({ ...e, rel: "extend", label: "" });
    else return null;
  }
  return { kind: "usecase", direction: flow.direction, nodes, edges };
}
