import type { ActiveProjectWorktrees, WorktreeFeedbackState, WorktreeInfo } from "./types";

/** Statuses that count as "this worktree is executing right now".
 *
 *  `awaiting_permission` is listed alongside the two active statuses deliberately.
 *  It is redundant with the feedback-state test below, because the server sets both
 *  fields in one place — but the item stays eligible even if the two ever disagree,
 *  and a worktree wrongly dropped from the ticker is invisible rather than merely
 *  mislabelled. */
const ACTIVE_STATUSES = ["starting", "running", "awaiting_permission"] as const;

/** One entry in the active-worktree ticker.
 *
 *  Display fields only. Nothing here may carry question text, prompts, tool input or
 *  output, terminal content, filesystem paths, or session ids: this object reaches the
 *  DOM, and the ticker is a read-only navigation surface, not a transcript. */
export interface TickerItem {
  /** Identity, and what selection is keyed on. */
  branch: string;
  /** What the user reads — the label when set, otherwise the branch. */
  name: string;
  status: string;
  feedbackState: WorktreeFeedbackState;
  /** Precomputed so the component does not re-derive the rule that decides styling
   *  and ordering. */
  needsFeedback: boolean;
  selected: boolean;
}

function needsFeedback(worktree: WorktreeInfo): boolean {
  return worktree.feedbackState !== "none";
}

/** Whether a worktree belongs in the ticker at all.
 *
 *  Kept as one readable expression so it can be checked line by line against the
 *  spec. The creation term is explicit rather than implied by status: nothing in the
 *  data model stops a worktree mid-creation from also reporting a status, so relying
 *  on status alone would let half-built worktrees flicker through the ticker. */
function isEligible(worktree: WorktreeInfo): boolean {
  if (worktree.archived) return false;
  if (worktree.kind === "main") return false;
  if (worktree.creating) return false;

  return (
    ACTIVE_STATUSES.includes(worktree.status as (typeof ACTIVE_STATUSES)[number])
    || needsFeedback(worktree)
  );
}

function displayName(worktree: WorktreeInfo): string {
  const label = worktree.label?.trim();
  return label ? label : worktree.branch;
}

/**
 * The ticker's items, feedback-needed first, in snapshot order within each group.
 *
 * Pure: no store access, no side effects, no JSX. The ordering relies on
 * `Array.prototype.filter` preserving input order rather than on a comparator, because
 * the snapshot already arrives branch-sorted and re-sorting inside a group would make
 * items swap places between five-second polls for no visible reason.
 */
export function deriveTickerItems(
  worktrees: WorktreeInfo[],
  selectedBranch: string | null,
): TickerItem[] {
  const eligible = worktrees.filter(isEligible);

  const toItem = (worktree: WorktreeInfo): TickerItem => ({
    branch: worktree.branch,
    name: displayName(worktree),
    status: worktree.status,
    feedbackState: worktree.feedbackState,
    needsFeedback: needsFeedback(worktree),
    selected: worktree.branch === selectedBranch,
  });

  return [
    ...eligible.filter(needsFeedback).map(toItem),
    ...eligible.filter((worktree) => !needsFeedback(worktree)).map(toItem),
  ];
}

/** A ticker item that knows which project it came from.
 *
 *  Separate from [`TickerItem`] rather than replacing it, so the single-project
 *  derivation keeps its narrower shape and the eligibility rules stay defined once. */
export interface CrossProjectTickerItem extends TickerItem {
  /** Stable identity across projects. `branch` alone is not unique — two projects can
   *  each have a `main-work`, and anything keyed on branch would collapse them. */
  key: string;
  projectPrefix: string;
  projectName: string;
  /** Belongs to a project other than the one being viewed. Drives both the project
   *  label and the fact that selecting it is a navigation rather than a callback. */
  foreign: boolean;
}

/**
 * Ticker items for every loaded project, feedback-needed first.
 *
 * Delegates per project to `deriveTickerItems`, so eligibility and within-project
 * ordering are not reimplemented here and cannot drift from the single-project path.
 * Across projects the rule is: anything waiting on the user comes first, and project
 * order (the endpoint's registry order) is preserved inside each group — a worktree
 * blocked on a human matters more than which project it lives in.
 *
 * `selectedBranch` applies only within the active project. A foreign project with a
 * same-named branch must not render as selected.
 */
export function deriveCrossProjectTickerItems(
  projects: ActiveProjectWorktrees[],
  activePrefix: string,
  selectedBranch: string | null,
): CrossProjectTickerItem[] {
  const all: CrossProjectTickerItem[] = projects.flatMap((project) => {
    const foreign = project.prefix !== activePrefix;
    return deriveTickerItems(project.worktrees, foreign ? null : selectedBranch).map((item) => ({
      ...item,
      key: `${project.prefix}/${item.branch}`,
      projectPrefix: project.prefix,
      projectName: project.name,
      foreign,
    }));
  });

  return [...all.filter((item) => item.needsFeedback), ...all.filter((item) => !item.needsFeedback)];
}
