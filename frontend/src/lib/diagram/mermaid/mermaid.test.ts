import { beforeAll, describe, expect, it } from "vitest";
import mermaid from "mermaid";
import type { GraphDoc, GraphEdge, GraphNode, SequenceDoc } from "../model";
import { detectKind, parseDiagram, serializeDiagram } from "./index";

/**
 * Every serializer is held to two properties: Mermaid itself accepts what it
 * writes, and parsing it back gives the same model. `mermaid.parse` needs no
 * layout engine, so unlike rendering it is checkable under happy-dom.
 */

beforeAll(() => {
  mermaid.initialize({ startOnLoad: false, securityLevel: "strict" });
});

async function expectValid(src: string) {
  await expect(mermaid.parse(src)).resolves.toBeTruthy();
}

const node = (id: string, shape: string, over: Partial<GraphNode> = {}): GraphNode => ({
  id,
  shape,
  label: id,
  attributes: [],
  methods: [],
  ...over,
});
let edgeSeq = 0;
const edge = (source: string, target: string, rel: string, over: Partial<GraphEdge> = {}): GraphEdge => ({
  id: `e${++edgeSeq}`,
  source,
  target,
  rel,
  label: "",
  sourceCard: "",
  targetCard: "",
  ...over,
});
function graph(kind: GraphDoc["kind"], nodes: GraphNode[], edges: GraphEdge[], direction: GraphDoc["direction"] = "TB"): GraphDoc {
  // Parsers number edges e1..en in order; match that.
  return { kind, direction, nodes, edges: edges.map((e, i) => ({ ...e, id: `e${i + 1}` })) };
}

/**
 * Graph statements are written scope by scope (a composite's contents inside
 * its block), so order is not preserved - and edge ids are renumbered on
 * parse. Neither carries meaning, so the comparison ignores both.
 */
function normalize(doc: GraphDoc | SequenceDoc | null) {
  if (!doc || doc.kind === "sequence") return doc;
  const edgeKey = (e: GraphEdge) => `${e.source}>${e.target}>${e.rel}>${e.label}`;
  return {
    ...doc,
    nodes: [...doc.nodes].sort((a, b) => a.id.localeCompare(b.id)),
    edges: doc.edges.map(({ id: _id, ...e }) => e).sort((a, b) => edgeKey(a as GraphEdge).localeCompare(edgeKey(b as GraphEdge))),
  };
}

async function roundTrip(doc: GraphDoc | SequenceDoc) {
  const src = serializeDiagram(doc);
  await expectValid(src);
  expect(detectKind(src)).toBe(doc.kind);
  expect(normalize(parseDiagram(src).doc)).toEqual(normalize(doc));
  return src;
}

describe("class diagrams", () => {
  const doc = graph(
    "class",
    [
      node("Order", "class", { attributes: ["+String id", "-Money total"], methods: ["+submit() bool"], pos: { x: 10, y: 20 } }),
      node("Line", "class", { pos: { x: 200, y: 20 } }),
      node("Payable", "interface", { methods: ["+pay()"], pos: { x: 0, y: 300 } }),
      node("Status", "enum", { attributes: ["OPEN", "CLOSED"], pos: { x: 300, y: 300 } }),
      node("Base", "abstract", { pos: { x: 500, y: 0 } }),
    ],
    [
      edge("Line", "Order", "composition", { sourceCard: "many", targetCard: "1", label: "lines" }),
      edge("Order", "Payable", "realization"),
      edge("Order", "Base", "inheritance"),
      edge("Order", "Status", "association", { label: "has a" }),
      edge("Order", "Line", "aggregation"),
      edge("Order", "Status", "dependency"),
      edge("Line", "Status", "link"),
      edge("Line", "Base", "linkDashed"),
    ],
    "LR",
  );

  it("round-trips every relationship, member and stereotype", async () => {
    const src = await roundTrip(doc);
    expect(src).toContain('Order "1" *-- "many" Line : lines');
    expect(src).toContain("Base <|-- Order");
    expect(src).toContain("<<interface>>");
  });

  it("reads hand-written forms", () => {
    const parsed = parseDiagram(
      ["classDiagram", "  Animal <|-- Duck", "  Duck : +swim()", "  Duck : +int age", "  <<interface>> Animal", "  Zoo --|> Place"].join("\n"),
    ).doc as GraphDoc;
    expect(parsed.nodes.find((n) => n.id === "Duck")).toMatchObject({ methods: ["+swim()"], attributes: ["+int age"] });
    expect(parsed.nodes.find((n) => n.id === "Animal")?.shape).toBe("interface");
    expect(parsed.edges[0]).toMatchObject({ source: "Duck", target: "Animal", rel: "inheritance" });
    expect(parsed.edges[1]).toMatchObject({ source: "Zoo", target: "Place", rel: "inheritance" });
  });

  it("refuses what it cannot keep", () => {
    expect(parseDiagram("classDiagram\n  A <|--|> B").doc).toBeNull();
    expect(parseDiagram("classDiagram\n  class A\n  style A fill:#f00").doc).toBeNull();
    expect(parseDiagram("classDiagram\n  %% a note to self\n  class A").doc).toBeNull();
  });
});

describe("flowcharts", () => {
  const doc = graph(
    "flowchart",
    [
      node("n1", "terminal", { label: "Start" }),
      node("g1", "subgraph", { label: "Review stage" }),
      node("n2", "decision", { label: "Approved?", parent: "g1" }),
      node("n3", "process", { label: "Ship it", parent: "g1" }),
      node("n4", "io", { label: "Read input" }),
      node("n5", "database", { label: "Store" }),
      node("n6", "rounded", { label: "Rounded" }),
      node("n7", "circle", { label: "C" }),
    ],
    [
      edge("n1", "n2", "arrow"),
      edge("n2", "n3", "arrow", { label: "yes" }),
      edge("n2", "n4", "dotted", { label: "no" }),
      edge("n4", "n5", "thick"),
      edge("n5", "n6", "open"),
      edge("n6", "n7", "dottedOpen"),
    ],
  );

  it("round-trips shapes, groups and edge styles", async () => {
    await roundTrip(doc);
  });

  it("keeps positions and group sizes in one comment", async () => {
    const placed: GraphDoc = {
      ...doc,
      nodes: doc.nodes.map((n, i) => ({
        ...n,
        pos: { x: i * 10, y: i * 20 },
        ...(n.shape === "subgraph" ? { size: { w: 300, h: 200 } } : {}),
      })),
    };
    const src = await roundTrip(placed);
    expect(src.split("\n").filter((l) => l.includes("sebenza:layout"))).toHaveLength(1);
    expect(src).toContain('"g1":[10,20,300,200]');
  });

  it("reads chains and inline shapes", () => {
    const parsed = parseDiagram("graph LR\n  A[Start] --> B{Ok?} -->|yes| C([Done]);\n  B -.-> A").doc as GraphDoc;
    expect(parsed.direction).toBe("LR");
    expect(parsed.nodes.map((n) => [n.id, n.shape, n.label])).toEqual([
      ["A", "process", "Start"],
      ["B", "decision", "Ok?"],
      ["C", "terminal", "Done"],
    ]);
    expect(parsed.edges.map((e) => [e.source, e.target, e.rel, e.label])).toEqual([
      ["A", "B", "arrow", ""],
      ["B", "C", "arrow", "yes"],
      ["B", "A", "dotted", ""],
    ]);
  });

  it("refuses what it cannot keep", () => {
    expect(parseDiagram("flowchart TB\n  A & B --> C").doc).toBeNull();
    expect(parseDiagram("flowchart TB\n  A --> B\n  click A callback").doc).toBeNull();
    expect(parseDiagram("flowchart TB\n  classDef hot fill:#f00").doc).toBeNull();
    expect(parseDiagram("flowchart TB\n  subgraph x\n  A").doc).toBeNull();
  });
});

describe("use case diagrams", () => {
  const doc = graph(
    "usecase",
    [
      node("a1", "actor", { label: "Operator" }),
      node("b1", "boundary", { label: "Sebenza" }),
      node("u1", "usecase", { label: "Write spec", parent: "b1" }),
      node("u2", "usecase", { label: "Draw diagram", parent: "b1" }),
      node("u3", "usecase", { label: "Validate", parent: "b1" }),
    ],
    [
      edge("a1", "u1", "association"),
      edge("u1", "u2", "include"),
      edge("u3", "u1", "extend"),
      edge("a1", "u3", "directed", { label: "sometimes" }),
    ],
    "LR",
  );

  it("round-trips as a legible flowchart", async () => {
    const src = await roundTrip(doc);
    expect(src).toContain("%% sebenza:kind usecase");
    expect(src).toContain('a1["«actor» Operator"]');
    expect(src).toContain('u1 -.->|"«include»"| u2');
  });

  it("is a plain flowchart without its marker", () => {
    const src = serializeDiagram(doc).replace(/.*sebenza:kind.*\n/, "");
    expect(detectKind(src)).toBe("flowchart");
  });
});

describe("state diagrams", () => {
  const doc = graph(
    "state",
    [
      node("root__start", "start"),
      node("s1", "state", { label: "Idle" }),
      node("c1", "composite", { label: "Working" }),
      node("c1__start", "start", { parent: "c1" }),
      node("s2", "state", { label: "Fetching", parent: "c1" }),
      node("ch1", "choice"),
      node("root__end", "end"),
    ],
    [
      edge("root__start", "s1", "transition"),
      edge("s1", "c1", "transition", { label: "go" }),
      edge("c1__start", "s2", "transition"),
      edge("c1", "ch1", "transition"),
      edge("ch1", "s1", "transition", { label: "retry" }),
      edge("ch1", "root__end", "transition", { label: "done" }),
    ],
    "LR",
  );

  it("round-trips pseudo-states, composites and choices", async () => {
    const src = await roundTrip(doc);
    expect(src).toContain("[*] --> s1");
    expect(src).toContain("state ch1 <<choice>>");
  });

  it("reads hand-written forms", () => {
    const parsed = parseDiagram("stateDiagram\n  [*] --> Still\n  Still --> Moving : push\n  Moving : In motion\n  Moving --> [*]").doc as GraphDoc;
    expect(parsed.nodes.map((n) => [n.id, n.shape, n.label])).toEqual([
      ["root__start", "start", "root__start"],
      ["Still", "state", "Still"],
      ["Moving", "state", "In motion"],
      ["root__end", "end", "root__end"],
    ]);
  });

  it("refuses what it cannot keep", () => {
    expect(parseDiagram("stateDiagram-v2\n  state f <<fork>>").doc).toBeNull();
    expect(parseDiagram("stateDiagram-v2\n  note right of A : hi").doc).toBeNull();
  });
});

describe("ER diagrams", () => {
  const doc = graph(
    "er",
    [
      node("CUSTOMER", "entity", { attributes: ["string name PK", 'string email UK "login"'] }),
      node("ORDER", "entity", { attributes: ["int id PK", "int customer FK"] }),
      node("LINE_ITEM", "entity"),
    ],
    [
      edge("CUSTOMER", "ORDER", "identifying", { sourceCard: "one", targetCard: "many", label: "places" }),
      edge("ORDER", "LINE_ITEM", "nonIdentifying", { sourceCard: "zeroOne", targetCard: "oneMany", label: "contains many" }),
    ],
  );

  it("round-trips cardinality, attributes and empty entities", async () => {
    const src = await roundTrip(doc);
    expect(src).toContain('CUSTOMER ||--o{ ORDER : "places"');
    expect(src).toContain('ORDER |o..|{ LINE_ITEM : "contains many"');
  });

  it("reads an unquoted label", () => {
    const parsed = parseDiagram("erDiagram\n  A }|..|| B : owns").doc as GraphDoc;
    expect(parsed.edges[0]).toMatchObject({ sourceCard: "oneMany", targetCard: "one", rel: "nonIdentifying", label: "owns" });
  });
});

describe("sequence diagrams", () => {
  const doc: SequenceDoc = {
    kind: "sequence",
    autonumber: true,
    participants: [
      { id: "U", label: "Operator", kind: "actor" },
      { id: "S", label: "Server", kind: "participant" },
      { id: "DB", label: "DB", kind: "participant" },
    ],
    items: [
      { t: "msg", from: "U", to: "S", arrow: "->>", text: "save draft", act: "+" },
      { t: "start", block: "alt", text: "body unchanged" },
      { t: "msg", from: "S", to: "DB", arrow: "-)", text: "write" },
      { t: "else", text: "conflict" },
      { t: "msg", from: "S", to: "U", arrow: "--x", text: "409" },
      { t: "end" },
      { t: "start", block: "loop", text: "every 5s" },
      { t: "note", pos: "over", who: ["U", "S"], text: "autosave" },
      { t: "end" },
      { t: "activate", who: "DB", on: true },
      { t: "msg", from: "S", to: "U", arrow: "-->>", text: "ok", act: "-" },
      { t: "activate", who: "DB", on: false },
      { t: "msg", from: "DB", to: "DB", arrow: "->", text: "compact" },
    ],
  };

  it("round-trips participants, messages, blocks and notes", async () => {
    const src = await roundTrip(doc);
    expect(src).toContain("actor U as Operator");
    expect(src).toContain("  participant DB\n");
    expect(src).toContain("U->>+S: save draft");
    expect(src).toContain("\n  else conflict\n");
  });

  it("closes unbalanced blocks and drops stray dividers on the way out", async () => {
    const src = serializeDiagram({
      ...doc,
      items: [{ t: "end" }, { t: "start", block: "opt", text: "maybe" }, { t: "else", text: "nope" }, { t: "msg", from: "U", to: "S", arrow: "->>", text: "hi" }],
    });
    await expectValid(src);
    expect(src).not.toContain("else");
    expect(src.trimEnd().endsWith("end")).toBe(true);
  });

  it("adds participants a message mentions without declaring", () => {
    const parsed = parseDiagram("sequenceDiagram\n  Alice->>Bob: Hi\n  Bob-->>Alice: Yo").doc as SequenceDoc;
    expect(parsed.participants.map((p) => p.id)).toEqual(["Alice", "Bob"]);
  });

  it("refuses what it cannot keep", () => {
    expect(parseDiagram("sequenceDiagram\n  rect rgb(0,0,0)\n  end").doc).toBeNull();
    expect(parseDiagram("sequenceDiagram\n  box Aqua Group\n  end").doc).toBeNull();
  });
});

describe("hostile labels", () => {
  const evil = 'x"]\nclick n1 call alert(1)\n%%{init: {"securityLevel":"loose"}}%%; y';

  it("cannot start a new statement in a flowchart", async () => {
    const doc = graph("flowchart", [node("n1", "process", { label: evil })], [edge("n1", "n1", "arrow", { label: evil })]);
    const src = serializeDiagram(doc);
    expect(src.split("\n").some((l) => /^\s*(click|%%\{)/.test(l))).toBe(false);
    await expectValid(src);
    const back = parseDiagram(src).doc as GraphDoc;
    expect(back.nodes[0].label).toBe(evil.replace(/\s+/g, " "));
  });

  it("cannot start a new statement in a sequence diagram", async () => {
    const doc: SequenceDoc = {
      kind: "sequence",
      autonumber: false,
      participants: [{ id: "A", label: evil, kind: "participant" }],
      items: [{ t: "msg", from: "A", to: "A", arrow: "->>", text: evil }],
    };
    const src = serializeDiagram(doc);
    expect(src.split("\n")).toHaveLength(3);
    expect(src.replace(/#59;/g, "")).not.toContain(";");
    await expectValid(src);
  });

  it("cannot break out of a class body or state label", async () => {
    const cls = graph("class", [node("A", "class", { attributes: ["}\nclass Evil {"], methods: [] })], []);
    const csrc = serializeDiagram(cls);
    expect(csrc).not.toContain("class Evil {");
    await expectValid(csrc);
    const st = graph("state", [node("s1", "state", { label: evil })], []);
    await expectValid(serializeDiagram(st));
  });

  it("ignores a malformed layout comment instead of trusting it", () => {
    const parsed = parseDiagram('flowchart TB\n  a\n  %% sebenza:layout {"a":["<img>",1],"b":[1,2]}').doc as GraphDoc;
    expect(parsed.nodes[0].pos).toBeUndefined();
    const broken = parseDiagram("flowchart TB\n  a\n  %% sebenza:layout {not json").doc as GraphDoc;
    expect(broken.nodes).toHaveLength(1);
  });
});

it("detects no kind for unknown diagrams", () => {
  expect(parseDiagram("gantt\n  title x")).toEqual({ kind: null, doc: null });
  expect(detectKind("%% comment\npie title Pets")).toBeNull();
});
