import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const drafts = vi.fn();
const draft = vi.fn();
const patch = vi.fn();

vi.mock("./api", () => ({
  fetchInboxDrafts: (...a: unknown[]) => drafts(...a),
  fetchInboxDraft: (...a: unknown[]) => draft(...a),
  patchInboxDraft: (...a: unknown[]) => patch(...a),
  createInboxDraft: vi.fn(),
  deleteInboxDraft: vi.fn(),
  saveInboxDraftBody: vi.fn(),
  loadInboxControlToken: () => Promise.resolve(),
  fetchProjects: () => Promise.resolve([{ prefix: "demo", name: "demo" }]),
}));

// Mermaid pulls a large async graph that has no place in a list-behaviour test.
vi.mock("./inboxMarkdown", () => ({
  renderDraftMarkdown: (s: string) => Promise.resolve(`<p>${s}</p>`),
  sanitize: (s: string) => s,
}));

import InboxView from "./InboxView";

const summary = (over: Partial<Record<string, unknown>> = {}) => ({
  id: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  title: "Rework the scorer",
  status: "Draft",
  updatedAt: "2026-09-28T00:00:00Z",
  project: null,
  isRaw: false,
  ...over,
});

beforeEach(() => {
  vi.clearAllMocks();
  drafts.mockResolvedValue({ drafts: [summary()] });
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
    expect(await screen.findByText("unresolved")).toBeInTheDocument();
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
});
