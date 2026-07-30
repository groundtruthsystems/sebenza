import { describe, expect, it } from "vitest";
import {
  branchesWithAgentStatus,
  buildWorktreeListRows,
  countAgentStatusesIn,
  countArchivedMatches,
  filterWorktrees,
} from "./worktree-list";
import type { WorktreeInfo, WorktreeListRow } from "./types";

function createWorktree(branch: string, overrides: Partial<WorktreeInfo> = {}): WorktreeInfo {
  return {
    branch,
    kind: "linked",
    label: null,
    archived: false,
    agent: "waiting",
    mux: "",
    path: `/repo/__worktrees/${branch}`,
    dir: `/repo/__worktrees/${branch}`,
    dirty: false,
    unpushed: false,
    status: "idle",
    feedbackState: "none",
    elapsed: "",
    profile: null,
    agentName: null,
    agentLabel: null,
    agentTerminalStale: false,
    services: [],
    paneCount: 1,
    prs: [],
    creating: false,
    creationPhase: null,
    source: "ui",
    oneshot: null,
    tabs: [],
    activeTabId: null,
    ...overrides,
  };
}

describe("buildWorktreeListRows", () => {
  it("nests child worktrees under their base worktree when it exists", () => {
    const rows = buildWorktreeListRows([
      createWorktree("feature/base"),
      createWorktree("feature/child-a", { baseBranch: "feature/base" }),
      createWorktree("feature/grandchild", { baseBranch: "feature/child-a" }),
      createWorktree("feature/child-b", { baseBranch: "feature/base" }),
    ]);

    expect(rows.map((row) => [row.worktree.branch, row.depth])).toEqual([
      ["feature/base", 0],
      ["feature/child-a", 1],
      ["feature/grandchild", 2],
      ["feature/child-b", 1],
    ]);
  });

  it("keeps a worktree at the top level when its base worktree is not visible", () => {
    const rows = buildWorktreeListRows([
      createWorktree("feature/child", { baseBranch: "feature/missing-base" }),
      createWorktree("feature/other"),
    ]);

    expect(rows.map((row) => [row.worktree.branch, row.depth])).toEqual([
      ["feature/child", 0],
      ["feature/other", 0],
    ]);
  });

  it("filters archived worktrees out by default and matches profile text", () => {
    const worktrees = filterWorktrees([
      createWorktree("feature/active", { profile: "sandbox" }),
      createWorktree("feature/archived", { archived: true, profile: "default" }),
    ], {
      query: "sand",
      showArchived: false,
    });

    expect(worktrees.map((worktree) => worktree.branch)).toEqual(["feature/active"]);
  });

  it("matches label text when searching worktrees", () => {
    const worktrees = filterWorktrees([
      createWorktree("feature/random-fallback", { label: "Search ranking" }),
      createWorktree("feature/other"),
    ], {
      query: "ranking",
      showArchived: false,
    });

    expect(worktrees.map((worktree) => worktree.branch)).toEqual(["feature/random-fallback"]);
  });

  it("counts archived matches separately from visible rows", () => {
    const count = countArchivedMatches([
      createWorktree("feature/alpha", { archived: true }),
      createWorktree("feature/beta", { archived: true }),
      createWorktree("feature/gamma"),
    ], "beta");

    expect(count).toBe(1);
  });
});

function createRow(branch: string, overrides: Partial<WorktreeInfo> = {}): WorktreeListRow {
  return { worktree: createWorktree(branch, { mux: "✓", ...overrides }), depth: 0 };
}

describe("countAgentStatusesIn", () => {
  const rows = [
    createRow("a", { agent: "waiting" }),
    createRow("b", { agent: "error" }),
    createRow("c", { agent: "waiting" }),
    createRow("d", { agent: "working" }),
    createRow("e", { agent: "idle" }),
  ];

  it("counts waiting and error rows within the given branch set", () => {
    expect(countAgentStatusesIn(rows, new Set(["b", "c"]))).toEqual({
      waiting: 1,
      "awaiting-permission": 0,
      error: 1,
      "done-unread": 0,
    });
  });

  it("returns zero counts for an empty set", () => {
    expect(countAgentStatusesIn(rows, new Set())).toEqual({ waiting: 0, "awaiting-permission": 0, error: 0, "done-unread": 0 });
  });

  it("ignores working and idle rows even when in the set", () => {
    expect(countAgentStatusesIn(rows, new Set(["d", "e"]))).toEqual({
      waiting: 0,
      "awaiting-permission": 0,
      error: 0,
      "done-unread": 0,
    });
  });

  it("does not count rows whose agent icon would be hidden (closed or creating)", () => {
    const closedRows = [
      createRow("closed", { agent: "waiting", mux: "" }),
      createRow("creating", { agent: "error", creating: true }),
      createRow("live", { agent: "waiting" }),
    ];
    expect(countAgentStatusesIn(closedRows, new Set(["closed", "creating", "live"]))).toEqual({
      waiting: 1,
      "awaiting-permission": 0,
      error: 0,
      "done-unread": 0,
    });
  });

  it("counts done rows only when they are unread", () => {
    const doneRows = [
      createRow("seen", { agent: "done" }),
      createRow("unseen-a", { agent: "done" }),
      createRow("unseen-b", { agent: "done" }),
    ];
    const branches = new Set(["seen", "unseen-a", "unseen-b"]);
    const notified = new Set(["unseen-a", "unseen-b"]);
    expect(countAgentStatusesIn(doneRows, branches, notified)).toEqual({
      waiting: 0,
      "awaiting-permission": 0,
      error: 0,
      "done-unread": 2,
    });
  });
});

describe("branchesWithAgentStatus", () => {
  const rows = [
    createRow("first", { agent: "waiting" }),
    createRow("closed", { agent: "waiting", mux: "" }),
    createRow("err", { agent: "error" }),
    createRow("second", { agent: "waiting" }),
  ];

  it("returns matching branches in list order, skipping hidden-icon rows", () => {
    expect(branchesWithAgentStatus(rows, "waiting")).toEqual(["first", "second"]);
    expect(branchesWithAgentStatus(rows, "error")).toEqual(["err"]);
  });

  it("restricts matches to the given branch set when provided", () => {
    expect(branchesWithAgentStatus(rows, "waiting", new Set(["second"]))).toEqual(["second"]);
  });
});

describe("buildWorktreeListRows with the repo row", () => {
  it("hoists the repo row to the top of the tree", () => {
    // The server sorts by branch, so without hoisting "main" would land
    // alphabetically among the worktrees branched off it.
    const rows = buildWorktreeListRows([
      createWorktree("aaa-first"),
      createWorktree("main", { kind: "main" }),
      createWorktree("zzz-last"),
    ]);
    expect(rows[0].worktree.branch).toBe("main");
    expect(rows[0].depth).toBe(0);
  });

  it("nests main-based worktrees under the repo row", () => {
    // The tree gains its trunk: previously every main-based worktree was depth 0.
    const rows = buildWorktreeListRows([
      createWorktree("main", { kind: "main" }),
      createWorktree("feature/a", { baseBranch: "main" }),
      createWorktree("feature/b", { baseBranch: "main" }),
    ]);
    expect(rows.map((r) => [r.worktree.branch, r.depth])).toEqual([
      ["main", 0],
      ["feature/a", 1],
      ["feature/b", 1],
    ]);
  });

  it("keeps grandchildren nesting under their own parent", () => {
    const rows = buildWorktreeListRows([
      createWorktree("main", { kind: "main" }),
      createWorktree("feature/a", { baseBranch: "main" }),
      createWorktree("feature/a-fix", { baseBranch: "feature/a" }),
    ]);
    expect(rows.map((r) => r.depth)).toEqual([0, 1, 2]);
  });

  it("flattens children to depth zero when the repo row is filtered out by search", () => {
    // parentBranchOf only nests when the base branch is present in the list.
    const filtered = filterWorktrees(
      [
        createWorktree("main", { kind: "main" }),
        createWorktree("feature/a", { baseBranch: "main" }),
      ],
      { query: "feature", showArchived: false },
    );
    const rows = buildWorktreeListRows(filtered);
    expect(rows).toHaveLength(1);
    expect(rows[0].worktree.branch).toBe("feature/a");
    expect(rows[0].depth).toBe(0);
  });

  it("does not treat the repo row as its own parent", () => {
    // The main entry carries no baseBranch, but guard the self-parent case too.
    const rows = buildWorktreeListRows([
      createWorktree("main", { kind: "main", baseBranch: "main" }),
    ]);
    expect(rows).toHaveLength(1);
    expect(rows[0].depth).toBe(0);
  });

  it("leaves ordering unchanged when there is no repo row", () => {
    const rows = buildWorktreeListRows([
      createWorktree("aaa-first"),
      createWorktree("zzz-last"),
    ]);
    expect(rows.map((r) => r.worktree.branch)).toEqual(["aaa-first", "zzz-last"]);
  });
});
