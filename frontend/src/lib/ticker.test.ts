import { describe, expect, it } from "vitest";
import { deriveCrossProjectTickerItems, deriveTickerItems } from "./ticker";
import type { ActiveProjectWorktrees, WorktreeInfo } from "./types";

function createWorktree(branch: string, overrides: Partial<WorktreeInfo> = {}): WorktreeInfo {
  return {
    branch,
    kind: "linked",
    label: null,
    archived: false,
    agent: "working",
    mux: "✓",
    path: `/repo/__worktrees/${branch}`,
    dir: `/repo/__worktrees/${branch}`,
    dirty: false,
    unpushed: false,
    status: "running",
    feedbackState: "none",
    elapsed: "1m",
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

const branchesOf = (worktrees: WorktreeInfo[], selected: string | null = null): string[] =>
  deriveTickerItems(worktrees, selected).map((item) => item.branch);

describe("deriveTickerItems eligibility", () => {
  it("includes worktrees that are executing", () => {
    const worktrees = [
      createWorktree("starting-wt", { status: "starting" }),
      createWorktree("running-wt", { status: "running" }),
      createWorktree("awaiting-wt", {
        status: "awaiting_permission",
        feedbackState: "permission_request",
      }),
    ];

    expect(branchesOf(worktrees)).toEqual(["awaiting-wt", "starting-wt", "running-wt"]);
  });

  it("includes a worktree awaiting feedback even when it has stopped executing", () => {
    // The whole point of the ticker: something blocked on the user must stay visible
    // after the agent has gone quiet, or it disappears exactly when it needs attention.
    const worktrees = [
      createWorktree("blocked-but-idle", {
        status: "idle",
        feedbackState: "permission_request",
      }),
    ];

    expect(branchesOf(worktrees)).toEqual(["blocked-but-idle"]);
  });

  it("excludes worktrees that are neither executing nor waiting on the user", () => {
    const worktrees = [
      createWorktree("idle-wt", { status: "idle" }),
      createWorktree("stopped-wt", { status: "stopped" }),
      createWorktree("error-wt", { status: "error" }),
      createWorktree("closed-wt", { status: "closed" }),
    ];

    expect(branchesOf(worktrees)).toEqual([]);
  });

  it("excludes archived worktrees, the main checkout, and worktrees still being created", () => {
    const worktrees = [
      createWorktree("archived-wt", { archived: true }),
      createWorktree("main", { kind: "main" }),
      createWorktree("creating-wt", { creating: true, creationPhase: "starting_session" }),
    ];

    // Each would otherwise qualify on status alone, so these are real exclusions rather
    // than incidental ones.
    expect(branchesOf(worktrees)).toEqual([]);
  });

  it("excludes a worktree being created even when it also reports a feedback state", () => {
    // Nothing in the data model prevents a creating worktree from carrying a status or
    // feedback state, so the creation exclusion has to win on its own.
    const worktrees = [
      createWorktree("creating-wt", {
        creating: true,
        creationPhase: "starting_session",
        status: "awaiting_permission",
        feedbackState: "permission_request",
      }),
    ];

    expect(branchesOf(worktrees)).toEqual([]);
  });

  it("renders nothing when no worktree qualifies", () => {
    expect(deriveTickerItems([], null)).toEqual([]);
    expect(deriveTickerItems([createWorktree("idle-wt", { status: "idle" })], null)).toEqual([]);
  });
});

describe("deriveTickerItems ordering", () => {
  it("puts worktrees awaiting feedback before executing ones", () => {
    const worktrees = [
      createWorktree("running-a"),
      createWorktree("blocked-a", { feedbackState: "permission_request" }),
      createWorktree("running-b"),
      createWorktree("blocked-b", { feedbackState: "user_question" }),
    ];

    expect(branchesOf(worktrees)).toEqual(["blocked-a", "blocked-b", "running-a", "running-b"]);
  });

  it("preserves snapshot order within each group", () => {
    // The snapshot arrives branch-sorted from the server. Re-sorting inside a group would
    // make items swap places between polls for no reason the user can see.
    const worktrees = [
      createWorktree("z-running"),
      createWorktree("a-running"),
      createWorktree("z-blocked", { feedbackState: "permission_request" }),
      createWorktree("a-blocked", { feedbackState: "permission_request" }),
    ];

    expect(branchesOf(worktrees)).toEqual(["z-blocked", "a-blocked", "z-running", "a-running"]);
  });

  it("is stable across repeated derivations of the same input", () => {
    const worktrees = [
      createWorktree("running-a"),
      createWorktree("blocked-a", { feedbackState: "permission_request" }),
      createWorktree("running-b"),
    ];

    expect(branchesOf(worktrees)).toEqual(branchesOf(worktrees));
  });
});

describe("deriveTickerItems item shape", () => {
  it("names an item by its label when it has one, and its branch otherwise", () => {
    const worktrees = [
      createWorktree("feature/search", { label: "Search rewrite" }),
      createWorktree("feature/upload"),
    ];

    expect(deriveTickerItems(worktrees, null).map((item) => item.name)).toEqual([
      "Search rewrite",
      "feature/upload",
    ]);
  });

  it("falls back to the branch when a label is present but blank", () => {
    const worktrees = [createWorktree("feature/search", { label: "   " })];

    expect(deriveTickerItems(worktrees, null)[0].name).toBe("feature/search");
  });

  it("marks the selected worktree and only that one", () => {
    const worktrees = [createWorktree("running-a"), createWorktree("running-b")];

    const items = deriveTickerItems(worktrees, "running-b");

    expect(items.map((item) => item.selected)).toEqual([false, true]);
  });

  it("marks nothing as selected when the selection is not in the ticker", () => {
    const worktrees = [createWorktree("running-a")];

    expect(deriveTickerItems(worktrees, "some-other-branch").every((item) => !item.selected)).toBe(
      true,
    );
  });

  it("carries the status and feedback state through for display", () => {
    const worktrees = [
      createWorktree("blocked-wt", {
        status: "awaiting_permission",
        feedbackState: "permission_request",
      }),
    ];

    const [item] = deriveTickerItems(worktrees, null);

    expect(item.status).toBe("awaiting_permission");
    expect(item.feedbackState).toBe("permission_request");
    expect(item.needsFeedback).toBe(true);
  });

  it("exposes only display fields, never agent or session content", () => {
    // The item is what reaches the DOM. Prompts, tool output, terminal content, paths and
    // session ids must not travel with it — see the design's data-classification section.
    const worktrees = [
      createWorktree("feature/search", {
        label: "Search rewrite",
        feedbackState: "permission_request",
      }),
    ];

    const [item] = deriveTickerItems(worktrees, null);

    expect(Object.keys(item).sort()).toEqual(
      ["branch", "feedbackState", "name", "needsFeedback", "selected", "status"].sort(),
    );
  });
});

describe("deriveCrossProjectTickerItems", () => {
  const project = (
    prefix: string,
    name: string,
    worktrees: WorktreeInfo[],
  ): ActiveProjectWorktrees => ({ prefix, name, worktrees });

  it("carries each item's project identity", () => {
    const items = deriveCrossProjectTickerItems(
      [project("alpha", "Alpha", [createWorktree("feat-a")])],
      "alpha",
      null,
    );

    expect(items).toHaveLength(1);
    expect(items[0].projectPrefix).toBe("alpha");
    expect(items[0].projectName).toBe("Alpha");
  });

  it("keeps the same branch in two projects as two distinct items", () => {
    // Branch alone stops being a unique key across projects, so anything keying on it
    // would collapse these into one row and lose a running worktree.
    const items = deriveCrossProjectTickerItems(
      [
        project("alpha", "Alpha", [createWorktree("main-work")]),
        project("beta", "Beta", [createWorktree("main-work")]),
      ],
      "alpha",
      null,
    );

    expect(items).toHaveLength(2);
    expect(items.map((i) => i.key)).toEqual(["alpha/main-work", "beta/main-work"]);
    expect(new Set(items.map((i) => i.key)).size).toBe(2);
  });

  it("marks items from other projects as foreign and the active project's as not", () => {
    const items = deriveCrossProjectTickerItems(
      [
        project("alpha", "Alpha", [createWorktree("feat-a")]),
        project("beta", "Beta", [createWorktree("feat-b")]),
      ],
      "alpha",
      null,
    );

    expect(items.map((i) => [i.projectPrefix, i.foreign])).toEqual([
      ["alpha", false],
      ["beta", true],
    ]);
  });

  it("orders feedback-needed items first across every project", () => {
    // A worktree waiting on the user matters more than which project it lives in.
    const items = deriveCrossProjectTickerItems(
      [
        project("alpha", "Alpha", [
          createWorktree("alpha-running"),
          createWorktree("alpha-blocked", { feedbackState: "permission_request" }),
        ]),
        project("beta", "Beta", [
          createWorktree("beta-running"),
          createWorktree("beta-blocked", { feedbackState: "permission_request" }),
        ]),
      ],
      "alpha",
      null,
    );

    expect(items.map((i) => i.branch)).toEqual([
      "alpha-blocked",
      "beta-blocked",
      "alpha-running",
      "beta-running",
    ]);
  });

  it("preserves project order within each group", () => {
    const items = deriveCrossProjectTickerItems(
      [
        project("zulu", "Zulu", [createWorktree("z1")]),
        project("alpha", "Alpha", [createWorktree("a1")]),
      ],
      "zulu",
      null,
    );

    // Registry order, not alphabetical — matching what the endpoint returns.
    expect(items.map((i) => i.projectPrefix)).toEqual(["zulu", "alpha"]);
  });

  it("only marks a selection inside the active project", () => {
    // The same branch name in another project must not appear selected just because the
    // active project has a worktree by that name.
    const items = deriveCrossProjectTickerItems(
      [
        project("alpha", "Alpha", [createWorktree("shared")]),
        project("beta", "Beta", [createWorktree("shared")]),
      ],
      "alpha",
      "shared",
    );

    expect(items.map((i) => [i.projectPrefix, i.selected])).toEqual([
      ["alpha", true],
      ["beta", false],
    ]);
  });

  it("applies the same eligibility rules as the single-project derivation", () => {
    const items = deriveCrossProjectTickerItems(
      [
        project("alpha", "Alpha", [
          createWorktree("idle-wt", { status: "idle" }),
          createWorktree("archived-wt", { archived: true }),
          createWorktree("main", { kind: "main" }),
          createWorktree("running-wt"),
        ]),
      ],
      "alpha",
      null,
    );

    expect(items.map((i) => i.branch)).toEqual(["running-wt"]);
  });

  it("skips projects with nothing running", () => {
    // The endpoint reports a loaded-but-quiet project as an empty list; it must not
    // produce an empty group or a placeholder item.
    const items = deriveCrossProjectTickerItems(
      [
        project("quiet", "Quiet", [createWorktree("idle-wt", { status: "idle" })]),
        project("empty", "Empty", []),
        project("busy", "Busy", [createWorktree("running-wt")]),
      ],
      "busy",
      null,
    );

    expect(items.map((i) => i.projectPrefix)).toEqual(["busy"]);
  });

  it("returns nothing when no project has qualifying work", () => {
    expect(deriveCrossProjectTickerItems([], "alpha", null)).toEqual([]);
    expect(
      deriveCrossProjectTickerItems([project("alpha", "Alpha", [])], "alpha", null),
    ).toEqual([]);
  });
});
