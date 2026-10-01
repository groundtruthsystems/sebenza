/**
 * Pure helpers for the inbox's collaboration surfaces — ordering, the
 * proposal diff, the confirm hash — kept out of the components so the rules
 * are testable without rendering anything.
 */
import {
  InboxDraftHelpOutputSchema,
  type InboxAgentJob,
  type InboxDraftHelpOutput,
} from "./api-contract";

export type Priority = "P0" | "P1" | "P2" | "P3";
export type PrioritySource = "agent" | "operator";
export type AuthorKind = "operator" | "worktree_agent" | "system_agent";

export const PRIORITIES: Priority[] = ["P0", "P1", "P2", "P3"];

export interface WorktreeRef {
  project: string;
  branch: string;
}

export interface SortableItem {
  id: string;
  priority?: Priority;
  createdAt?: string;
  isRaw: boolean;
}

/**
 * The server's `inbox_order`, reproduced so a list the client re-sorts (after
 * a local priority change, say) never disagrees with the next fetch: priority,
 * then newest created, then id descending; unparseable items last.
 */
export function sortInboxItems<T extends SortableItem>(items: T[]): T[] {
  const rank = (p?: Priority) => PRIORITIES.indexOf(p ?? "P2");
  const desc = (a = "", b = "") => (a < b ? 1 : a > b ? -1 : 0);
  return [...items].sort((a, b) => {
    if (a.isRaw !== b.isRaw) return a.isRaw ? 1 : -1;
    if (a.isRaw) return desc(a.id, b.id);
    return (
      rank(a.priority) - rank(b.priority) ||
      desc(a.createdAt, b.createdAt) ||
      desc(a.id, b.id)
    );
  });
}

export interface DiffLine {
  op: "same" | "add" | "del";
  text: string;
}

/** Past this many lines a side, the LCS table gets expensive; show the
 *  change as a whole replacement instead. Proposals are far smaller. */
const MAX_DIFF_LINES = 2000;

/** A line diff (LCS), enough to show an operator what their edit changed. */
export function lineDiff(before: string, after: string): DiffLine[] {
  const a = before.split("\n");
  const b = after.split("\n");
  if (a.length > MAX_DIFF_LINES || b.length > MAX_DIFF_LINES) {
    return [
      ...a.map((text) => ({ op: "del" as const, text })),
      ...b.map((text) => ({ op: "add" as const, text })),
    ];
  }
  // lcs[i][j] = length of the LCS of a[i..] and b[j..].
  const lcs = Array.from({ length: a.length + 1 }, () =>
    new Array<number>(b.length + 1).fill(0),
  );
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      lcs[i][j] =
        a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
    }
  }
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) {
      out.push({ op: "same", text: a[i] });
      i++;
      j++;
    } else if (lcs[i + 1][j] >= lcs[i][j + 1]) {
      out.push({ op: "del", text: a[i++] });
    } else {
      out.push({ op: "add", text: b[j++] });
    }
  }
  while (i < a.length) out.push({ op: "del", text: a[i++] });
  while (j < b.length) out.push({ op: "add", text: b[j++] });
  return out;
}

/** The server's `content_hash`: SHA-1 hex of the UTF-8 text. An authored
 *  resolution quotes it so the confirm binds to exactly what was typed. */
export async function sha1Hex(text: string): Promise<string> {
  const digest = await crypto.subtle.digest(
    "SHA-1",
    new TextEncoder().encode(text),
  );
  return Array.from(new Uint8Array(digest), (b) =>
    b.toString(16).padStart(2, "0"),
  ).join("");
}

/** `acme · fix-x`: the project's directory, not its whole path. */
export function worktreeLabel(key: WorktreeRef): string {
  const dir = key.project.replace(/\/+$/, "").split("/").pop() || key.project;
  return `${dir} · ${key.branch}`;
}

export function authorLabel(kind: AuthorKind): string {
  switch (kind) {
    case "operator":
      return "operator";
    case "worktree_agent":
      return "worktree agent";
    case "system_agent":
      return "system agent";
  }
}

/** A finished draft-help job's proposal, or null for anything else. */
export function draftHelpOutput(job: InboxAgentJob): InboxDraftHelpOutput | null {
  if (job.kind !== "draft_help" || job.status !== "succeeded") return null;
  const parsed = InboxDraftHelpOutputSchema.safeParse(job.output);
  return parsed.success ? parsed.data : null;
}

/** Whether an error from the client is a 409 (stale hash). */
export function isStale(err: unknown): boolean {
  return /\b409\b|conflict|changed/i.test(String((err as Error)?.message ?? err));
}
