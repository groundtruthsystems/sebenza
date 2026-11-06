import { describe, expect, it } from "vitest";
import { COLLAPSE_TASKS, LABEL_MAX, escapeLabel, graphToMermaid } from "./graphToMermaid";
import { MERMAID_LIMITS } from "./inboxMarkdown";
import type { TaskGraph } from "./api";

type Task = TaskGraph["worktrees"][number]["tracks"][number]["phases"][number]["tasks"][number];

const task = (wt: number, track: string, id: string, over: Partial<Task> = {}): Task => ({
  id: `k:${wt}/${track}/${id}`,
  taskId: id,
  name: `${id} name`,
  status: "doing",
  dependsOn: [],
  request: null,
  ...over,
});

function graph(tasks: Task[], edges: TaskGraph["edges"] = [], branch = "feat-x"): TaskGraph {
  return {
    draftId: "01D",
    generatedAt: "t",
    etag: "e",
    truncated: false,
    progress: { done: 0, total: tasks.length },
    failed: 0,
    unavailable: 0,
    worktrees: [
      {
        id: "w:0",
        branch,
        projectPath: "/code/acme",
        state: "ok",
        errorCode: null,
        progress: { done: 0, total: tasks.length },
        agent: null,
        tracks: [
          {
            id: "t:0/t",
            trackId: "t",
            description: "Graph",
            status: "doing",
            currentPhase: "phase-1",
            designing: false,
            errorCode: null,
            progress: { done: 0, total: tasks.length },
            phases: [
              {
                id: "p:0/t/phase-1",
                phaseId: "phase-1",
                name: "P",
                status: "doing",
                progress: { done: 0, total: tasks.length },
                tasks,
              },
            ],
          },
        ],
      },
    ],
    edges,
  };
}

const dep = (from: string, to: string, over: Partial<TaskGraph["edges"][number]> = {}) => ({
  from,
  to,
  kind: "depends_on" as const,
  dangling: false,
  flag: null,
  ...over,
});

describe("graphToMermaid", () => {
  // TS-10: worktrees become subgraphs, tasks nodes, dependencies arrows.
  it("draws worktrees, tracks, tasks and dependency arrows", () => {
    const a = task(0, "t", "a", { status: "done" });
    const b = task(0, "t", "b");
    const { source } = graphToMermaid(graph([a, b], [dep(b.id, a.id)]));
    expect(source.startsWith("flowchart LR")).toBe(true);
    expect(source).toMatch(/subgraph n\d+\["feat-x · acme \(0\/2\)"\]/);
    expect(source).toMatch(/n\d+\["a name · done"\]/);
    expect((source.match(/ --> /g) ?? []).length).toBe(1);
    // Internal ids never reach mermaid: they are re-keyed.
    expect(source).not.toContain("k:0/t/a");
  });

  // TS-12: a dangling dependency is visibly flagged.
  it("flags dangling and cyclic edges", () => {
    const b = task(0, "t", "b");
    const c = task(0, "t", "c");
    const { source } = graphToMermaid(
      graph([b, c], [
        dep(b.id, "x:no-such", { dangling: true, flag: "unknown" }),
        dep(b.id, c.id, { flag: "cycle" }),
      ]),
    );
    expect(source).toMatch(/n\d+ -\.->\|unknown\| n\d+/);
    expect(source).toMatch(/class n\d+ dangling/);
    expect(source).toMatch(/n\d+ ==>\|cycle\| n\d+/);
  });

  // TS-48: repo-controlled names cannot inject markup or mermaid syntax.
  it("escapes and caps every label", () => {
    const evil = '"><img src=x onerror=alert(1)>]; click n1 call alert()\nend';
    const t = task(0, "t", "a", { name: evil + "x".repeat(200) });
    const { source } = graphToMermaid(graph([t], [], `feat"]--><script>`));
    expect(source).not.toMatch(/<img|<script|onerror=alert\(1\)>/);
    expect(source).not.toContain('"]-->');
    for (const label of source.match(/\["[^"]*"\]/g) ?? []) {
      expect(label.length).toBeLessThanOrEqual(LABEL_MAX + 4 + 40);
    }
    expect(escapeLabel("a\nb")).not.toContain("\n");
    expect(escapeLabel("x".repeat(500)).length).toBeLessThanOrEqual(LABEL_MAX + 40);
  });

  // TS-55: a large track collapses to one summary node until expanded, and
  // mermaid's size limits are set explicitly.
  it("collapses big tracks until expanded", () => {
    const tasks = Array.from({ length: COLLAPSE_TASKS + 1 }, (_, i) => task(0, "t", `k${i}`));
    const big = graph(tasks);
    const collapsed = graphToMermaid(big);
    expect(collapsed.collapsed).toEqual(["t:0/t"]);
    expect(collapsed.source).toMatch(/n\d+\[\["Graph \(0\/151\) · phase-1"\]\]/);
    expect(collapsed.source).not.toContain("k0 name");
    const open = graphToMermaid(big, new Set(["t:0/t"]));
    expect(open.collapsed).toEqual([]);
    expect(open.source).toContain("k0 name");
    expect(MERMAID_LIMITS.maxEdges).toBeGreaterThanOrEqual(1000);
    expect(MERMAID_LIMITS.maxTextSize).toBeGreaterThanOrEqual(100_000);
  });
});
