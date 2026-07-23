import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import TopBar from "./TopBar";
import { useStore } from "../store";
import type { WorktreeInfo } from "./types";

function createWorktree(
  branch: string,
  overrides: Partial<WorktreeInfo> = {},
): WorktreeInfo {
  return {
    branch,
    label: null,
    archived: false,
    agent: "claude",
    mux: "✓",
    path: `/tmp/${branch}`,
    dir: `/tmp/${branch}`,
    dirty: false,
    unpushed: false,
    status: "running",
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

function commonProps() {
  return {
    linkedRepos: [],
    onclose: vi.fn(),
    onarchive: vi.fn(),
    onmerge: vi.fn(),
    onremove: vi.fn(),
    onsettings: vi.fn(),
    onCiClick: vi.fn(),
    onReviewsClick: vi.fn(),
  };
}

function renderTopBar(branch: string, overrides: Partial<WorktreeInfo> = {}) {
  return render(<TopBar name={branch} worktree={createWorktree(branch, overrides)} {...commonProps()} />);
}

describe("TopBar", () => {
  beforeEach(() => {
    // `sshHost`, `notificationHistory`, and `unreadCount` are now read from the store.
    useStore.setState({ sshHost: "", notificationHistory: [], unreadCount: 0 });
  });

  afterEach(() => {
    cleanup();
  });

  it("truncates worktree names longer than 30 characters in the header", () => {
    const branch = "feature/abcdefghijklmnopqrstuvwxyz-1234567890";

    renderTopBar(branch);

    const truncated = `${branch.slice(0, 27)}...`;
    const header = screen.getByText(truncated);

    expect(truncated).toHaveLength(30);
    expect(header).toHaveAttribute("title", branch);
  });

  it("shows short worktree names without truncation", () => {
    const branch = "feature/short-name";

    renderTopBar(branch);

    const header = screen.getByText(branch);

    expect(header).toHaveAttribute("title", branch);
  });

  it("shows workspace labels above the real branch name", () => {
    const branch = "feature/random-fallback";

    render(
      <TopBar
        name={branch}
        worktree={createWorktree(branch, { label: "Search ranking" })}
        {...commonProps()}
        oneditlabel={vi.fn()}
      />,
    );

    expect(screen.getByText("Search ranking")).toBeInTheDocument();
    expect(screen.getByText(branch)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Edit workspace label" })).toBeInTheDocument();
  });

  it("does not render stale terminal state in the web top bar", () => {
    const branch = "feature/stale-terminal";

    render(
      <TopBar
        name={branch}
        worktree={createWorktree(branch, { agentTerminalStale: true })}
        {...commonProps()}
      />,
    );

    expect(screen.queryByText("Terminal stale")).not.toBeInTheDocument();
  });

  it("keeps desktop PR badges inside a wrapping header container", () => {
    const branch = "feature/header-wrap";
    const { container } = renderTopBar(branch, {
      prs: [
        {
          repo: "origin",
          number: 42,
          state: "open",
          url: "https://github.com/example/repo/pull/42",
          updatedAt: "2026-03-23T12:00:00.000Z",
          ciStatus: "success",
          ciChecks: [],
          comments: [],
        },
      ],
    });

    const badgeContainer = container.querySelector(".topbar-main-prs");
    const repoGroup = badgeContainer?.querySelector(".repo-group");

    expect(badgeContainer).not.toBeNull();
    expect(badgeContainer?.className).toContain("flex-1");
    expect(repoGroup).not.toBeNull();
    expect(repoGroup?.className).toContain("flex-wrap");
  });
});
