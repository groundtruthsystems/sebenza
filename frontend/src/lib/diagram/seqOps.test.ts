import { describe, expect, it } from "vitest";
import type { SeqItem, SequenceDoc } from "./model";
import {
  addParticipant,
  canBranch,
  canWrap,
  deleteRows,
  depths,
  moveParticipant,
  removeParticipant,
  renameParticipant,
  wrapRows,
} from "./seqOps";

const msg = (from: string, to: string, text = "m"): SeqItem => ({ t: "msg", from, to, arrow: "->>", text });

const doc = (items: SeqItem[]): SequenceDoc => ({
  kind: "sequence",
  autonumber: false,
  participants: [
    { id: "A", label: "A", kind: "actor" },
    { id: "B", label: "B", kind: "participant" },
  ],
  items,
});

describe("sequence ops", () => {
  it("adds participants with unique ids", () => {
    let d = doc([]);
    ({ doc: d } = addParticipant(d, "participant"));
    const { id } = addParticipant(d, "participant");
    expect(id).toBe("Service2");
  });

  it("renames a participant everywhere it is used", () => {
    const d = doc([msg("A", "B"), { t: "note", pos: "over", who: ["A", "B"], text: "n" }, { t: "activate", who: "B", on: true }]);
    const r = renameParticipant(d, "B", "Order service");
    expect(r.id).toBe("Order_service");
    expect(r.doc.participants[1]).toMatchObject({ id: "Order_service", label: "Order service" });
    expect(r.doc.items).toEqual([
      msg("A", "Order_service"),
      { t: "note", pos: "over", who: ["A", "Order_service"], text: "n" },
      { t: "activate", who: "Order_service", on: true },
    ]);
  });

  it("removing a participant removes its rows", () => {
    expect(removeParticipant(doc([msg("A", "B"), msg("A", "A")]), "B").items).toEqual([msg("A", "A")]);
  });

  it("reorders participants within bounds", () => {
    expect(moveParticipant(doc([]), "A", 1).participants.map((p) => p.id)).toEqual(["B", "A"]);
    expect(moveParticipant(doc([]), "A", -1).participants.map((p) => p.id)).toEqual(["A", "B"]);
  });

  it("wraps a balanced range and refuses an unbalanced one", () => {
    const d = doc([msg("A", "B"), msg("B", "A")]);
    const wrapped = wrapRows(d, 0, 1, "loop");
    expect(wrapped.items.map((i) => i.t)).toEqual(["start", "msg", "msg", "end"]);
    expect(canWrap(wrapped.items, 0, 1)).toBe(false);
    expect(canWrap(wrapped.items, 0, 3)).toBe(true);
    expect(depths(wrapped.items)).toEqual([0, 1, 1, 0]);
  });

  it("allows a branch only inside blocks that have one", () => {
    const alt = wrapRows(doc([msg("A", "B")]), 0, 0, "alt");
    const loop = wrapRows(doc([msg("A", "B")]), 0, 0, "loop");
    expect(canBranch(alt.items, 1)).toBe(true);
    expect(canBranch(alt.items, 0)).toBe(true);
    expect(canBranch(loop.items, 1)).toBe(false);
    expect(canBranch(doc([msg("A", "B")]).items, 0)).toBe(false);
  });

  it("deleting a block's start unwraps it, keeping the rows inside", () => {
    const d = doc([
      { t: "start", block: "alt", text: "ok" },
      msg("A", "B", "yes"),
      { t: "else", text: "no" },
      msg("B", "A", "nope"),
      { t: "end" },
    ]);
    expect(deleteRows(d, 0, 0).items).toEqual([msg("A", "B", "yes"), msg("B", "A", "nope")]);
    expect(deleteRows(d, 4, 4).items).toEqual([msg("A", "B", "yes"), msg("B", "A", "nope")]);
    expect(deleteRows(d, 2, 2).items.map((i) => i.t)).toEqual(["start", "msg", "msg", "end"]);
  });
});
