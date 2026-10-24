import { describe, expect, it } from "vitest";
import { fenceAt, insertFence, listFences, replaceFence } from "./fences";

const BODY = [
  "# Spec",
  "",
  "```mermaid",
  "classDiagram",
  "  class A",
  "```",
  "",
  "prose",
  "",
  "```mermaid",
  "flowchart TB",
  "  a --> b",
  "```",
  "",
].join("\n");

describe("listFences", () => {
  it("finds every mermaid fence in order with its source", () => {
    const fences = listFences(BODY);
    expect(fences.map((f) => f.source)).toEqual([
      "classDiagram\n  class A",
      "flowchart TB\n  a --> b",
    ]);
    expect(fences.map((f) => f.index)).toEqual([0, 1]);
    expect(BODY.slice(fences[1].start, fences[1].end)).toBe(
      "```mermaid\nflowchart TB\n  a --> b\n```",
    );
  });

  it("ignores other code fences", () => {
    expect(listFences("```ts\nconst a = 1\n```\n")).toEqual([]);
  });

  it("is repeatable - no regex state leaks between calls", () => {
    expect(listFences(BODY)).toHaveLength(2);
    expect(listFences(BODY)).toHaveLength(2);
  });
});

describe("replaceFence", () => {
  it("rewrites only the chosen fence", () => {
    const next = replaceFence(BODY, 1, "flowchart LR\n  x --> y");
    expect(next).toBe(BODY.replace("flowchart TB\n  a --> b", "flowchart LR\n  x --> y"));
  });

  it("returns null when the fence no longer exists", () => {
    expect(replaceFence(BODY, 5, "x")).toBeNull();
  });
});

describe("insertFence", () => {
  it("pads with blank lines mid-paragraph", () => {
    expect(insertFence("one two", 3, "flowchart TB")).toBe(
      "one\n\n```mermaid\nflowchart TB\n```\n\n two",
    );
  });

  it("appends cleanly to an empty body", () => {
    expect(insertFence("", 0, "classDiagram")).toBe("```mermaid\nclassDiagram\n```\n");
  });

  it("does not double existing blank lines", () => {
    expect(insertFence("a\n\nb", 3, "x")).toBe("a\n\n```mermaid\nx\n```\n\nb");
  });

  it("produces a fence listFences can find again", () => {
    const next = insertFence(BODY, BODY.length, "erDiagram\n  A ||--o{ B : has");
    expect(listFences(next).map((f) => f.source)[2]).toBe("erDiagram\n  A ||--o{ B : has");
  });
});

describe("fenceAt", () => {
  it("finds the fence around an offset", () => {
    const offset = BODY.indexOf("a --> b");
    expect(fenceAt(BODY, offset)?.index).toBe(1);
    expect(fenceAt(BODY, BODY.indexOf("prose"))).toBeNull();
  });
});
