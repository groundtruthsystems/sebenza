import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DraftHelpPanel, DraftMergeView } from "./DraftHelp";

function panel(over: Partial<Parameters<typeof DraftHelpPanel>[0]> = {}) {
  const props = {
    current: "old line",
    pending: false,
    error: "",
    proposal: null,
    onrequest: vi.fn(),
    onaccept: vi.fn(),
    ondiscard: vi.fn(),
    onclose: vi.fn(),
    ...over,
  };
  render(<DraftHelpPanel {...props} />);
  return props;
}

describe("DraftHelpPanel", () => {
  it("asks for help with an optional instruction", async () => {
    const user = userEvent.setup();
    const { onrequest } = panel();
    await user.type(screen.getByLabelText("Draft help instruction"), "  tighten it ");
    await user.click(screen.getByRole("button", { name: "Ask agent" }));
    expect(onrequest).toHaveBeenCalledWith("tighten it");
  });

  it("cannot ask twice while a job runs", () => {
    panel({ pending: true });
    expect(screen.getByRole("button", { name: /drafting/i })).toBeDisabled();
  });

  it("shows the proposal, its summary and what it changes", async () => {
    const user = userEvent.setup();
    const { onaccept, ondiscard } = panel({
      proposal: { jobKind: "draft_help", proposed_body: "new line", summary: "Reworded." },
    });
    const region = screen.getByRole("region", { name: "Draft help proposal" });
    expect(within(region).getByText("Reworded.")).toBeInTheDocument();
    expect(within(region).getByText("old line")).toHaveAttribute("data-op", "del");
    expect(within(region).getByText("new line")).toHaveAttribute("data-op", "add");
    await user.click(within(region).getByRole("button", { name: "Accept" }));
    expect(onaccept).toHaveBeenCalled();
    await user.click(within(region).getByRole("button", { name: "Discard" }));
    expect(ondiscard).toHaveBeenCalled();
  });

  it("shows a failure", () => {
    panel({ error: "system agent is disabled" });
    expect(screen.getByText("system agent is disabled")).toBeInTheDocument();
  });

  it("warns about PHI and the model provider", () => {
    panel();
    expect(screen.getByRole("note")).toHaveTextContent(/PHI.*model provider/i);
  });
});

describe("DraftMergeView", () => {
  function merge() {
    const props = {
      theirs: "disk version",
      theirsLabel: "On disk",
      mine: "proposed version",
      saving: false,
      onsave: vi.fn(),
      oncancel: vi.fn(),
    };
    render(<DraftMergeView {...props} />);
    return props;
  }

  // TS-04: both versions shown; nothing saved until the operator resolves.
  it("shows both versions and saves only the resolved text", async () => {
    const user = userEvent.setup();
    const { onsave } = merge();
    expect(screen.getByLabelText("On disk")).toHaveTextContent("disk version");
    expect(screen.getByLabelText("Proposed")).toHaveTextContent("proposed version");
    expect(onsave).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Use on disk" }));
    expect(screen.getByLabelText("Merged body")).toHaveValue("disk version");
    await user.type(screen.getByLabelText("Merged body"), " + mine");
    expect(onsave).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Save merged" }));
    expect(onsave).toHaveBeenCalledWith("disk version + mine");
  });

  it("can take the proposal whole, or back out", async () => {
    const user = userEvent.setup();
    const { onsave, oncancel } = merge();
    await user.click(screen.getByRole("button", { name: "Use on disk" }));
    await user.click(screen.getByRole("button", { name: "Use proposed" }));
    expect(screen.getByLabelText("Merged body")).toHaveValue("proposed version");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(oncancel).toHaveBeenCalled();
    expect(onsave).not.toHaveBeenCalled();
  });
});
