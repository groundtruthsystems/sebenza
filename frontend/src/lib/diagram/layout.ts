import dagre from "@dagrejs/dagre";
import { isGroup, UNLABELLED_SHAPES, type GraphDoc, type GraphNode } from "./model";

/** Room around a group's children, with extra at the top for its title. */
const PAD = 24;
const PAD_TOP = 36;

export interface Size {
  width: number;
  height: number;
}

/**
 * A node's drawn size before React Flow has measured it, close enough for a
 * first layout. Measured sizes, when the canvas has them, are passed instead.
 */
export function estimateSize(doc: GraphDoc, n: GraphNode): Size {
  if (UNLABELLED_SHAPES.has(n.shape)) return { width: 28, height: 28 };
  if (n.size) return { width: n.size.w, height: n.size.h };
  const lines = [n.label, ...n.attributes, ...n.methods];
  const widest = Math.max(...lines.map((l) => l.length), 4);
  const width = Math.min(320, Math.max(120, widest * 7.5 + 32));
  if (doc.kind === "class" || doc.kind === "er") {
    const rows = n.attributes.length + n.methods.length;
    return { width, height: 36 + Math.max(rows, 1) * 18 + (doc.kind === "class" ? 16 : 0) };
  }
  if (n.shape === "actor") return { width: Math.max(80, n.label.length * 7.5), height: 84 };
  if (n.shape === "decision") return { width: width + 30, height: 80 };
  return { width, height: 48 };
}

/**
 * Lay the whole diagram out with dagre and return it with fresh positions.
 * Groups become dagre clusters, so their children are placed inside them and
 * their size follows.
 */
export function autoLayout(doc: GraphDoc, measured: Map<string, Size> = new Map()): GraphDoc {
  const g = new dagre.graphlib.Graph({ compound: true, multigraph: true });
  g.setGraph({ rankdir: doc.direction, nodesep: 50, ranksep: 70, marginx: 20, marginy: 20 });
  g.setDefaultEdgeLabel(() => ({}));

  const hasChildren = new Set(doc.nodes.map((n) => n.parent).filter(Boolean) as string[]);
  const cluster = (n: GraphNode) => isGroup(doc, n) && hasChildren.has(n.id);
  for (const n of doc.nodes) {
    if (cluster(n)) {
      g.setNode(n.id, {});
    } else {
      const size = measured.get(n.id) ?? estimateSize(doc, n);
      g.setNode(n.id, { width: size.width, height: size.height });
    }
  }
  for (const n of doc.nodes) {
    if (n.parent && g.hasNode(n.parent)) g.setParent(n.id, n.parent);
  }
  // Dagre cannot route an edge that ends on a cluster; such edges only
  // affect drawing, not placement, so they are left out of the layout.
  const clusters = new Set(doc.nodes.filter(cluster).map((n) => n.id));
  for (const e of doc.edges) {
    if (clusters.has(e.source) || clusters.has(e.target) || e.source === e.target) continue;
    if (!g.hasNode(e.source) || !g.hasNode(e.target)) continue;
    g.setEdge(e.source, e.target, { minlen: 1 }, e.id);
  }
  dagre.layout(g);

  // Absolute top-left boxes, groups padded so their title has room.
  const box = new Map<string, { x: number; y: number; w: number; h: number }>();
  for (const n of doc.nodes) {
    const l = g.node(n.id);
    if (!l || l.x === undefined) continue;
    let b = { x: l.x - l.width / 2, y: l.y - l.height / 2, w: l.width, h: l.height };
    if (clusters.has(n.id)) b = { x: b.x - PAD, y: b.y - PAD_TOP, w: b.w + PAD * 2, h: b.h + PAD + PAD_TOP };
    box.set(n.id, b);
  }

  const nodes = doc.nodes.map((n) => {
    const b = box.get(n.id);
    if (!b) return n;
    const p = n.parent ? box.get(n.parent) : undefined;
    const next: GraphNode = { ...n, pos: { x: Math.round(b.x - (p?.x ?? 0)), y: Math.round(b.y - (p?.y ?? 0)) } };
    if (clusters.has(n.id)) next.size = { w: Math.round(b.w), h: Math.round(b.h) };
    else if (isGroup(doc, n) && !n.size) next.size = { w: 260, h: 160 };
    return next;
  });
  return { ...doc, nodes };
}

/** Whether any node still needs a position. */
export function needsLayout(doc: GraphDoc): boolean {
  return doc.nodes.some((n) => !n.pos);
}

/**
 * Give unplaced nodes a position. A diagram with no positions at all (fresh
 * from hand-written source) is laid out whole; otherwise the newcomers line up
 * below what is already placed, so typing one node into the source does not
 * rearrange the rest.
 */
export function placeMissing(doc: GraphDoc): GraphDoc {
  if (!needsLayout(doc)) return doc;
  if (doc.nodes.every((n) => !n.pos)) return autoLayout(doc);
  const placed = doc.nodes.filter((n) => n.pos && !n.parent);
  const bottom = Math.max(...placed.map((n) => n.pos!.y + estimateSize(doc, n).height), 0);
  let x = Math.min(...placed.map((n) => n.pos!.x), 0);
  return {
    ...doc,
    nodes: doc.nodes.map((n) => {
      if (n.pos) return n;
      const pos = n.parent ? { x: 24, y: 44 } : { x, y: bottom + 60 };
      if (!n.parent) x += estimateSize(doc, n).width + 40;
      return { ...n, pos };
    }),
  };
}
