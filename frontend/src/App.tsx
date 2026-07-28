import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import WorktreeList from "./lib/WorktreeList";
import TopBar from "./lib/TopBar";
import Terminal, { type TerminalHandle } from "./lib/Terminal";
import ConfirmDialog from "./lib/ConfirmDialog";
import CreateWorktreeDialog from "./lib/CreateWorktreeDialog";
import SettingsDialog from "./lib/SettingsDialog";
import CiDetailsDialog from "./lib/CiDetailsDialog";
import CommentReviewDialog from "./lib/CommentReviewDialog";
import PaneBar from "./lib/PaneBar";
import ToastStack from "./lib/ToastStack";
import MobileChatSurface from "./lib/MobileChatSurface";
import WorktreeLabelDialog from "./lib/WorktreeLabelDialog";
import SidebarRepoRow from "./lib/SidebarRepoRow";
import ProjectSwitcher from "./lib/ProjectSwitcher";
import MigrationBanner from "./lib/MigrationBanner";
import Toggle from "./lib/Toggle";
import TabBar from "./lib/TabBar";
import { agentCan } from "./lib/agent-capabilities";
import DiffDialog from "./lib/DiffDialog";
import TracksBoard from "./lib/TracksBoard";
import type {
  AvailableBranch,
  AppNotification,
  CreateWorktreeRequest,
  PrEntry,
  WorktreeInfo,
} from "./lib/types";
import {
  errorMessage,
  worktreeCreationPhaseLabel,
  saveSelectedWorktree,
  resolveSelectedBranch,
  applyTheme,
  saveSidebarWidth,
} from "./lib/utils";
import {
  buildWorktreeListRows,
  countArchivedMatches,
  filterWorktrees,
  matchesWorktreeSearch,
} from "./lib/worktree-list";
import { getTheme } from "./lib/themes";
import {
  activePrefix,
  api,
  createWorktreeTab,
  createWorktreeAgentTab,
  createWorktreeShellTab,
  deleteWorktreeTab,
  fetchWorktrees,
  refreshWorktreeAgentTerminal,
  selectWorktreeTab,
  setWorktreeLabel,
  subscribeNotifications,
} from "./lib/api";
import { useStore } from "./store";

const DEFAULT_POLL_INTERVAL_MS = 5000;
const ACTIVE_CREATE_POLL_INTERVAL_MS = 1000;
const AUTO_DISMISS_MS = 4000;
const MAX_HISTORY = 10;

const MIN_SIDEBAR_WIDTH = 140;
const MAX_SIDEBAR_WIDTH = 500;
const SIDEBAR_KEYBOARD_STEP = 10;

type BranchCacheKey = "local" | "remote";

function clampSidebarWidth(w: number): number {
  return Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, w));
}

export default function App() {
  // --- store-owned state ---
  const config = useStore((s) => s.config);
  const setConfig = useStore((s) => s.setConfig);
  const worktrees = useStore((s) => s.worktrees);
  const hasLoadedWorktrees = useStore((s) => s.hasLoadedWorktrees);
  const availableBranches = useStore((s) => s.availableBranches);
  const setAvailableBranches = useStore((s) => s.setAvailableBranches);
  const baseBranches = useStore((s) => s.baseBranches);
  const setBaseBranches = useStore((s) => s.setBaseBranches);
  const selectedBranch = useStore((s) => s.selectedBranch);
  const selectBranch = useStore((s) => s.selectBranch);
  const searchQuery = useStore((s) => s.searchQuery);
  const setSearchQuery = useStore((s) => s.setSearchQuery);
  const showArchivedWorktrees = useStore((s) => s.showArchivedWorktrees);
  const setShowArchivedWorktrees = useStore((s) => s.setShowArchivedWorktrees);
  const includeRemoteBranches = useStore((s) => s.includeRemoteBranches);
  const setIncludeRemoteBranches = useStore((s) => s.setIncludeRemoteBranches);
  const theme = useStore((s) => s.theme);
  const useWebChatUi = useStore((s) => s.useWebChatUi);
  const sidebarWidth = useStore((s) => s.sidebarWidth);
  const setSidebarWidth = useStore((s) => s.setSidebarWidth);
  const info = useStore((s) => s.info);
  const success = useStore((s) => s.success);
  const error = useStore((s) => s.error);

  // --- App-local state ---
  const [removingBranches, setRemovingBranches] = useState<Set<string>>(new Set());
  const [openingBranches, setOpeningBranches] = useState<Set<string>>(new Set());
  const [archivingBranches, setArchivingBranches] = useState<Set<string>>(new Set());
  const [refreshingAgentTerminalBranches, setRefreshingAgentTerminalBranches] = useState<Set<string>>(
    new Set(),
  );
  const [terminalSessionRevisions, setTerminalSessionRevisions] = useState<Record<string, number>>({});
  const [notifiedBranches, setNotifiedBranches] = useState<Set<string>>(new Set());

  const [isMobile, setIsMobile] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [activePane, setActivePane] = useState(0);
  const [tabBusy, setTabBusy] = useState(false);
  const [pendingCreateCount, setPendingCreateCount] = useState(0);
  const [pendingCreateBranchHint, setPendingCreateBranchHint] = useState<string | null>(null);
  const [isResizingSidebar, setIsResizingSidebar] = useState(false);
  const [viewMode, setViewMode] = useState<"terminal" | "tracks">("terminal");
  // Land on the terminal when switching worktrees.
  useEffect(() => {
    setViewMode("terminal");
  }, [selectedBranch]);

  // Dialog visibility (mirrors the Svelte booleans)
  const [showCreateDialog, setShowCreateDialog] = useState(false);
  const [showSettingsDialog, setShowSettingsDialog] = useState(false);
  const [showDiffDialog, setShowDiffDialog] = useState(false);
  const [removeBranch, setRemoveBranch] = useState<string | null>(null);
  const [mergeBranch, setMergeBranch] = useState<string | null>(null);
  const [labelBranch, setLabelBranch] = useState<string | null>(null);
  const [labelLoading, setLabelLoading] = useState(false);
  const [labelError, setLabelError] = useState("");
  const [ciDetailsPr, setCiDetailsPr] = useState<PrEntry | null>(null);
  const [commentReviewPr, setCommentReviewPr] = useState<PrEntry | null>(null);
  const [pullMainConfirm, setPullMainConfirm] = useState(false);
  const [pullMainLoading, setPullMainLoading] = useState(false);
  const [pullMainError, setPullMainError] = useState("");
  const [pullMainForce, setPullMainForce] = useState(false);
  const [pullLinkedRepoAlias, setPullLinkedRepoAlias] = useState<string | null>(null);
  const [pullLinkedRepoLoading, setPullLinkedRepoLoading] = useState(false);
  const [pullLinkedRepoError, setPullLinkedRepoError] = useState("");
  const [pullLinkedRepoForce, setPullLinkedRepoForce] = useState(false);

  const [lockedBaseBranch, setLockedBaseBranch] = useState<string | null>(null);
  const [availableBranchesLoading, setAvailableBranchesLoading] = useState(false);
  const [availableBranchesError, setAvailableBranchesError] = useState<string | null>(null);
  const [baseBranchesLoading, setBaseBranchesLoading] = useState(false);
  const [baseBranchesError, setBaseBranchesError] = useState<string | null>(null);

  // --- refs (non-render mutable) ---
  const latestAutoSelectCreateId = useRef(-1);
  const nextCreateRequestId = useRef(0);
  const nextAvailableBranchFetchId = useRef(0);
  const nextBaseBranchFetchId = useRef(0);
  const availableBranchCache = useRef<Partial<Record<BranchCacheKey, AvailableBranch[]>>>({});
  const availableBranchRequests = useRef<Partial<Record<BranchCacheKey, Promise<AvailableBranch[]>>>>(
    {},
  );
  const baseBranchCache = useRef<AvailableBranch[] | null>(null);
  const baseBranchRequest = useRef<Promise<AvailableBranch[]> | null>(null);
  const applyPollIntervalRef = useRef<((intervalMs: number) => void) | null>(null);
  const terminalRef = useRef<TerminalHandle>(null);
  const worktreeSearchInputRef = useRef<HTMLInputElement>(null);

  function agentCapabilitiesFor(worktree: WorktreeInfo | undefined) {
    if (!worktree?.agentName) return undefined;
    return config.agents.find((candidate) => candidate.id === worktree.agentName)?.capabilities;
  }

  /** Fail closed: an agent we cannot find capabilities for gets no chat tab, rather
   *  than falling back to a hardcoded id list. Hiding a tab is recoverable; offering
   *  chat for an agent that cannot serve it is not. */
  function supportsWorktreeChat(worktree: WorktreeInfo | undefined): boolean {
    return agentCan(config.agents, worktree?.agentName, "inAppChat");
  }

  // --- derived values ---
  const terminalTheme = useMemo(() => getTheme(theme).terminal, [theme]);
  const trimmedWorktreeSearch = useMemo(() => searchQuery.trim(), [searchQuery]);
  const archivedWorktreeCount = useMemo(
    () => worktrees.filter((w) => w.archived).length,
    [worktrees],
  );
  const hiddenArchivedMatchCount = useMemo(
    () => (showArchivedWorktrees ? 0 : countArchivedMatches(worktrees, trimmedWorktreeSearch)),
    [showArchivedWorktrees, worktrees, trimmedWorktreeSearch],
  );
  const visibleWorktrees = useMemo(
    () =>
      filterWorktrees(worktrees, {
        query: trimmedWorktreeSearch,
        showArchived: showArchivedWorktrees,
      }),
    [worktrees, trimmedWorktreeSearch, showArchivedWorktrees],
  );
  const visibleWorktreeRows = useMemo(
    () => buildWorktreeListRows(visibleWorktrees),
    [visibleWorktrees],
  );
  const creatingWorktrees = useMemo(() => worktrees.filter((w) => w.creating), [worktrees]);
  const backendCreatingCount = creatingWorktrees.length;
  const activeCreateCount = Math.max(pendingCreateCount, backendCreatingCount);
  const hasCreatingWorktrees = activeCreateCount > 0;
  const selectableWorktrees = useMemo(
    () => visibleWorktrees.filter((w) => !removingBranches.has(w.branch)),
    [visibleWorktrees, removingBranches],
  );
  const createIndicatorLabel =
    activeCreateCount === 1 ? "Creating..." : `Creating ${activeCreateCount}...`;
  const selectedVisibleWorktree = useMemo(
    () =>
      selectedBranch && !removingBranches.has(selectedBranch)
        ? visibleWorktrees.find((w) => w.branch === selectedBranch)
        : undefined,
    [selectedBranch, removingBranches, visibleWorktrees],
  );
  const selectedWorktree = useMemo(
    () =>
      selectedBranch && !removingBranches.has(selectedBranch)
        ? worktrees.find((w) => w.branch === selectedBranch)
        : undefined,
    [selectedBranch, removingBranches, worktrees],
  );
  const labelWorktree = useMemo(
    () => (labelBranch ? worktrees.find((w) => w.branch === labelBranch) : undefined),
    [labelBranch, worktrees],
  );
  const canConnect =
    !!selectedBranch && selectedWorktree?.mux === "✓" && !selectedWorktree?.creating;
  const showWebChat = useWebChatUi && canConnect && supportsWorktreeChat(selectedWorktree);
  // The tab bar shows for any connectable terminal worktree: a shell or a fresh
  // provider session is always available. Only *forking* needs a forkable
  // session, which is a per-agent capability.
  const showTabBar = canConnect && !showWebChat;
  const canFork = agentCan(config.agents, selectedWorktree?.agentName, "fork");
  const isMainRepoSelected = selectedWorktree?.kind === "main";
  const isSelectedOpening = selectedBranch ? openingBranches.has(selectedBranch) : false;
  const isSelectedArchiving = selectedBranch ? archivingBranches.has(selectedBranch) : false;
  const isSelectedAgentTerminalRefreshing = selectedBranch
    ? refreshingAgentTerminalBranches.has(selectedBranch)
    : false;
  const selectedTerminalKey = selectedBranch
    ? `${selectedBranch}:${terminalSessionRevisions[selectedBranch] ?? 0}`
    : "";
  const pollIntervalMs = hasCreatingWorktrees
    ? ACTIVE_CREATE_POLL_INTERVAL_MS
    : DEFAULT_POLL_INTERVAL_MS;
  const worktreeListEmptyMessage = trimmedWorktreeSearch
    ? hiddenArchivedMatchCount > 0
      ? "Archived matches are hidden."
      : `No matches for "${trimmedWorktreeSearch}".`
    : archivedWorktreeCount > 0 && !showArchivedWorktrees
      ? "No active worktrees."
      : "No worktrees found.";

  const paneBarPanes = useMemo(() => {
    const count = selectedWorktree?.paneCount ?? 0;
    if (count < 2) return [];
    return Array.from({ length: count }, (_, i) => ({
      index: i,
      label: String(i + 1),
    }));
  }, [selectedWorktree]);
  const showPaneBar = isMobile && canConnect && !showWebChat && paneBarPanes.length > 0;

  // --- branch cache helpers ---
  function getAvailableBranchCacheKey(includeRemote: boolean): BranchCacheKey {
    return includeRemote ? "remote" : "local";
  }

  function fetchAvailableBranchesCached(includeRemote: boolean): Promise<AvailableBranch[]> {
    const key = getAvailableBranchCacheKey(includeRemote);
    const cached = availableBranchCache.current[key];
    if (cached) return Promise.resolve(cached);

    const inFlight = availableBranchRequests.current[key];
    if (inFlight) return inFlight;

    const request = api
      .fetchAvailableBranches({ query: { includeRemote } })
      .then((data) => {
        availableBranchCache.current[key] = data.branches;
        return data.branches;
      })
      .finally(() => {
        delete availableBranchRequests.current[key];
      });

    availableBranchRequests.current[key] = request;
    return request;
  }

  function fetchBaseBranchesCached(): Promise<AvailableBranch[]> {
    if (baseBranchCache.current) return Promise.resolve(baseBranchCache.current);
    if (baseBranchRequest.current) return baseBranchRequest.current;

    baseBranchRequest.current = api
      .fetchBaseBranches()
      .then((data) => {
        baseBranchCache.current = data.branches;
        return data.branches;
      })
      .finally(() => {
        baseBranchRequest.current = null;
      });

    return baseBranchRequest.current;
  }

  function invalidateBranchCaches(): void {
    availableBranchCache.current = {};
    availableBranchRequests.current = {};
    baseBranchCache.current = null;
    baseBranchRequest.current = null;
    setAvailableBranches([]);
    setAvailableBranchesError(null);
    setAvailableBranchesLoading(false);
    setBaseBranches([]);
    setBaseBranchesError(null);
    setBaseBranchesLoading(false);
  }

  const refresh = useCallback(async () => {
    try {
      const next = await fetchWorktrees();
      useStore.getState().setWorktrees(next);
      useStore.getState().setHasLoadedWorktrees(true);
    } catch (err) {
      console.error("Failed to refresh:", err);
    }
  }, []);

  function openDiffDialog(): void {
    setShowDiffDialog(true);
  }

  function handleSelectToast(id: string): void {
    const toast = useStore.getState().toasts().find((item) => item.id === id);
    if (!toast || toast.source !== "notification") return;

    useStore.getState().dismissNotification(toast.notificationId);
    api.dismissNotification({ params: { id: toast.notificationId } }).catch(() => {});
    selectBranch(toast.branch);
    setNotifiedBranches((prev) => new Set([...prev].filter((branch) => branch !== toast.branch)));
    if (isMobile) setSidebarOpen(false);
  }

  // --- effects: derived reactions ---
  useEffect(() => {
    const nextSelectedBranch = resolveSelectedBranch(
      selectedBranch,
      trimmedWorktreeSearch ? selectedWorktree : selectedVisibleWorktree,
      selectableWorktrees,
      hasLoadedWorktrees,
    );
    if (nextSelectedBranch !== selectedBranch) {
      selectBranch(nextSelectedBranch);
    }
  }, [
    selectedBranch,
    trimmedWorktreeSearch,
    selectedWorktree,
    selectedVisibleWorktree,
    selectableWorktrees,
    hasLoadedWorktrees,
    selectBranch,
  ]);

  useEffect(() => {
    setTerminalSessionRevisions((prev) => {
      const branches = new Set(worktrees.map((worktree) => worktree.branch));
      const nextEntries = Object.entries(prev).filter(([branch]) => branches.has(branch));
      if (nextEntries.length === Object.keys(prev).length) return prev;
      return Object.fromEntries(nextEntries);
    });
  }, [worktrees]);

  useEffect(() => {
    if (pendingCreateCount === 0 || latestAutoSelectCreateId.current === -1) return;
    const target = pendingCreateBranchHint
      ? worktrees.find((w) => w.branch === pendingCreateBranchHint)
      : creatingWorktrees.length === 1
        ? creatingWorktrees[0]
        : undefined;
    if (!target) return;
    revealWorktreeInFilters(target.branch);
    selectBranch(target.branch);
    if (isMobile) setSidebarOpen(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingCreateCount, pendingCreateBranchHint, worktrees, creatingWorktrees, isMobile]);

  useEffect(() => {
    applyPollIntervalRef.current?.(pollIntervalMs);
  }, [pollIntervalMs]);

  useEffect(() => {
    if (!hasLoadedWorktrees) return;
    if (selectedWorktree) {
      saveSelectedWorktree(selectedWorktree.branch);
      return;
    }
    if (selectableWorktrees.length === 0) {
      saveSelectedWorktree(null);
    }
  }, [hasLoadedWorktrees, selectedWorktree, selectableWorktrees]);

  useEffect(() => {
    if (!showCreateDialog) return;

    const cached = availableBranchCache.current[getAvailableBranchCacheKey(includeRemoteBranches)];
    if (cached) {
      setAvailableBranches(cached);
      setAvailableBranchesLoading(false);
      setAvailableBranchesError(null);
      return;
    }

    const fetchId = ++nextAvailableBranchFetchId.current;
    setAvailableBranchesLoading(true);
    setAvailableBranchesError(null);

    fetchAvailableBranchesCached(includeRemoteBranches)
      .then((branches) => {
        if (fetchId !== nextAvailableBranchFetchId.current) return;
        setAvailableBranches(branches);
      })
      .catch((err: unknown) => {
        if (fetchId !== nextAvailableBranchFetchId.current) return;
        setAvailableBranchesError(errorMessage(err));
      })
      .finally(() => {
        if (fetchId !== nextAvailableBranchFetchId.current) return;
        setAvailableBranchesLoading(false);
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [showCreateDialog, includeRemoteBranches]);

  useEffect(() => {
    if (!showCreateDialog) return;

    if (baseBranchCache.current) {
      setBaseBranches(baseBranchCache.current);
      setBaseBranchesLoading(false);
      setBaseBranchesError(null);
      return;
    }

    const fetchId = ++nextBaseBranchFetchId.current;
    setBaseBranches([]);
    setBaseBranchesLoading(true);
    setBaseBranchesError(null);

    fetchBaseBranchesCached()
      .then((branches) => {
        if (fetchId !== nextBaseBranchFetchId.current) return;
        setBaseBranches(branches);
      })
      .catch((err: unknown) => {
        if (fetchId !== nextBaseBranchFetchId.current) return;
        setBaseBranchesError(errorMessage(err));
      })
      .finally(() => {
        if (fetchId !== nextBaseBranchFetchId.current) return;
        setBaseBranchesLoading(false);
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [showCreateDialog]);

  useEffect(() => {
    document.title = config.name ? `${config.name} - Dashboard` : "Dev Dashboard";
  }, [config.name]);

  // --- create / mutation handlers ---
  function openCreateDialog(): void {
    setIncludeRemoteBranches(false);
    setLockedBaseBranch(null);
    setShowCreateDialog(true);
  }

  function openSubworktreeDialog(parentBranch: string): void {
    setIncludeRemoteBranches(false);
    setLockedBaseBranch(parentBranch);
    setShowCreateDialog(true);
  }

  async function handleCreate(request: CreateWorktreeRequest) {
    const requestId = nextCreateRequestId.current++;
    const shouldAutoSelectCreatedWorktree = selectedWorktree == null;
    const requestedAgentIds =
      request.agents && request.agents.length > 0
        ? request.agents
        : request.agent
          ? [request.agent]
          : [config.defaultAgentId];
    const expectedCreatedCount = requestedAgentIds.length;
    if (shouldAutoSelectCreatedWorktree) {
      latestAutoSelectCreateId.current = requestId;
    }
    setPendingCreateCount((count) => count + expectedCreatedCount);
    if (shouldAutoSelectCreatedWorktree) {
      setPendingCreateBranchHint(expectedCreatedCount > 1 ? null : (request.branch ?? null));
    }
    setShowCreateDialog(false);
    setLockedBaseBranch(null);

    try {
      const createPromise = api.createWorktree({ body: request });
      void refresh();
      const result = await createPromise;
      if (shouldAutoSelectCreatedWorktree) {
        setPendingCreateBranchHint(result.primaryBranch);
      }
      invalidateBranchCaches();
      await refresh();
      if (shouldAutoSelectCreatedWorktree && requestId === latestAutoSelectCreateId.current) {
        selectBranch(result.primaryBranch);
        if (isMobile) setSidebarOpen(false);
      }
    } catch (err) {
      error(`Failed to create: ${errorMessage(err)}`);
    } finally {
      setPendingCreateCount((count) => Math.max(0, count - expectedCreatedCount));
      if (shouldAutoSelectCreatedWorktree && requestId === latestAutoSelectCreateId.current) {
        setPendingCreateBranchHint(null);
        latestAutoSelectCreateId.current = -1;
      }
    }
  }

  function selectNeighborOf(branch: string) {
    if (selectedBranch !== branch) return;
    const orderedWorktrees = visibleWorktreeRows.map((row) => row.worktree);
    const idx = orderedWorktrees.findIndex((w) => w.branch === branch);
    const previous = orderedWorktrees[idx - 1];
    const next = orderedWorktrees[idx + 1];
    const neighbor = [previous, next].find(
      (candidate) => candidate && !removingBranches.has(candidate.branch),
    );
    selectBranch(neighbor ? neighbor.branch : null);
  }

  function revealWorktreeInFilters(branch: string): void {
    const worktree = worktrees.find((candidate) => candidate.branch === branch);
    if (!worktree) return;
    if (worktree.archived) {
      setShowArchivedWorktrees(true);
    }
    if (trimmedWorktreeSearch && !matchesWorktreeSearch(worktree, trimmedWorktreeSearch)) {
      setSearchQuery("");
    }
  }

  function handleSelectWorktree(branch: string): void {
    revealWorktreeInFilters(branch);
    selectBranch(branch);
    setNotifiedBranches((prev) => new Set([...prev].filter((candidate) => candidate !== branch)));
    if (isMobile) setSidebarOpen(false);
  }

  async function handleRemove() {
    const branch = removeBranch;
    if (!branch) return;
    setRemoveBranch(null);
    selectNeighborOf(branch);

    setRemovingBranches((prev) => new Set([...prev, branch]));
    try {
      await api.removeWorktree({ params: { name: branch } });
      invalidateBranchCaches();
      await refresh();
    } catch (err) {
      error(`Failed to remove: ${errorMessage(err)}`);
    } finally {
      setRemovingBranches((prev) => new Set([...prev].filter((b) => b !== branch)));
    }
  }

  async function handleMerge() {
    const branch = mergeBranch;
    if (!branch) return;
    setMergeBranch(null);
    selectNeighborOf(branch);

    setRemovingBranches((prev) => new Set([...prev, branch]));
    try {
      await api.mergeWorktree({ params: { name: branch } });
      invalidateBranchCaches();
      await refresh();
    } catch (err) {
      error(`Failed to merge: ${errorMessage(err)}`);
    } finally {
      setRemovingBranches((prev) => new Set([...prev].filter((b) => b !== branch)));
    }
  }

  function openLabelDialog(): void {
    if (!selectedWorktree) return;
    setLabelBranch(selectedWorktree.branch);
    setLabelError("");
  }

  function applyWorktreeLabel(branch: string, label: string | null): void {
    const current = useStore.getState().worktrees;
    useStore
      .getState()
      .setWorktrees(
        current.map((worktree) => (worktree.branch === branch ? { ...worktree, label } : worktree)),
      );
  }

  async function handleLabelChange(label: string | null): Promise<void> {
    const branch = labelBranch;
    if (!branch) return;

    setLabelLoading(true);
    setLabelError("");
    try {
      const nextLabel = await setWorktreeLabel(branch, label);
      applyWorktreeLabel(branch, nextLabel);
      setLabelBranch(null);
    } catch (err) {
      setLabelError(errorMessage(err));
    } finally {
      setLabelLoading(false);
    }
  }

  async function handlePullMain(): Promise<void> {
    setPullMainLoading(true);
    setPullMainError("");
    try {
      const result = await api.pullMain({
        body: { ...(pullMainForce ? { force: true } : {}) },
      });
      if (result.status === "updated" || result.status === "already_up_to_date") {
        setPullMainConfirm(false);
        setPullMainForce(false);
        const message =
          result.status === "updated"
            ? `Pulled latest "${config.mainBranch ?? "main"}" from remote`
            : `"${config.mainBranch ?? "main"}" is already up to date`;
        if (result.status === "updated") {
          success(message);
        } else {
          info(message);
        }
      } else if (result.status === "merge_failed" && !pullMainForce) {
        setPullMainForce(true);
        setPullMainError(
          `Fast-forward failed: ${result.error ?? "unknown error"}.\nForce pull will reset main to match remote.`,
        );
      } else if (result.status === "skipped_wrong_branch") {
        // Forcing would not help — force-pull skips for the same reason.
        setPullMainForce(false);
        setPullMainError(
          `The repository is not on "${config.mainBranch ?? "main"}" right now, so nothing was pulled.\nSwitch it back before pulling.`,
        );
      } else {
        setPullMainError(result.error ?? result.status);
      }
    } catch (err) {
      setPullMainError(errorMessage(err));
    } finally {
      setPullMainLoading(false);
    }
  }

  async function handlePullLinkedRepo(): Promise<void> {
    if (!pullLinkedRepoAlias) return;
    setPullLinkedRepoLoading(true);
    setPullLinkedRepoError("");
    try {
      const result = await api.pullMain({
        body: {
          ...(pullLinkedRepoForce ? { force: true } : {}),
          ...(pullLinkedRepoAlias ? { repo: pullLinkedRepoAlias } : {}),
        },
      });
      if (result.status === "updated" || result.status === "already_up_to_date") {
        setPullLinkedRepoAlias(null);
        setPullLinkedRepoForce(false);
      } else if (result.status === "merge_failed" && !pullLinkedRepoForce) {
        setPullLinkedRepoForce(true);
        setPullLinkedRepoError(
          `Fast-forward failed: ${result.error ?? "unknown error"}.\nForce pull will reset to match remote.`,
        );
      } else if (result.status === "skipped_wrong_branch") {
        // Forcing would not help — force-pull skips for the same reason.
        setPullLinkedRepoForce(false);
        setPullLinkedRepoError(
          "The repository is not on its main branch right now, so nothing was pulled.\nSwitch it back before pulling.",
        );
      } else {
        setPullLinkedRepoError(result.error ?? result.status);
      }
    } catch (err) {
      setPullLinkedRepoError(errorMessage(err));
    } finally {
      setPullLinkedRepoLoading(false);
    }
  }

  async function openSelectedWorktree(): Promise<void> {
    const branch = selectedBranch;
    if (!branch) return;
    setOpeningBranches((prev) => new Set([...prev, branch]));
    try {
      await api.openWorktree({ params: { name: branch }, body: {} });
      await refresh();
    } catch (err) {
      error(`Failed to open worktree: ${errorMessage(err)}`);
    } finally {
      setOpeningBranches((prev) => new Set([...prev].filter((x) => x !== branch)));
    }
  }

  async function toggleWorktreeArchived(branch: string): Promise<void> {
    const worktree = worktrees.find((candidate) => candidate.branch === branch);
    if (!worktree || worktree.creating) return;
    const nextArchived = !worktree.archived;
    const actionLabel = nextArchived ? "archive" : "restore";

    setArchivingBranches((prev) => new Set([...prev, branch]));
    try {
      await api.setWorktreeArchived({
        params: { name: branch },
        body: { archived: nextArchived },
      });
      await refresh();
    } catch (err) {
      alert(`Failed to ${actionLabel} worktree: ${errorMessage(err)}`);
    } finally {
      setArchivingBranches((prev) => new Set([...prev].filter((candidate) => candidate !== branch)));
    }
  }

  async function closeWorktree(branch: string): Promise<void> {
    selectNeighborOf(branch);
    try {
      await api.closeWorktree({ params: { name: branch } });
      await refresh();
    } catch (err) {
      error(`Failed to close worktree: ${errorMessage(err)}`);
    }
  }

  async function handleRefreshAgentTerminal(branch: string): Promise<void> {
    if (refreshingAgentTerminalBranches.has(branch)) return;
    setRefreshingAgentTerminalBranches((prev) => new Set([...prev, branch]));
    try {
      await refreshWorktreeAgentTerminal(branch);
      await refresh();
      setTerminalSessionRevisions((prev) => ({
        ...prev,
        [branch]: (prev[branch] ?? 0) + 1,
      }));
      success("Agent terminal refreshed");
    } catch (err) {
      error(`Failed to refresh terminal: ${errorMessage(err)}`);
    } finally {
      setRefreshingAgentTerminalBranches((prev) =>
        new Set([...prev].filter((candidate) => candidate !== branch)),
      );
    }
  }

  async function handleCreateTab(): Promise<void> {
    const branch = selectedBranch;
    if (!branch || tabBusy) return;
    setTabBusy(true);
    try {
      await createWorktreeTab(branch);
      await refresh();
    } catch (err) {
      error(`Failed to create tab: ${errorMessage(err)}`);
    } finally {
      setTabBusy(false);
    }
  }

  async function handleCreateAgentTab(agentId: string): Promise<void> {
    const branch = selectedBranch;
    if (!branch || tabBusy) return;
    setTabBusy(true);
    try {
      await createWorktreeAgentTab(branch, agentId);
      await refresh();
    } catch (err) {
      error(`Failed to start session: ${errorMessage(err)}`);
    } finally {
      setTabBusy(false);
    }
  }

  async function handleCreateShellTab(): Promise<void> {
    const branch = selectedBranch;
    if (!branch || tabBusy) return;
    setTabBusy(true);
    try {
      await createWorktreeShellTab(branch);
      await refresh();
    } catch (err) {
      error(`Failed to open shell: ${errorMessage(err)}`);
    } finally {
      setTabBusy(false);
    }
  }

  async function handleSelectTab(tabId: string): Promise<void> {
    const branch = selectedBranch;
    if (!branch || tabBusy) return;
    setTabBusy(true);
    try {
      await selectWorktreeTab(branch, tabId);
      await refresh();
    } catch (err) {
      error(`Failed to switch tab: ${errorMessage(err)}`);
    } finally {
      setTabBusy(false);
    }
  }

  async function handleDeleteTab(tabId: string): Promise<void> {
    const branch = selectedBranch;
    if (!branch || tabBusy) return;
    setTabBusy(true);
    try {
      await deleteWorktreeTab(branch, tabId);
      await refresh();
    } catch (err) {
      error(`Failed to delete tab: ${errorMessage(err)}`);
    } finally {
      setTabBusy(false);
    }
  }

  async function handleArchiveToggle() {
    const branch = selectedBranch;
    if (!branch) return;
    await toggleWorktreeArchived(branch);
  }

  async function handleClose() {
    const branch = selectedBranch;
    if (!branch) return;
    await closeWorktree(branch);
  }

  function selectNeighborWorktree(direction: -1 | 1) {
    const selectable = visibleWorktrees.filter((w) => !removingBranches.has(w.branch));
    if (selectable.length === 0) return;
    if (!selectedBranch) {
      selectBranch(selectable[direction === 1 ? 0 : selectable.length - 1].branch);
      return;
    }
    const idx = selectable.findIndex((w) => w.branch === selectedBranch);
    const next = idx + direction;
    if (next >= 0 && next < selectable.length) {
      selectBranch(selectable[next].branch);
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    // Ignore shortcuts when a dialog is open (let dialog handle its own keys)
    if (showCreateDialog || removeBranch || mergeBranch || pullMainConfirm || pullLinkedRepoAlias)
      return;

    const mod = e.metaKey || e.ctrlKey;
    if (!mod) return;

    if (e.key === "ArrowUp") {
      e.preventDefault();
      selectNeighborWorktree(-1);
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      selectNeighborWorktree(1);
    } else if (e.key === "k" || e.key === "K") {
      e.preventDefault();
      openCreateDialog();
    } else if (e.key === "m" || e.key === "M") {
      e.preventDefault();
      // Neither merging nor removing applies to the repository's own checkout.
      if (selectedBranch && !isMainRepoSelected) setMergeBranch(selectedBranch);
    } else if (e.key === "d" || e.key === "D") {
      e.preventDefault();
      if (selectedBranch && !isMainRepoSelected) setRemoveBranch(selectedBranch);
    } else if (e.key === "Enter") {
      if (
        selectedWorktree &&
        selectedWorktree.mux !== "✓" &&
        !selectedWorktree.creating &&
        !isSelectedOpening
      ) {
        e.preventDefault();
        openSelectedWorktree();
      }
    }
  }

  function handlePaneSelect(pane: number) {
    setActivePane(pane);
    terminalRef.current?.sendSelectPane(pane);
  }

  // Window keyboard shortcuts — re-bound each render so it reads the latest state.
  useEffect(() => {
    window.addEventListener("keydown", handleKeydown);
    return () => window.removeEventListener("keydown", handleKeydown);
  });

  // --- sidebar resize ---
  function handleResizeStart(e: React.PointerEvent) {
    e.preventDefault();
    setIsResizingSidebar(true);
    const startX = e.clientX;
    const startWidth = useStore.getState().sidebarWidth;

    function onPointerMove(ev: PointerEvent) {
      setSidebarWidth(clampSidebarWidth(startWidth + ev.clientX - startX));
    }

    function onPointerUp() {
      setIsResizingSidebar(false);
      saveSidebarWidth(useStore.getState().sidebarWidth);
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
    }

    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
  }

  function handleResizeKeydown(e: React.KeyboardEvent) {
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      const delta = e.key === "ArrowRight" ? SIDEBAR_KEYBOARD_STEP : -SIDEBAR_KEYBOARD_STEP;
      const next = clampSidebarWidth(useStore.getState().sidebarWidth + delta);
      setSidebarWidth(next);
      saveSidebarWidth(next);
    }
  }

  // --- mount: config, polling, notifications, mobile detection ---
  useEffect(() => {
    applyTheme(useStore.getState().theme);
    api
      .fetchConfig()
      .then((c) => {
        useStore.getState().setConfig(c);
      })
      .catch(() => {});
    refresh();
    let intervalMs = DEFAULT_POLL_INTERVAL_MS;
    let interval: ReturnType<typeof setInterval> | undefined;

    function handleNotification(n: AppNotification): void {
      useStore.getState().addNotification(n);
      // Only suppress the unread dot when the user is actually looking at this branch
      // (selected and tab visible); otherwise a finished run should still surface.
      const viewingThisBranch = n.branch === useStore.getState().selectedBranch && !document.hidden;
      if (!viewingThisBranch) {
        setNotifiedBranches((prev) => new Set([...prev, n.branch]));
      }
      // Auto-dismiss after timeout
      setTimeout(() => {
        useStore.getState().dismissNotification(n.id);
      }, AUTO_DISMISS_MS);
      // Browser notification when tab is hidden
      if (document.hidden && Notification.permission === "granted") {
        new Notification(n.message, { body: n.url ?? n.branch, tag: `wm-${n.id}` });
      }
    }

    function handleSseDismiss(id: number): void {
      useStore.getState().dismissNotification(id);
    }

    function handleInitialNotification(n: AppNotification): void {
      useStore.setState((state) => {
        if (state.notificationHistory.some((x) => x.id === n.id)) return {};
        return {
          notificationHistory: [n, ...state.notificationHistory].slice(0, MAX_HISTORY),
        };
      });
    }

    const unsubNotifications = subscribeNotifications(
      handleNotification,
      handleSseDismiss,
      handleInitialNotification,
    );
    // Request notification permission (no-op if already granted/denied)
    if (Notification.permission === "default") {
      Notification.requestPermission().catch(() => {});
    }

    // Pause polling when tab is hidden or idle (no interaction for 60s).
    let idleTimer: ReturnType<typeof setTimeout>;
    let idle = false;

    function startPolling(): void {
      if (interval) clearInterval(interval);
      if (document.hidden || idle) return;
      interval = setInterval(refresh, intervalMs);
    }

    applyPollIntervalRef.current = (nextIntervalMs: number): void => {
      if (intervalMs === nextIntervalMs) return;
      intervalMs = nextIntervalMs;
      startPolling();
    };
    startPolling();

    function resetIdleTimer(): void {
      if (idle) {
        idle = false;
        refresh();
        startPolling();
      }
      clearTimeout(idleTimer);
      idleTimer = setTimeout(() => {
        idle = true;
        if (interval) clearInterval(interval);
      }, 60_000);
    }

    document.addEventListener("click", resetIdleTimer);
    document.addEventListener("keydown", resetIdleTimer);
    resetIdleTimer();

    function onVisibilityChange(): void {
      if (document.hidden) {
        if (interval) clearInterval(interval);
      } else {
        resetIdleTimer();
        refresh();
        startPolling();
      }
    }
    document.addEventListener("visibilitychange", onVisibilityChange);

    const mq = window.matchMedia("(max-width: 768px)");
    setIsMobile(mq.matches);
    if (mq.matches) setSidebarOpen(true);
    function onMqChange(e: MediaQueryListEvent): void {
      setIsMobile(e.matches);
    }
    mq.addEventListener("change", onMqChange);

    return () => {
      if (interval) clearInterval(interval);
      applyPollIntervalRef.current = null;
      clearTimeout(idleTimer);
      document.removeEventListener("click", resetIdleTimer);
      document.removeEventListener("keydown", resetIdleTimer);
      document.removeEventListener("visibilitychange", onVisibilityChange);
      mq.removeEventListener("change", onMqChange);
      unsubNotifications();
    };
  }, [refresh]);

  return (
    <>
      <div
        className={`flex h-dvh bg-surface text-primary ${isResizingSidebar ? "select-none" : ""}`}
        style={isResizingSidebar ? { cursor: "col-resize" } : undefined}
      >
        {/* Sidebar: fixed overlay on mobile, static on desktop */}
        {(!isMobile || sidebarOpen) && (
          <>
            {isMobile && (
              <div
                className="fixed inset-0 bg-black/50 z-40"
                onClick={() => setSidebarOpen(false)}
                onKeyDown={(e) => {
                  if (e.key === "Escape") setSidebarOpen(false);
                }}
              ></div>
            )}
            <aside
              className={`${
                isMobile ? "fixed inset-0 z-50 w-full" : ""
              } bg-sidebar border-r border-edge flex flex-col overflow-hidden shrink-0`}
              style={isMobile ? undefined : { width: sidebarWidth }}
            >
              <div className="p-4 border-b border-edge">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-1 min-w-0">
                    <h1 className="text-base font-semibold truncate">{config.name ?? "Dashboard"}</h1>
                    <ProjectSwitcher current={activePrefix} />
                  </div>
                  <div className="flex items-center gap-2">
                    <button
                      className="h-8 px-2 gap-1.5 rounded-md border border-edge bg-surface text-accent text-xs flex items-center justify-center cursor-pointer hover:bg-hover disabled:opacity-50 disabled:cursor-not-allowed"
                      onClick={() => openCreateDialog()}
                      title="New Worktree (Cmd+K)"
                    >
                      <span className="text-lg leading-none">+</span> New
                    </button>
                    {isMobile && (
                      <button
                        className="h-8 w-8 rounded-md border border-edge bg-surface text-muted text-sm flex items-center justify-center cursor-pointer hover:bg-hover"
                        onClick={() => setSidebarOpen(false)}
                        title="Close sidebar"
                      >
                        &times;
                      </button>
                    )}
                  </div>
                </div>
                {activeCreateCount > 0 && (
                  <div className="mt-2 flex items-center gap-1 text-[10px] text-muted">
                    <span className="spinner"></span>
                    {" "}
                    {createIndicatorLabel}
                  </div>
                )}
                <div className="mt-3 flex flex-col gap-2">
                  <div className="relative">
                    <input
                      type="search"
                      ref={worktreeSearchInputRef}
                      value={searchQuery}
                      onChange={(e) => setSearchQuery(e.currentTarget.value)}
                      className="w-full h-7 rounded-md border border-edge bg-surface px-2 pr-6 text-xs text-primary placeholder:text-muted focus:outline-none focus:border-accent"
                      placeholder="Search worktrees"
                      aria-label="Search worktrees"
                    />
                    {trimmedWorktreeSearch && (
                      <button
                        type="button"
                        className="absolute top-1/2 right-1 -translate-y-1/2 h-4 w-4 flex items-center justify-center rounded text-muted hover:text-primary"
                        onClick={() => {
                          setSearchQuery("");
                          worktreeSearchInputRef.current?.focus();
                        }}
                        aria-label="Clear worktree search"
                      >
                        &times;
                      </button>
                    )}
                  </div>
                  <div className="flex items-center gap-2 text-[11px] text-muted">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <Toggle
                        checked={showArchivedWorktrees}
                        size="sm"
                        aria-label="Show archived worktrees"
                        onToggle={(checked) => {
                          setShowArchivedWorktrees(checked);
                        }}
                      />
                      <span>
                        Show archived{archivedWorktreeCount > 0 ? ` (${archivedWorktreeCount})` : ""}
                      </span>
                    </label>
                  </div>
                </div>
              </div>
              <WorktreeList
                rows={visibleWorktreeRows}
                removing={removingBranches}
                initializing={openingBranches}
                archiving={archivingBranches}
                notifiedBranches={notifiedBranches}
                emptyMessage={worktreeListEmptyMessage}
                onselect={handleSelectWorktree}
                onclose={closeWorktree}
                onarchive={toggleWorktreeArchived}
                onmerge={(branch) => {
                  setMergeBranch(branch);
                }}
                onremove={(b) => setRemoveBranch(b)}
                oncreatesubworktree={openSubworktreeDialog}
                onpull={() => {
                  setPullMainConfirm(true);
                  setPullMainForce(false);
                  setPullMainError("");
                }}
              />
              {(config.linkedRepos ?? [])
                .filter((lr) => lr.dir)
                .map((lr) => (
                  <SidebarRepoRow
                    key={lr.alias}
                    label={lr.alias}
                    onpull={() => {
                      setPullLinkedRepoAlias(lr.alias);
                      setPullLinkedRepoForce(false);
                      setPullLinkedRepoError("");
                    }}
                  />
                ))}
              {!isMobile && (
                <div className="shrink-0 border-t border-edge px-4 py-3 text-[11px] text-muted flex flex-col gap-1">
                  <div className="flex justify-between">
                    <span>Navigate</span>
                    <kbd className="opacity-60">Cmd+Up/Down</kbd>
                  </div>
                  <div className="flex justify-between">
                    <span>New worktree</span>
                    <kbd className="opacity-60">Cmd+K</kbd>
                  </div>
                  <div className="flex justify-between">
                    <span>Merge</span>
                    <kbd className="opacity-60">Cmd+M</kbd>
                  </div>
                  <div className="flex justify-between">
                    <span>Remove</span>
                    <kbd className="opacity-60">Cmd+D</kbd>
                  </div>
                </div>
              )}
            </aside>
            {!isMobile && (
              <div
                className={`w-1 shrink-0 cursor-col-resize hover:bg-accent/50 transition-colors${
                  isResizingSidebar ? " bg-accent" : ""
                }`}
                onPointerDown={handleResizeStart}
                onKeyDown={handleResizeKeydown}
                role="separator"
                aria-label="Resize sidebar"
                aria-orientation="vertical"
                aria-valuenow={sidebarWidth}
                aria-valuemin={MIN_SIDEBAR_WIDTH}
                aria-valuemax={MAX_SIDEBAR_WIDTH}
                tabIndex={0}
              ></div>
            )}
          </>
        )}

        <main className="flex-1 min-w-0 flex flex-col overflow-hidden">
          <MigrationBanner />
          <TopBar
            name={selectedWorktree?.branch ?? null}
            worktree={selectedWorktree}
            linkedRepos={config.linkedRepos ?? []}
            isMobile={isMobile}
            ontogglesidebar={() => setSidebarOpen((open) => !open)}
            onclose={handleClose}
            onarchive={handleArchiveToggle}
            onmerge={() => {
              if (selectedBranch) setMergeBranch(selectedBranch);
            }}
            onremove={() => {
              if (selectedBranch) setRemoveBranch(selectedBranch);
            }}
            oneditlabel={openLabelDialog}
            onsettings={() => setShowSettingsDialog(true)}
            ondirtyclick={openDiffDialog}
            onCiClick={(pr) => setCiDetailsPr(pr)}
            onReviewsClick={(pr) => setCommentReviewPr(pr)}
            onbellopen={() => useStore.getState().clearUnread()}
            onnotificationselect={handleSelectWorktree}
            activeView={viewMode}
            onviewchange={setViewMode}
            archiving={isSelectedArchiving}
          />

          {viewMode === "tracks" && selectedWorktree ? (
            <TracksBoard key={selectedBranch ?? ""} worktree={selectedWorktree} />
          ) : showWebChat ? (
            <MobileChatSurface
              key={selectedBranch ?? ""}
              worktree={selectedWorktree!}
              supportsChat={supportsWorktreeChat(selectedWorktree)}
              onConversationMessageSent={() => void refresh()}
            />
          ) : canConnect ? (
            <>
              {showTabBar && selectedWorktree && (
                <TabBar
                  tabs={selectedWorktree.tabs}
                  activeTabId={selectedWorktree.activeTabId}
                  agents={config.agents}
                  busy={tabBusy}
                  canFork={canFork}
                  oncreate={handleCreateTab}
                  oncreateshell={handleCreateShellTab}
                  oncreateagent={handleCreateAgentTab}
                  onselect={handleSelectTab}
                  ondelete={handleDeleteTab}
                />
              )}
              <Terminal
                key={selectedTerminalKey}
                ref={terminalRef}
                worktree={selectedBranch!}
                isMobile={isMobile}
                initialPane={isMobile ? activePane : undefined}
                terminalTheme={terminalTheme}
                agentTerminalStale={selectedWorktree?.agentTerminalStale ?? false}
                refreshingAgentTerminal={isSelectedAgentTerminalRefreshing}
                onrefreshagentterminal={() => {
                  if (selectedBranch) void handleRefreshAgentTerminal(selectedBranch);
                }}
              />
            </>
          ) : selectedWorktree?.creating ? (
            <div className="flex-1 flex items-center justify-center px-6">
              <div className="flex flex-col items-center gap-3 text-center">
                <span
                  className="spinner"
                  style={{ width: "24px", height: "24px", borderWidth: "2px" }}
                ></span>
                <div>
                  <p className="text-sm text-primary font-medium">
                    {selectedWorktree.label ?? selectedWorktree.branch}
                  </p>
                  {selectedWorktree.label && (
                    <p className="text-[10px] text-muted">{selectedWorktree.branch}</p>
                  )}
                </div>
                <p className="text-xs text-muted">
                  {worktreeCreationPhaseLabel(selectedWorktree.creationPhase)}
                </p>
              </div>
            </div>
          ) : selectedWorktree ? (
            <div className="flex-1 flex items-center justify-center px-6">
              <div className="flex flex-col items-center gap-4 text-center">
                <div>
                  <p className="text-sm text-primary font-medium">
                    {selectedWorktree.label ?? selectedWorktree.branch}
                  </p>
                  {selectedWorktree.label && (
                    <p className="text-[10px] text-muted">{selectedWorktree.branch}</p>
                  )}
                </div>
                <div className="flex flex-col items-center gap-1">
                  {isMainRepoSelected ? (
                    <>
                      {/* No profile or agent applies to the repo checkout — show
                          where it lives instead. */}
                      <span className="text-xs text-muted">{selectedWorktree.path}</span>
                      <span className="text-xs text-muted">
                        Opens a terminal in the main repository.
                      </span>
                    </>
                  ) : (
                    <>
                      {selectedWorktree.profile && (
                        <span className="text-xs text-muted">
                          Profile: {selectedWorktree.profile}
                        </span>
                      )}
                      {(selectedWorktree.agentLabel ?? selectedWorktree.agentName) && (
                        <span className="text-xs text-muted">
                          Agent: {selectedWorktree.agentLabel ?? selectedWorktree.agentName}
                        </span>
                      )}
                      {selectedWorktree.agentName && !supportsWorktreeChat(selectedWorktree) && (
                        <span className="text-xs text-muted">
                          This agent runs in the terminal only.
                        </span>
                      )}
                    </>
                  )}
                </div>
                <button
                  className="mt-2 px-5 py-2 rounded-md bg-accent text-white text-sm font-medium cursor-pointer border-none hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2"
                  onClick={openSelectedWorktree}
                  disabled={isSelectedOpening}
                >
                  {isSelectedOpening ? (
                    <>
                      <span
                        className="spinner"
                        style={{ width: "14px", height: "14px", borderWidth: "1.5px" }}
                      ></span>
                      {" "}
                      Opening...
                    </>
                  ) : (
                    "Open Session"
                  )}
                </button>
              </div>
            </div>
          ) : (
            <div className="flex-1 flex items-center justify-center text-muted text-sm">
              <p>Select a worktree from the sidebar to connect</p>
            </div>
          )}

          {viewMode !== "tracks" && showPaneBar && (
            <PaneBar activePane={activePane} panes={paneBarPanes} onselect={handlePaneSelect} />
          )}
        </main>
      </div>

      {showCreateDialog && (
        <CreateWorktreeDialog
          profiles={config.profiles}
          agents={config.agents}
          defaultProfileName={config.defaultProfileName}
          defaultAgentId={config.defaultAgentId}
          autoNameEnabled={config.autoName}
          includeRemoteBranches={includeRemoteBranches}
          onIncludeRemoteBranches={setIncludeRemoteBranches}
          availableBranches={availableBranches}
          availableBranchesLoading={availableBranchesLoading}
          availableBranchesError={availableBranchesError}
          baseBranches={baseBranches}
          baseBranchesLoading={baseBranchesLoading}
          baseBranchesError={baseBranchesError}
          lockedBaseBranch={lockedBaseBranch}
          startupEnvs={config.startupEnvs ?? {}}
          oncreate={handleCreate}
          oncancel={() => {
            setShowCreateDialog(false);
            setLockedBaseBranch(null);
          }}
        />
      )}

      {labelBranch && labelWorktree && (
        <WorktreeLabelDialog
          branch={labelWorktree.branch}
          initialLabel={labelWorktree.label}
          loading={labelLoading}
          error={labelError}
          onconfirm={(label) => {
            void handleLabelChange(label);
          }}
          onclear={() => {
            void handleLabelChange(null);
          }}
          oncancel={() => {
            setLabelBranch(null);
            setLabelError("");
          }}
        />
      )}

      {removeBranch && (
        <ConfirmDialog
          message={`Remove worktree "${removeBranch}"? This action cannot be undone.`}
          onconfirm={handleRemove}
          oncancel={() => setRemoveBranch(null)}
        />
      )}

      {mergeBranch && (
        <ConfirmDialog
          message={`Merge worktree "${mergeBranch}" into main? The worktree will be removed after merging.`}
          confirmLabel="Merge"
          variant="accent"
          onconfirm={handleMerge}
          oncancel={() => setMergeBranch(null)}
        />
      )}

      {pullMainConfirm && (
        <ConfirmDialog
          message={
            pullMainForce
              ? `Force pull "${config.mainBranch ?? "main"}"? This will discard any local commits on main.`
              : `Pull latest "${config.mainBranch ?? "main"}" from remote?`
          }
          confirmLabel={pullMainForce ? "Force Pull" : "Pull"}
          variant={pullMainForce ? "danger" : "accent"}
          loading={pullMainLoading}
          error={pullMainError}
          onconfirm={handlePullMain}
          oncancel={() => {
            setPullMainConfirm(false);
            setPullMainForce(false);
          }}
        />
      )}

      {pullLinkedRepoAlias && (
        <ConfirmDialog
          message={
            pullLinkedRepoForce
              ? `Force pull "${pullLinkedRepoAlias}"? This will discard any local commits.`
              : `Pull latest "${pullLinkedRepoAlias}" from remote?`
          }
          confirmLabel={pullLinkedRepoForce ? "Force Pull" : "Pull"}
          variant={pullLinkedRepoForce ? "danger" : "accent"}
          loading={pullLinkedRepoLoading}
          error={pullLinkedRepoError}
          onconfirm={handlePullLinkedRepo}
          oncancel={() => {
            setPullLinkedRepoAlias(null);
            setPullLinkedRepoForce(false);
          }}
        />
      )}

      {showSettingsDialog && (
        <SettingsDialog
          autoRemoveOnMerge={config.autoRemoveOnMerge ?? false}
          onautoremovechange={(enabled) => {
            setConfig({ ...config, autoRemoveOnMerge: enabled });
          }}
          onagentschange={(agents) => {
            setConfig({ ...config, agents });
          }}
          onclose={() => setShowSettingsDialog(false)}
        />
      )}

      {ciDetailsPr && (
        <CiDetailsDialog
          pr={ciDetailsPr}
          branch={selectedWorktree?.branch ?? ""}
          onclose={() => setCiDetailsPr(null)}
          onfixsuccess={() => {
            setCiDetailsPr(null);
          }}
        />
      )}

      {commentReviewPr && (
        <CommentReviewDialog
          pr={commentReviewPr}
          branch={selectedWorktree?.branch ?? ""}
          onclose={() => setCommentReviewPr(null)}
          onsendsuccess={() => {
            setCommentReviewPr(null);
          }}
        />
      )}

      {showDiffDialog && selectedBranch && (
        <DiffDialog
          branch={selectedBranch}
          onclose={() => setShowDiffDialog(false)}
        />
      )}

      <ToastStack onselect={handleSelectToast} />
    </>
  );
}
