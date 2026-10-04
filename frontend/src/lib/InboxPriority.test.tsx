import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import PriorityControl, { PriorityBadge } from "./InboxPriority";

describe("PriorityBadge", () => {
  it("says who set the priority", () => {
    const { rerender } = render(<PriorityBadge priority="P1" source="agent" />);
    expect(screen.getByText("P1")).toHaveAttribute(
      "title",
      expect.stringMatching(/system agent/i),
    );
    rerender(<PriorityBadge priority="P0" source="operator" />);
    expect(screen.getByText("P0")).toHaveAttribute(
      "title",
      expect.stringMatching(/operator/i),
    );
  });
});

describe("PriorityControl", () => {
  it("sets an override", async () => {
    const user = userEvent.setup();
    const onchange = vi.fn();
    render(<PriorityControl priority="P2" source="agent" onchange={onchange} />);
    expect(screen.getByLabelText("Priority")).toHaveValue("P2");
    expect(screen.getByText(/set by agent/i)).toBeInTheDocument();
    await user.selectOptions(screen.getByLabelText("Priority"), "P0");
    expect(onchange).toHaveBeenCalledWith("P0");
  });

  it("offers to clear only an operator override", async () => {
    const user = userEvent.setup();
    const onchange = vi.fn();
    const { rerender } = render(
      <PriorityControl priority="P2" source="agent" onchange={onchange} />,
    );
    expect(screen.queryByRole("button", { name: /clear override/i })).toBeNull();

    rerender(<PriorityControl priority="P1" source="operator" onchange={onchange} />);
    expect(screen.getByText(/operator override/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /clear override/i }));
    expect(onchange).toHaveBeenCalledWith(null);
  });
});
