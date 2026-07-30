import type { WorktreeFeedbackState } from "./types";
import type { CrossProjectTickerItem } from "./ticker";

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
  items: CrossProjectTickerItem[];
  /** Receives the whole item: the caller needs the project to choose between an in-app
   *  selection and a navigation to another project. */
  onselect: (item: CrossProjectTickerItem) => void;
}

/**
 * Full-width strip of the worktrees that are executing or waiting on the user.
 *
 * Presentation only: it reads no store and holds no state. `App` derives the items and
 * decides what selecting one means, so ticker and sidebar selection cannot drift apart
 * for the active project. Selecting an item navigates and nothing more — it never
 * resolves, approves, or rejects the request that made the item appear.
 *
 * Spans projects: an item from another project is labelled with that project's name,
 * because otherwise a foreign branch looks local while behaving quite differently when
 * clicked.
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
              key={item.key}
              type="button"
              aria-current={item.selected ? "true" : undefined}
              onClick={() => onselect(item)}
              className={`flex shrink-0 items-center gap-1.5 rounded border px-2 py-1 text-xs transition-colors ${
                item.needsFeedback
                  ? "border-warning/60 bg-warning/10 text-primary"
                  : "border-edge bg-surface text-muted hover:text-primary"
              } ${item.selected ? "ring-1 ring-accent" : ""}`}
            >
              {item.foreign && (
                <span className="shrink-0 rounded bg-hover px-1 text-[10px] text-muted">
                  {item.projectName}
                </span>
              )}
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
