import { create } from "zustand";
import type {
  AppConfig,
  AppNotification,
  AvailableBranch,
  PrEntry,
  ToastInput,
  ToastItem,
  UiToastItem,
  WorktreeInfo,
} from "./lib/types";
import type { ThemeKey } from "./lib/themes";
import {
  SSH_STORAGE_KEY,
  applyTheme,
  loadSavedSelectedWorktree,
  loadSavedSidebarWidth,
  loadSavedTheme,
  loadUseWebChatUi,
  saveSelectedWorktree,
  saveSidebarWidth,
  saveUseWebChatUi,
} from "./lib/utils";

const AUTO_DISMISS_MS = 4000;
const MAX_HISTORY = 10;

/** Which modal (if any) is open, plus its payload. Mirrors the flag-per-dialog
 *  state the Svelte App orchestrator held; a single discriminated field keeps
 *  only one modal open at a time. */
export type Dialog =
  | { kind: "none" }
  | { kind: "create" }
  | { kind: "settings" }
  | { kind: "remove"; branch: string }
  | { kind: "merge"; branch: string }
  | { kind: "label"; branch: string }
  | { kind: "diff"; branch: string }
  | { kind: "ci"; pr: PrEntry }
  | { kind: "commentReview"; pr: PrEntry }
  | { kind: "pullMain" }
  | { kind: "pullLinked"; alias: string };

let nextToastId = 0;

/** Pure merge of notification-derived toasts + UI toasts. Kept out of the store
 *  selector so components can `useMemo` it — selecting a freshly-built array via
 *  the hook each render trips React 19 / Zustand's snapshot-cache check. */
export function deriveToasts(
  notifications: AppNotification[],
  uiToasts: UiToastItem[],
): ToastItem[] {
  const fromNotifications: ToastItem[] = notifications.map((n) => ({
    id: `notification:${n.id}`,
    source: "notification",
    notificationId: n.id,
    tone:
      n.type === "runtime_error"
        ? "error"
        : n.type === "agent_stopped" || n.type === "worktree_auto_removed"
          ? "success"
          : "info",
    message: n.message,
    ...(n.url ? { detail: n.url } : {}),
    branch: n.branch,
  }));
  return [...fromNotifications, ...uiToasts];
}

export interface StoreState {
  // --- data ---
  config: AppConfig;
  worktrees: WorktreeInfo[];
  hasLoadedWorktrees: boolean;
  availableBranches: AvailableBranch[];
  baseBranches: AvailableBranch[];

  // --- ui ---
  selectedBranch: string | null;
  searchQuery: string;
  showArchivedWorktrees: boolean;
  includeRemoteBranches: boolean;
  sshHost: string;
  theme: ThemeKey;
  useWebChatUi: boolean;
  sidebarWidth: number;
  dialog: Dialog;

  // --- notifications / toasts ---
  notifications: AppNotification[];
  uiToasts: UiToastItem[];
  notificationHistory: AppNotification[];
  unreadCount: number;

  // --- setters ---
  setConfig: (config: AppConfig) => void;
  setWorktrees: (worktrees: WorktreeInfo[]) => void;
  setHasLoadedWorktrees: (loaded: boolean) => void;
  setAvailableBranches: (branches: AvailableBranch[]) => void;
  setBaseBranches: (branches: AvailableBranch[]) => void;

  selectBranch: (branch: string | null) => void;
  setSearchQuery: (query: string) => void;
  setShowArchivedWorktrees: (show: boolean) => void;
  setIncludeRemoteBranches: (include: boolean) => void;
  setSshHost: (host: string) => void;
  setTheme: (theme: ThemeKey) => void;
  setUseWebChatUi: (use: boolean) => void;
  setSidebarWidth: (width: number) => void;
  openDialog: (dialog: Dialog) => void;
  closeDialog: () => void;

  // --- toast controller (replaces the Svelte toast context) ---
  showToast: (toast: ToastInput) => void;
  info: (message: string, detail?: string) => void;
  success: (message: string, detail?: string) => void;
  error: (message: string, detail?: string) => void;
  dismissUiToast: (id: string) => void;

  // --- notifications ---
  addNotification: (notification: AppNotification) => void;
  dismissNotification: (id: number) => void;
  clearUnread: () => void;

  /** Derived toast list (notifications + ui toasts), computed on read. */
  toasts: () => ToastItem[];
}

export const useStore = create<StoreState>((set, get) => ({
  config: {
    name: "",
    services: [],
    profiles: [],
    agents: [],
    launchers: [],
    defaultProfileName: "",
    defaultAgentId: "claude",
    autoName: false,
    startupEnvs: {},
    linkedRepos: [],
    autoRemoveOnMerge: false,
    projectDir: "",
    mainBranch: "",
  },
  worktrees: [],
  hasLoadedWorktrees: false,
  availableBranches: [],
  baseBranches: [],

  selectedBranch: loadSavedSelectedWorktree(),
  searchQuery: "",
  showArchivedWorktrees: false,
  includeRemoteBranches: false,
  sshHost: localStorage.getItem(SSH_STORAGE_KEY) ?? "",
  theme: loadSavedTheme(),
  useWebChatUi: loadUseWebChatUi(),
  sidebarWidth: loadSavedSidebarWidth(),
  dialog: { kind: "none" },

  notifications: [],
  uiToasts: [],
  notificationHistory: [],
  unreadCount: 0,

  setConfig: (config) => set({ config }),
  setWorktrees: (worktrees) => set({ worktrees }),
  setHasLoadedWorktrees: (hasLoadedWorktrees) => set({ hasLoadedWorktrees }),
  setAvailableBranches: (availableBranches) => set({ availableBranches }),
  setBaseBranches: (baseBranches) => set({ baseBranches }),

  selectBranch: (branch) => {
    saveSelectedWorktree(branch);
    set({ selectedBranch: branch });
  },
  setSearchQuery: (searchQuery) => set({ searchQuery }),
  setShowArchivedWorktrees: (showArchivedWorktrees) => set({ showArchivedWorktrees }),
  setIncludeRemoteBranches: (includeRemoteBranches) => set({ includeRemoteBranches }),
  setSshHost: (host) => {
    localStorage.setItem(SSH_STORAGE_KEY, host);
    set({ sshHost: host });
  },
  setTheme: (theme) => {
    applyTheme(theme);
    set({ theme });
  },
  setUseWebChatUi: (use) => {
    saveUseWebChatUi(use);
    set({ useWebChatUi: use });
  },
  setSidebarWidth: (width) => {
    saveSidebarWidth(width);
    set({ sidebarWidth: width });
  },
  openDialog: (dialog) => set({ dialog }),
  closeDialog: () => set({ dialog: { kind: "none" } }),

  showToast: (toast) => {
    const id = `ui:${nextToastId++}`;
    const item: UiToastItem = { ...toast, id, source: "ui" };
    set((state) => ({ uiToasts: [...state.uiToasts, item] }));
    setTimeout(() => get().dismissUiToast(id), AUTO_DISMISS_MS);
  },
  info: (message, detail) => get().showToast({ tone: "info", message, detail }),
  success: (message, detail) => get().showToast({ tone: "success", message, detail }),
  error: (message, detail) => get().showToast({ tone: "error", message, detail }),
  dismissUiToast: (id) =>
    set((state) => ({ uiToasts: state.uiToasts.filter((t) => t.id !== id) })),

  addNotification: (notification) =>
    set((state) => ({
      notifications: [...state.notifications, notification],
      notificationHistory: [notification, ...state.notificationHistory].slice(0, MAX_HISTORY),
      unreadCount: state.unreadCount + 1,
    })),
  dismissNotification: (id) =>
    set((state) => ({ notifications: state.notifications.filter((n) => n.id !== id) })),
  clearUnread: () => set({ unreadCount: 0 }),

  toasts: () => deriveToasts(get().notifications, get().uiToasts),
}));
