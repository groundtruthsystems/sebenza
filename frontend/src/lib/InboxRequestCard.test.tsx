import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import InboxRequestCard from "./InboxRequestCard";
import { InboxRequestSchema, type InboxRequest } from "./api-contract";

const request = (over: Partial<InboxRequest> = {}): InboxRequest => ({
  requestId: "R1",
  worktree: { project: "/code/acme", branch: "fix-x" },
  title: "Which loader?",
  body: "Should I use the **new** loader?",
  status: "proposed",
  flagged: false,
  proposalId: "P1",
  proposal: "Use the new loader.\nKeep the old one behind a flag.",
  proposalHash: "ph",
  proposalWarnings: [],
  contentHash: null,
  confirmedText: null,
  lastReason: null,
  lastError: null,
  attempts: 0,
  warnings: [],
  openedAt: "2026-09-30T10:00:00Z",
  archivedAt: null,
  archivedBy: null,
  dismissedAt: null,
  dismissedBy: null,
  summary: { request: "fix-x: Which loader?", result: "Answer proposed" },
  notice: null,
  ...over,
});

function setup(r: InboxRequest, over: Record<string, unknown> = {}) {
  const props = {
    onconfirm: vi.fn().mockResolvedValue(undefined),
    onreject: vi.fn().mockResolvedValue(undefined),
    onredeliver: vi.fn().mockResolvedValue(undefined),
    onretry: vi.fn().mockResolvedValue(undefined),
    ondismiss: vi.fn().mockResolvedValue(undefined),
    onarchive: vi.fn().mockResolvedValue(undefined),
    onretrynotice: vi.fn().mockResolvedValue(undefined),
    onhistory: vi.fn().mockResolvedValue({
      request: r,
      answers: [{ kind: "confirmed", text: "use A", at: "2026-09-30T11:00:00Z" }],
      events: [
        { kind: "created", from: null, to: "open", actor: "worktree_agent", at: "2026-09-30T10:00:00Z" },
        { kind: "dismissed", from: "open", to: "dismissed", actor: "operator:operator_session", at: "2026-09-30T12:00:00Z" },
      ],
    }),
    ...over,
  };
  render(<InboxRequestCard request={r} {...props} />);
  return props;
}

describe("InboxRequestCard", () => {
  it("shows the request, its worktree and its status", () => {
    setup(request());
    expect(screen.getByText("Which loader?")).toBeInTheDocument();
    expect(screen.getByText("new").tagName).toBe("STRONG");
    expect(screen.getByText("acme · fix-x")).toBeInTheDocument();
    expect(screen.getByText("proposed")).toBeInTheDocument();
  });

  it("shows the proposal verbatim, markup included", () => {
    setup(request({ proposal: "run <b>this</b>\n  indented" }));
    const verbatim = screen.getByLabelText("Proposal");
    expect(verbatim.textContent).toBe("run <b>this</b>\n  indented");
    expect(verbatim.querySelector("b")).toBeNull();
  });

  it("confirms the proposal as shown, quoting its hash", async () => {
    const user = userEvent.setup();
    const { onconfirm } = setup(request());
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    expect(onconfirm).toHaveBeenCalledWith({ contentHash: "ph" });
  });

  // TS-30: an edit shows the verbatim proposal and a diff, and sends both the
  // edited text and the hash of what was shown.
  it("diffs an edited proposal and confirms the edit", async () => {
    const user = userEvent.setup();
    const { onconfirm } = setup(request());
    await user.click(screen.getByRole("button", { name: "Edit" }));
    const editor = screen.getByLabelText("Edited resolution");
    await user.clear(editor);
    await user.type(editor, "Use the new loader.\nDrop the old one.");

    expect(screen.getByLabelText("Proposal").textContent).toContain(
      "Keep the old one behind a flag.",
    );
    const diff = screen.getByLabelText("Changes to the proposal");
    expect(within(diff).getByText("Keep the old one behind a flag.")).toHaveAttribute(
      "data-op",
      "del",
    );
    expect(within(diff).getByText("Drop the old one.")).toHaveAttribute(
      "data-op",
      "add",
    );

    await user.click(screen.getByRole("button", { name: "Confirm edit" }));
    expect(onconfirm).toHaveBeenCalledWith({
      contentHash: "ph",
      body: "Use the new loader.\nDrop the old one.",
    });
  });

  // UC-06c: no proposal, so the operator writes one; its own hash is quoted.
  it("lets the operator write a resolution when there is no proposal", async () => {
    const user = userEvent.setup();
    const { onconfirm } = setup(
      request({ status: "open", proposal: null, proposalId: null, proposalHash: null }),
    );
    expect(screen.queryByLabelText("Proposal")).toBeNull();
    const confirm = screen.getByRole("button", { name: "Confirm" });
    expect(confirm).toBeDisabled();
    await user.type(screen.getByLabelText("Resolution"), "abc");
    await user.click(confirm);
    expect(onconfirm).toHaveBeenCalledWith({
      contentHash: "a9993e364706816aba3e25717850c26c9cd0d89d",
      body: "abc",
    });
  });

  it("rejects only with a reason", async () => {
    const user = userEvent.setup();
    const { onreject } = setup(request());
    await user.click(screen.getByRole("button", { name: "Reject" }));
    const send = screen.getByRole("button", { name: "Confirm reject" });
    expect(send).toBeDisabled();
    await user.type(screen.getByLabelText("Reason"), "wrong loader");
    await user.click(send);
    expect(onreject).toHaveBeenCalledWith("wrong loader");
  });

  // TS-55: a failed delivery stays visible, flagged, and redeliverable.
  it("offers redelivery after a failed delivery", async () => {
    const user = userEvent.setup();
    const { onredeliver } = setup(
      request({
        status: "delivery_failed",
        flagged: true,
        lastError: "no pane for fix-x",
        confirmedText: "Use the new loader.",
      }),
    );
    expect(screen.getByText("flagged")).toBeInTheDocument();
    expect(screen.getByText(/no pane for fix-x/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Confirm" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Redeliver" }));
    expect(onredeliver).toHaveBeenCalled();
  });

  // TS-55: a failed triage stays open and flagged, with a retry.
  it("offers a triage retry on a flagged open request", async () => {
    const user = userEvent.setup();
    const { onretry } = setup(
      request({
        status: "open",
        flagged: true,
        proposal: null,
        proposalId: null,
        proposalHash: null,
        lastError: "triage timed out",
      }),
    );
    expect(screen.getByText("flagged")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Retry triage" }));
    expect(onretry).toHaveBeenCalled();
  });

  it("badges scan hits on the request and the proposal", () => {
    setup(
      request({
        warnings: ["aws_access_key"],
        proposalWarnings: ["likely PHI: SSN"],
      }),
    );
    expect(screen.getByText("aws_access_key")).toBeInTheDocument();
    expect(screen.getByText("likely PHI: SSN")).toBeInTheDocument();
  });

  it("shows the last rejection reason", () => {
    setup(request({ status: "open", proposal: null, lastReason: "too vague" }));
    expect(screen.getByText(/too vague/)).toBeInTheDocument();
  });

  it("is read-only once resolved, showing what was sent", () => {
    setup(request({ status: "resolved", confirmedText: "Sent this." }));
    expect(screen.getByText("Sent this.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /confirm|reject|edit/i })).toBeNull();
  });

  it("says when triage is running", () => {
    setup(request({ status: "open", proposal: null }), { triaging: true });
    expect(screen.getByText(/triaging/i)).toBeInTheDocument();
  });

  it("reports a failed action and keeps the card", async () => {
    const user = userEvent.setup();
    setup(request(), {
      onconfirm: vi.fn().mockRejectedValue(new Error("HTTP 409")),
    });
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    expect(await screen.findByText(/changed since you opened it/i)).toBeInTheDocument();
  });

  // TS-53 (client half): a solution problem has no worktree; it parses and
  // renders, and a legacy request without `origin` still parses.
  it("parses and shows a solution problem with no worktree", () => {
    const { worktree: _wt, ...wire } = request();
    const parsed = InboxRequestSchema.parse({
      ...wire,
      origin: "solution_problem",
      problemId: "P-1",
    });
    expect(parsed.worktree).toBeUndefined();
    expect(parsed.origin).toBe("solution_problem");
    setup(parsed);
    expect(screen.getByText("solution problem")).toBeInTheDocument();

    const legacy = InboxRequestSchema.parse(request());
    expect(legacy.origin).toBeUndefined();
    expect(legacy.worktree).toEqual({ project: "/code/acme", branch: "fix-x" });
    expect(InboxRequestSchema.parse({ ...request(), origin: "from_the_future" }).origin)
      .toBeUndefined();
  });
});

// --- Requests-tab management (track sebenza_polishing, phase 2) ----------

describe("InboxRequestCard lifecycle", () => {
  // TS-37: the summary of the request and its result is shown, archived too.
  it("shows the template summary, also once archived", () => {
    setup(
      request({
        status: "resolved",
        archivedAt: "2026-10-01T00:00:00Z",
        archivedBy: "operator_session",
        summary: { request: "fix-x: Which loader?", result: "Answer delivered" },
      }),
    );
    const summary = screen.getByLabelText("Summary");
    expect(summary).toHaveTextContent("fix-x: Which loader?");
    expect(summary).toHaveTextContent("Answer delivered");
    expect(screen.getByText("archived")).toBeInTheDocument();
  });

  it("dismisses an unanswered request", async () => {
    const user = userEvent.setup();
    const { ondismiss } = setup(request({ status: "open" }));
    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(ondismiss).toHaveBeenCalled();
  });

  it("offers no Dismiss once answered or dismissed", () => {
    setup(request({ status: "resolved" }));
    expect(screen.queryByRole("button", { name: "Dismiss" })).toBeNull();
  });

  // TS-35: Archive is a native button, disabled unless confirmed or resolved.
  it("enables Archive only for confirmed or resolved requests", async () => {
    const user = userEvent.setup();
    const { onarchive } = setup(request({ status: "resolved" }));
    const archive = screen.getByRole("button", { name: "Archive" });
    expect(archive).toBeEnabled();
    await user.click(archive);
    expect(onarchive).toHaveBeenCalled();
  });

  it("shows Archive disabled for unanswered and already archived requests", () => {
    setup(request({ status: "open" }));
    expect(screen.getByRole("button", { name: "Archive" })).toBeDisabled();
  });

  it("shows Archive disabled once archived", () => {
    setup(request({ status: "confirmed", archivedAt: "2026-10-01T00:00:00Z" }));
    expect(screen.getByRole("button", { name: "Archive" })).toBeDisabled();
  });

  it("marks a dismissed request and its pending notice", () => {
    setup(
      request({
        status: "dismissed",
        notice: { key: "R1:dismissal", state: "pending", failures: 0, lastAttempt: 0, lastAttemptAt: null, lastError: null },
      }),
    );
    expect(screen.getByText("dismissed")).toBeInTheDocument();
    expect(screen.queryByText("notice not delivered")).toBeNull();
    expect(screen.queryByRole("button", { name: "Retry notice" })).toBeNull();
  });

  it("shows the notice-not-delivered badge with Retry notice", async () => {
    const user = userEvent.setup();
    const { onretrynotice } = setup(
      request({
        status: "dismissed",
        notice: { key: "R1:dismissal", state: "failed", failures: 6, lastAttempt: 6, lastAttemptAt: "t", lastError: "pane gone" },
      }),
    );
    expect(screen.getByText("notice not delivered")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Retry notice" }));
    expect(onretrynotice).toHaveBeenCalled();
  });

  // TS-38 (UI): the history view lists every event with its actor.
  it("opens the history with every event and answer", async () => {
    const user = userEvent.setup();
    const { onhistory } = setup(request({ status: "dismissed" }));
    const toggle = screen.getByRole("button", { name: "History" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    await user.click(toggle);
    expect(onhistory).toHaveBeenCalled();
    const list = await screen.findByRole("list", { name: "Request history" });
    const rows = within(list).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(rows[1]).toHaveTextContent("dismissed");
    expect(rows[1]).toHaveTextContent("operator:operator_session");
    expect(screen.getByText("use A")).toBeInTheDocument();
    expect(toggle).toHaveAttribute("aria-expanded", "true");
  });

  // NFR-04: the new controls are keyboard operable.
  it("operates Dismiss from the keyboard", async () => {
    const user = userEvent.setup();
    const { ondismiss } = setup(request({ status: "open" }));
    const dismiss = screen.getByRole("button", { name: "Dismiss" });
    dismiss.focus();
    await user.keyboard("{Enter}");
    expect(ondismiss).toHaveBeenCalled();
  });
});
