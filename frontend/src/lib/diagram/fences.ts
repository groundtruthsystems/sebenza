/**
 * Finding and rewriting ```mermaid fences in a draft body.
 *
 * The preview renderer (`inboxMarkdown.ts`) and the diagram editor must agree
 * on what counts as a diagram and in what order, or "edit diagram 2" in the
 * preview would open a different fence than the one clicked. Both therefore
 * use `mermaidFencePattern` from here rather than their own regex.
 */

/**
 * A fresh global regex for one ```mermaid fence; group 1 is its source.
 *
 * A function rather than a shared constant: a global regex carries
 * `lastIndex` between calls, which would make a second scan start mid-body.
 */
export function mermaidFencePattern(): RegExp {
  return /```mermaid\n([\s\S]*?)```/g;
}

export interface Fence {
  /** Ordinal among the body's mermaid fences, from 0. */
  index: number;
  /** Offset of the opening backticks. */
  start: number;
  /** Offset just past the closing backticks. */
  end: number;
  /** The diagram source between the fences, without the trailing newline. */
  source: string;
}

export function listFences(body: string): Fence[] {
  const fences: Fence[] = [];
  for (const m of body.matchAll(mermaidFencePattern())) {
    const start = m.index;
    fences.push({
      index: fences.length,
      start,
      end: start + m[0].length,
      source: m[1].replace(/\n$/, ""),
    });
  }
  return fences;
}

/** The fence whose span contains `offset`, if any. */
export function fenceAt(body: string, offset: number): Fence | null {
  return listFences(body).find((f) => offset >= f.start && offset <= f.end) ?? null;
}

export function fenceText(source: string): string {
  return "```mermaid\n" + source.replace(/\s+$/, "") + "\n```";
}

/**
 * Replace the source of fence `index`, leaving every other byte alone.
 * Returns null when there is no such fence - the body changed underneath.
 */
export function replaceFence(body: string, index: number, source: string): string | null {
  const fence = listFences(body)[index];
  if (!fence) return null;
  return body.slice(0, fence.start) + fenceText(source) + body.slice(fence.end);
}

/**
 * Insert a new fence at `offset`, padded with blank lines so it never fuses
 * with the paragraph around it.
 */
export function insertFence(body: string, offset: number, source: string): string {
  const at = Math.max(0, Math.min(offset, body.length));
  const before = body.slice(0, at);
  const after = body.slice(at);
  const lead = before === "" || before.endsWith("\n\n") ? "" : before.endsWith("\n") ? "\n" : "\n\n";
  const trail = after === "" ? "\n" : after.startsWith("\n\n") ? "" : after.startsWith("\n") ? "\n" : "\n\n";
  return before + lead + fenceText(source) + trail + after;
}
