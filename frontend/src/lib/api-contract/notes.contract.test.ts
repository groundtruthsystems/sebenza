import { describe, expect, it } from "vitest";
import {
  InboxDraftSchema,
  InboxNoteConflictSchema,
  InboxNoteWriteSchema,
  InboxNotesSchema,
} from "./schemas";

// TS-70 (notes): the Zod contract parses the shapes the Rust routes emit
// (NoteWire, notes_json, note_outcome's 409), so a drift fails here.
const note = {
  id: "n1",
  type: "general",
  title: "Note",
  body: "Graph view.",
  hash: "0123abcd",
};

describe("notes contract", () => {
  it("parses list, write and conflict bodies", () => {
    expect(InboxNotesSchema.parse({ notes: [note], listHash: "lh" }).notes[0].type).toBe(
      "general",
    );
    expect(
      InboxNoteWriteSchema.parse({ notes: [note], listHash: "lh", note }).note.id,
    ).toBe("n1");
    expect(
      InboxNoteConflictSchema.parse({
        error: "the note changed since you read it",
        current: null,
        listHash: "lh",
      }).current,
    ).toBeNull();
  });

  it("defaults notes on a draft from an older server", () => {
    const draft = InboxDraftSchema.parse({
      id: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
      title: "T",
      status: "Draft",
      createdAt: "",
      updatedAt: "",
      body: "",
      bodyHash: "h",
      project: null,
      raw: null,
    });
    expect(draft.notes).toEqual([]);
    expect(draft.listHash).toBe("");
  });
});
