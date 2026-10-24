import { pseudoStateId, type GraphDoc, type GraphNode } from "../model";
import {
  applyLayout,
  directionLine,
  isPlainComment,
  layoutLine,
  lineText,
  parseDirection,
  quoted,
  readMeta,
  statements,
  unLineText,
  unquote,
  type Meta,
} from "./text";

/**
 * State diagrams (`stateDiagram-v2`).
 *
 * Mermaid's `[*]` is the initial state as a transition source and the final
 * state as a target, scoped to the enclosing composite. The model gives each
 * its own node with a canonical id (`pseudoStateId`) so its position survives
 * a round trip.
 */

const PSEUDO = new Set(["start", "end"]);

export function stateToMermaid(doc: GraphDoc): string {
  const out = ["stateDiagram-v2"];
  const dir = directionLine(doc);
  if (dir) out.push(dir);
  const byId = new Map(doc.nodes.map((n) => [n.id, n]));
  const parentOf = (n: GraphNode | undefined) =>
    n?.parent && byId.has(n.parent) ? n.parent : undefined;
  const ref = (id: string) => (PSEUDO.has(byId.get(id)?.shape ?? "") ? "[*]" : id);

  // An edge is written inside the composite its pseudo-state belongs to, or
  // that both ends share; anything else crosses scopes and goes at the top.
  const scopeOf = (source: string, target: string) => {
    const s = byId.get(source);
    const t = byId.get(target);
    if (PSEUDO.has(s?.shape ?? "")) return parentOf(s);
    if (PSEUDO.has(t?.shape ?? "")) return parentOf(t);
    return parentOf(s) === parentOf(t) ? parentOf(s) : undefined;
  };

  const emit = (scope: string | undefined, indent: string) => {
    for (const n of doc.nodes) {
      if (parentOf(n) !== scope || PSEUDO.has(n.shape)) continue;
      if (n.shape === "choice") {
        out.push(`${indent}state ${n.id} <<choice>>`);
      } else {
        out.push(`${indent}state ${quoted(n.label)} as ${n.id}`);
      }
      if (n.shape === "composite") {
        out.push(`${indent}state ${n.id} {`);
        emit(n.id, indent + "  ");
        out.push(`${indent}}`);
      }
    }
    for (const e of doc.edges) {
      if (scopeOf(e.source, e.target) !== scope) continue;
      const label = e.label.trim() ? ` : ${lineText(e.label)}` : "";
      out.push(`${indent}${ref(e.source)} --> ${ref(e.target)}${label}`);
    }
  };
  emit(undefined, "  ");
  const layout = layoutLine(doc.nodes);
  if (layout) out.push(`  ${layout}`);
  return out.join("\n");
}

const ID = String.raw`[A-Za-z_][\w]*`;
const REF = String.raw`(\[\*\]|${ID})`;

/** Parse a state diagram, or null if it uses anything the canvas cannot keep. */
export function stateFromMermaid(src: string): GraphDoc | null {
  const lines = statements(src);
  if (lines[0] !== "stateDiagram-v2" && lines[0] !== "stateDiagram") return null;
  const doc: GraphDoc = { kind: "state", direction: "TB", nodes: [], edges: [] };
  const meta: Meta = {};
  const byId = new Map<string, GraphNode>();
  const scope: string[] = [];
  const current = () => scope[scope.length - 1];

  const touch = (id: string, shape = "state"): GraphNode => {
    let n = byId.get(id);
    if (!n) {
      n = { id, shape, label: id, attributes: [], methods: [], parent: current() };
      byId.set(id, n);
      doc.nodes.push(n);
    }
    return n;
  };
  const pseudo = (shape: "start" | "end") => touch(pseudoStateId(shape, current()), shape);

  for (const line of lines.slice(1)) {
    if (readMeta(line, meta)) continue;
    if (isPlainComment(line)) return null;
    let m: RegExpExecArray | null;
    if ((m = /^direction\s+(\w+)$/.exec(line))) {
      const d = parseDirection(m[1]);
      if (!d || scope.length) return null;
      doc.direction = d;
    } else if ((m = new RegExp(String.raw`^state\s+("[^"]*")\s+as\s+(${ID})(\s*\{)?$`).exec(line))) {
      const n = touch(m[2]);
      n.label = unquote(m[1]);
      if (m[3]) {
        n.shape = "composite";
        scope.push(n.id);
      }
    } else if ((m = new RegExp(String.raw`^state\s+(${ID})\s*<<choice>>$`).exec(line))) {
      touch(m[1]).shape = "choice";
    } else if ((m = new RegExp(String.raw`^state\s+(${ID})(\s*\{)?$`).exec(line))) {
      const n = touch(m[1]);
      if (m[2]) {
        n.shape = "composite";
        scope.push(n.id);
      }
    } else if (line === "}") {
      if (!scope.pop()) return null;
    } else if ((m = new RegExp(String.raw`^${REF}\s*-->\s*${REF}\s*(?::\s*(.*))?$`).exec(line))) {
      const s = m[1] === "[*]" ? pseudo("start") : touch(m[1]);
      const t = m[2] === "[*]" ? pseudo("end") : touch(m[2]);
      doc.edges.push({
        id: `e${doc.edges.length + 1}`,
        source: s.id,
        target: t.id,
        rel: "transition",
        label: m[3] ? unLineText(m[3]) : "",
        sourceCard: "",
        targetCard: "",
      });
    } else if ((m = new RegExp(String.raw`^(${ID})\s*:\s*(.+)$`).exec(line))) {
      touch(m[1]).label = unLineText(m[2]);
    } else if ((m = new RegExp(String.raw`^(${ID})$`).exec(line))) {
      touch(m[1]);
    } else {
      return null;
    }
  }
  if (scope.length) return null;
  return applyLayout(doc, meta.layout);
}
