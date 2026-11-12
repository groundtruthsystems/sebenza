import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import DesignTab from "./DesignTab";
import { ApiRefusal } from "./solutionApi";
import { fakeApi, view } from "./solutionTestApi";

afterEach(cleanup);

const reviewed = view(
  { documents: { "requirements.md": "h1", "solution-design.md": "h2" } },
  {
    phase: "revising",
    round: 1,
    rounds: [{ round: { level: "solution", scope: "01ID", cycle: 1, round: 1 }, verdict: "changes", reviewer: { agent: "codex" }, problemIds: ["P-1"], at: "t" }],
  },
);

describe("DesignTab", () => {
  // TS-09 + TS-12 (UI): both documents show, and so does "Round 1 of 3".
  it("shows the documents and the round of three", async () => {
    const api = fakeApi({ view: vi.fn().mockResolvedValue(reviewed) });
    render(<DesignTab draftId="01ID" api={api} pollMs={60_000} />);
    expect(await screen.findByText("Round 1 of 3")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "requirements.md" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "solution-design.md" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Next review round" }));
    expect(api.review).toHaveBeenCalledWith("01ID");
  });

  // TS-49: a document carrying script and hostile mermaid executes nothing.
  it("renders hostile documents inert", async () => {
    const hostile = [
      "# Design",
      '<script>window.__pwned = 1</script>',
      '<img src=x onerror="window.__pwned = 2">',
      "```mermaid",
      'graph TD; A["<img src=x onerror=window.__pwned=3>"] --> B',
      'click A call alert("x")',
      "```",
    ].join("\n");
    const api = fakeApi({
      view: vi.fn().mockResolvedValue(reviewed),
      doc: vi.fn().mockResolvedValue({ name: "solution-design.md", content: hostile, hash: "h2" }),
    });
    render(<DesignTab draftId="01ID" api={api} pollMs={60_000} />);
    await userEvent.click(await screen.findByRole("button", { name: "solution-design.md" }));
    const article = await screen.findByRole("article", { name: "Document solution-design.md" });
    await waitFor(() => expect(article.querySelector("h1")).not.toBeNull());
    expect(article.querySelector("script")).toBeNull();
    expect(article.querySelector("[onerror]")).toBeNull();
    expect(article.innerHTML).not.toMatch(/onerror=|javascript:/i);
    expect((window as unknown as { __pwned?: number }).__pwned).toBeUndefined();
  });

  // TS-16 (UI): a hand edit is saved against its hash; resolve closes only
  // the problem it is on.
  it("saves a hand edit against its hash and resolves one problem", async () => {
    const escalated = view(
      { documents: { "solution-design.md": "h2" } },
      {
        phase: "escalated",
        round: 3,
        problems: [
          { problemId: "c1-P-1", requestId: "r1", title: "IdP", status: "open" },
          { problemId: "c1-P-2", requestId: "r2", title: "Currency", status: "open" },
        ],
      },
    );
    const api = fakeApi({
      view: vi.fn().mockResolvedValue(escalated),
      doc: vi.fn().mockResolvedValue({ name: "solution-design.md", content: "# D", hash: "h2" }),
    });
    render(<DesignTab draftId="01ID" api={api} pollMs={60_000} />);
    await userEvent.click(await screen.findByRole("button", { name: "solution-design.md" }));
    await userEvent.click(await screen.findByRole("button", { name: "Edit" }));
    const box = screen.getByLabelText("Edit solution-design.md");
    await userEvent.clear(box);
    await userEvent.type(box, "# D v2");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(api.saveDoc).toHaveBeenCalledWith("01ID", "solution-design.md", "# D v2", "h2");
    const p2 = screen.getByLabelText("Problem c1-P-2");
    await userEvent.click(within(p2).getByRole("button", { name: "Resolve" }));
    expect(api.resolve).toHaveBeenCalledTimes(1);
    expect(vi.mocked(api.resolve).mock.calls[0][1]).toBe("c1-P-2");
    // With problems open, the design cannot be approved yet.
    expect(screen.queryByRole("button", { name: "Approve design" })).toBeNull();
  });

  // TS-17 (UI): the per-item reviewer is saved; a refusal shows its reason.
  it("saves per-item agents and shows a not_independent refusal", async () => {
    const saveItemAgents = vi
      .fn()
      .mockRejectedValueOnce(new ApiRefusal(422, "not_independent", "the solution reviewer is not independent: claude-sonnet-5-5 is less capable than the designer's claude-opus-5-5"))
      .mockResolvedValue({ effective: null });
    const api = fakeApi({ saveItemAgents });
    render(<DesignTab draftId="01ID" api={api} pollMs={60_000} />);
    const picker = await screen.findByLabelText("Solution reviewer agent");
    await waitFor(() => expect(within(picker).getAllByRole("option").length).toBe(4));
    await userEvent.selectOptions(picker, "claude");
    await userEvent.type(screen.getByLabelText("Solution reviewer model"), "claude-sonnet-5-5");
    await userEvent.click(screen.getByRole("button", { name: "Save agents" }));
    expect(saveItemAgents).toHaveBeenCalledWith("01ID", {
      reviewer: { solution: { agent: "claude", model: "claude-sonnet-5-5" } },
    });
    expect(await screen.findByText(/less capable.*\(not_independent\)/)).toBeInTheDocument();
    // grok is never offered as a reviewer.
    const grok = within(picker).getAllByRole("option").find((o) => o.textContent?.startsWith("Grok")) as HTMLOptionElement;
    expect(grok.disabled).toBe(true);
  });

  it("approves the design and each handover against what was shown", async () => {
    const handover = view(
      {
        documents: { "solution-design.md": "h2", "handover/acme.md": "ih" },
        designApprovalValid: true,
        handovers: [
          { project: "/code/acme", slug: "acme", status: "drafted", instructionHash: "ih", approvalValid: false, copyStale: false },
          { project: "/code/portal", slug: "portal", status: "converted", instructionHash: "ph", approvalValid: true, copyStale: true },
        ],
      },
      { phase: "handover", round: 1 },
    );
    const api = fakeApi({ view: vi.fn().mockResolvedValue(handover) });
    render(<DesignTab draftId="01ID" api={api} pollMs={60_000} />);
    const acme = await screen.findByLabelText("Handover acme");
    await userEvent.click(within(acme).getByRole("button", { name: "Approve and create worktree" }));
    expect(api.approveHandover).toHaveBeenCalledWith("01ID", "acme", "ih");
    const portal = screen.getByLabelText("Handover portal");
    expect(within(portal).getByText("worktree copy is stale")).toBeInTheDocument();
    expect(within(portal).queryByRole("button", { name: "Approve and create worktree" })).toBeNull();
    expect(screen.getByText("Design approved as it stands.")).toBeInTheDocument();
  });

  it("edits the global defaults", async () => {
    const api = fakeApi();
    render(<DesignTab draftId="01ID" api={api} pollMs={60_000} />);
    const picker = await screen.findByLabelText("Default solution reviewer agent");
    await waitFor(() => expect(within(picker).getAllByRole("option").length).toBe(4));
    await userEvent.selectOptions(picker, "codex");
    await userEvent.click(screen.getByRole("button", { name: "Save defaults" }));
    expect(api.saveConfig).toHaveBeenCalledWith({ solution: { reviewer: { agent: "codex", model: null } }, project: {} });
  });
});
