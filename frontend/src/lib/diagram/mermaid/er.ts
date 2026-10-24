import type { GraphDoc, GraphNode } from "../model";
import {
  applyLayout,
  directionLine,
  isPlainComment,
  layoutLine,
  memberText,
  oneLine,
  parseDirection,
  readMeta,
  statements,
  type Meta,
} from "./text";

/** Entity relationship diagrams, with crow's-foot cardinality at each end. */

const LEFT: Record<string, string> = { one: "||", zeroOne: "|o", many: "}o", oneMany: "}|" };
const RIGHT: Record<string, string> = { one: "||", zeroOne: "o|", many: "o{", oneMany: "|{" };
const OF_LEFT = Object.fromEntries(Object.entries(LEFT).map(([k, v]) => [v, k]));
const OF_RIGHT = Object.fromEntries(Object.entries(RIGHT).map(([k, v]) => [v, k]));

const NAME = String.raw`[A-Za-z_][\w-]*`;
const RELATION = new RegExp(
  String.raw`^(${NAME})\s+(\|\||\|o|\}o|\}\|)(--|\.\.)(\|\||o\||o\{|\|\{)\s+(${NAME})\s*:\s*(?:"([^"]*)"|(.+))$`,
);

export function erToMermaid(doc: GraphDoc): string {
  const out = ["erDiagram"];
  const dir = directionLine(doc);
  if (dir) out.push(dir);
  for (const n of doc.nodes) {
    const attrs = n.attributes.map(memberText).filter(Boolean);
    if (attrs.length === 0) {
      out.push(`  ${n.id} {`, "  }");
      continue;
    }
    out.push(`  ${n.id} {`);
    for (const a of attrs) out.push(`    ${a}`);
    out.push("  }");
  }
  for (const e of doc.edges) {
    const l = LEFT[e.sourceCard] ?? LEFT.one;
    const r = RIGHT[e.targetCard] ?? RIGHT.many;
    const line = e.rel === "nonIdentifying" ? ".." : "--";
    // Mermaid requires a relationship label; quoted so it may contain spaces.
    const label = oneLine(e.label).replace(/"/g, "'") || "relates to";
    out.push(`  ${e.source} ${l}${line}${r} ${e.target} : "${label}"`);
  }
  const layout = layoutLine(doc.nodes);
  if (layout) out.push(`  ${layout}`);
  return out.join("\n");
}

/** Parse an ER diagram, or null if it uses anything the canvas cannot keep. */
export function erFromMermaid(src: string): GraphDoc | null {
  const lines = statements(src);
  if (lines[0] !== "erDiagram") return null;
  const doc: GraphDoc = { kind: "er", direction: "TB", nodes: [], edges: [] };
  const meta: Meta = {};
  const byId = new Map<string, GraphNode>();
  const touch = (id: string): GraphNode => {
    let n = byId.get(id);
    if (!n) {
      n = { id, shape: "entity", label: id, attributes: [], methods: [] };
      byId.set(id, n);
      doc.nodes.push(n);
    }
    return n;
  };

  let open: GraphNode | null = null;
  for (const line of lines.slice(1)) {
    if (readMeta(line, meta)) continue;
    if (isPlainComment(line)) return null;
    if (open) {
      if (line === "}") open = null;
      else open.attributes.push(line);
      continue;
    }
    let m: RegExpExecArray | null;
    if ((m = /^direction\s+(\w+)$/.exec(line))) {
      const d = parseDirection(m[1]);
      if (!d) return null;
      doc.direction = d;
    } else if ((m = new RegExp(String.raw`^(${NAME})\s*\{\s*(\})?$`).exec(line))) {
      const n = touch(m[1]);
      if (!m[2]) open = n;
    } else if ((m = RELATION.exec(line))) {
      const source = touch(m[1]);
      const target = touch(m[5]);
      doc.edges.push({
        id: `e${doc.edges.length + 1}`,
        source: source.id,
        target: target.id,
        rel: m[3] === ".." ? "nonIdentifying" : "identifying",
        label: (m[6] ?? m[7] ?? "").trim(),
        sourceCard: OF_LEFT[m[2]],
        targetCard: OF_RIGHT[m[4]],
      });
    } else if ((m = new RegExp(String.raw`^(${NAME})$`).exec(line))) {
      touch(m[1]);
    } else {
      return null;
    }
  }
  if (open) return null;
  return applyLayout(doc, meta.layout);
}
