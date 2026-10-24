import type { SeqArrow, SeqBlock, SeqItem, SequenceDoc } from "../model";
import { isPlainComment, lineText, readMeta, statements, unLineText, type Meta } from "./text";

/** Sequence diagrams. Rows map one-to-one onto Mermaid statements. */

export const SEQ_ARROWS: { value: SeqArrow; label: string }[] = [
  { value: "->>", label: "Call (->>)" },
  { value: "-->>", label: "Reply (-->>)" },
  { value: "-)", label: "Async (-))" },
  { value: "--)", label: "Async reply (--))" },
  { value: "-x", label: "Failed call (-x)" },
  { value: "--x", label: "Failed reply (--x)" },
  { value: "->", label: "Line (->)" },
  { value: "-->", label: "Dotted line (-->)" },
];

export const SEQ_BLOCKS: SeqBlock[] = ["alt", "opt", "loop", "par", "critical", "break"];

/** The keyword that divides a block into branches, if it has one. */
export const BRANCH_WORD: Partial<Record<SeqBlock, string>> = {
  alt: "else",
  par: "and",
  critical: "option",
};

export function sequenceToMermaid(doc: SequenceDoc): string {
  const out = ["sequenceDiagram"];
  if (doc.autonumber) out.push("  autonumber");
  for (const p of doc.participants) {
    const alias = p.label.trim() && p.label !== p.id ? ` as ${lineText(p.label)}` : "";
    out.push(`  ${p.kind} ${p.id}${alias}`);
  }
  // Blocks are balanced on the way out: a stray `end` or `else` is dropped
  // and an unclosed block is closed, so a half-edited diagram still renders.
  const stack: SeqBlock[] = [];
  const pad = () => "  ".repeat(stack.length + 1);
  for (const item of doc.items) {
    switch (item.t) {
      case "msg":
        out.push(
          `${pad()}${item.from}${item.arrow}${item.act ?? ""}${item.to}: ${lineText(item.text)}`.trimEnd(),
        );
        break;
      case "note":
        out.push(`${pad()}Note ${item.pos} ${item.who.join(",")}: ${lineText(item.text)}`.trimEnd());
        break;
      case "activate":
        out.push(`${pad()}${item.on ? "activate" : "deactivate"} ${item.who}`);
        break;
      case "start":
        out.push(`${pad()}${item.block} ${lineText(item.text)}`.trimEnd());
        stack.push(item.block);
        break;
      case "else": {
        const word = stack.length ? BRANCH_WORD[stack[stack.length - 1]] : undefined;
        if (!word) break;
        out.push(`${"  ".repeat(stack.length)}${word} ${lineText(item.text)}`.trimEnd());
        break;
      }
      case "end":
        if (!stack.length) break;
        stack.pop();
        out.push(`${pad()}end`);
        break;
    }
  }
  while (stack.length) {
    stack.pop();
    out.push(`${pad()}end`);
  }
  return out.join("\n");
}

const ID = String.raw`[A-Za-z_][\w]*`;
const ARROW = String.raw`(-->>|->>|--\)|-\)|--x|-x|-->|->)`;
const MESSAGE = new RegExp(String.raw`^(${ID})\s*${ARROW}\s*([+-])?\s*(${ID})\s*:(.*)$`);
const NOTE = new RegExp(String.raw`^[Nn]ote\s+(over|left of|right of)\s+(${ID}(?:\s*,\s*${ID})?)\s*:(.*)$`);

/** Parse a sequence diagram, or null if it uses anything the canvas cannot keep. */
export function sequenceFromMermaid(src: string): SequenceDoc | null {
  const lines = statements(src);
  if (lines[0] !== "sequenceDiagram") return null;
  const doc: SequenceDoc = { kind: "sequence", autonumber: false, participants: [], items: [] };
  const meta: Meta = {};
  const known = new Set<string>();
  const ensure = (id: string) => {
    if (known.has(id)) return;
    known.add(id);
    doc.participants.push({ id, label: id, kind: "participant" });
  };
  let depth = 0;

  for (const line of lines.slice(1)) {
    if (readMeta(line, meta)) continue;
    if (isPlainComment(line)) return null;
    let m: RegExpExecArray | null;
    if (line === "autonumber") {
      doc.autonumber = true;
    } else if ((m = new RegExp(String.raw`^(participant|actor)\s+(${ID})(?:\s+as\s+(.+))?$`).exec(line))) {
      if (known.has(m[2])) return null; // re-declaring mid-diagram changes its box
      known.add(m[2]);
      doc.participants.push({
        id: m[2],
        label: m[3] ? unLineText(m[3]) : m[2],
        kind: m[1] as "participant" | "actor",
      });
    } else if ((m = MESSAGE.exec(line))) {
      ensure(m[1]);
      ensure(m[4]);
      const item: SeqItem = {
        t: "msg",
        from: m[1],
        to: m[4],
        arrow: m[2] as SeqArrow,
        text: unLineText(m[5]),
      };
      if (m[3]) item.act = m[3] as "+" | "-";
      doc.items.push(item);
    } else if ((m = NOTE.exec(line))) {
      const who = m[2].split(",").map((s) => s.trim());
      who.forEach(ensure);
      doc.items.push({ t: "note", pos: m[1] as "over" | "left of" | "right of", who, text: unLineText(m[3]) });
    } else if ((m = new RegExp(String.raw`^(activate|deactivate)\s+(${ID})$`).exec(line))) {
      ensure(m[2]);
      doc.items.push({ t: "activate", who: m[2], on: m[1] === "activate" });
    } else if ((m = /^(alt|opt|loop|par|critical|break)(?:\s+(.*))?$/.exec(line))) {
      doc.items.push({ t: "start", block: m[1] as SeqBlock, text: unLineText(m[2] ?? "") });
      depth++;
    } else if ((m = /^(else|and|option)(?:\s+(.*))?$/.exec(line))) {
      if (!depth) return null;
      doc.items.push({ t: "else", text: unLineText(m[2] ?? "") });
    } else if (line === "end") {
      if (!depth) return null;
      depth--;
      doc.items.push({ t: "end" });
    } else {
      return null;
    }
  }
  if (depth) return null;
  return doc;
}
