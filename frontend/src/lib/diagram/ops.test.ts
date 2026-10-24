import { describe, expect, it } from "vitest";
import { emptyGraph, type GraphDoc } from "./model";
import { addEdge, addNode, deleteItems, reverseEdge, setParent, updateNode } from "./ops";
import { serializeDiagram } from "./mermaid";

const at = { x: 0, y: 0 };

describe("graph ops", () => {
  it("names classes uniquely and re-keys them on rename", () => {
    let doc = emptyGraph("class");
    let a: string, b: string;
    ({ doc, id: a } = addNode(doc, "class", at));
    ({ doc, id: b } = addNode(doc, "class", at));
    expect([a, b]).toEqual(["Class", "Class2"]);
    ({ doc } = addEdge(doc, b, a));
    let renamed: string;
    ({ doc, id: renamed } = updateNode(doc, a, { label: "Order line" }));
    expect(renamed).toBe("Order_line");
    expect(doc.edges[0].target).toBe("Order_line");
    expect(doc.nodes[0]).toMatchObject({ id: "Order_line", label: "Order_line" });
    // A rename onto an existing name is made unique, not merged.
    ({ id: renamed } = updateNode(doc, b, { label: "Order_line" }));
    expect(renamed).toBe("Order_line2");
  });

  it("keeps flowchart ids stable when the label changes", () => {
    let doc = emptyGraph("flowchart");
    let id: string;
    ({ doc, id } = addNode(doc, "process", at));
    const next = updateNode(doc, id, { label: "Anything at all" });
    expect(next.id).toBe(id);
    expect(next.doc.nodes[0].label).toBe("Anything at all");
  });

  it("allows one initial state per scope", () => {
    let doc = emptyGraph("state");
    let first: string, second: string;
    ({ doc, id: first } = addNode(doc, "start", at));
    ({ doc, id: second } = addNode(doc, "start", at));
    expect(first).toBe(second);
    expect(doc.nodes).toHaveLength(1);
  });

  it("re-keys an initial state moved into a composite", () => {
    let doc = emptyGraph("state");
    let c: string, s: string, t: string;
    ({ doc, id: c } = addNode(doc, "composite", { x: 100, y: 100 }));
    ({ doc, id: s } = addNode(doc, "start", { x: 150, y: 150 }));
    ({ doc, id: t } = addNode(doc, "state", { x: 300, y: 300 }));
    ({ doc } = addEdge(doc, s, t));
    let moved: string;
    ({ doc, id: moved } = setParent(doc, s, c));
    expect(moved).toBe("s1__start");
    expect(doc.nodes.find((n) => n.id === moved)).toMatchObject({ parent: c, pos: { x: 50, y: 50 } });
    expect(doc.edges[0].source).toBe(moved);
  });

  it("will not nest a group inside its own child", () => {
    let doc = emptyGraph("flowchart");
    let outer: string, inner: string;
    ({ doc, id: outer } = addNode(doc, "subgraph", at));
    ({ doc, id: inner } = addNode(doc, "subgraph", at, outer));
    expect(setParent(doc, outer, inner).doc).toBe(doc);
  });

  it("deleting a group keeps its children, in place, at the level above", () => {
    const doc: GraphDoc = {
      kind: "flowchart",
      direction: "TB",
      nodes: [
        { id: "g", shape: "subgraph", label: "G", attributes: [], methods: [], pos: { x: 100, y: 50 } },
        { id: "a", shape: "process", label: "A", attributes: [], methods: [], parent: "g", pos: { x: 10, y: 10 } },
      ],
      edges: [{ id: "e1", source: "a", target: "g", rel: "arrow", label: "", sourceCard: "", targetCard: "" }],
    };
    const next = deleteItems(doc, ["g"]);
    expect(next.nodes).toEqual([{ ...doc.nodes[1], parent: undefined, pos: { x: 110, y: 60 } }]);
    expect(next.edges).toEqual([]);
  });

  it("reverses an edge with its multiplicities", () => {
    let doc = emptyGraph("class");
    ({ doc } = addNode(doc, "class", at));
    ({ doc } = addNode(doc, "class", at));
    let e: string;
    ({ doc, id: e } = addEdge(doc, "Class", "Class2"));
    doc = { ...doc, edges: [{ ...doc.edges[0], sourceCard: "1", targetCard: "*" }] };
    expect(reverseEdge(doc, e).edges[0]).toMatchObject({ source: "Class2", target: "Class", sourceCard: "*", targetCard: "1" });
  });

  it("gives new ER relationships a valid default", () => {
    let doc = emptyGraph("er");
    ({ doc } = addNode(doc, "entity", at));
    ({ doc } = addNode(doc, "entity", at));
    ({ doc } = addEdge(doc, "ENTITY", "ENTITY2"));
    expect(serializeDiagram(doc)).toContain('ENTITY ||--o{ ENTITY2 : "has"');
  });
});
