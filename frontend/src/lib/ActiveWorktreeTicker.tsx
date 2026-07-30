import type { WorktreeFeedbackState } from "./types";
import type { TickerItem } from "./ticker";

/** Wording matches the sidebar's existing status vocabulary ("needs approval") so the
 *  two surfaces do not describe the same state differently.
 *
 *  Text rather than colour alone: a colour-only signal is invisible to a meaningful
 *  share of users and to the accessibility tree, and "this one is waiting on you" is
 *  the single thing the ticker exists to say. */
const feedbackLabels: Partial<Record<WorktreeFeedbackState, string>> = {
  permission_request: "needs approval",
  user_question: "needs an answer",
};

export interface ActiveWorktreeTickerProps {
  items: TickerItem[];
  /** Takes a branch, not a display name — selection is keyed on branch everywhere. */
  onselect: (branch: string) => void;
}

/**
 * Full-width strip of the worktrees that are executing or waiting on the user.
 *
 * Presentation only: it reads no store and holds no state. `App` derives the items and
 * passes the same selection callback the sidebar uses, so ticker and sidebar selection
 * cannot drift apart. Selecting an item navigates and nothing more — it never resolves,
 * approves, or rejects the request that made the item appear.
 */
export default function ActiveWorktreeTicker({
  items,
  onselect,
}: ActiveWorktreeTickerProps): React.JSX.Element | null {
  // Render nothing rather than an empty bar: the workspace below must be exactly as
  // tall as it was before the ticker existed.
  if (items.length === 0) return null;

  return (
    <nav
      aria-label="Active worktrees"
      className="shrink-0 border-b border-edge bg-sidebar"
    >
      {/* Horizontal scroll, never a marquee — items must not move unless the user
          moves them. */}
      <div data-ticker-scroll className="flex items-center gap-2 overflow-x-auto px-3 py-1.5">
        {items.map((item) => {
          const feedbackLabel = item.needsFeedback ? feedbackLabels[item.feedbackState] : undefined;

          return (
            <button
              key={item.branch}
              type="button"
              aria-current={item.selected ? "true" : undefined}
              onClick={() => onselect(item.branch)}
              className={`flex shrink-0 items-center gap-1.5 rounded border px-2 py-1 text-xs transition-colors ${
                item.needsFeedback
                  ? "border-warning/60 bg-warning/10 text-primary"
                  : "border-edge bg-surface text-muted hover:text-primary"
              } ${item.selected ? "ring-1 ring-accent" : ""}`}
            >
              <span className="max-w-[14rem] truncate font-medium">{item.name}</span>
              {feedbackLabel && (
                <span className="shrink-0 text-[10px] uppercase tracking-wide text-warning">
                  {feedbackLabel}
                </span>
              )}
            </button>
          );
        })}
      </div>
    </nav>
  );
}
