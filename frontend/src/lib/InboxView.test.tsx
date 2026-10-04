import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const drafts = vi.fn();
const draft = vi.fn();
const patch = vi.fn();
const del = vi.fn();
const save = vi.fn();
const setPriority = vi.fn();
const comments = vi.fn();
const postComment = vi.fn();
const requests = vi.fn();
const confirmRequest = vi.fn();
const draftHelp = vi.fn();
type StreamCallbacks = {
  onJob: (job: Record<string, unknown>) => void;
  onError: (message: string) => void;
};
let stream: StreamCallbacks | null = null;

vi.mock("./api", () => ({
  fetchInboxDrafts: (...a: unknown[]) => drafts(...a),
  fetchInboxDraft: (...a: unknown[]) => draft(...a),
  patchInboxDraft: (...a: unknown[]) => patch(...a),
  createInboxDraft: vi.fn(),
  deleteInboxDraft: (...a: unknown[]) => del(...a),
  saveInboxDraftBody: (...a: unknown[]) => save(...a),
  loadInboxControlToken: () => Promise.resolve(),
  fetchProjects: () => Promise.resolve([{ prefix: "demo", name: "demo" }]),
  setInboxPriority: (...a: unknown[]) => setPriority(...a),
  fetchInboxComments: (...a: unknown[]) => comments(...a),
  postInboxComment: (...a: unknown[]) => postComment(...a),
  fetchInboxRequests: (...a: unknown[]) => requests(...a),
  confirmInboxRequest: (...a: unknown[]) => confirmRequest(...a),
  rejectInboxRequest: vi.fn(),
  redeliverInboxRequest: vi.fn(),
  retryInboxTriage: vi.fn(),
  redactInboxComment: vi.fn(),
  requestInboxDraftHelp: (...a: unknown[]) => draftHelp(...a),
  fetchInboxAgentJob: vi.fn(),
  connectInboxAgentStream: (_id: string, cb: StreamCallbacks) => {
    stream = cb;
    return () => {
      stream = null;
    };
  },
}));

// Mermaid pulls a large async graph that has no place in a list-behaviour test.
vi.mock("./inboxMarkdown", () => ({
  renderDraftMarkdown: (s: string) => Promise.resolve(`<p>${s}</p>`),
  sanitize: (s: string) => s,
  renderCommentMarkdown: (s: string) => `<p>${s}</p>`,
}));

import InboxView from "./InboxView";

beforeAll(() => {
  if (!HTMLDialogElement.prototype.showModal) {
    HTMLDialogElement.prototype.showModal = function () {
      this.open = true;
    };
    HTMLDialogElement.prototype.close = function () {
      this.open = false;
    };
  }
});

const summary = (over: Partial<Record<string, unknown>> = {}) => ({
  id: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  title: "Rework the scorer",
  status: "Draft",
  updatedAt: "2026-09-28T00:00:00Z",
  project: null,
  isRaw: false,
  ...over,
});

const ID = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

const full = (over: Partial<Record<string, unknown>> = {}) => ({
  id: ID,
  title: "Rework the scorer",
  status: "Draft",
  createdAt: "2026-09-28T00:00:00Z",
  updatedAt: "2026-09-28T00:00:00Z",
  body: "## notes",
  bodyHash: "h1",
  project: null,
  priority: "P2",
  prioritySource: "agent",
  conversions: [],
  raw: null,
  ...over,
});

const job = (over: Partial<Record<string, unknown>> = {}) => ({
  jobId: "J1",
  draftId: ID,
  kind: "draft_help",
  requestId: null,
  attempt: 1,
  status: "succeeded",
  output: { jobKind: "draft_help", proposed_body: "proposed body", summary: "Tightened." },
  error: null,
  reseeded: false,
  enqueuedAt: "",
  startedAt: null,
  finishedAt: null,
  ...over,
});

beforeEach(() => {
  vi.clearAllMocks();
  stream = null;
  drafts.mockResolvedValue({ drafts: [summary()] });
  comments.mockResolvedValue({ overall: [], worktrees: [] });
  requests.mockResolvedValue({ requests: [] });
});

describe("InboxView", () => {
  it("lists drafts", async () => {
    render(<InboxView />);
    expect(await screen.findByText("Rework the scorer")).toBeInTheDocument();
  });

  it("shows a resolved project by name and an unresolved one as unresolved", async () => {
    drafts.mockResolvedValue({
      drafts: [
        summary({
          id: "01AAA",
          title: "Linked",
          project: { path: "/code/acme", name: "acme", resolved: true },
        }),
        summary({
          id: "01BBB",
          title: "Moved",
          project: { path: "/gone", name: null, resolved: false },
        }),
      ],
    });
    render(<InboxView />);
    expect(await screen.findByText("acme")).toBeInTheDocument();
    expect(await screen.findByText("unresolved project")).toBeInTheDocument();
  });

  it("labels an unparseable draft rather than showing a blank row", async () => {
    drafts.mockResolvedValue({
      drafts: [summary({ title: "", isRaw: true })],
    });
    render(<InboxView />);
    expect(await screen.findByText("(unparseable)")).toBeInTheDocument();
  });

  it("asks the server for dropped drafts only when the filter is on", async () => {
    const user = userEvent.setup();
    render(<InboxView />);
    await waitFor(() => expect(drafts).toHaveBeenCalled());
    expect(drafts).toHaveBeenLastCalledWith(
      expect.objectContaining({ includeDropped: false }),
    );

    await user.click(screen.getByLabelText(/show dropped/i));
    await waitFor(() =>
      expect(drafts).toHaveBeenLastCalledWith(
        expect.objectContaining({ includeDropped: true }),
      ),
    );
  });

  it("passes the search box through to the server", async () => {
    const user = userEvent.setup();
    render(<InboxView />);
    await waitFor(() => expect(drafts).toHaveBeenCalled());
    await user.type(screen.getByLabelText(/search drafts/i), "scorer");
    await waitFor(() =>
      expect(drafts).toHaveBeenLastCalledWith(
        expect.objectContaining({ search: "scorer" }),
      ),
    );
  });

  it("opens a draft into the editor", async () => {
    const user = userEvent.setup();
    draft.mockResolvedValue({
      id: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
      title: "Rework the scorer",
      status: "Draft",
      body: "## notes",
      bodyHash: "h1",
      project: null,
      raw: null,
    });
    render(<InboxView />);
    await user.click(await screen.findByText("Rework the scorer"));
    expect(await screen.findByLabelText("Draft body")).toHaveValue("## notes");
  });

  it("shows the parse error for an unparseable draft instead of an editor", async () => {
    const user = userEvent.setup();
    drafts.mockResolvedValue({ drafts: [summary({ title: "", isRaw: true })] });
    draft.mockResolvedValue({
      id: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
      title: "",
      status: "Draft",
      body: "",
      bodyHash: "",
      project: null,
      raw: { text: "---\nbroken\n", error: "bad yaml" },
    });
    render(<InboxView />);
    await user.click(await screen.findByText("(unparseable)"));
    expect(await screen.findByText(/bad yaml/)).toBeInTheDocument();
    expect(screen.queryByLabelText("Draft body")).not.toBeInTheDocument();
  });

  it("confirms before deleting, and warns harder for a promoted draft", async () => {
    const user = userEvent.setup();
    drafts.mockResolvedValue({
      drafts: [summary({ title: "Shipped", status: "Promoted" })],
    });
    draft.mockResolvedValue({
      id: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
      title: "Shipped",
      status: "Promoted",
      body: "x",
      bodyHash: "h",
      project: null,
      raw: null,
    });
    del.mockResolvedValue({ ok: true });

    render(<InboxView />);
    await user.click(await screen.findByText("Shipped"));
    await user.click(await screen.findByRole("button", { name: "Delete" }));

    // The warning names the real consequence, not a generic "are you sure".
    expect(
      await screen.findByText(/only record of the prompts/i),
    ).toBeInTheDocument();
    expect(del).not.toHaveBeenCalled();

    // Two "Delete" buttons exist once the dialog is up: the toolbar's and the
    // dialog's confirm. The confirm is the one that acts.
    const dialog = document.querySelector("dialog")!;
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      // `confirmed` must be true or the server refuses a promoted draft.
      expect(del).toHaveBeenCalledWith("01ARZ3NDEKTSV4RRFFQ69G5FAV", true),
    );
  });

  it("cancelling the delete leaves the draft alone", async () => {
    const user = userEvent.setup();
    draft.mockResolvedValue({
      id: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
      title: "Rework the scorer",
      status: "Draft",
      body: "x",
      bodyHash: "h",
      project: null,
      raw: null,
    });
    render(<InboxView />);
    await user.click(await screen.findByText("Rework the scorer"));
    await user.click(await screen.findByRole("button", { name: "Delete" }));
    const dialog = document.querySelector("dialog")!;
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(del).not.toHaveBeenCalled();
  });

  it("badges each item's priority and who set it", async () => {
    drafts.mockResolvedValue({
      drafts: [summary({ priority: "P1", prioritySource: "operator" })],
    });
    render(<InboxView />);
    const badge = await screen.findByText("P1");
    expect(badge).toHaveAttribute("title", expect.stringMatching(/operator/i));
  });

  it("lists by priority, then newest", async () => {
    drafts.mockResolvedValue({
      drafts: [
        summary({ id: "01C", title: "Later", priority: "P3", createdAt: "2026-09-30T00:00:00Z" }),
        summary({ id: "01B", title: "Newer", priority: "P1", createdAt: "2026-09-29T00:00:00Z" }),
        summary({ id: "01A", title: "Urgent", priority: "P0", createdAt: "2026-09-01T00:00:00Z" }),
        summary({ id: "01D", title: "Older", priority: "P1", createdAt: "2026-09-02T00:00:00Z" }),
      ],
    });
    render(<InboxView />);
    await screen.findByText("Urgent");
    const titles = screen
      .getAllByRole("listitem")
      .map((li) => within(li).queryByText(/^(Urgent|Newer|Older|Later)$/)?.textContent);
    expect(titles).toEqual(["Urgent", "Newer", "Older", "Later"]);
  });

  // TS-55: a failed triage or delivery flags the item in the list.
  it("flags an item whose requests need the operator", async () => {
    drafts.mockResolvedValue({
      drafts: [
        summary({ id: "01A", title: "Stuck", flagged: true }),
        summary({ id: "01B", title: "Fine", flagged: false }),
      ],
    });
    render(<InboxView />);
    const stuck = (await screen.findByText("Stuck")).closest("li")!;
    expect(within(stuck).getByLabelText(/needs attention/i)).toBeInTheDocument();
    const fine = screen.getByText("Fine").closest("li")!;
    expect(within(fine).queryByLabelText(/needs attention/i)).toBeNull();
  });

  it("sets a priority override from the open item", async () => {
    const user = userEvent.setup();
    draft.mockResolvedValue(full());
    setPriority.mockResolvedValue(full({ priority: "P0", prioritySource: "operator" }));
    render(<InboxView />);
    await user.click(await screen.findByText("Rework the scorer"));
    await user.selectOptions(await screen.findByLabelText("Priority"), "P0");
    expect(setPriority).toHaveBeenCalledWith(ID, "P0");
    expect(await screen.findByText(/operator override/i)).toBeInTheDocument();
    await waitFor(() => expect(drafts.mock.calls.length).toBeGreaterThan(1));
  });

  // TS-66: the editor carries the PHI / provider warning while open.
  it("warns in the editor that PHI is prohibited and content goes to the model provider", async () => {
    const user = userEvent.setup();
    draft.mockResolvedValue(full());
    render(<InboxView />);
    await user.click(await screen.findByText("Rework the scorer"));
    await screen.findByLabelText("Draft body");
    const notes = screen.getAllByRole("note");
    expect(notes.some((n) => /PHI/.test(n.textContent ?? "") && /model provider/i.test(n.textContent ?? ""))).toBe(true);
  });

  // TS-02: proposal shown; accepting puts it in the editor via the hash-gated PUT.
  it("applies an accepted draft-help proposal through the hash-gated save", async () => {
    const user = userEvent.setup();
    draft.mockResolvedValue(full());
    draftHelp.mockResolvedValue({ jobId: "J1" });
    save.mockResolvedValue(full({ body: "proposed body", bodyHash: "h2" }));
    render(<InboxView />);
    await user.click(await screen.findByText("Rework the scorer"));
    await user.click(await screen.findByRole("button", { name: "Draft help" }));
    await user.type(screen.getByLabelText("Draft help instruction"), "tighten");
    await user.click(screen.getByRole("button", { name: "Ask agent" }));
    expect(draftHelp).toHaveBeenCalledWith(ID, "tighten");

    await waitFor(() => expect(stream).not.toBeNull());
    // Another job's result is not ours to show.
    act(() => stream!.onJob(job({ jobId: "OTHER", output: { jobKind: "draft_help", proposed_body: "nope", summary: "x" } })));
    expect(screen.queryByText("nope")).toBeNull();
    act(() => stream!.onJob(job()));

    const region = await screen.findByRole("region", { name: "Draft help proposal" });
    expect(within(region).getByText("Tightened.")).toBeInTheDocument();
    expect(save).not.toHaveBeenCalled();
    await user.click(within(region).getByRole("button", { name: "Accept" }));
    await waitFor(() => expect(save).toHaveBeenCalledWith(ID, "h1", "proposed body"));
    expect(screen.getByLabelText("Draft body")).toHaveValue("proposed body");
  });

  // TS-04: the body moved; both versions shown; saved only once resolved.
  it("opens a merge view on a 409 and saves only the resolved text", async () => {
    const user = userEvent.setup();
    draft
      .mockResolvedValueOnce(full())
      .mockResolvedValue(full({ body: "disk edit", bodyHash: "h2" }));
    draftHelp.mockResolvedValue({ jobId: "J1" });
    save
      .mockRejectedValueOnce(new Error("HTTP 409"))
      .mockResolvedValue(full({ body: "merged", bodyHash: "h3" }));
    render(<InboxView />);
    await user.click(await screen.findByText("Rework the scorer"));
    await user.click(await screen.findByRole("button", { name: "Draft help" }));
    await user.click(screen.getByRole("button", { name: "Ask agent" }));
    await waitFor(() => expect(stream).not.toBeNull());
    act(() => stream!.onJob(job()));
    await user.click(await screen.findByRole("button", { name: "Accept" }));

    const merge = await screen.findByRole("region", { name: "Resolve conflict" });
    expect(within(merge).getByLabelText("On disk")).toHaveTextContent("disk edit");
    expect(within(merge).getByLabelText("Proposed")).toHaveTextContent("proposed body");
    expect(save).toHaveBeenCalledTimes(1);

    const merged = within(merge).getByLabelText("Merged body");
    await user.clear(merged);
    await user.type(merged, "merged");
    expect(save).toHaveBeenCalledTimes(1);
    await user.click(within(merge).getByRole("button", { name: "Save merged" }));
    await waitFor(() => expect(save).toHaveBeenLastCalledWith(ID, "h2", "merged"));
    await waitFor(() =>
      expect(screen.queryByRole("region", { name: "Resolve conflict" })).toBeNull(),
    );
    expect(screen.getByLabelText("Draft body")).toHaveValue("merged");
  });

  it("shows the item's requests and confirms one", async () => {
    const user = userEvent.setup();
    draft.mockResolvedValue(full());
    const req = {
      requestId: "R1",
      worktree: { project: "/code/acme", branch: "fix-x" },
      title: "Which loader?",
      body: "?",
      status: "proposed",
      flagged: false,
      proposalId: "P1",
      proposal: "Use the new one.",
      proposalHash: "ph",
      proposalWarnings: [],
      contentHash: null,
      confirmedText: null,
      lastReason: null,
      lastError: null,
      attempts: 0,
      warnings: [],
      openedAt: "2026-09-30T00:00:00Z",
    };
    requests.mockResolvedValue({ requests: [req] });
    confirmRequest.mockResolvedValue({ request: { ...req, status: "resolved" } });
    render(<InboxView />);
    await user.click(await screen.findByText("Rework the scorer"));
    await user.click(await screen.findByRole("tab", { name: /Requests/ }));
    expect(await screen.findByText("Which loader?")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    expect(confirmRequest).toHaveBeenCalledWith(ID, "R1", { contentHash: "ph" });
    await waitFor(() => expect(requests.mock.calls.length).toBeGreaterThan(1));
  });

  it("posts a comment and refreshes the threads", async () => {
    const user = userEvent.setup();
    draft.mockResolvedValue(
      full({
        conversions: [
          { projectPath: "/code/acme", branch: "fix-x", prompt: "p", outcome: "created", at: "" },
        ],
      }),
    );
    postComment.mockResolvedValue({
      comment: {
        eventId: "E1",
        ts: "",
        author: "operator",
        caller: null,
        kind: "note",
        body: "hi",
        title: null,
        requestId: null,
        parentEventId: null,
        warnings: [],
        redacted: false,
      },
    });
    render(<InboxView />);
    await user.click(await screen.findByText("Rework the scorer"));
    await user.click(await screen.findByRole("tab", { name: /Comments/ }));
    await user.selectOptions(await screen.findByLabelText("Comment thread"), "acme · fix-x");
    await user.type(screen.getByLabelText("Comment"), "hi");
    await user.click(screen.getByRole("button", { name: "Post comment" }));
    expect(postComment).toHaveBeenCalledWith(ID, "hi", {
      project: "/code/acme",
      branch: "fix-x",
    });
    await waitFor(() => expect(comments.mock.calls.length).toBeGreaterThan(1));
  });

  it("refreshes requests and priority when a triage job finishes", async () => {
    const user = userEvent.setup();
    draft.mockResolvedValue(full());
    render(<InboxView />);
    await user.click(await screen.findByText("Rework the scorer"));
    await waitFor(() => expect(stream).not.toBeNull());
    const before = requests.mock.calls.length;
    act(() =>
      stream!.onJob(job({ kind: "triage", requestId: "R1", status: "succeeded", output: null })),
    );
    await waitFor(() => expect(requests.mock.calls.length).toBeGreaterThan(before));
  });
});
