import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
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
const updateNote = vi.fn();
const graphFetch = vi.fn();
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
  updateInboxNote: (...a: unknown[]) => updateNote(...a),
  addInboxNote: vi.fn(),
  deleteInboxNote: vi.fn(),
  reorderInboxNotes: vi.fn(),
  fetchInboxSummary: () => Promise.resolve({ openRequests: 0 }),
  fetchInboxGraph: (...a: unknown[]) => graphFetch(...a),
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
  // Mirrors the real renderer's edit button for a diagram, so the preview's
  // click delegation can be exercised.
  renderDraftMarkdown: (s: string, o?: { editableDiagrams?: boolean }) =>
    Promise.resolve(
      o?.editableDiagrams && s.includes("```mermaid")
        ? `<p>prose</p><div class="diagram-block"><button type="button" data-diagram-edit="0">Edit diagram</button></div>`
        : `<p>${s}</p>`,
    ),
  sanitize: (s: string) => s,
  renderCommentMarkdown: (s: string) => `<p>${s}</p>`,
  renderMermaidSvg: () => Promise.resolve("<svg></svg>"),
}));

// The solution tabs talk to their own client; list behaviour never needs it.
vi.mock("./solutionApi", async () => {
  const { fakeApi } = await import("./solutionTestApi");
  const actual = await vi.importActual<typeof import("./solutionApi")>("./solutionApi");
  return { ...actual, solutionApi: fakeApi() };
});

import InboxView from "./InboxView";
import * as inboxApi from "./api";
import { solutionApi } from "./solutionApi";
import { view as solutionView } from "./solutionTestApi";

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
    expect(postComment).toHaveBeenCalledWith(
      ID,
      "hi",
      { project: "/code/acme", branch: "fix-x" },
      true,
    );
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

  describe("diagrams", () => {
    const FENCE_BODY = "## notes\n\n```mermaid\nclassDiagram\n  class Order\n```\n\nafter";

    it("inserts a diagram drawn from the toolbar at the cursor", async () => {
      const user = userEvent.setup();
      draft.mockResolvedValue(full({ body: "## notes" }));
      render(<InboxView />);
      await user.click(await screen.findByText("Rework the scorer"));
      const textarea = (await screen.findByLabelText("Draft body")) as HTMLTextAreaElement;
      textarea.setSelectionRange(8, 8);

      await user.click(screen.getByRole("button", { name: "Insert or edit a diagram" }));
      await user.click(await screen.findByRole("button", { name: /Class diagram/ }));
      await user.click(screen.getByRole("button", { name: "+ Class" }));
      await user.click(screen.getByRole("button", { name: "Insert diagram" }));

      expect(screen.queryByRole("dialog", { name: "Diagram editor" })).toBeNull();
      expect(textarea.value).toMatch(/^## notes\n\n```mermaid\nclassDiagram\n  class Class\n  %% sebenza:layout .*\n```\n$/);
    });

    it("edits a diagram from the preview's edit button, leaving the prose alone", async () => {
      const user = userEvent.setup();
      draft.mockResolvedValue(full({ body: FENCE_BODY }));
      render(<InboxView />);
      await user.click(await screen.findByText("Rework the scorer"));
      await user.click(await screen.findByRole("button", { name: "Edit diagram" }));

      await user.click(await screen.findByRole("tab", { name: "source" }));
      const src = screen.getByLabelText("Mermaid source");
      await user.clear(src);
      await user.type(src, "flowchart LR\n  a --> b");
      await user.click(screen.getByRole("button", { name: "Update diagram" }));

      expect(screen.getByLabelText("Draft body")).toHaveValue(
        "## notes\n\n```mermaid\nflowchart LR\n  a --> b\n```\n\nafter",
      );
    });

    it("opens the fence under the cursor rather than inserting a new one", async () => {
      const user = userEvent.setup();
      draft.mockResolvedValue(full({ body: FENCE_BODY }));
      render(<InboxView />);
      await user.click(await screen.findByText("Rework the scorer"));
      const textarea = (await screen.findByLabelText("Draft body")) as HTMLTextAreaElement;
      const inside = FENCE_BODY.indexOf("class Order");
      textarea.setSelectionRange(inside, inside);

      await user.click(screen.getByRole("button", { name: "Insert or edit a diagram" }));
      expect(await screen.findByRole("button", { name: "Update diagram" })).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "Cancel" }));
      expect(textarea).toHaveValue(FENCE_BODY);
    });
  });
});

describe("InboxView notes", () => {
  const A = { id: "n1", type: "general", title: "Problem", body: "Graph view", hash: "ha" };
  const B = { id: "ac11", type: "acceptance_criteria", title: "Acceptance Criteria", body: "- AC", hash: "hb" };
  const multi = (over: Partial<Record<string, unknown>> = {}) =>
    full({ body: "<!-- markers -->", notes: [A, B], listHash: "lh", ...over });

  it("edits a multi-note item note by note", async () => {
    const user = userEvent.setup();
    draft.mockResolvedValue(multi());
    render(<InboxView />);
    await user.click(await screen.findByText("Rework the scorer"));
    expect(await screen.findByRole("region", { name: "Note: Acceptance Criteria" })).toBeInTheDocument();
    expect(screen.queryByLabelText("Draft body")).toBeNull();
  });

  it("keeps the plain editor for a single default note, with Add note", async () => {
    const user = userEvent.setup();
    draft.mockResolvedValue(
      full({ notes: [{ id: "n1", type: "general", title: "Note", body: "## notes", hash: "h" }], listHash: "lh" }),
    );
    render(<InboxView />);
    await user.click(await screen.findByText("Rework the scorer"));
    expect(await screen.findByLabelText("Draft body")).toHaveValue("## notes");
    expect(screen.getByRole("button", { name: "Add note" })).toBeInTheDocument();
  });

  // TS-67: notes edited after the last conversion are flagged.
  it("flags notes changed since conversion", async () => {
    const user = userEvent.setup();
    draft.mockResolvedValue(
      multi({ conversions: [{ projectPath: "/p", branch: "b", prompt: "", outcome: "created", at: "t", notesListHash: "old" }] }),
    );
    render(<InboxView />);
    await user.click(await screen.findByText("Rework the scorer"));
    expect(await screen.findByText("Notes changed since conversion")).toBeInTheDocument();
  });

  // TS-72: draft help aimed at one note replaces that note only.
  it("applies draft help to the note it was asked about", async () => {
    const user = userEvent.setup();
    draft.mockResolvedValue(multi());
    draftHelp.mockResolvedValue({ jobId: "J1" });
    updateNote.mockResolvedValue({ kind: "ok", notes: [A, { ...B, body: "proposed body", hash: "hb2" }], listHash: "lh2" });
    render(<InboxView />);
    await user.click(await screen.findByText("Rework the scorer"));
    const b = await screen.findByRole("region", { name: "Note: Acceptance Criteria" });
    await user.click(within(b).getByRole("button", { name: "Improve with agent" }));
    await user.click(screen.getByRole("button", { name: "Ask agent" }));
    expect(draftHelp).toHaveBeenCalledWith(ID, undefined, "ac11");
    await waitFor(() => expect(stream).not.toBeNull());
    act(() => stream!.onJob(job()));
    const region = await screen.findByRole("region", { name: "Draft help proposal" });
    await user.click(within(region).getByRole("button", { name: "Accept" }));
    await waitFor(() =>
      expect(updateNote).toHaveBeenCalledWith(ID, "ac11", { body: "proposed body" }, "hb"),
    );
    expect(updateNote).toHaveBeenCalledTimes(1);
    expect(save).not.toHaveBeenCalled();
  });
});

describe("InboxView graph tab", () => {
  it("switches the item page to its graph", async () => {
    const user = userEvent.setup();
    draft.mockResolvedValue(full());
    graphFetch.mockResolvedValue({
      kind: "graph",
      graph: {
        draftId: ID, generatedAt: "t", etag: "e", truncated: false,
        progress: { done: 0, total: 0 }, failed: 0, unavailable: 0, worktrees: [], edges: [],
      },
    });
    render(<InboxView />);
    await user.click(await screen.findByText("Rework the scorer"));
    expect(await screen.findByLabelText("Draft body")).toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Graph" }));
    expect(await screen.findByText(/not been converted into worktrees yet/)).toBeInTheDocument();
    expect(graphFetch).toHaveBeenCalledWith(ID, undefined, expect.any(AbortSignal));
    expect(screen.queryByLabelText("Draft body")).toBeNull();
    await user.click(screen.getByRole("tab", { name: "Notes" }));
    expect(await screen.findByLabelText("Draft body")).toBeInTheDocument();
  });

  // TS-01: "+ Solution" creates a solution item, which opens on exactly
  // Coordinator, Design and Graph; a note keeps Notes and Graph (TS-55).
  it("opens a new solution item on Coordinator, Design and Graph", async () => {
    const user = userEvent.setup();
    const solution = full({ id: "01SOL", title: "Billing", kind: "solution", notes: [], listHash: "" });
    vi.mocked(inboxApi.createInboxDraft).mockResolvedValue(solution as never);
    drafts.mockResolvedValue({ drafts: [summary({ id: "01SOL", title: "Billing" })] });
    draft.mockResolvedValue(solution);
    render(<InboxView />);
    await user.click(await screen.findByTitle("New solution"));
    await user.type(screen.getByPlaceholderText("What is this about?"), "Billing");
    await user.click(screen.getByRole("button", { name: "Create" }));
    expect(inboxApi.createInboxDraft).toHaveBeenCalledWith("Billing", "solution");
    const tabs = await screen.findByRole("tablist", { name: "Item view" });
    await waitFor(() =>
      expect(within(tabs).getAllByRole("tab").map((t) => t.textContent)).toEqual([
        "Coordinator",
        "Design",
        "Graph",
      ]),
    );
    expect(await screen.findByRole("region", { name: "Coordinator chat" })).toBeInTheDocument();
  });

  it("keeps Notes and Graph for a plain note", async () => {
    draft.mockResolvedValue(full());
    render(<InboxView />);
    await userEvent.click(await screen.findByText("Rework the scorer"));
    const tabs = await screen.findByRole("tablist", { name: "Item view" });
    expect(within(tabs).getAllByRole("tab").map((t) => t.textContent)).toEqual(["Notes", "Graph"]);
  });
});

describe("InboxView full screen", () => {
  const ids = Array.from({ length: 30 }, (_, i) => `req-${String(i + 1).padStart(2, "0")}`);
  const listed = [
    summary({ id: "req-a", title: "Sample request A" }),
    summary({ id: "req-b", title: "Sample request B" }),
    summary({ id: "req-s", title: "Sample solution" }),
    ...ids.map((id) => summary({ id, title: `Sample request ${id}` })),
  ];
  const notes = [
    { id: "n1", type: "general", title: "Problem", body: "Slow invoices", hash: "h1" },
    { id: "n2", type: "constraints", title: "Constraints", body: "Only synthetic data", hash: "h2" },
  ];
  const items: Record<string, unknown> = {
    "req-a": full({ id: "req-a", title: "Sample request A", body: "Body of A" }),
    "req-b": full({ id: "req-b", title: "Sample request B", body: "Body of B" }),
    "req-s": full({ id: "req-s", title: "Sample solution", kind: "solution", body: "Brief body", notes, listHash: "lh" }),
  };
  const fsRegion = () => screen.queryByRole("region", { name: "Full-screen request" });
  const list = () => screen.getByRole("list", { name: "Requests" });
  const row = (id: string) => list().querySelector<HTMLElement>(`[data-item-id="${id}"]`);

  beforeEach(() => {
    window.history.replaceState(null, "", "/inbox");
    drafts.mockResolvedValue({ drafts: listed });
    draft.mockImplementation((id: string) =>
      id in items ? Promise.resolve(items[id]) : Promise.reject(new Error("Not found (404)")),
    );
    Element.prototype.scrollIntoView = vi.fn();
  });

  afterEach(() => {
    window.history.replaceState(null, "", "/inbox");
  });

  async function openFromList(user: ReturnType<typeof userEvent.setup>) {
    await user.click(await screen.findByText("Sample request B"));
    await waitFor(() => expect(row("req-b")).toHaveAttribute("aria-current", "true"));
    list().scrollTop = 300;
    await user.click(screen.getByRole("button", { name: "Open Sample request B full screen" }));
    expect(await screen.findByRole("region", { name: "Full-screen request" })).toBeInTheDocument();
    list().scrollTop = 0;
  }

  function expectListRestored() {
    expect(fsRegion()).toBeNull();
    expect(window.location.pathname + window.location.search).toBe("/inbox");
    expect(row("req-b")).toHaveAttribute("aria-current", "true");
    expect(list().scrollTop).toBe(300);
  }

  // TS-01: entry from the list saves route, selection and scroll; Close restores them.
  it("opens from the list and Close restores the list context", async () => {
    const user = userEvent.setup();
    const push = vi.spyOn(window.history, "pushState");
    render(<InboxView />);
    await openFromList(user);
    expect(push).toHaveBeenCalledWith(
      expect.objectContaining({ fullscreen: { route: "/inbox", selection: "req-b", scroll: 300 } }),
      "",
      "/inbox?item=req-b&view=full",
    );
    expect(within(fsRegion()!).getByRole("heading", { name: "Sample request B" })).toBeInTheDocument();
    const length = window.history.length;
    await user.click(screen.getByRole("button", { name: "Close full screen" }));
    expectListRestored();
    expect(window.history.length).toBe(length);
    push.mockRestore();
  });

  // TS-02: Esc and browser Back restore the list context too.
  it("restores the list context on Esc and on Back", async () => {
    const user = userEvent.setup();
    render(<InboxView />);
    await openFromList(user);
    await user.keyboard("{Escape}");
    expectListRestored();
    await user.click(screen.getByRole("button", { name: "Open Sample request B full screen" }));
    expect(await screen.findByRole("region", { name: "Full-screen request" })).toBeInTheDocument();
    list().scrollTop = 0;
    act(() => window.history.back());
    await waitFor(() => expect(fsRegion()).toBeNull());
    expectListRestored();
  });

  // TS-03: entry from the detail returns to the detail on Close, Esc and Back.
  it.each(["close", "esc", "back"])("returns to the request detail (%s)", async (how) => {
    const user = userEvent.setup();
    window.history.replaceState(null, "", "/inbox?item=req-b");
    render(<InboxView />);
    expect(await screen.findByLabelText("Draft body")).toHaveValue("Body of B");
    await user.click(screen.getByRole("button", { name: "Open request full screen" }));
    expect(window.location.search).toBe("?item=req-b&view=full");
    expect(await screen.findByRole("region", { name: "Full-screen request" })).toBeInTheDocument();
    if (how === "close") await user.click(screen.getByRole("button", { name: "Close full screen" }));
    else if (how === "esc") await user.keyboard("{Escape}");
    else act(() => window.history.back());
    await waitFor(() => expect(fsRegion()).toBeNull());
    expect(window.location.search).toBe("?item=req-b");
    expect(row("req-b")).toHaveAttribute("aria-current", "true");
    expect(screen.getByLabelText("Draft body")).toHaveValue("Body of B");
  });

  // TS-04: the brief, the requirements and a note each open full screen via doc=.
  it.each([
    ["brief", "Open brief full screen", "Brief body"],
    ["requirements", "Open requirements full screen", "# R"],
    ["n2", "Open note Constraints full screen", "Only synthetic data"],
  ])("opens the %s document full screen and returns to the detail", async (key, opener, text) => {
    const user = userEvent.setup();
    window.history.replaceState(null, "", "/inbox?item=req-s");
    render(<InboxView />);
    await user.click(await screen.findByRole("button", { name: opener }));
    expect(new URLSearchParams(window.location.search).get("doc")).toBe(key);
    const region = await screen.findByRole("region", { name: "Full-screen request" });
    expect(await within(region).findByText(text)).toBeInTheDocument();
    await user.click(within(region).getByRole("button", { name: "Close full screen" }));
    expect(fsRegion()).toBeNull();
    expect(window.location.search).toBe("?item=req-s");
    expect(row("req-s")).toHaveAttribute("aria-current", "true");
  });

  // TS-05: after a direct load, Close and Esc land on the request, selected and in view.
  it.each(["close", "esc"])("lands on the request after a direct load (%s)", async (how) => {
    const user = userEvent.setup();
    window.history.replaceState(null, "", "/inbox?item=req-a&view=full");
    render(<InboxView />);
    const region = await screen.findByRole("region", { name: "Full-screen request" });
    expect(await within(region).findByRole("heading", { name: "Sample request A" })).toBeInTheDocument();
    await screen.findByText("Sample request B");
    if (how === "close") await user.click(screen.getByRole("button", { name: "Close full screen" }));
    else await user.keyboard("{Escape}");
    expect(fsRegion()).toBeNull();
    expect(window.location.search).toBe("?item=req-a");
    await waitFor(() => expect(row("req-a")).toHaveAttribute("aria-current", "true"));
    const scrolled = vi.mocked(Element.prototype.scrollIntoView).mock.contexts;
    expect(scrolled).toContain(row("req-a"));
  });

  // TS-06: after a direct load, Back follows browser history without re-entering.
  it("follows browser history on Back after a direct load", async () => {
    window.history.replaceState(null, "", "/elsewhere");
    window.history.pushState(null, "", "/inbox?item=req-a&view=full");
    render(<InboxView />);
    expect(await screen.findByRole("region", { name: "Full-screen request" })).toBeInTheDocument();
    const push = vi.spyOn(window.history, "pushState");
    const replace = vi.spyOn(window.history, "replaceState");
    act(() => window.history.back());
    await waitFor(() => expect(fsRegion()).toBeNull());
    expect(window.location.pathname).toBe("/elsewhere");
    expect(push).not.toHaveBeenCalled();
    expect(replace).not.toHaveBeenCalled();
    push.mockRestore();
    replace.mockRestore();
  });

  // TS-07: an unknown or inaccessible id, or an unknown doc key, shows not-found.
  it.each([
    ["/inbox?item=req-missing&view=full", "Not found (404)"],
    ["/inbox?item=req-locked&view=full", "Forbidden (403)"],
  ])("shows not-found for %s", async (url, error) => {
    draft.mockImplementation((id: string) =>
      id in items ? Promise.resolve(items[id]) : Promise.reject(new Error(error)),
    );
    window.history.replaceState(null, "", url);
    render(<InboxView />);
    const region = await screen.findByRole("region", { name: "Full-screen request" });
    expect(await within(region).findByRole("alert")).toHaveTextContent(/not found/i);
    expect(within(region).getByRole("button", { name: "Close full screen" })).toBeInTheDocument();
  });

  it("shows not-found for an unknown document of a valid request", async () => {
    window.history.replaceState(null, "", "/inbox?item=req-a&view=full&doc=nope");
    render(<InboxView />);
    const region = await screen.findByRole("region", { name: "Full-screen request" });
    expect(await within(region).findByRole("alert")).toHaveTextContent(/document was not found/i);
  });

  // TS-11: focus returns to the opener; Close has an accessible name.
  it("returns focus to the control that opened it", async () => {
    const user = userEvent.setup();
    render(<InboxView />);
    await screen.findByText("Sample request B");
    const opener = screen.getByRole("button", { name: "Open Sample request B full screen" });
    opener.focus();
    await user.keyboard("{Enter}");
    const close = await screen.findByRole("button", { name: "Close full screen" });
    expect(close).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(fsRegion()).toBeNull();
    expect(opener).toHaveFocus();
  });

  // TS-08: a handover's full-screen URL opens the Design tab and that handover, not the request view.
  it("opens a handover full screen from its URL", async () => {
    const card = {
      project: "/code/acme", slug: "acme", status: "drafted" as const, instructionHash: "ih",
      approvalValid: false, copyStale: false, rejection: null,
    };
    const viewMock = vi.mocked(solutionApi.view);
    viewMock.mockResolvedValue(solutionView({ handovers: [card] }));
    try {
      window.history.replaceState(null, "", "/inbox?item=req-s&handover=acme&view=full");
      render(<InboxView />);
      expect(await screen.findByRole("region", { name: "Full-screen handover acme" })).toBeInTheDocument();
      expect(fsRegion()).toBeNull();
      expect(screen.getByRole("tab", { name: "Design" })).toHaveAttribute("aria-selected", "true");
    } finally {
      viewMock.mockResolvedValue(solutionView());
    }
  });
});
