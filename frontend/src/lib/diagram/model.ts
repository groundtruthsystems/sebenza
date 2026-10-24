/**
 * The diagram editor's in-memory model.
 *
 * Mermaid source in the draft is the stored form; these types are only what
 * the canvas edits between parsing and serializing. Every kind except sequence
 * diagrams is a graph of nodes and edges, so they share `GraphDoc` and differ
 * in the shapes and relationships `KIND_SPECS` allows.
 */

export type GraphKind = "class" | "flowchart" | "state" | "er" | "usecase";
export type DiagramKind = GraphKind | "sequence";
export type Direction = "TB" | "LR" | "BT" | "RL";

export interface Point {
  x: number;
  y: number;
}

export interface GraphNode {
  id: string;
  /** One of the kind's `shapes`. */
  shape: string;
  label: string;
  /** Class attributes, or ER entity attributes. */
  attributes: string[];
  /** Class methods. */
  methods: string[];
  /** The group (subgraph, composite state, system boundary) holding it. */
  parent?: string;
  /** Relative to the parent, as React Flow positions children. */
  pos?: Point;
  /** Groups only: their drawn size. */
  size?: { w: number; h: number };
}

export interface GraphEdge {
  id: string;
  source: string;
  target: string;
  /** One of the kind's `rels`. */
  rel: string;
  label: string;
  /** Multiplicity or ER cardinality at each end; "" when unset. */
  sourceCard: string;
  targetCard: string;
}

export interface GraphDoc {
  kind: GraphKind;
  direction: Direction;
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export type SeqArrow = "->>" | "-->>" | "-)" | "--)" | "->" | "-->" | "-x" | "--x";
export type SeqBlock = "alt" | "loop" | "opt" | "par" | "critical" | "break";

export interface SeqParticipant {
  id: string;
  label: string;
  kind: "participant" | "actor";
}

/**
 * Sequence rows are flat, the way Mermaid writes them: a block is a `start`
 * row, optional `else` rows and an `end` row. That keeps reordering, wrapping
 * and serializing all one-row-at-a-time operations.
 */
export type SeqItem =
  | { t: "msg"; from: string; to: string; arrow: SeqArrow; text: string; act?: "+" | "-" }
  | { t: "note"; pos: "over" | "left of" | "right of"; who: string[]; text: string }
  | { t: "start"; block: SeqBlock; text: string }
  | { t: "else"; text: string }
  | { t: "end" }
  | { t: "activate"; who: string; on: boolean };

export interface SequenceDoc {
  kind: "sequence";
  autonumber: boolean;
  participants: SeqParticipant[];
  items: SeqItem[];
}

export type AnyDoc = GraphDoc | SequenceDoc;

export interface Choice {
  value: string;
  label: string;
}

export interface KindSpec {
  title: string;
  blurb: string;
  shapes: Choice[];
  rels: Choice[];
  defaultShape: string;
  defaultRel: string;
  /** The shape that contains other nodes, when the kind has one. */
  groupShape?: string;
  /**
   * Mermaid uses the name itself as the identifier (classes, ER entities), so
   * the label must be an identifier and renaming re-keys the node.
   */
  namedById: boolean;
  /** How edge ends are annotated. */
  cards: "none" | "free" | "er";
  hasAttributes: boolean;
  hasMethods: boolean;
}

export const ER_CARDS: Choice[] = [
  { value: "one", label: "exactly one (1)" },
  { value: "zeroOne", label: "zero or one (0..1)" },
  { value: "many", label: "zero or more (0..*)" },
  { value: "oneMany", label: "one or more (1..*)" },
];

export const KIND_SPECS: Record<GraphKind, KindSpec> = {
  class: {
    title: "Class diagram",
    blurb: "Classes, interfaces and how they relate",
    shapes: [
      { value: "class", label: "Class" },
      { value: "interface", label: "Interface" },
      { value: "abstract", label: "Abstract class" },
      { value: "enum", label: "Enumeration" },
    ],
    rels: [
      { value: "association", label: "Association" },
      { value: "inheritance", label: "Inheritance" },
      { value: "realization", label: "Realization" },
      { value: "composition", label: "Composition" },
      { value: "aggregation", label: "Aggregation" },
      { value: "dependency", label: "Dependency" },
      { value: "link", label: "Link" },
      { value: "linkDashed", label: "Dashed link" },
    ],
    defaultShape: "class",
    defaultRel: "association",
    namedById: true,
    cards: "free",
    hasAttributes: true,
    hasMethods: true,
  },
  flowchart: {
    title: "Flowchart / activity",
    blurb: "Steps, decisions and the flow between them",
    shapes: [
      { value: "process", label: "Process" },
      { value: "rounded", label: "Rounded step" },
      { value: "decision", label: "Decision" },
      { value: "terminal", label: "Start / end" },
      { value: "io", label: "Input / output" },
      { value: "database", label: "Database" },
      { value: "circle", label: "Circle" },
      { value: "subgraph", label: "Group" },
    ],
    rels: [
      { value: "arrow", label: "Arrow" },
      { value: "open", label: "Line" },
      { value: "dotted", label: "Dotted arrow" },
      { value: "dottedOpen", label: "Dotted line" },
      { value: "thick", label: "Thick arrow" },
    ],
    defaultShape: "process",
    defaultRel: "arrow",
    groupShape: "subgraph",
    namedById: false,
    cards: "none",
    hasAttributes: false,
    hasMethods: false,
  },
  state: {
    title: "State machine",
    blurb: "States, transitions and composite states",
    shapes: [
      { value: "state", label: "State" },
      { value: "start", label: "Initial" },
      { value: "end", label: "Final" },
      { value: "choice", label: "Choice" },
      { value: "composite", label: "Composite state" },
    ],
    rels: [{ value: "transition", label: "Transition" }],
    defaultShape: "state",
    defaultRel: "transition",
    groupShape: "composite",
    namedById: false,
    cards: "none",
    hasAttributes: false,
    hasMethods: false,
  },
  er: {
    title: "Entity relationship",
    blurb: "Entities, attributes and cardinality",
    shapes: [{ value: "entity", label: "Entity" }],
    rels: [
      { value: "identifying", label: "Identifying" },
      { value: "nonIdentifying", label: "Non-identifying" },
    ],
    defaultShape: "entity",
    defaultRel: "identifying",
    namedById: true,
    cards: "er",
    hasAttributes: true,
    hasMethods: false,
  },
  usecase: {
    title: "Use case",
    blurb: "Actors, use cases and the system boundary",
    shapes: [
      { value: "actor", label: "Actor" },
      { value: "usecase", label: "Use case" },
      { value: "boundary", label: "System boundary" },
    ],
    rels: [
      { value: "association", label: "Association" },
      { value: "directed", label: "Directed association" },
      { value: "include", label: "«include»" },
      { value: "extend", label: "«extend»" },
    ],
    defaultShape: "usecase",
    defaultRel: "association",
    groupShape: "boundary",
    namedById: false,
    cards: "none",
    hasAttributes: false,
    hasMethods: false,
  },
};

export const SEQUENCE_SPEC = {
  title: "Sequence diagram",
  blurb: "Participants and the messages between them",
};

/** Shapes drawn as fixed glyphs whose label is not shown or edited. */
export const UNLABELLED_SHAPES = new Set(["start", "end", "choice"]);

/** Turn free text into a Mermaid identifier: `Order line` -> `Order_line`. */
export function toIdentifier(text: string): string {
  const id = text.trim().replace(/[^A-Za-z0-9_]+/g, "_").replace(/^_+|_+$/g, "");
  if (!id) return "Unnamed";
  return /^[0-9]/.test(id) ? `_${id}` : id;
}

/** `base`, or `base2`, `base3`... - whichever is not in `taken`. */
export function uniqueId(base: string, taken: Iterable<string>): string {
  const used = new Set(taken);
  if (!used.has(base)) return base;
  for (let i = 2; ; i++) {
    if (!used.has(`${base}${i}`)) return `${base}${i}`;
  }
}

/** The canonical id of a state scope's `[*]` pseudo-state. */
export function pseudoStateId(shape: "start" | "end", parent: string | undefined): string {
  return `${parent ?? "root"}__${shape}`;
}

export function emptyGraph(kind: GraphKind): GraphDoc {
  return { kind, direction: kind === "usecase" ? "LR" : "TB", nodes: [], edges: [] };
}

export function emptySequence(): SequenceDoc {
  return { kind: "sequence", autonumber: false, participants: [], items: [] };
}

export function isGroup(doc: GraphDoc, node: GraphNode): boolean {
  return KIND_SPECS[doc.kind].groupShape === node.shape;
}

/** Ids of `id` and everything nested inside it. */
export function descendantsOf(doc: GraphDoc, id: string): Set<string> {
  const out = new Set([id]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const n of doc.nodes) {
      if (n.parent && out.has(n.parent) && !out.has(n.id)) {
        out.add(n.id);
        grew = true;
      }
    }
  }
  return out;
}

/** Nodes ordered so every group precedes its children, as React Flow requires. */
export function parentsFirst(nodes: GraphNode[]): GraphNode[] {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const depth = (n: GraphNode): number => {
    let d = 0;
    let p = n.parent ? byId.get(n.parent) : undefined;
    while (p && d < nodes.length) {
      d++;
      p = p.parent ? byId.get(p.parent) : undefined;
    }
    return d;
  };
  return nodes
    .map((n, i) => ({ n, i, d: depth(n) }))
    .sort((a, b) => a.d - b.d || a.i - b.i)
    .map((x) => x.n);
}
