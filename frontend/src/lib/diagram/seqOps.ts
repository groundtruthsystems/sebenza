import { toIdentifier, uniqueId, type SeqBlock, type SeqItem, type SequenceDoc } from "./model";
import { BRANCH_WORD } from "./mermaid/sequence";

/** Edits to a sequence diagram's participants and rows, as pure functions. */

export function addParticipant(doc: SequenceDoc, kind: "participant" | "actor"): { doc: SequenceDoc; id: string } {
  const base = kind === "actor" ? "User" : "Service";
  const id = uniqueId(base, doc.participants.map((p) => p.id));
  return { doc: { ...doc, participants: [...doc.participants, { id, label: id, kind }] }, id };
}

/**
 * Rename a participant. The id is what every message refers to, so it is
 * re-derived from the label and the rows follow it.
 */
export function renameParticipant(doc: SequenceDoc, id: string, label: string): { doc: SequenceDoc; id: string } {
  const others = doc.participants.filter((p) => p.id !== id).map((p) => p.id);
  const nextId = uniqueId(toIdentifier(label), others);
  const swap = (x: string) => (x === id ? nextId : x);
  return {
    id: nextId,
    doc: {
      ...doc,
      participants: doc.participants.map((p) => (p.id === id ? { ...p, id: nextId, label } : p)),
      items: doc.items.map((it) => {
        if (it.t === "msg") return { ...it, from: swap(it.from), to: swap(it.to) };
        if (it.t === "note") return { ...it, who: it.who.map(swap) };
        if (it.t === "activate") return { ...it, who: swap(it.who) };
        return it;
      }),
    },
  };
}

/** Remove a participant and every row that mentions it. */
export function removeParticipant(doc: SequenceDoc, id: string): SequenceDoc {
  return {
    ...doc,
    participants: doc.participants.filter((p) => p.id !== id),
    items: doc.items.filter((it) => {
      if (it.t === "msg") return it.from !== id && it.to !== id;
      if (it.t === "note") return !it.who.includes(id);
      if (it.t === "activate") return it.who !== id;
      return true;
    }),
  };
}

export function moveParticipant(doc: SequenceDoc, id: string, by: -1 | 1): SequenceDoc {
  const i = doc.participants.findIndex((p) => p.id === id);
  const j = i + by;
  if (i < 0 || j < 0 || j >= doc.participants.length) return doc;
  const participants = [...doc.participants];
  [participants[i], participants[j]] = [participants[j], participants[i]];
  return { ...doc, participants };
}

/** Insert rows at `index` (clamped to the end). */
export function insertItems(doc: SequenceDoc, index: number, items: SeqItem[]): SequenceDoc {
  const at = Math.max(0, Math.min(index, doc.items.length));
  return { ...doc, items: [...doc.items.slice(0, at), ...items, ...doc.items.slice(at)] };
}

export function updateItem(doc: SequenceDoc, index: number, patch: Partial<SeqItem>): SequenceDoc {
  return {
    ...doc,
    items: doc.items.map((it, i) => (i === index ? ({ ...it, ...patch } as SeqItem) : it)),
  };
}

/** For each `start` row its matching `end` row, and the reverse. */
export function matchBlocks(items: SeqItem[]): Map<number, number> {
  const pairs = new Map<number, number>();
  const stack: number[] = [];
  items.forEach((it, i) => {
    if (it.t === "start") stack.push(i);
    if (it.t === "end" && stack.length) {
      const s = stack.pop()!;
      pairs.set(s, i);
      pairs.set(i, s);
    }
  });
  return pairs;
}

/** Nesting depth of each row; a block's own start/else/end rows sit at its outer depth. */
export function depths(items: SeqItem[]): number[] {
  let d = 0;
  return items.map((it) => {
    if (it.t === "end") d = Math.max(0, d - 1);
    const mine = it.t === "else" ? Math.max(0, d - 1) : d;
    if (it.t === "start") d++;
    return mine;
  });
}

/** The block a row sits directly inside, if any. */
export function enclosingBlock(items: SeqItem[], index: number): { start: number; block: SeqBlock } | null {
  let level = 0;
  for (let i = index - 1; i >= 0; i--) {
    const it = items[i];
    if (it.t === "end") level++;
    if (it.t === "start") {
      if (level === 0) return { start: i, block: it.block };
      level--;
    }
  }
  return null;
}

/** Whether a branch divider (else/and/option) may follow row `index`. */
export function canBranch(items: SeqItem[], index: number): boolean {
  const row = items[index];
  const inside = row?.t === "start" ? { block: row.block } : enclosingBlock(items, index);
  return !!inside && !!BRANCH_WORD[inside.block];
}

/** A range can be wrapped only if it opens and closes the same blocks. */
export function canWrap(items: SeqItem[], from: number, to: number): boolean {
  if (from < 0 || to >= items.length || from > to) return false;
  let level = 0;
  for (let i = from; i <= to; i++) {
    const it = items[i];
    if (it.t === "start") level++;
    if (it.t === "end" && --level < 0) return false;
    if (it.t === "else" && level === 0) return false;
  }
  return level === 0;
}

export function wrapRows(doc: SequenceDoc, from: number, to: number, block: SeqBlock): SequenceDoc {
  if (!canWrap(doc.items, from, to)) return doc;
  const items = [...doc.items];
  items.splice(to + 1, 0, { t: "end" });
  items.splice(from, 0, { t: "start", block, text: block === "loop" ? "repeat" : "condition" });
  return { ...doc, items };
}

/**
 * Delete rows. Removing either end of a block unwraps it - the start, its
 * dividers and its end go, and the rows inside stay.
 */
export function deleteRows(doc: SequenceDoc, from: number, to: number): SequenceDoc {
  const pairs = matchBlocks(doc.items);
  const gone = new Set<number>();
  for (let i = from; i <= to; i++) {
    gone.add(i);
    const it = doc.items[i];
    if (it?.t !== "start" && it?.t !== "end") continue;
    const other = pairs.get(i);
    if (other === undefined) continue;
    gone.add(other);
    // Dividers of an unwrapped block go too.
    const [s, e] = it.t === "start" ? [i, other] : [other, i];
    for (let k = s + 1; k < e; k++) {
      if (doc.items[k].t === "else" && enclosingBlock(doc.items, k)?.start === s) gone.add(k);
    }
  }
  return { ...doc, items: doc.items.filter((_, i) => !gone.has(i)) };
}

/** Move one row up or down, refusing moves that would cross its own block's edge. */
export function moveRow(doc: SequenceDoc, index: number, by: -1 | 1): SequenceDoc {
  const j = index + by;
  if (j < 0 || j >= doc.items.length) return doc;
  const a = doc.items[index];
  if (a.t === "start" || a.t === "end" || a.t === "else") return doc;
  const items = [...doc.items];
  [items[index], items[j]] = [items[j], items[index]];
  return { ...doc, items };
}
