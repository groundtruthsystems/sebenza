import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import NotesPanel, { NoteAdder, isDefaultOnly, notesChangedSinceConversion } from "./NotesPanel";
import {
  addInboxNote,
  deleteInboxNote,
  reorderInboxNotes,
  updateInboxNote,
  type InboxNote,
} from "./api";

vi.mock("./api", () => ({
  addInboxNote: vi.fn(),
  updateInboxNote: vi.fn(),
  deleteInboxNote: vi.fn(),
  reorderInboxNotes: vi.fn(),
}));

const note = (over: Partial<InboxNote>): InboxNote => ({
  id: "n1",
  type: "general",
  title: "Note",
  body: "",
  hash: "h1",
  ...over,
});

const A = note({ id: "n1", title: "Problem", body: "Graph view", hash: "ha" });
const B = note({ id: "ac11", type: "acceptance_criteria", title: "Acceptance Criteria", body: "- AC", hash: "hb" });

function setup(over: Partial<Parameters<typeof NotesPanel>[0]> = {}) {
  const onchange = vi.fn();
  const onhelp = vi.fn();
  render(
    <NotesPanel
      draftId="01D"
      notes={[A, B]}
      listHash="lh"
      onchange={onchange}
      onhelp={onhelp}
      {...over}
    />,
  );
  return { onchange, onhelp };
}

const card = (title: string) => screen.getByRole("region", { name: `Note: ${title}` });

beforeEach(() => vi.clearAllMocks());

describe("NotesPanel", () => {
  it("shows every note with its title and type", () => {
    setup();
    expect(within(card("Problem")).getByLabelText("Note title")).toHaveValue("Problem");
    expect(within(card("Acceptance Criteria")).getByLabelText("Note type")).toHaveValue(
      "acceptance_criteria",
    );
  });

  // TS-21: a conflict on one note shows only on that note; others stay editable.
  it("keeps a conflict to the note that has it", async () => {
    const user = userEvent.setup();
    vi.mocked(updateInboxNote).mockResolvedValueOnce({
      kind: "conflict",
      current: { ...A, body: "Theirs", hash: "ha2" },
      listHash: "lh2",
    });
    setup();
    const a = card("Problem");
    await user.type(within(a).getByLabelText("Note body: Problem"), "!");
    await user.click(within(a).getByRole("button", { name: "Save note" }));
    expect(updateInboxNote).toHaveBeenCalledWith("01D", "n1", { type: "general", title: "Problem", body: "Graph view!" }, "ha");
    expect(within(a).getByRole("alert")).toHaveTextContent(/changed since you opened it/i);
    expect(within(card("Acceptance Criteria")).queryByRole("alert")).toBeNull();
    expect(within(card("Acceptance Criteria")).getByLabelText("Note body: Acceptance Criteria")).toBeEnabled();

    vi.mocked(updateInboxNote).mockResolvedValueOnce({
      kind: "ok",
      notes: [{ ...A, body: "Graph view!", hash: "ha3" }, B],
      listHash: "lh3",
    });
    await user.click(within(a).getByRole("button", { name: "Keep mine" }));
    expect(updateInboxNote).toHaveBeenLastCalledWith("01D", "n1", { type: "general", title: "Problem", body: "Graph view!" }, "ha2");
  });

  // TS-27: deleting asks first and sends nothing until confirmed.
  it("deletes only after confirmation", async () => {
    const user = userEvent.setup();
    vi.mocked(deleteInboxNote).mockResolvedValue({ kind: "ok", notes: [A], listHash: "lh2" });
    const { onchange } = setup();
    const b = card("Acceptance Criteria");
    await user.click(within(b).getByRole("button", { name: "Delete note" }));
    expect(deleteInboxNote).not.toHaveBeenCalled();
    await user.click(within(b).getByRole("button", { name: "Confirm delete" }));
    expect(deleteInboxNote).toHaveBeenCalledWith("01D", "ac11", "lh");
    expect(onchange).toHaveBeenCalledWith([A], "lh2");
  });

  it("reorders with move buttons against the list hash", async () => {
    const user = userEvent.setup();
    vi.mocked(reorderInboxNotes).mockResolvedValue({ kind: "ok", notes: [B, A], listHash: "lh2" });
    const { onchange } = setup();
    await user.click(within(card("Problem")).getByRole("button", { name: "Move note down" }));
    expect(reorderInboxNotes).toHaveBeenCalledWith("01D", ["ac11", "n1"], "lh");
    expect(onchange).toHaveBeenCalledWith([B, A], "lh2");
  });

  // TS-72: draft help is aimed at one note.
  it("asks the agent to improve one note", async () => {
    const user = userEvent.setup();
    const { onhelp } = setup();
    await user.click(within(card("Acceptance Criteria")).getByRole("button", { name: "Improve with agent" }));
    expect(onhelp).toHaveBeenCalledWith(B);
  });
});

describe("NoteAdder", () => {
  // TS-19 / TS-56: adding a typed note calls the API and shows the PHI notice.
  it("adds a typed note and reports the new list", async () => {
    const user = userEvent.setup();
    const added = note({ id: "ac11", type: "acceptance_criteria", title: "Acceptance Criteria" });
    vi.mocked(addInboxNote).mockResolvedValue({ kind: "ok", notes: [A, added], listHash: "lh2", note: added });
    const onchange = vi.fn();
    render(<NoteAdder draftId="01D" listHash="lh" onchange={onchange} />);
    expect(screen.getByText(/PHI/)).toBeInTheDocument();
    await user.selectOptions(screen.getByLabelText("New note type"), "acceptance_criteria");
    expect(screen.getByLabelText("New note title")).toHaveValue("Acceptance Criteria");
    await user.click(screen.getByRole("button", { name: "Add note" }));
    expect(addInboxNote).toHaveBeenCalledWith(
      "01D",
      { type: "acceptance_criteria", title: "Acceptance Criteria", body: "" },
      "lh",
    );
    expect(onchange).toHaveBeenCalledWith([A, added], "lh2");
  });

  it("says when the list moved underneath it", async () => {
    const user = userEvent.setup();
    vi.mocked(addInboxNote).mockResolvedValue({ kind: "conflict", current: null, listHash: "lh9" });
    render(<NoteAdder draftId="01D" listHash="lh" onchange={vi.fn()} onstale={vi.fn()} />);
    await user.type(screen.getByLabelText("New note title"), "Context");
    await user.click(screen.getByRole("button", { name: "Add note" }));
    expect(screen.getByRole("alert")).toHaveTextContent(/changed/i);
  });
});

describe("note helpers", () => {
  it("treats a lone default note as the plain body", () => {
    expect(isDefaultOnly([note({})])).toBe(true);
    expect(isDefaultOnly([note({ title: "Problem" })])).toBe(false);
    expect(isDefaultOnly([A, B])).toBe(false);
    expect(isDefaultOnly([])).toBe(true);
  });

  // TS-67: notes edited after conversion are flagged against the last wave.
  it("flags notes changed since the last conversion", () => {
    const conv = (h?: string) => ({ outcome: "created", notesListHash: h });
    expect(notesChangedSinceConversion([conv("old")], "new")).toBe(true);
    expect(notesChangedSinceConversion([conv("old"), conv("new")], "new")).toBe(false);
    expect(notesChangedSinceConversion([conv(undefined)], "new")).toBe(false);
    expect(notesChangedSinceConversion([], "new")).toBe(false);
  });
});
