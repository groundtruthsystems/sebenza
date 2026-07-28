import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { WorktreeListRow } from "./types";
import PrBadge from "./PrBadge";
import AgentStatusIcon, { agentIconVisible } from "./AgentStatusIcon";
import { worktreeCreationPhaseLabel } from "./utils";
import { useStore } from "../store";
import {
  OVERFLOW_STATUS_BAR_STATUSES,
  branchesWithAgentStatus,
  countAgentStatusesIn,
  type OverflowStatusBarStatus,
} from "./worktree-list";

type RowPosition = "above" | "visible" | "below";

// Measure the rendered bars (each sits `top-2`/`bottom-2` = 8px off the list edge)
// so the observer's margins occlude exactly the band each bar covers — no magic number.
const BAR_OFFSET = 8;

const statusLabels: Record<OverflowStatusBarStatus, string> = {
  waiting: "waiting",
  "awaiting-permission": "needs approval",
  error: "error",
  "done-unread": "unread",
};

export default function WorktreeList({
  rows,
  removing,
  initializing,
  archiving,
  notifiedBranches,
  emptyMessage = "No worktrees found.",
  onselect,
  onclose,
  onarchive,
  onmerge,
  onremove,
  oncreatesubworktree,
  onpull,
}: {
  rows: WorktreeListRow[];
  removing: Set<string>;
  initializing: Set<string>;
  archiving: Set<string>;
  notifiedBranches: Set<string>;
  emptyMessage?: string;
  onselect: (branch: string) => void;
  onclose: (branch: string) => void;
  onarchive: (branch: string) => void;
  onmerge: (branch: string) => void;
  onremove: (branch: string) => void;
  oncreatesubworktree: (branch: string) => void;
  /** Pull the main branch — offered only on the repo row. */
  onpull: (branch: string) => void;
}) {
  const selectedBranch = useStore((s) => s.selectedBranch);

  const [openMenuBranch, setOpenMenuBranch] = useState<string | null>(null);

  const listRef = useRef<HTMLUListElement | null>(null);
  const topBarRef = useRef<HTMLDivElement | null>(null);
  const bottomBarRef = useRef<HTMLDivElement | null>(null);

  const [rowPositions, setRowPositions] = useState<Map<string, RowPosition>>(new Map());
  const [cycleCursor, setCycleCursor] = useState<Record<string, string>>({});
  const [topBarHeight, setTopBarHeight] = useState(0);
  const [bottomBarHeight, setBottomBarHeight] = useState(0);

  function toggleMenu(branch: string): void {
    setOpenMenuBranch((current) => (current === branch ? null : branch));
  }

  function runMenuAction(branch: string, action: (branch: string) => void): void {
    setOpenMenuBranch(null);
    action(branch);
  }

  useEffect(() => {
    if (!openMenuBranch) return;

    function handleDocumentClick(event: MouseEvent): void {
      const target = event.target;
      if (!(target instanceof HTMLElement) || !target.closest("[data-worktree-row-menu]")) {
        setOpenMenuBranch(null);
      }
    }

    function handleEscape(event: KeyboardEvent): void {
      if (event.key === "Escape") {
        setOpenMenuBranch(null);
      }
    }

    document.addEventListener("click", handleDocumentClick);
    document.addEventListener("keydown", handleEscape);
    return () => {
      document.removeEventListener("click", handleDocumentClick);
      document.removeEventListener("keydown", handleEscape);
    };
  }, [openMenuBranch]);

  const rootMargin =
    `-${topBarHeight ? topBarHeight + BAR_OFFSET : 0}px 0px ` +
    `-${bottomBarHeight ? bottomBarHeight + BAR_OFFSET : 0}px 0px`;

  // The identity of the rows present, independent of per-row status churn — changes
  // only when worktrees are added or removed, not on every agent-status poll.
  const branchKey = useMemo(() => rows.map((row) => row.worktree.branch).join("\n"), [rows]);

  const aboveBranches = useMemo(() => branchesAt(rowPositions, "above"), [rowPositions]);
  const belowBranches = useMemo(() => branchesAt(rowPositions, "below"), [rowPositions]);
  const aboveCounts = countAgentStatusesIn(rows, aboveBranches, notifiedBranches);
  const belowCounts = countAgentStatusesIn(rows, belowBranches, notifiedBranches);
  const hasAbove = OVERFLOW_STATUS_BAR_STATUSES.some((s) => aboveCounts[s] > 0);
  const hasBelow = OVERFLOW_STATUS_BAR_STATUSES.some((s) => belowCounts[s] > 0);

  useLayoutEffect(() => {
    setTopBarHeight(topBarRef.current?.offsetHeight ?? 0);
  }, [hasAbove, aboveCounts.waiting, aboveCounts.error, aboveCounts["done-unread"]]);
  useLayoutEffect(() => {
    setBottomBarHeight(bottomBarRef.current?.offsetHeight ?? 0);
  }, [hasBelow, belowCounts.waiting, belowCounts.error, belowCounts["done-unread"]]);

  // Track whether each row is scrolled above, into, or below the viewport so the
  // top/bottom floating bars can summarise the agent statuses hidden in each direction.
  useEffect(() => {
    const root = listRef.current;
    if (!root) return;

    // Drop tracking for branches that have left the list.
    setRowPositions((prev) => {
      const present = new Set(rows.map((row) => row.worktree.branch));
      const pruned = new Map([...prev].filter(([branch]) => present.has(branch)));
      return pruned.size !== prev.size ? pruned : prev;
    });

    const observer = new IntersectionObserver(
      (entries) => {
        setRowPositions((prev) => {
          const next = new Map(prev);
          for (const entry of entries) {
            const target = entry.target;
            if (!(target instanceof HTMLElement)) continue;
            const branch = target.dataset.branch;
            if (!branch) continue;
            if (entry.isIntersecting) {
              next.set(branch, "visible");
            } else {
              const rootTop = entry.rootBounds?.top ?? 0;
              next.set(branch, entry.boundingClientRect.top < rootTop ? "above" : "below");
            }
          }
          return next;
        });
      },
      // Negative top/bottom margins keep rows tucked behind the floating bars counted as hidden.
      { root, rootMargin, threshold: 0 },
    );
    for (const li of root.querySelectorAll("[data-branch]")) {
      observer.observe(li);
    }
    return () => observer.disconnect();
    // re-observe only when rows are added/removed, and when the measured bar band changes
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [branchKey, rootMargin]);

  function cycleToStatus(status: OverflowStatusBarStatus, direction: "above" | "below"): void {
    const branches = branchesWithAgentStatus(
      rows,
      status,
      direction === "above" ? aboveBranches : belowBranches,
      notifiedBranches,
    );
    // Cycle nearest-to-the-fold first: below rows are already in that order, above
    // rows need reversing so the first click lands on the row just above the fold.
    if (direction === "above") branches.reverse();
    const listEl = listRef.current;
    if (branches.length === 0 || !listEl) return;
    const key = `${direction}:${status}`;
    // Advance from the last branch we scrolled to; if it has since scrolled into
    // view (no longer in the list), indexOf is -1 and we restart from the first.
    const nextIndex = (branches.indexOf(cycleCursor[key] ?? "") + 1) % branches.length;
    const nextBranch = branches[nextIndex];
    setCycleCursor((prev) => ({ ...prev, [key]: nextBranch }));
    const target = Array.from(listEl.querySelectorAll<HTMLElement>("[data-branch]")).find(
      (el) => el.dataset.branch === nextBranch,
    );
    target?.scrollIntoView({ behavior: "smooth", block: "center" });
  }

  function statusBar(counts: Record<OverflowStatusBarStatus, number>, direction: "above" | "below") {
    return (
      <div className="pointer-events-auto flex items-center gap-1 rounded-full border border-edge bg-surface/90 px-1.5 py-1 shadow-lg backdrop-blur">
        {OVERFLOW_STATUS_BAR_STATUSES.map((status) =>
          counts[status] > 0 ? (
            <button
              key={status}
              type="button"
              className="flex items-center gap-1 rounded-full px-1.5 py-0.5 text-[11px] tabular-nums hover:bg-hover cursor-pointer"
              title={`Scroll to next ${statusLabels[status]} worktree ${direction}`}
              onClick={() => cycleToStatus(status, direction)}
            >
              <AgentStatusIcon
                status={status === "done-unread" ? "done" : status}
                unread={status === "done-unread"}
                size={12}
              />
              <span>{counts[status]}</span>
            </button>
          ) : null,
        )}
      </div>
    );
  }

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <ul ref={listRef} className="list-none overflow-y-auto flex-1 min-h-0 p-2">
        {rows.length === 0 && (
          <li className="px-3 py-4 text-xs text-muted text-center">{emptyMessage}</li>
        )}
        {rows.map((row) => {
          const wt = row.worktree;
          const isActive = wt.branch === selectedBranch;
          const isRemoving = removing.has(wt.branch);
          const isClosed = wt.mux !== "✓";
          const isInitializing = initializing.has(wt.branch);
          const isArchiving = archiving.has(wt.branch);
          const isCreating = wt.creating;
          const isArchived = wt.archived;
          const isBusy = isRemoving || isInitializing;
          const hasLabel = !!wt.label;
          // The repository's own checkout: openable as a terminal, but never a
          // thing to merge, archive, remove or branch a sub-worktree from.
          const isMain = wt.kind === "main";
          const hasBadgeRow =
            isMain ||
            isArchived ||
            isCreating ||
            isInitializing ||
            isClosed ||
            wt.prs.length > 0 ||
            wt.source === "oneshot";
          return (
            <li
              key={wt.branch}
              data-branch={wt.branch}
              className={`mb-0.5 group relative ${isBusy ? "opacity-40 pointer-events-none" : ""}`}
            >
              <button
                type="button"
                disabled={isBusy}
                className={`w-full py-2.5 rounded-md border cursor-pointer flex flex-col gap-1 text-left text-inherit text-sm bg-transparent hover:bg-hover ${isActive ? "bg-active border-accent" : "border-transparent"} ${isClosed && !isInitializing && !isCreating ? "opacity-50" : ""} ${isArchived ? "opacity-70" : ""}`}
                style={{ paddingLeft: `${12 + row.depth * 18}px`, paddingRight: "40px" }}
                onClick={() => {
                  setOpenMenuBranch(null);
                  onselect(wt.branch);
                }}
              >
                <span className="flex min-w-0 items-start gap-2 pr-5">
                  {row.depth > 0 && <span className="shrink-0 text-muted/60">↳</span>}
                  <span className="min-w-0 flex flex-1 flex-col gap-1">
                    <span className="flex min-w-0 items-center gap-1.5" data-worktree-name-row>
                      <span className="min-w-0 flex flex-1 flex-col">
                        <span className="font-medium truncate">{wt.label ?? wt.branch}</span>
                        {hasLabel && (
                          <span className="text-[10px] leading-tight text-muted truncate">
                            {wt.branch}
                          </span>
                        )}
                      </span>
                      {!isCreating &&
                        !isInitializing &&
                        !isClosed &&
                        agentIconVisible(wt.agent, notifiedBranches.has(wt.branch)) && (
                          <span className="shrink-0">
                            <AgentStatusIcon
                              status={wt.agent}
                              size={14}
                              unread={notifiedBranches.has(wt.branch)}
                            />
                          </span>
                        )}
                    </span>
                    {hasBadgeRow && (
                      <span
                        className="flex min-w-0 flex-wrap items-center gap-1.5"
                        data-worktree-badge-row
                      >
                        {isMain && (
                          <span
                            className="shrink-0 text-[10px] px-1.5 py-0.5 rounded border border-edge text-muted"
                            title="The main repository checkout — terminal only"
                          >
                            repo
                          </span>
                        )}
                        {wt.source === "oneshot" && (
                          <span
                            className="shrink-0 text-[10px] px-1.5 py-0.5 rounded border border-edge text-muted"
                            title="Autonomous run — auto-closes when done"
                          >
                            oneshot
                          </span>
                        )}
                        {isArchived && (
                          <span className="shrink-0 text-[10px] px-1.5 py-0.5 rounded border border-edge text-muted">
                            archived
                          </span>
                        )}
                        {isCreating ? (
                          <span className="shrink-0 inline-flex items-center gap-1 text-[10px] text-muted">
                            <span className="spinner"></span>
                            {worktreeCreationPhaseLabel(wt.creationPhase)}...
                          </span>
                        ) : isInitializing ? (
                          <span className="shrink-0 text-[10px] text-muted">opening...</span>
                        ) : isClosed ? (
                          <span className="shrink-0 text-[10px] text-muted">closed</span>
                        ) : null}
                        {wt.prs.map((pr) => (
                          <PrBadge key={pr.repo} pr={pr} />
                        ))}
                      </span>
                    )}
                  </span>
                </span>
                <span className="flex gap-2 text-[11px] text-muted items-center flex-wrap">
                  {(wt.agentLabel ?? wt.agentName) && <span>{wt.agentLabel ?? wt.agentName}</span>}
                  {wt.profile && <span>{wt.profile}</span>}
                </span>
                {wt.services.length > 0 && (
                  <span className="flex gap-2 text-[11px] text-muted font-mono">
                    {wt.services.map((svc) =>
                      svc.port ? (
                        <span key={svc.name} className={svc.running ? "text-success" : ""}>
                          {svc.name}:{svc.port}
                        </span>
                      ) : null,
                    )}
                  </span>
                )}
              </button>
              <button
                type="button"
                disabled={isBusy}
                className="absolute top-2 right-2 w-6 h-6 rounded flex items-center justify-center text-muted hover:text-primary hover:bg-hover opacity-0 group-hover:opacity-100 transition-opacity cursor-pointer"
                title="Worktree actions"
                aria-label={`Actions for ${wt.branch}`}
                aria-haspopup="menu"
                aria-expanded={openMenuBranch === wt.branch}
                onClick={(event) => {
                  event.stopPropagation();
                  toggleMenu(wt.branch);
                }}
              >
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  width="14"
                  height="14"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <circle cx="12" cy="5" r="1" />
                  <circle cx="12" cy="12" r="1" />
                  <circle cx="12" cy="19" r="1" />
                </svg>
              </button>
              {openMenuBranch === wt.branch && (
                <div
                  className="absolute top-9 right-2 z-10 min-w-32 rounded-md border border-edge bg-surface shadow-lg p-1"
                  data-worktree-row-menu
                >
                  <button
                    type="button"
                    disabled={isClosed || isCreating}
                    className="w-full px-2 py-1.5 rounded text-left text-xs text-primary hover:bg-hover disabled:opacity-50 disabled:cursor-not-allowed"
                    onClick={(event) => {
                      event.stopPropagation();
                      runMenuAction(wt.branch, onclose);
                    }}
                  >
                    Close
                  </button>
                  {isMain ? (
                    <button
                      type="button"
                      className="w-full px-2 py-1.5 rounded text-left text-xs text-primary hover:bg-hover"
                      onClick={(event) => {
                        event.stopPropagation();
                        runMenuAction(wt.branch, onpull);
                      }}
                    >
                      Pull
                    </button>
                  ) : (
                    <>
                      <button
                        type="button"
                        disabled={isCreating || isArchiving}
                        className="w-full px-2 py-1.5 rounded text-left text-xs text-primary hover:bg-hover disabled:opacity-50 disabled:cursor-not-allowed"
                        onClick={(event) => {
                          event.stopPropagation();
                          runMenuAction(wt.branch, onarchive);
                        }}
                      >
                        {wt.archived ? "Restore" : "Archive"}
                      </button>
                      <button
                        type="button"
                        className="w-full px-2 py-1.5 rounded text-left text-xs text-primary hover:bg-hover"
                        onClick={(event) => {
                          event.stopPropagation();
                          runMenuAction(wt.branch, onmerge);
                        }}
                      >
                        Merge
                      </button>
                      <button
                        type="button"
                        disabled={isCreating}
                        className="w-full px-2 py-1.5 rounded text-left text-xs text-primary hover:bg-hover disabled:opacity-50 disabled:cursor-not-allowed"
                        onClick={(event) => {
                          event.stopPropagation();
                          runMenuAction(wt.branch, oncreatesubworktree);
                        }}
                      >
                        Create sub-worktree
                      </button>
                      <button
                        type="button"
                        className="w-full px-2 py-1.5 rounded text-left text-xs text-danger hover:bg-hover"
                        onClick={(event) => {
                          event.stopPropagation();
                          runMenuAction(wt.branch, onremove);
                        }}
                      >
                        Remove
                      </button>
                    </>
                  )}
                </div>
              )}
            </li>
          );
        })}
      </ul>
      {hasAbove && (
        <div
          ref={topBarRef}
          className="pointer-events-none absolute inset-x-0 top-2 flex justify-center"
        >
          {statusBar(aboveCounts, "above")}
        </div>
      )}
      {hasBelow && (
        <div
          ref={bottomBarRef}
          className="pointer-events-none absolute inset-x-0 bottom-2 flex justify-center"
        >
          {statusBar(belowCounts, "below")}
        </div>
      )}
    </div>
  );
}

function branchesAt(rowPositions: Map<string, RowPosition>, position: RowPosition): Set<string> {
  const set = new Set<string>();
  for (const [branch, value] of rowPositions) {
    if (value === position) set.add(branch);
  }
  return set;
}
