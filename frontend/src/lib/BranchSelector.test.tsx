import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import BranchSelector from "./BranchSelector";

const BRANCHES = [
  { name: "main" },
  { name: "release/base" },
];

describe("BranchSelector", () => {
  afterEach(() => {
    cleanup();
  });

  it("auto-focuses the search input each time it is reopened after escape", async () => {
    render(
      <BranchSelector
        label="Existing branch"
        branches={BRANCHES}
        initialOpen
        onselect={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(screen.getByLabelText("Existing branch search")).toHaveFocus();
    });

    fireEvent.keyDown(screen.getByLabelText("Existing branch search"), {
      key: "Escape",
    });

    await waitFor(() => {
      expect(screen.queryByLabelText("Existing branch search")).not.toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Existing branch" }));

    await waitFor(() => {
      expect(screen.getByLabelText("Existing branch search")).toHaveFocus();
    });
  });

  it("auto-focuses the search input each time it is reopened after focus leaves the selector", async () => {
    render(<BranchSelector label="Base branch" branches={BRANCHES} onselect={vi.fn()} />);

    const trigger = screen.getByRole("button", { name: "Base branch" });
    fireEvent.click(trigger);

    await waitFor(() => {
      expect(screen.getByLabelText("Base branch search")).toHaveFocus();
    });

    fireEvent.focusOut(screen.getByLabelText("Base branch search"), {
      relatedTarget: document.body,
    });

    await waitFor(() => {
      expect(screen.queryByLabelText("Base branch search")).not.toBeInTheDocument();
    });

    fireEvent.click(trigger);

    await waitFor(() => {
      expect(screen.getByLabelText("Base branch search")).toHaveFocus();
    });
  });

  it("keeps the selector open when the inline toggle row is clicked", async () => {
    const onInlineToggle = vi.fn();

    render(
      <BranchSelector
        label="Existing branch"
        branches={BRANCHES}
        initialOpen
        inlineToggleLabel="include remote"
        inlineToggleChecked={false}
        oninlinetoggle={onInlineToggle}
        onselect={vi.fn()}
      />,
    );

    const search = await screen.findByLabelText("Existing branch search");
    const availabilityRow = screen.getByText(/2 available/).parentElement as HTMLElement;

    fireEvent.mouseDown(availabilityRow);
    fireEvent.click(availabilityRow);

    expect(onInlineToggle).not.toHaveBeenCalled();
    expect(search).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Existing branch" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
  });

  it("keeps rendering the current branch list while a refresh is in flight", async () => {
    render(
      <BranchSelector
        label="Existing branch"
        branches={BRANCHES}
        loading
        initialOpen
        onselect={vi.fn()}
      />,
    );

    expect(await screen.findByRole("button", { name: "main" })).toBeInTheDocument();
    expect(screen.getByText("Updating...")).toBeInTheDocument();
    expect(screen.queryByText("Loading branches...")).not.toBeInTheDocument();
  });
});
