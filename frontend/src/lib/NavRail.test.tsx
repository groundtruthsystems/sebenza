import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import NavRail from "./NavRail";

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
