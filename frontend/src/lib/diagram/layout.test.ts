import { describe, expect, it } from "vitest";
import type { GraphDoc, GraphNode } from "./model";
import { autoLayout, needsLayout, placeMissing } from "./layout";

const n = (id: string, shape: string, parent?: string): GraphNode => ({
  id,
  shape,
  label: id,
  attributes: [],
  methods: [],
  parent,
});
const e = (source: string, target: string, i: number) => ({
  id: `e${i}`,
  source,
  target,
  rel: "arrow",
  label: "",
  sourceCard: "",
  targetCard: "",
});

describe("autoLayout", () => {
  it("places every node and ranks edges top to bottom", () => {
    const doc: GraphDoc = {
      kind: "flowchart",
      direction: "TB",
      nodes: [n("a", "process"), n("b", "process"), n("c", "process")],
      edges: [e("a", "b", 1), e("b", "c", 2)],
    };
    expect(needsLayout(doc)).toBe(true);
    const out = autoLayout(doc);
    expect(needsLayout(out)).toBe(false);
    const y = Object.fromEntries(out.nodes.map((x) => [x.id, x.pos!.y]));
    expect(y.a).toBeLessThan(y.b);
    expect(y.b).toBeLessThan(y.c);
  });

  it("ranks left to right when asked", () => {
    const doc: GraphDoc = {
      kind: "flowchart",
      direction: "LR",
      nodes: [n("a", "process"), n("b", "process")],
      edges: [e("a", "b", 1)],
    };
    const [a, b] = autoLayout(doc).nodes;
    expect(a.pos!.x).toBeLessThan(b.pos!.x);
  });

  it("sizes a group around its children, with child positions relative to it", () => {
    const doc: GraphDoc = {
      kind: "flowchart",
      direction: "TB",
      nodes: [n("g", "subgraph"), n("a", "process", "g"), n("b", "process", "g"), n("c", "process")],
      // An edge onto the group itself must not break the layout.
      edges: [e("a", "b", 1), e("c", "g", 2)],
    };
    const out = autoLayout(doc);
    const g = out.nodes.find((x) => x.id === "g")!;
    const a = out.nodes.find((x) => x.id === "a")!;
    expect(g.size!.w).toBeGreaterThan(100);
    expect(a.pos!.x).toBeGreaterThanOrEqual(0);
    expect(a.pos!.y).toBeGreaterThan(0);
    expect(a.pos!.x).toBeLessThan(g.size!.w);
  });

  it("gives an empty group a default size", () => {
    const doc: GraphDoc = { kind: "usecase", direction: "LR", nodes: [n("b", "boundary")], edges: [] };
    expect(autoLayout(doc).nodes[0].size).toEqual({ w: 260, h: 160 });
  });
});

describe("placeMissing", () => {
  it("places only the newcomers when the rest already have positions", () => {
    const doc: GraphDoc = {
      kind: "flowchart",
      direction: "TB",
      nodes: [{ ...n("a", "process"), pos: { x: 300, y: 100 } }, n("b", "process")],
      edges: [],
    };
    const out = placeMissing(doc);
    expect(out.nodes[0].pos).toEqual({ x: 300, y: 100 });
    expect(out.nodes[1].pos!.y).toBeGreaterThan(148);
  });

  it("leaves a fully placed diagram untouched", () => {
    const doc: GraphDoc = { kind: "flowchart", direction: "TB", nodes: [{ ...n("a", "process"), pos: { x: 1, y: 2 } }], edges: [] };
    expect(placeMissing(doc)).toBe(doc);
  });
});
