import {
  descendantsOf,
  KIND_SPECS,
  pseudoStateId,
  toIdentifier,
  uniqueId,
  type GraphDoc,
  type GraphEdge,
  type GraphNode,
  type Point,
} from "./model";

/**
 * Edits to a graph diagram, as pure functions from one doc to the next. The
 * canvas calls these; keeping them out of the components is what lets the
 * rules (re-keying on rename, one `[*]` per scope) be tested directly.
 */

const DEFAULT_LABEL: Record<string, string> = {
  class: "Class",
  interface: "Interface",
  abstract: "AbstractClass",
  enum: "Enumeration",
  entity: "ENTITY",
  process: "Step",
  rounded: "Step",
  decision: "Decision?",
  terminal: "Start",
  io: "Input",
  database: "Database",
  circle: "Point",
  subgraph: "Group",
  state: "State",
  composite: "Composite",
  choice: "",
  start: "",
  end: "",
  actor: "Actor",
  usecase: "Use case",
  boundary: "System",
};

const ID_PREFIX: Record<string, string> = { flowchart: "n", state: "s", usecase: "u" };

/** Add a node; returns the doc and the new node's id. */
export function addNode(
  doc: GraphDoc,
  shape: string,
  pos: Point,
  parent?: string,
): { doc: GraphDoc; id: string } {
  const taken = doc.nodes.map((n) => n.id);
  const spec = KIND_SPECS[doc.kind];
  let id: string;
  if (shape === "start" || shape === "end") {
    // A state scope has exactly one [*] of each role.
    id = pseudoStateId(shape, parent);
    if (taken.includes(id)) return { doc, id };
  } else if (spec.namedById) {
    id = uniqueId(DEFAULT_LABEL[shape] ?? "Node", taken);
  } else {
    id = uniqueId(`${ID_PREFIX[doc.kind] ?? "n"}${doc.nodes.length + 1}`, taken);
  }
  const node: GraphNode = {
    id,
    shape,
    label: spec.namedById ? id : DEFAULT_LABEL[shape] ?? "",
    attributes: [],
    methods: [],
    pos,
  };
  if (parent) node.parent = parent;
  if (shape === spec.groupShape) node.size = { w: 280, h: 180 };
  if (doc.kind === "er") node.attributes = ["int id PK"];
  return { doc: { ...doc, nodes: [...doc.nodes, node] }, id };
}

/** Change an id everywhere it is referenced. */
function rekey(doc: GraphDoc, from: string, to: string): GraphDoc {
  if (from === to) return doc;
  return {
    ...doc,
    nodes: doc.nodes.map((n) => ({
      ...n,
      id: n.id === from ? to : n.id,
      parent: n.parent === from ? to : n.parent,
    })),
    edges: doc.edges.map((e) => ({
      ...e,
      source: e.source === from ? to : e.source,
      target: e.target === from ? to : e.target,
    })),
  };
}

/**
 * Update a node. For kinds where Mermaid uses the name as the identifier
 * (classes, entities), a rename re-keys the node; the returned id is its id
 * afterwards so the caller can keep it selected.
 */
export function updateNode(
  doc: GraphDoc,
  id: string,
  patch: Partial<Omit<GraphNode, "id">>,
): { doc: GraphDoc; id: string } {
  let next: GraphDoc = {
    ...doc,
    nodes: doc.nodes.map((n) => (n.id === id ? { ...n, ...patch } : n)),
  };
  let nextId = id;
  if (KIND_SPECS[doc.kind].namedById && patch.label !== undefined) {
    const others = doc.nodes.filter((n) => n.id !== id).map((n) => n.id);
    nextId = uniqueId(toIdentifier(patch.label), others);
    next = rekey(next, id, nextId);
    next = { ...next, nodes: next.nodes.map((n) => (n.id === nextId ? { ...n, label: nextId } : n)) };
  }
  return { doc: next, id: nextId };
}

/**
 * Delete nodes and edges. A deleted group's children move up to its parent
 * and keep their place on screen.
 */
export function deleteItems(doc: GraphDoc, nodeIds: Iterable<string>, edgeIds: Iterable<string> = []): GraphDoc {
  const gone = new Set(nodeIds);
  const goneEdges = new Set(edgeIds);
  const byId = new Map(doc.nodes.map((n) => [n.id, n]));
  const nodes = doc.nodes
    .filter((n) => !gone.has(n.id))
    .map((n) => {
      let parent = n.parent;
      let pos = n.pos;
      while (parent && gone.has(parent)) {
        const p = byId.get(parent);
        if (pos && p?.pos) pos = { x: pos.x + p.pos.x, y: pos.y + p.pos.y };
        parent = p?.parent;
      }
      return parent === n.parent ? n : { ...n, parent, pos };
    });
  const edges = doc.edges.filter((e) => !goneEdges.has(e.id) && !gone.has(e.source) && !gone.has(e.target));
  return { ...doc, nodes, edges };
}

export function addEdge(doc: GraphDoc, source: string, target: string): { doc: GraphDoc; id: string } {
  const taken = new Set(doc.edges.map((e) => e.id));
  let i = doc.edges.length + 1;
  while (taken.has(`e${i}`)) i++;
  const spec = KIND_SPECS[doc.kind];
  const edge: GraphEdge = {
    id: `e${i}`,
    source,
    target,
    rel: spec.defaultRel,
    label: "",
    sourceCard: spec.cards === "er" ? "one" : "",
    targetCard: spec.cards === "er" ? "many" : "",
  };
  if (doc.kind === "er") edge.label = "has";
  return { doc: { ...doc, edges: [...doc.edges, edge] }, id: edge.id };
}

export function updateEdge(doc: GraphDoc, id: string, patch: Partial<Omit<GraphEdge, "id">>): GraphDoc {
  return { ...doc, edges: doc.edges.map((e) => (e.id === id ? { ...e, ...patch } : e)) };
}

export function reverseEdge(doc: GraphDoc, id: string): GraphDoc {
  return {
    ...doc,
    edges: doc.edges.map((e) =>
      e.id === id
        ? { ...e, source: e.target, target: e.source, sourceCard: e.targetCard, targetCard: e.sourceCard }
        : e,
    ),
  };
}

/** Positions of every node in canvas coordinates, following parent offsets. */
export function absolutePositions(doc: GraphDoc): Map<string, Point> {
  const byId = new Map(doc.nodes.map((n) => [n.id, n]));
  const out = new Map<string, Point>();
  const resolve = (n: GraphNode, depth = 0): Point => {
    const known = out.get(n.id);
    if (known) return known;
    const own = n.pos ?? { x: 0, y: 0 };
    const p = n.parent ? byId.get(n.parent) : undefined;
    const abs = p && depth < doc.nodes.length ? add(resolve(p, depth + 1), own) : own;
    out.set(n.id, abs);
    return abs;
  };
  doc.nodes.forEach((n) => resolve(n));
  return out;
}

function add(a: Point, b: Point): Point {
  return { x: a.x + b.x, y: a.y + b.y };
}

/**
 * Move a node into a group (or out to the top level) without it jumping on
 * screen. Refuses to nest a group inside itself. A state's `[*]` is re-keyed
 * to its new scope's canonical id.
 */
export function setParent(doc: GraphDoc, id: string, parent: string | undefined): { doc: GraphDoc; id: string } {
  const node = doc.nodes.find((n) => n.id === id);
  if (!node || node.parent === parent) return { doc, id };
  if (parent && descendantsOf(doc, id).has(parent)) return { doc, id };
  const abs = absolutePositions(doc);
  const mine = abs.get(id) ?? { x: 0, y: 0 };
  const base = parent ? abs.get(parent) ?? { x: 0, y: 0 } : { x: 0, y: 0 };
  let next: GraphDoc = {
    ...doc,
    nodes: doc.nodes.map((n) =>
      n.id === id ? { ...n, parent, pos: { x: mine.x - base.x, y: mine.y - base.y } } : n,
    ),
  };
  let nextId = id;
  if (node.shape === "start" || node.shape === "end") {
    nextId = pseudoStateId(node.shape, parent);
    if (next.nodes.some((n) => n.id === nextId && n !== node)) return { doc, id };
    next = rekey(next, id, nextId);
  }
  return { doc: next, id: nextId };
}

/** Move nodes to new positions (already relative to their parents). */
export function moveNodes(doc: GraphDoc, moves: Map<string, Point>): GraphDoc {
  if (moves.size === 0) return doc;
  return {
    ...doc,
    nodes: doc.nodes.map((n) => {
      const p = moves.get(n.id);
      return p ? { ...n, pos: { x: Math.round(p.x), y: Math.round(p.y) } } : n;
    }),
  };
}

export function resizeNode(doc: GraphDoc, id: string, pos: Point, w: number, h: number): GraphDoc {
  return {
    ...doc,
    nodes: doc.nodes.map((n) =>
      n.id === id ? { ...n, pos: { x: Math.round(pos.x), y: Math.round(pos.y) }, size: { w: Math.round(w), h: Math.round(h) } } : n,
    ),
  };
}
