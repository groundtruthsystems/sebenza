import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import AgentPicker, { refusal } from "./AgentPicker";
import type { FactoryAgent } from "./solutionApi";

const agents: FactoryAgent[] = [
  { agent: "claude", installed: true, enforcement: "verified", canReview: true },
  { agent: "codex", installed: false, enforcement: "verified", canReview: true },
  { agent: "grok", installed: true, enforcement: "unverified", canReview: false },
];

afterEach(cleanup);

describe("AgentPicker", () => {
  // TS-03: every agent is listed; an uninstalled one is disabled.
  it("lists Claude, Codex and Grok and disables what is not installed", () => {
    render(<AgentPicker label="Coordinator" agents={agents} value={null} onChange={vi.fn()} />);
    const select = screen.getByLabelText("Coordinator agent");
    const options = within(select).getAllByRole("option");
    expect(options.map((o) => o.textContent)).toEqual([
      "Default",
      "Claude",
      "Codex",
      "Grok (read-only unverified)",
    ]);
    const codex = options.find((o) => o.textContent === "Codex") as HTMLOptionElement;
    expect(codex.disabled).toBe(true);
    expect(codex.title).toMatch(/not installed/);
    const grok = options.find((o) => o.textContent?.startsWith("Grok")) as HTMLOptionElement;
    expect(grok.disabled).toBe(false);
  });

  // TS-56: grok carries the badge, and as a reviewer it is refused with a reason.
  it("badges grok and refuses it as a reviewer with a clear reason", () => {
    const { rerender } = render(
      <AgentPicker label="Solution reviewer" reviewer agents={agents} value={null} onChange={vi.fn()} />,
    );
    expect(screen.getByText("Grok: read-only unverified")).toBeInTheDocument();
    const grok = within(screen.getByLabelText("Solution reviewer agent"))
      .getAllByRole("option")
      .find((o) => o.textContent?.startsWith("Grok")) as HTMLOptionElement;
    expect(grok.disabled).toBe(true);
    expect(grok.title).toMatch(/cannot review: its read-only enforcement is unverified/);
    // A saved grok reviewer (from config) shows the refusal inline.
    rerender(
      <AgentPicker label="Solution reviewer" reviewer agents={agents} value={{ agent: "grok" }} onChange={vi.fn()} />,
    );
    expect(screen.getByRole("alert").textContent).toMatch(/Grok cannot review/);
    expect(refusal(agents[2], false)).toBeNull();
  });

  it("reports the chosen agent and model", async () => {
    const onChange = vi.fn();
    render(<AgentPicker label="Designer" agents={agents} value={{ agent: "claude" }} onChange={onChange} />);
    await userEvent.selectOptions(screen.getByLabelText("Designer agent"), "grok");
    expect(onChange).toHaveBeenLastCalledWith({ agent: "grok", model: null });
    await userEvent.type(screen.getByLabelText("Designer model"), "x");
    expect(onChange).toHaveBeenLastCalledWith({ agent: "claude", model: "x" });
  });
});
