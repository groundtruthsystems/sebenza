import "./TopBar.css";
import { useEffect, useState } from "react";
import type { WorktreeInfo, PrEntry, LinkedRepoInfo } from "./types";
import { useStore } from "../store";
import RepoGroup from "./RepoGroup";
import Btn from "./Btn";
import NotificationItem from "./NotificationItem";
import { launchWorktree } from "./api";
import { errorMessage } from "./utils";

function truncateWorktreeName(value: string | null, maxLength: number): string | null {
  if (!value || value.length <= maxLength) return value;
  return `${value.slice(0, maxLength - 3)}...`;
}

export default function TopBar({
  name,
  worktree,
  linkedRepos = [],
  isMobile = false,
  ontogglesidebar,
  onclose,
  onarchive,
  onmerge,
  onremove,
  oneditlabel,
  onsettings,
  onCiClick,
  onReviewsClick,
  ondirtyclick,
  onbellopen,
  onnotificationselect,
  activeView = "terminal",
  onviewchange,
  archiving = false,
}: {
  name: string | null;
  worktree: WorktreeInfo | undefined;
  linkedRepos?: LinkedRepoInfo[];
  isMobile?: boolean;
  ontogglesidebar?: () => void;
  onclose: () => void;
  onarchive: () => void;
  onmerge: () => void;
  onremove: () => void;
  oneditlabel?: () => void;
  onsettings: () => void;
  onCiClick: (pr: PrEntry) => void;
  onReviewsClick: (pr: PrEntry) => void;
  ondirtyclick?: () => void;
  onbellopen?: () => void;
  onnotificationselect?: (branch: string) => void;
  activeView?: "terminal" | "tracks";
  onviewchange?: (view: "terminal" | "tracks") => void;
  archiving?: boolean;
}) {
  const launchers = useStore((s) => s.config.launchers);
  const notificationHistory = useStore((s) => s.notificationHistory);
  const unreadCount = useStore((s) => s.unreadCount);
  const success = useStore((s) => s.success);
  const error = useStore((s) => s.error);

  const [bellOpen, setBellOpen] = useState(false);
  const [moreOpen, setMoreOpen] = useState(false);
  const [openInOpen, setOpenInOpen] = useState(false);

  function toggleBell(): void {
    const next = !bellOpen;
    setBellOpen(next);
    if (next) onbellopen?.();
  }

  async function handleLaunch(launcherId: string, label: string): Promise<void> {
    if (!worktree) return;
    setOpenInOpen(false);
    try {
      await launchWorktree(worktree.branch, launcherId);
      success(`Opening in ${label}…`);
    } catch (e: unknown) {
      error(`Failed to open in ${label}`, errorMessage(e));
    }
  }

  useEffect(() => {
    function handleClickOutside(e: MouseEvent): void {
      const target = e.target;
      if (target instanceof Element) {
        if (!target.closest(".bell-container")) setBellOpen(false);
        if (!target.closest(".more-container")) setMoreOpen(false);
        if (!target.closest(".openin-container")) setOpenInOpen(false);
      }
    }
    window.addEventListener("click", handleClickOutside);
    return () => window.removeEventListener("click", handleClickOutside);
  }, []);

  const headerName = worktree?.label ?? name;
  const displayName = truncateWorktreeName(headerName, 30);
  const displayBranch = worktree?.label ? truncateWorktreeName(name, 44) : null;

  // Split PRs into main repo vs linked repo groups
  const mainPrs = (worktree?.prs ?? []).filter(
    (pr) => !pr.repo || !linkedRepos.some((lr) => lr.alias === pr.repo),
  );

  const linkedRepoGroups = linkedRepos
    .map((lr) => ({
      alias: lr.alias,
      dir: lr.dir,
      prs: (worktree?.prs ?? []).filter((pr) => pr.repo === lr.alias),
    }))
    .filter((g) => g.prs.length > 0);

  const hasMoreContent = mainPrs.length > 0 || linkedRepoGroups.length > 0;

  return (
    <div className="bg-topbar border-b border-edge">
      <div className="flex items-stretch min-h-12">
        {/* Left + middle: rows of repo groups */}
        <div className="flex-1 min-w-0 flex flex-col justify-center px-4 py-2.5 gap-1.5">
          {/* Main row: branch name + worktree-level badges + main repo PR badges */}
          <div className="topbar-main-row flex items-start gap-3 min-w-0">
            <div className="topbar-main-meta flex items-center gap-3 min-w-0">
              {isMobile && ontogglesidebar && (
                <button
                  type="button"
                  className="p-1 -ml-1 cursor-pointer bg-transparent border-none text-muted hover:text-primary"
                  onClick={ontogglesidebar}
                  title="Toggle sidebar"
                >
                  <svg
                    xmlns="http://www.w3.org/2000/svg"
                    width="20"
                    height="20"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <line x1="3" y1="6" x2="21" y2="6" />
                    <line x1="3" y1="12" x2="21" y2="12" />
                    <line x1="3" y1="18" x2="21" y2="18" />
                  </svg>
                </button>
              )}
              <span className="min-w-0 flex flex-col leading-tight">
                <span className="flex items-center gap-1.5 min-w-0">
                  <span className="min-w-0 text-sm font-semibold truncate" title={headerName ?? undefined}>
                    {displayName ?? "Select a worktree"}
                  </span>
                  {worktree && oneditlabel && (
                    <button
                      type="button"
                      className="shrink-0 p-0.5 rounded text-muted hover:text-primary hover:bg-hover"
                      title="Edit workspace label"
                      aria-label="Edit workspace label"
                      onClick={oneditlabel}
                    >
                      <svg
                        xmlns="http://www.w3.org/2000/svg"
                        width="13"
                        height="13"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      >
                        <path d="M12 20h9" />
                        <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" />
                      </svg>
                    </button>
                  )}
                </span>
                {displayBranch && (
                  <span className="text-[10px] text-muted truncate" title={name ?? undefined}>
                    {displayBranch}
                  </span>
                )}
              </span>
              {worktree?.archived && (
                <span className="shrink-0 text-[10px] px-1.5 py-0.5 rounded border border-edge text-muted">
                  Archived
                </span>
              )}
              {(worktree?.dirty || worktree?.unpushed) && (
                <button
                  type="button"
                  className="shrink-0 text-[10px] px-1.5 py-0.5 rounded border border-warning/40 text-warning bg-transparent cursor-pointer hover:bg-warning/10"
                  onClick={ondirtyclick}
                >
                  {worktree.dirty ? "dirty" : "unpushed"}
                </button>
              )}
              {worktree && launchers.length > 0 && (
                <div className="openin-container relative shrink-0">
                  <button
                    type="button"
                    className="flex items-center gap-1 text-[10px] px-1.5 py-0.5 rounded border border-edge text-muted bg-transparent cursor-pointer hover:bg-hover hover:text-primary"
                    title="Open the worktree in an external editor"
                    onClick={() => setOpenInOpen(!openInOpen)}
                  >
                    Open in
                    <svg
                      xmlns="http://www.w3.org/2000/svg"
                      width="10"
                      height="10"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    >
                      <polyline points="6 9 12 15 18 9" />
                    </svg>
                  </button>
                  {openInOpen && (
                    <div className="absolute left-0 top-full mt-1 z-30 min-w-[150px] rounded-md border border-edge bg-sidebar shadow-lg overflow-hidden">
                      <ul className="list-none max-h-64 overflow-y-auto">
                        {launchers.map((l) => (
                          <li key={l.id}>
                            <button
                              type="button"
                              className="w-full px-3 py-2 text-left text-sm bg-transparent border-none text-primary cursor-pointer hover:bg-hover"
                              onClick={() => handleLaunch(l.id, l.label)}
                            >
                              {l.label}
                            </button>
                          </li>
                        ))}
                      </ul>
                    </div>
                  )}
                </div>
              )}
            </div>
            {!isMobile && (
              <div className="topbar-main-prs min-w-0 flex-1">
                <RepoGroup
                  prs={mainPrs}
                  services={worktree?.services ?? []}
                  onCiClick={onCiClick}
                  onReviewsClick={onReviewsClick}
                />
              </div>
            )}
          </div>

          {/* Linked repo rows (desktop only) */}
          {!isMobile &&
            linkedRepoGroups.map((group) => (
              <RepoGroup
                key={group.alias}
                label={group.alias}
                prs={group.prs}
                onCiClick={onCiClick}
                onReviewsClick={onReviewsClick}
              />
            ))}
        </div>

        {/* Right: action buttons (pinned, vertically centered) */}
        <div className="shrink-0 flex gap-2 items-center px-4">
          {worktree && onviewchange && (
            <div className="flex rounded-md border border-edge overflow-hidden mr-1">
              {(["terminal", "tracks"] as const).map((view) => (
                <button
                  key={view}
                  type="button"
                  className={`px-2.5 py-1 text-[11px] cursor-pointer transition-colors ${
                    activeView === view
                      ? "bg-active text-primary"
                      : "bg-surface text-muted hover:bg-hover hover:text-primary"
                  }`}
                  onClick={() => onviewchange(view)}
                >
                  {view === "terminal" ? (isMobile ? "Term" : "Terminal") : "Tracks"}
                </button>
              ))}
            </div>
          )}
          {worktree && (
            <>
              {worktree.mux === "✓" && (
                <Btn variant="default" onClick={onclose} title="Close worktree window">
                  {isMobile ? "C" : "Close"}
                </Btn>
              )}
              {/* The repository's own checkout can be closed and opened in an
                  editor, but never archived, merged or removed. */}
              {worktree.kind !== "main" && (
                <>
                  <Btn
                    variant="accent-outline"
                    onClick={onarchive}
                    disabled={archiving || worktree.creating}
                    title={worktree.archived ? "Restore archived worktree" : "Archive worktree"}
                  >
                    {isMobile
                      ? worktree.archived
                        ? "Re"
                        : "A"
                      : worktree.archived
                        ? "Restore"
                        : "Archive"}
                  </Btn>
                  <Btn variant="accent-outline" onClick={onmerge} title="Merge worktree">
                    {isMobile ? "M" : "Merge"}
                  </Btn>
                  <Btn variant="danger-outline" onClick={onremove} title="Remove worktree">
                    {isMobile ? "R" : "Remove"}
                  </Btn>
                </>
              )}
            </>
          )}

          {isMobile && worktree && hasMoreContent && (
            <div className="more-container relative">
              <button
                type="button"
                className="p-1.5 rounded-md cursor-pointer bg-transparent border border-transparent text-muted hover:text-primary hover:border-edge"
                title="More info"
                onClick={() => {
                  setMoreOpen(!moreOpen);
                }}
              >
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  width="16"
                  height="16"
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

              {moreOpen && (
                <div className="more-dropdown">
                  <div className="flex flex-col gap-2 p-3">
                    <RepoGroup prs={mainPrs} onCiClick={onCiClick} onReviewsClick={onReviewsClick} />
                    {linkedRepoGroups.map((group) => (
                      <RepoGroup
                        key={group.alias}
                        label={group.alias}
                        prs={group.prs}
                        onCiClick={onCiClick}
                        onReviewsClick={onReviewsClick}
                      />
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}

          <div className="bell-container relative ml-3">
            <button
              type="button"
              className="relative p-1.5 rounded-md cursor-pointer bg-transparent border border-transparent text-muted hover:text-primary hover:border-edge"
              title="Notifications"
              onClick={toggleBell}
            >
              <svg
                xmlns="http://www.w3.org/2000/svg"
                width="16"
                height="16"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" />
                <path d="M10.3 21a1.94 1.94 0 0 0 3.4 0" />
              </svg>
              {unreadCount > 0 && (
                <span className="absolute -top-0.5 -right-0.5 w-4 h-4 rounded-full bg-accent text-white text-[10px] flex items-center justify-center leading-none">
                  {unreadCount > 9 ? "9+" : unreadCount}
                </span>
              )}
            </button>

            {bellOpen && (
              <div className="bell-dropdown">
                <div className="text-xs font-semibold text-muted px-3 py-2 border-b border-edge">
                  Notifications
                </div>
                {notificationHistory.length === 0 ? (
                  <div className="px-3 py-4 text-xs text-muted text-center">No notifications yet</div>
                ) : (
                  <ul className="list-none max-h-64 overflow-y-auto">
                    {notificationHistory.map((n) => (
                      <li key={n.id}>
                        <button
                          type="button"
                          className="w-full px-3 py-2 text-left bg-transparent border-none text-inherit cursor-pointer hover:bg-hover flex items-center gap-2"
                          onClick={() => {
                            onnotificationselect?.(n.branch);
                            setBellOpen(false);
                          }}
                        >
                          <NotificationItem notification={n} showTimestamp />
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}
          </div>

          <button
            type="button"
            className="p-1.5 rounded-md cursor-pointer bg-transparent border border-transparent text-muted hover:text-primary hover:border-edge"
            title="Settings"
            onClick={onsettings}
          >
            <svg
              xmlns="http://www.w3.org/2000/svg"
              width="16"
              height="16"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
              <circle cx="12" cy="12" r="3" />
            </svg>
          </button>
        </div>
      </div>
    </div>
  );
}
