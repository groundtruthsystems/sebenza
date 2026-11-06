import { afterEach, describe, expect, it, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import NavRail, { INBOX_SUMMARY_POLL_MS } from "./NavRail";
import { fetchInboxSummary } from "./api";

vi.mock("./api", () => ({
  fetchInboxSummary: vi.fn(async () => ({ openRequests: 0 })),
}));

const summary = vi.mocked(fetchInboxSummary);

describe("NavRail", () => {
  it("marks the active destination", () => {
    render(<NavRail active="inbox" projectBase="/demo" />);
    expect(screen.getByLabelText("Inbox")).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(screen.getByLabelText("Registry")).not.toHaveAttribute(
      "aria-current",
    );
  });

  it("returns to the terminal view in place when the dashboard owns it", async () => {
    const onSelectView = vi.fn();
    const user = userEvent.setup();
    render(<NavRail active="worktrees" onSelectView={onSelectView} />);

    await user.click(screen.getByLabelText("Worktrees"));
    expect(onSelectView).toHaveBeenCalledWith("terminal");
  });

  it("navigates into the project when it cannot flip view state", () => {
    render(<NavRail active="inbox" projectBase="/demo" />);
    expect(screen.getByLabelText("Worktrees")).toHaveAttribute("href", "/demo/");
  });

  it("hides worktrees rather than linking nowhere", () => {
    // With no project registered there is nothing to switch to; a dead link
    // would be worse than an absent one.
    render(<NavRail active="inbox" projectBase="" />);
    expect(screen.queryByLabelText("Worktrees")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Inbox")).toBeInTheDocument();
    expect(screen.getByLabelText("Registry")).toBeInTheDocument();
  });

  it("leads with the inbox", () => {
    render(<NavRail active="inbox" projectBase="/demo" />);
    const labels = [...document.querySelectorAll(".nav-rail-btn")].map((b) =>
      b.getAttribute("aria-label"),
    );
    expect(labels).toEqual(["Inbox", "Worktrees", "Registry"]);
  });

  it("offers no tracks destination — tracks is a view of a worktree", () => {
    render(<NavRail active="worktrees" projectBase="/demo" />);
    expect(screen.queryByLabelText("Tracks")).not.toBeInTheDocument();
  });
});

describe("NavRail inbox badge", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  // Nervous-system bug TS-08: the Inbox icon shows how many requests are
  // waiting on the operator, and hides the badge once none are.
  it("shows open requests and clears the badge when none remain", async () => {
    vi.useFakeTimers();
    summary
      .mockResolvedValueOnce({ openRequests: 3 })
      .mockResolvedValueOnce({ openRequests: 0 });

    render(<NavRail active="worktrees" />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(screen.getByTestId("inbox-badge")).toHaveTextContent("3");
    expect(screen.getByLabelText("3 open requests")).toBeInTheDocument();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(INBOX_SUMMARY_POLL_MS);
    });
    expect(screen.queryByTestId("inbox-badge")).toBeNull();
  });
});
