/**
 * Shared text handling for the Mermaid serializers.
 *
 * Labels are user text going into a language with its own syntax. Everything
 * here exists so a label can never end a statement early and start another -
 * a newline in a label would otherwise let it write `click` or `classDef`.
 */

import type { GraphDoc, GraphNode } from "../model";

/** Collapse whitespace runs (including newlines) to single spaces. */
export function oneLine(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

/** A label inside `"..."` in a flowchart or state diagram. */
export function quoted(text: string): string {
  return `"${oneLine(text).replace(/"/g, "#quot;")}"`;
}

export function unquote(text: string): string {
  const t = text.trim();
  const inner = t.length >= 2 && t.startsWith('"') && t.endsWith('"') ? t.slice(1, -1) : t;
  return inner.replace(/#quot;/g, '"');
}

/**
 * Free text running to the end of a line (sequence messages, notes, class
 * relation labels). `;` ends a statement in Mermaid, so it is written as its
 * entity code.
 */
export function lineText(text: string): string {
  return oneLine(text).replace(/;/g, "#59;");
}

export function unLineText(text: string): string {
  return text.trim().replace(/#59;/g, ";");
}

/** A class member or ER attribute: one line, no block braces. */
export function memberText(text: string): string {
  return oneLine(text.replace(/[{}]/g, ""));
}

// --- the editor's own metadata, carried as Mermaid comments ---------------

const META = /^\s*%%\s*sebenza:(\w+)\s*(.*)$/;

export interface Meta {
  kind?: string;
  layout?: Record<string, number[]>;
}

/** Read a `%% sebenza:<key> <value>` line into `into`; false if it is not one. */
export function readMeta(line: string, into: Meta): boolean {
  const m = META.exec(line);
  if (!m) return false;
  if (m[1] === "kind") {
    into.kind = m[2].trim();
  } else if (m[1] === "layout") {
    into.layout = parseLayout(m[2]);
  }
  return true;
}

/**
 * The layout comment is part of a draft anyone may have edited, so it is
 * validated rather than trusted: only arrays of finite numbers survive.
 */
function parseLayout(json: string): Record<string, number[]> {
  try {
    const raw: unknown = JSON.parse(json);
    if (!raw || typeof raw !== "object" || Array.isArray(raw)) return {};
    const out: Record<string, number[]> = {};
    for (const [k, v] of Object.entries(raw as Record<string, unknown>)) {
      if (Array.isArray(v) && v.length >= 2 && v.every((x) => Number.isFinite(x))) {
        out[k] = v as number[];
      }
    }
    return out;
  } catch {
    return {};
  }
}

export function layoutLine(nodes: GraphNode[]): string | null {
  const layout: Record<string, number[]> = {};
  for (const n of nodes) {
    if (!n.pos) continue;
    const xy = [Math.round(n.pos.x), Math.round(n.pos.y)];
    layout[n.id] = n.size ? [...xy, Math.round(n.size.w), Math.round(n.size.h)] : xy;
  }
  if (Object.keys(layout).length === 0) return null;
  return `%% sebenza:layout ${JSON.stringify(layout)}`;
}

export function applyLayout(doc: GraphDoc, layout: Record<string, number[]> | undefined): GraphDoc {
  if (!layout) return doc;
  for (const n of doc.nodes) {
    const l = layout[n.id];
    if (!l) continue;
    n.pos = { x: l[0], y: l[1] };
    if (l.length >= 4) n.size = { w: l[2], h: l[3] };
  }
  return doc;
}

/** Non-blank lines, with any trailing `;` statement terminator removed. */
export function statements(src: string): string[] {
  return src
    .split("\n")
    .map((l) => l.trim().replace(/;$/, "").trim())
    .filter((l) => l !== "");
}

/** Plain `%%` comments carry nothing the canvas can keep, so they force source mode. */
export function isPlainComment(line: string): boolean {
  return line.startsWith("%%");
}

export function directionLine(doc: GraphDoc, keyword = "direction"): string | null {
  return doc.direction === "TB" ? null : `  ${keyword} ${doc.direction}`;
}

export function parseDirection(text: string): GraphDoc["direction"] | null {
  const d = text.trim().toUpperCase();
  if (d === "TD" || d === "TB") return "TB";
  if (d === "LR" || d === "RL" || d === "BT") return d;
  return null;
}
