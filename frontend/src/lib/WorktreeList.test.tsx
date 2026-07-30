import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import WorktreeList from "./WorktreeList";
import { useStore } from "../store";
import type { WorktreeInfo, WorktreeListRow } from "./types";

function createWorktree(branch: string, overrides: Partial<WorktreeInfo> = {}): WorktreeInfo {
  return {
    branch,
    kind: "linked",
    label: null,
    archived: false,
    agent: "claude",
    mux: "✓",
    path: `/tmp/${branch}`,
    dir: `/tmp/${branch}`,
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

function createRow(worktree: WorktreeInfo, depth = 0): WorktreeListRow {
  return { worktree, depth };
}

function baseProps() {
  return {
    removing: new Set<string>(),
    initializing: new Set<string>(),
    archiving: new Set<string>(),
    notifiedBranches: new Set<string>(),
    onselect: vi.fn(),
    onclose: vi.fn(),
    onarchive: vi.fn(),
    onmerge: vi.fn(),
    oncreatesubworktree: vi.fn(),
    onremove: vi.fn(),
    onpull: vi.fn(),
  };
}

describe("WorktreeList", () => {
  beforeEach(() => {
    // `selectedBranch` now lives in the store instead of a prop.
    useStore.setState({ selectedBranch: null });
  });

  afterEach(() => {
    cleanup();
  });

  it("calls onremove without selecting the row when the remove button is clicked", () => {
    const onselect = vi.fn();
    const onremove = vi.fn();

    const { container } = render(
      <WorktreeList
        {...baseProps()}
        rows={[createRow(createWorktree("feature/list-actions"))]}
        onselect={onselect}
        onremove={onremove}
      />,
    );

    fireEvent.click(within(container).getByRole("button", { name: /actions for feature\/list-actions/i }));
    fireEvent.click(within(container).getByRole("button", { name: "Remove" }));

    expect(onremove).toHaveBeenCalledWith("feature/list-actions");
    expect(onselect).not.toHaveBeenCalled();
  });

  it("disables row actions while a worktree is being removed", () => {
    const { container } = render(
      <WorktreeList
        {...baseProps()}
        rows={[createRow(createWorktree("feature/list-removing"))]}
        removing={new Set(["feature/list-removing"])}
      />,
    );

    expect(screen.getByText("feature/list-removing").closest("button")).toBeDisabled();
    expect(within(container).getByRole("button", { name: /actions for feature\/list-removing/i })).toBeDisabled();
  });

  it("shows a three-dot menu with row actions", () => {
    const onarchive = vi.fn();

    render(
      <WorktreeList
        {...baseProps()}
        rows={[createRow(createWorktree("feature/menu-actions"))]}
        onarchive={onarchive}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /actions for feature\/menu-actions/i }));

    expect(screen.getByRole("button", { name: "Close" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Archive" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Merge" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Remove" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    expect(onarchive).toHaveBeenCalledWith("feature/menu-actions");
  });

  it("calls oncreatesubworktree with the row branch from the menu", () => {
    const oncreatesubworktree = vi.fn();

    render(
      <WorktreeList
        {...baseProps()}
        rows={[createRow(createWorktree("feature/sub-base"))]}
        oncreatesubworktree={oncreatesubworktree}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /actions for feature\/sub-base/i }));
    fireEvent.click(screen.getByRole("button", { name: "Create sub-worktree" }));

    expect(oncreatesubworktree).toHaveBeenCalledWith("feature/sub-base");
  });

  it("renders labels as the primary row name with the branch below", () => {
    render(
      <WorktreeList
        {...baseProps()}
        rows={[createRow({ ...createWorktree("feature/random-fallback"), label: "Search ranking" })]}
      />,
    );

    expect(screen.getByText("Search ranking")).toBeInTheDocument();
    expect(screen.getByText("feature/random-fallback")).toBeInTheDocument();
  });

  it("places archived and closed row badges below the worktree name", () => {
    render(
      <WorktreeList
        {...baseProps()}
        rows={[
          createRow({
            ...createWorktree("feature/very-long-archived-closed-name"),
            archived: true,
            mux: "",
          }),
        ]}
      />,
    );

    const name = screen.getByText("feature/very-long-archived-closed-name");
    const archived = screen.getByText("archived");
    const closed = screen.getByText("closed");
    const nameRow = name.closest("[data-worktree-name-row]");
    const badgeRow = archived.closest("[data-worktree-badge-row]");

    if (!nameRow || !badgeRow) {
      throw new Error("Expected separate name and badge rows");
    }

    expect(nameRow).not.toContainElement(archived);
    expect(badgeRow).toContainElement(archived);
    expect(badgeRow).toContainElement(closed);
  });

  it("disables the archive action while the row is archiving", () => {
    render(
      <WorktreeList
        {...baseProps()}
        rows={[createRow(createWorktree("feature/archiving"))]}
        archiving={new Set<string>(["feature/archiving"])}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /actions for feature\/archiving/i }));

    expect(screen.getByRole("button", { name: "Archive" })).toBeDisabled();
  });
  it("renders a repo badge on the main row", () => {
    render(
      <WorktreeList
        {...baseProps()}
        rows={[createRow(createWorktree("main", { kind: "main" }))]}
      />,
    );
    expect(screen.getByText("repo")).toBeInTheDocument();
  });

  it("does not render a repo badge on a linked row", () => {
    render(<WorktreeList {...baseProps()} rows={[createRow(createWorktree("feature/x"))]} />);
    expect(screen.queryByText("repo")).toBeNull();
  });

  it("selects the main row like any other row", () => {
    const props = baseProps();
    render(
      <WorktreeList {...props} rows={[createRow(createWorktree("main", { kind: "main" }))]} />,
    );
    fireEvent.click(screen.getByText("main"));
    expect(props.onselect).toHaveBeenCalledWith("main");
  });

  it("offers Pull on the main row and routes it to onpull", () => {
    const props = baseProps();
    render(
      <WorktreeList {...props} rows={[createRow(createWorktree("main", { kind: "main" }))]} />,
    );
    fireEvent.click(screen.getByLabelText("Actions for main"));
    fireEvent.click(screen.getByRole("button", { name: "Pull" }));
    expect(props.onpull).toHaveBeenCalledWith("main");
  });

  it("still offers Close on an open main row", () => {
    const props = baseProps();
    render(
      <WorktreeList
        {...props}
        rows={[createRow(createWorktree("main", { kind: "main", mux: "\u2713" }))]}
      />,
    );
    fireEvent.click(screen.getByLabelText("Actions for main"));
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(props.onclose).toHaveBeenCalledWith("main");
  });
});
