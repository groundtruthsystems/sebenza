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

  it("flips view state in place when the dashboard owns it", async () => {
    const onSelectView = vi.fn();
    const user = userEvent.setup();
    render(<NavRail active="worktrees" onSelectView={onSelectView} />);

    await user.click(screen.getByLabelText("Tracks"));
    expect(onSelectView).toHaveBeenCalledWith("tracks");

    await user.click(screen.getByLabelText("Worktrees"));
    expect(onSelectView).toHaveBeenCalledWith("terminal");
  });

  it("navigates into the project when it cannot flip view state", () => {
    render(<NavRail active="inbox" projectBase="/demo" />);
    expect(screen.getByLabelText("Tracks")).toHaveAttribute(
      "href",
      "/demo/?view=tracks",
    );
    expect(screen.getByLabelText("Worktrees")).toHaveAttribute(
      "href",
      "/demo/?view=terminal",
    );
  });

  it("hides project destinations rather than linking nowhere", () => {
    // With no project registered there is nothing to switch to; a dead link
    // would be worse than an absent one.
    render(<NavRail active="inbox" projectBase="" />);
    expect(screen.queryByLabelText("Tracks")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Worktrees")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Inbox")).toBeInTheDocument();
    expect(screen.getByLabelText("Registry")).toBeInTheDocument();
  });

  it("always offers the inbox and the registry", () => {
    render(<NavRail active="tracks" projectBase="/demo" />);
    expect(screen.getByLabelText("Inbox")).toHaveAttribute("href", "/inbox");
    expect(screen.getByLabelText("Registry")).toHaveAttribute(
      "href",
      "/registry",
    );
  });
});
