import type { GraphDoc, GraphNode } from "../model";
import {
  applyLayout,
  isPlainComment,
  layoutLine,
  parseDirection,
  quoted,
  readMeta,
  statements,
  unquote,
  type Meta,
} from "./text";

/**
 * Flowcharts, which also carry activity diagrams and - with a kind marker -
 * use case diagrams (see `usecase.ts`).
 */

const SHAPES: Record<string, [string, string]> = {
  process: ["[", "]"],
  rounded: ["(", ")"],
  decision: ["{", "}"],
  terminal: ["([", "])"],
  io: ["[/", "/]"],
  database: ["[(", ")]"],
  circle: ["((", "))"],
};
/** Openers longest-first, so `([` is not read as `(`. */
const OPENERS = Object.entries(SHAPES).sort((a, b) => b[1][0].length - a[1][0].length);

const EDGE_TOKEN: Record<string, string> = {
  arrow: "-->",
  open: "---",
  dotted: "-.->",
  dottedOpen: "-.-",
  thick: "==>",
};
const REL_OF_TOKEN = Object.fromEntries(Object.entries(EDGE_TOKEN).map(([k, v]) => [v, k]));
const EDGE_RE = /^\s*(-->|---|-\.->|-\.-|==>)(?:\|("[^"]*"|[^|]*)\|)?\s*/;

export function flowchartToMermaid(doc: GraphDoc, kindMarker?: string): string {
  const out = [`flowchart ${doc.direction}`];
  if (kindMarker) out.push(`  %% sebenza:kind ${kindMarker}`);
  // Orphans (a parent that no longer exists) are written at the top level
  // rather than silently dropped.
  const ids = new Set(doc.nodes.map((n) => n.id));
  const parentOf = (n: GraphNode) => (n.parent && ids.has(n.parent) ? n.parent : undefined);
  const children = (parent: string | undefined) => doc.nodes.filter((n) => parentOf(n) === parent);
  const emit = (parent: string | undefined, indent: string) => {
    for (const n of children(parent)) {
      if (n.shape === "subgraph") {
        out.push(`${indent}subgraph ${n.id} [${quoted(n.label)}]`);
        emit(n.id, indent + "  ");
        out.push(`${indent}end`);
      } else {
        const [open, close] = SHAPES[n.shape] ?? SHAPES.process;
        out.push(`${indent}${n.id}${open}${quoted(n.label)}${close}`);
      }
    }
  };
  emit(undefined, "  ");
  for (const e of doc.edges) {
    const token = EDGE_TOKEN[e.rel] ?? EDGE_TOKEN.arrow;
    const label = e.label.trim() ? `|${quoted(e.label)}|` : "";
    out.push(`  ${e.source} ${token}${label} ${e.target}`);
  }
  const layout = layoutLine(doc.nodes);
  if (layout) out.push(`  ${layout}`);
  return out.join("\n");
}

export interface ParsedFlowchart {
  doc: GraphDoc;
  meta: Meta;
}

/** Parse a flowchart, or null if it uses anything the canvas cannot keep. */
export function flowchartFromMermaid(src: string): ParsedFlowchart | null {
  const lines = statements(src);
  const head = /^(?:flowchart|graph)(?:\s+(\w+))?$/.exec(lines[0] ?? "");
  if (!head) return null;
  const direction = head[1] ? parseDirection(head[1]) : "TB";
  if (!direction) return null;
  const doc: GraphDoc = { kind: "flowchart", direction, nodes: [], edges: [] };
  const meta: Meta = {};
  const byId = new Map<string, GraphNode>();
  const scope: string[] = [];

  const touch = (id: string): GraphNode => {
    let n = byId.get(id);
    if (!n) {
      n = { id, shape: "process", label: id, attributes: [], methods: [] };
      byId.set(id, n);
      doc.nodes.push(n);
    }
    // Mermaid places a node in the subgraph it is mentioned in.
    const current = scope[scope.length - 1];
    if (current && !n.parent && current !== id) n.parent = current;
    return n;
  };

  for (const line of lines.slice(1)) {
    if (readMeta(line, meta)) continue;
    if (isPlainComment(line)) return null;
    let m: RegExpExecArray | null;
    if ((m = /^subgraph\s+(\w+)\s*(?:\[\s*("[^"]*"|[^\]]*)\s*\])?$/.exec(line))) {
      const n = touch(m[1]);
      n.shape = "subgraph";
      n.label = m[2] !== undefined ? unquote(m[2]) : m[1];
      scope.push(m[1]);
      continue;
    }
    if (line === "end") {
      if (!scope.pop()) return null;
      continue;
    }
    if (!statement(line, touch, doc)) return null;
  }
  if (scope.length) return null;
  return { doc: applyLayout(doc, meta.layout), meta };
}

/** `a["A"] -->|x| b{"B"} --> c` - one or more nodes joined by edges. */
function statement(
  line: string,
  touch: (id: string) => GraphNode,
  doc: GraphDoc,
): boolean {
  let rest = line;
  let prev: GraphNode | null = null;
  let pending: { rel: string; label: string } | null = null;
  while (rest) {
    const ref = nodeRef(rest);
    if (!ref) return false;
    const n = touch(ref.id);
    if (ref.shape) {
      n.shape = ref.shape;
      n.label = ref.label;
    }
    if (prev && pending) {
      doc.edges.push({
        id: `e${doc.edges.length + 1}`,
        source: prev.id,
        target: n.id,
        rel: pending.rel,
        label: pending.label,
        sourceCard: "",
        targetCard: "",
      });
    }
    rest = ref.rest;
    if (!rest) break;
    const e = EDGE_RE.exec(rest);
    if (!e) return false;
    pending = { rel: REL_OF_TOKEN[e[1]], label: e[2] !== undefined ? unquote(e[2]) : "" };
    prev = n;
    rest = rest.slice(e[0].length);
    if (!rest) return false;
  }
  return true;
}

function nodeRef(text: string): { id: string; shape?: string; label: string; rest: string } | null {
  const idm = /^(\w+)/.exec(text);
  if (!idm) return null;
  const id = idm[1];
  let rest = text.slice(id.length);
  for (const [shape, [open, close]] of OPENERS) {
    if (!rest.startsWith(open)) continue;
    const body = rest.slice(open.length);
    let label: string;
    let after: string;
    if (body.startsWith('"')) {
      const endQuote = body.indexOf('"', 1);
      if (endQuote < 0 || !body.slice(endQuote + 1).startsWith(close)) return null;
      label = unquote(body.slice(0, endQuote + 1));
      after = body.slice(endQuote + 1 + close.length);
    } else {
      const end = body.indexOf(close);
      if (end < 0) return null;
      label = body.slice(0, end).trim();
      after = body.slice(end + close.length);
    }
    return { id, shape, label, rest: after.trim() };
  }
  return { id, label: id, rest: rest.trim() };
}
