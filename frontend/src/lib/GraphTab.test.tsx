import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, within } from "@testing-library/react";
import GraphTab, { GRAPH_POLL_MS } from "./GraphTab";
import { fetchInboxGraph, type TaskGraph } from "./api";
import { renderMermaidSvg } from "./inboxMarkdown";

vi.mock("./api", () => ({ fetchInboxGraph: vi.fn() }));
vi.mock("./inboxMarkdown", () => ({
  renderMermaidSvg: vi.fn(async () => "<svg data-testid='graph-svg'></svg>"),
}));

const fetchGraph = vi.mocked(fetchInboxGraph);
const renderSvg = vi.mocked(renderMermaidSvg);

const G = (etag = "e1"): TaskGraph => ({
  draftId: "01D",
  generatedAt: "2026-11-05T10:00:00Z",
  etag,
  truncated: false,
  progress: { done: 1, total: 2 },
  failed: 1,
  unavailable: 1,
  worktrees: [
    {
      id: "w:0", branch: "feat-x", projectPath: "/code/acme", state: "ok", errorCode: null,
      progress: { done: 1, total: 2 },
      agent: { status: "running", feedbackState: "permission_request", agent: "claude" },
      tracks: [{
        id: "t:0/t", trackId: "t", description: "Graph view", status: "doing",
        currentPhase: "phase-1", designing: false, errorCode: null,
        progress: { done: 1, total: 2 },
        phases: [{
          id: "p:0/t/phase-1", phaseId: "phase-1", name: "Build", status: "doing",
          progress: { done: 1, total: 2 },
          tasks: [
            { id: "k:0/t/a", taskId: "a", name: "Write tests", status: "done", dependsOn: [], request: null },
            { id: "k:0/t/b", taskId: "b", name: "Implement", status: "blocked", dependsOn: ["a"],
              request: { requestId: "01R", status: "open", source: "inbox" } },
          ],
        }],
      }],
    },
    { id: "w:1", branch: "feat-y", projectPath: "/code/beta", state: "unavailable", errorCode: "missing",
      progress: { done: 0, total: 0 }, agent: null, tracks: [] },
    { id: "w:2", branch: "feat-z", projectPath: "/code/beta", state: "failed", errorCode: "conversion_failed",
      progress: { done: 0, total: 0 }, agent: null, tracks: [] },
  ],
  edges: [],
});

let visibility: DocumentVisibilityState = "visible";
beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers();
  visibility = "visible";
  vi.spyOn(document, "visibilityState", "get").mockImplementation(() => visibility);
  fetchGraph.mockResolvedValue({ kind: "graph", graph: G() });
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const flush = () => act(async () => { await vi.advanceTimersByTimeAsync(0); });
const tick = () => act(async () => { await vi.advanceTimersByTimeAsync(GRAPH_POLL_MS); });

describe("GraphTab", () => {
  // TS-02 / TS-05 / TS-76: a card per worktree with project, progress, state
  // and a waiting-on-you badge.
  it("shows each worktree with project, progress, state and agent badges", async () => {
    render(<GraphTab draftId="01D" />);
    await flush();
    const x = screen.getByRole("region", { name: "Worktree feat-x" });
    expect(within(x).getByText("acme")).toBeInTheDocument();
    expect(within(x).getByText("1/2 tasks")).toBeInTheDocument();
    expect(within(x).getByText("waiting on you")).toBeInTheDocument();
    expect(within(screen.getByRole("region", { name: "Worktree feat-y" })).getByText("Unavailable")).toBeInTheDocument();
    expect(within(screen.getByRole("region", { name: "Worktree feat-z" })).getByText("Failed")).toBeInTheDocument();
    expect(screen.getByText(/1\/2 tasks across 3 worktrees/)).toBeInTheDocument();
    expect(screen.getByTestId("graph-svg")).toBeInTheDocument();
  });

  // TS-07: drilling into a worktree lists its tracks, phases and tasks.
  it("drills into a worktree's tracks, phases and tasks", async () => {
    render(<GraphTab draftId="01D" />);
    await flush();
    const x = screen.getByRole("region", { name: "Worktree feat-x" });
    await act(async () => within(x).getByRole("button", { name: "Show tracks" }).click());
    expect(within(x).getByText("Graph view")).toBeInTheDocument();
    expect(within(x).getByText(/phase-1/)).toBeInTheDocument();
    expect(within(x).getByText("Implement")).toBeInTheDocument();
    expect(within(x).getByText("request open")).toBeInTheDocument();
  });

  // TS-59 / TS-14: fetched on open and on Refresh; polled only while visible.
  it("fetches on open and refresh, and polls only while visible", async () => {
    render(<GraphTab draftId="01D" />);
    await flush();
    expect(fetchGraph).toHaveBeenCalledTimes(1);
    await act(async () => screen.getByRole("button", { name: "Refresh" }).click());
    await flush();
    expect(fetchGraph).toHaveBeenCalledTimes(2);
    await tick();
    expect(fetchGraph).toHaveBeenCalledTimes(3);
    visibility = "hidden";
    await tick();
    expect(fetchGraph).toHaveBeenCalledTimes(3);
  });

  // TS-60: a tick while a fetch is pending does not start another.
  it("keeps one request in flight", async () => {
    render(<GraphTab draftId="01D" />);
    await flush();
    fetchGraph.mockReturnValue(new Promise(() => {}));
    await tick();
    await tick();
    expect(fetchGraph).toHaveBeenCalledTimes(2);
  });

  // TS-15: a failed refresh keeps the last graph, marked stale; an unchanged
  // graph is not re-rendered.
  it("keeps the last graph on failure and skips unchanged renders", async () => {
    render(<GraphTab draftId="01D" />);
    await flush();
    expect(renderSvg).toHaveBeenCalledTimes(1);
    fetchGraph.mockResolvedValueOnce({ kind: "unchanged" });
    await tick();
    expect(renderSvg).toHaveBeenCalledTimes(1);
    expect(fetchGraph).toHaveBeenLastCalledWith("01D", "e1", expect.any(AbortSignal));
    fetchGraph.mockRejectedValueOnce(new Error("HTTP 500"));
    await tick();
    expect(screen.getByRole("status")).toHaveTextContent(/stale/i);
    expect(screen.getByRole("region", { name: "Worktree feat-x" })).toBeInTheDocument();
  });

  // TS-16: leaving the tab aborts the request in flight.
  it("aborts the in-flight request on unmount", async () => {
    let signal: AbortSignal | undefined;
    fetchGraph.mockImplementation((_id, _etag, s) => {
      signal = s;
      return new Promise(() => {});
    });
    const { unmount } = render(<GraphTab draftId="01D" />);
    await flush();
    unmount();
    expect(signal?.aborted).toBe(true);
  });
});
