import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ActiveWorktreeTicker from "./ActiveWorktreeTicker";
import type { TickerItem } from "./ticker";

function item(branch: string, overrides: Partial<TickerItem> = {}): TickerItem {
  return {
    branch,
    name: branch,
    status: "running",
    feedbackState: "none",
    needsFeedback: false,
    selected: false,
    ...overrides,
  };
}

afterEach(cleanup);

describe("ActiveWorktreeTicker", () => {
  it("renders nothing at all when no worktree qualifies", () => {
    // Not an empty bar and not a reserved strip of height — the workspace below must be
    // exactly as tall as it was before the ticker existed.
    const { container } = render(<ActiveWorktreeTicker items={[]} onselect={() => {}} />);

    expect(container).toBeEmptyDOMElement();
  });

  it("is a labelled navigation region containing one button per worktree", () => {
    render(
      <ActiveWorktreeTicker
        items={[item("feature/search"), item("feature/upload")]}
        onselect={() => {}}
      />,
    );

    const region = screen.getByRole("navigation", { name: /active worktrees/i });
    expect(within(region).getAllByRole("button")).toHaveLength(2);
  });

  it("shows the item name rather than the raw branch when they differ", () => {
    render(
      <ActiveWorktreeTicker
        items={[item("feature/search", { name: "Search rewrite" })]}
        onselect={() => {}}
      />,
    );

    expect(screen.getByRole("button", { name: /Search rewrite/ })).toBeTruthy();
  });

  it("states in text that a worktree needs a response, not only in colour", () => {
    // A colour-only signal is invisible to a meaningful share of users and to anyone
    // reading the accessibility tree, which is the one thing the ticker exists to convey.
    render(
      <ActiveWorktreeTicker
        items={[
          item("blocked-wt", {
            needsFeedback: true,
            feedbackState: "permission_request",
            status: "awaiting_permission",
          }),
        ]}
        onselect={() => {}}
      />,
    );

    const button = screen.getByRole("button", { name: /blocked-wt/ });
    expect(button.textContent).toMatch(/needs approval/i);
  });

  it("distinguishes a question from a permission request in its text", () => {
    render(
      <ActiveWorktreeTicker
        items={[item("asked-wt", { needsFeedback: true, feedbackState: "user_question" })]}
        onselect={() => {}}
      />,
    );

    expect(screen.getByRole("button", { name: /asked-wt/ }).textContent).toMatch(/needs an answer/i);
  });

  it("exposes the selected worktree to assistive technology", () => {
    render(
      <ActiveWorktreeTicker
        items={[item("running-a"), item("running-b", { selected: true })]}
        onselect={() => {}}
      />,
    );

    expect(screen.getByRole("button", { name: /running-a/ }).getAttribute("aria-current")).toBeNull();
    expect(screen.getByRole("button", { name: /running-b/ }).getAttribute("aria-current")).toBe(
      "true",
    );
  });

  it("calls onselect once with the branch when an item is clicked", () => {
    const onselect = vi.fn();
    render(
      <ActiveWorktreeTicker
        items={[item("feature/search", { name: "Search rewrite" })]}
        onselect={onselect}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /Search rewrite/ }));

    // The branch, not the display name: selection is keyed on branch everywhere else.
    expect(onselect).toHaveBeenCalledTimes(1);
    expect(onselect).toHaveBeenCalledWith("feature/search");
  });

  it("renders a branch name as text rather than markup", () => {
    // Branch names are untrusted local metadata. The ticker gives them more prominence
    // than the sidebar does, so pin the escaping here.
    const hostile = "feature/<img src=x onerror=alert(1)>";
    const { container } = render(
      <ActiveWorktreeTicker items={[item(hostile, { name: hostile })]} onselect={() => {}} />,
    );

    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByRole("button", { name: new RegExp("onerror") })).toBeTruthy();
  });

  it("scrolls horizontally rather than animating on its own", () => {
    // No marquee: items must not move unless the user moves them.
    const { container } = render(
      <ActiveWorktreeTicker
        items={[item("a"), item("b"), item("c")]}
        onselect={() => {}}
      />,
    );

    const scroller = container.querySelector("[data-ticker-scroll]");
    expect(scroller).not.toBeNull();
    expect(scroller?.className).toMatch(/overflow-x-auto/);
    expect(container.innerHTML).not.toMatch(/animate-/);
  });
});
