import type { GraphDoc, GraphEdge, GraphNode } from "../model";
import {
  applyLayout,
  directionLine,
  isPlainComment,
  layoutLine,
  lineText,
  memberText,
  parseDirection,
  readMeta,
  statements,
  unLineText,
  type Meta,
} from "./text";

/**
 * Class diagrams.
 *
 * Edges point from the dependent end to the end that carries the UML marker:
 * child -> parent for inheritance, part -> whole for composition. Mermaid puts
 * those markers on the left (`Parent <|-- Child`), so they are written
 * target-first; plain arrows are written source-first.
 */

const STEREOTYPE: Record<string, string> = {
  interface: "interface",
  abstract: "abstract",
  enum: "enumeration",
};
const SHAPE_OF_STEREOTYPE: Record<string, string> = {
  interface: "interface",
  abstract: "abstract",
  enumeration: "enum",
  enum: "enum",
};

/** rel -> [token, written target-first?] */
const REL_TOKEN: Record<string, [string, boolean]> = {
  inheritance: ["<|--", true],
  realization: ["<|..", true],
  composition: ["*--", true],
  aggregation: ["o--", true],
  association: ["-->", false],
  dependency: ["..>", false],
  link: ["--", false],
  linkDashed: ["..", false],
};

const NAME = String.raw`[A-Za-z_][\w]*`;
const RELATION = new RegExp(
  String.raw`^(${NAME})\s*(?:"([^"]*)"\s*)?(<\||\*|o|<)?(--|\.\.)(\|>|\*|o|>)?\s*(?:"([^"]*)"\s*)?(${NAME})\s*(?::\s*(.*))?$`,
);

export function classToMermaid(doc: GraphDoc): string {
  const out = ["classDiagram"];
  const dir = directionLine(doc);
  if (dir) out.push(dir);
  for (const n of doc.nodes) {
    const stereo = STEREOTYPE[n.shape];
    const members = [...n.attributes, ...n.methods].map(memberText).filter(Boolean);
    if (!stereo && members.length === 0) {
      out.push(`  class ${n.id}`);
      continue;
    }
    out.push(`  class ${n.id} {`);
    if (stereo) out.push(`    <<${stereo}>>`);
    for (const m of members) out.push(`    ${m}`);
    out.push("  }");
  }
  for (const e of doc.edges) {
    const [token, targetFirst] = REL_TOKEN[e.rel] ?? REL_TOKEN.association;
    const card = (c: string) => (c.trim() ? ` "${c.replace(/"/g, "'").trim()}"` : "");
    const label = e.label.trim() ? ` : ${lineText(e.label)}` : "";
    out.push(
      targetFirst
        ? `  ${e.target}${card(e.targetCard)} ${token}${card(e.sourceCard)} ${e.source}${label}`
        : `  ${e.source}${card(e.sourceCard)} ${token}${card(e.targetCard)} ${e.target}${label}`,
    );
  }
  const layout = layoutLine(doc.nodes);
  if (layout) out.push(`  ${layout}`);
  return out.join("\n");
}

/** Parse a class diagram, or null if it uses anything the canvas cannot keep. */
export function classFromMermaid(src: string): GraphDoc | null {
  const lines = statements(src);
  if (lines[0] !== "classDiagram") return null;
  const doc: GraphDoc = { kind: "class", direction: "TB", nodes: [], edges: [] };
  const meta: Meta = {};
  const byId = new Map<string, GraphNode>();
  const node = (id: string): GraphNode => {
    let n = byId.get(id);
    if (!n) {
      n = { id, shape: "class", label: id, attributes: [], methods: [] };
      byId.set(id, n);
      doc.nodes.push(n);
    }
    return n;
  };
  const addMember = (n: GraphNode, raw: string) => {
    const m = raw.trim();
    const stereo = /^<<\s*(\w+)\s*>>$/.exec(m);
    if (stereo) {
      const shape = SHAPE_OF_STEREOTYPE[stereo[1].toLowerCase()];
      if (!shape) return false;
      n.shape = shape;
      return true;
    }
    (m.includes("(") ? n.methods : n.attributes).push(m);
    return true;
  };

  let open: GraphNode | null = null;
  for (const line of lines.slice(1)) {
    if (readMeta(line, meta)) continue;
    if (isPlainComment(line)) return null;
    if (open) {
      if (line === "}") {
        open = null;
      } else if (!addMember(open, line)) {
        return null;
      }
      continue;
    }
    let m: RegExpExecArray | null;
    if ((m = /^direction\s+(\w+)$/.exec(line))) {
      const d = parseDirection(m[1]);
      if (!d) return null;
      doc.direction = d;
    } else if ((m = new RegExp(String.raw`^class\s+(${NAME})\s*(\{)?\s*(\})?$`).exec(line))) {
      const n = node(m[1]);
      if (m[2] && !m[3]) open = n;
    } else if ((m = new RegExp(String.raw`^<<\s*(\w+)\s*>>\s*(${NAME})$`).exec(line))) {
      if (!addMember(node(m[2]), `<<${m[1]}>>`)) return null;
    } else if ((m = new RegExp(String.raw`^(${NAME})\s*:\s*(.+)$`).exec(line)) && !RELATION.test(line)) {
      if (!addMember(node(m[1]), m[2])) return null;
    } else if ((m = RELATION.exec(line))) {
      const edge = relation(m);
      if (!edge) return null;
      node(edge.source);
      node(edge.target);
      doc.edges.push({ ...edge, id: `e${doc.edges.length + 1}` });
    } else {
      return null;
    }
  }
  if (open) return null;
  return applyLayout(doc, meta.layout);
}

function relation(m: RegExpExecArray): Omit<GraphEdge, "id"> | null {
  const [, left, leftCard = "", lmark = "", line, rmark = "", rightCard = "", right, label = ""] = m;
  const dashed = line === "..";
  const text = unLineText(label);
  // [rel, source is the right-hand name?]
  let rel: string;
  let sourceIsRight: boolean;
  if (lmark && rmark) return null; // two-way markers: no single UML reading
  if (lmark === "<|") [rel, sourceIsRight] = [dashed ? "realization" : "inheritance", true];
  else if (rmark === "|>") [rel, sourceIsRight] = [dashed ? "realization" : "inheritance", false];
  else if (lmark === "*") [rel, sourceIsRight] = ["composition", true];
  else if (rmark === "*") [rel, sourceIsRight] = ["composition", false];
  else if (lmark === "o") [rel, sourceIsRight] = ["aggregation", true];
  else if (rmark === "o") [rel, sourceIsRight] = ["aggregation", false];
  else if (lmark === "<") [rel, sourceIsRight] = [dashed ? "dependency" : "association", true];
  else if (rmark === ">") [rel, sourceIsRight] = [dashed ? "dependency" : "association", false];
  else [rel, sourceIsRight] = [dashed ? "linkDashed" : "link", false];
  return sourceIsRight
    ? { source: right, target: left, rel, label: text, sourceCard: rightCard, targetCard: leftCard }
    : { source: left, target: right, rel, label: text, sourceCard: leftCard, targetCard: rightCard };
}
