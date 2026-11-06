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

// TS-38 (vitest reader): the SPA's reader parses the very fixture the daemon
// writes and the agent-extensions-skills repo copies.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { InboxNotesManifestSchema } from "./schemas";

describe("inbox-notes.json contract", () => {
  it("parses the golden fixture and ignores unknown fields", () => {
    const raw = JSON.parse(
      readFileSync(
        resolve(process.cwd(), "../crates/common/testdata/contracts/inbox-notes.v1.json"),
        "utf8",
      ),
    ) as Record<string, unknown>;
    const m = InboxNotesManifestSchema.parse({ ...raw, addedLater: true });
    expect(m.schema).toBe("sebenza-inbox-notes-v1");
    expect(m.notes.filter((n) => n.type === "acceptance_criteria").map((n) => n.body)).toEqual([
      "- shows worktrees",
    ]);
    expect(m.siblings[0].branch).toBe("agent-extensions-skills");
  });
});
