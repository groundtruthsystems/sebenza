/**
 * Pure helpers for the inbox's collaboration surfaces — ordering, the
 * proposal diff, the confirm hash — kept out of the components so the rules
 * are testable without rendering anything.
 */
import type { InboxAgentJob, InboxDraftHelpOutput } from "./api-contract";

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

export function sortInboxItems<T extends SortableItem>(_items: T[]): T[] {
  throw new Error("todo");
}

export interface DiffLine {
  op: "same" | "add" | "del";
  text: string;
}

export function lineDiff(_before: string, _after: string): DiffLine[] {
  throw new Error("todo");
}

export async function sha1Hex(_text: string): Promise<string> {
  throw new Error("todo");
}

export function worktreeLabel(_key: WorktreeRef): string {
  throw new Error("todo");
}

export function authorLabel(_kind: AuthorKind): string {
  throw new Error("todo");
}

export function draftHelpOutput(_job: InboxAgentJob): InboxDraftHelpOutput | null {
  throw new Error("todo");
}
