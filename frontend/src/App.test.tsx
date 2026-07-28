import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AgentsUiWorktreeConversationResponse,
  AppConfig,
  AppNotification,
  WorktreeInfo,
} from "./lib/types";

const { MockFitAddon, MockTerminal, MockWebSocket } = vi.hoisted(() => {
  class MockFitAddon {
    static instances: MockFitAddon[] = [];

    fit = vi.fn();

    constructor() {
      MockFitAddon.instances.push(this);
    }
  }

  class MockTerminal {
    static instances: MockTerminal[] = [];

    options: { theme?: unknown } = {};
    cols = 80;
    rows = 24;
    modes = { mouseTrackingMode: "none" };
    parser = { registerOscHandler: vi.fn(() => true) };
    loadAddon = vi.fn();
    onSelectionChange = vi.fn();
    attachCustomKeyEventHandler = vi.fn();
    focus = vi.fn();
    writeln = vi.fn();
    write = vi.fn();
    clearSelection = vi.fn();
    dispose = vi.fn();

    constructor(_options: unknown) {
      MockTerminal.instances.push(this);
    }

    open(container: HTMLElement): void {
      const xterm = document.createElement("div");
      xterm.className = "xterm";
      const viewport = document.createElement("div");
      viewport.className = "xterm-viewport";
      xterm.appendChild(viewport);
      container.appendChild(xterm);
    }

    onData(_handler: (data: string) => void): void {}

    getSelection(): string {
      return "";
    }

    hasSelection(): boolean {
      return false;
    }
  }

  class MockWebSocket {
    static readonly CONNECTING = 0;
    static readonly OPEN = 1;
    static readonly CLOSING = 2;
    static readonly CLOSED = 3;
    static instances: MockWebSocket[] = [];

    readonly url: string;
    readyState = MockWebSocket.CONNECTING;
    sent: string[] = [];
    onopen: ((event: Event) => void) | null = null;
    onmessage: ((event: MessageEvent<string>) => void) | null = null;
    onclose: ((event: CloseEvent) => void) | null = null;
    onerror: ((event: Event) => void) | null = null;

    constructor(url: string | URL) {
      this.url = String(url);
      MockWebSocket.instances.push(this);
    }

    send(data: string): void {
      this.sent.push(data);
    }

    close(): void {
      this.readyState = MockWebSocket.CLOSED;
    }
  }

  return { MockFitAddon, MockTerminal, MockWebSocket };
});

vi.mock("@xterm/xterm", () => ({ Terminal: MockTerminal }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: MockFitAddon }));
vi.mock("@xterm/addon-web-links", () => ({
  WebLinksAddon: class MockWebLinksAddon {},
}));

vi.mock("./lib/api", () => ({
  api: {
    closeWorktree: vi.fn(),
    createWorktree: vi.fn(),
    dismissNotification: vi.fn(),
    fetchAvailableBranches: vi.fn(),
    fetchBaseBranches: vi.fn(),
    fetchCiLogs: vi.fn(),
    fetchConfig: vi.fn(),
    fetchWorktreeDiff: vi.fn(),
    mergeWorktree: vi.fn(),
    openWorktree: vi.fn(),
    pullMain: vi.fn(),
    removeWorktree: vi.fn(),
    setWorktreeArchived: vi.fn(),
    sendWorktreePrompt: vi.fn(),
  },
  attachWorktreeConversation: vi.fn(),
  connectWorktreeConversationStream: vi.fn(),
  fetchWorktreeConversationHistory: vi.fn(),
  fetchWorktrees: vi.fn(),
  interruptWorktreeConversation: vi.fn(),
  refreshWorktreeAgentTerminal: vi.fn(),
  sendWorktreeConversationMessage: vi.fn(),
  setWorktreeLabel: vi.fn(),
  subscribeNotifications: vi.fn(),
  uploadFiles: vi.fn(),
  activePrefix: "",
  apiBase: "",
  fetchProjects: vi.fn(async () => []),
  fetchInstances: vi.fn(async () => []),
  setUpProject: vi.fn(),
  removeProject: vi.fn(),
  createWorktreeTab: vi.fn(),
  createWorktreeShellTab: vi.fn(),
  createWorktreeAgentTab: vi.fn(),
  deleteWorktreeTab: vi.fn(),
  selectWorktreeTab: vi.fn(),
}));

import App from "./App";
import {
  api,
  attachWorktreeConversation,
  createWorktreeAgentTab,
  connectWorktreeConversationStream,
  fetchWorktrees,
  refreshWorktreeAgentTerminal,
  setWorktreeLabel,
  subscribeNotifications,
} from "./lib/api";
import { useStore } from "./store";
import { builtinAgentCapabilities } from "./lib/api-contract/test-fixtures";

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason?: unknown) => void;
}

class MockNotification {
  static permission: NotificationPermission = "denied";

  static requestPermission = vi.fn(async () => "denied" as const);

  constructor(_title: string, _options?: NotificationOptions) {}
}

const originalMatchMedia = window.matchMedia;
const originalNotification = globalThis.Notification;
const originalDialogShowModal = HTMLDialogElement.prototype.showModal;
const originalDialogClose = HTMLDialogElement.prototype.close;
const originalWebSocket = globalThis.WebSocket;
const originalResizeObserver = globalThis.ResizeObserver;
const originalRequestAnimationFrame = globalThis.requestAnimationFrame;

class MockResizeObserver {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function createConfig(overrides: Partial<AppConfig> = {}): AppConfig {
  return {
    name: "repo",
    services: [],
    startupEnvs: {},
    profiles: [{ name: "default" }],
    agents: [
      {
        id: "claude",
        label: "Claude",
        kind: "builtin",
        capabilities: builtinAgentCapabilities(),
      },
      {
        id: "codex",
        label: "Codex",
        kind: "builtin",
        capabilities: builtinAgentCapabilities(),
      },
    ],
    launchers: [],
    defaultProfileName: "default",
    defaultAgentId: "claude",
    autoName: false,
    linkedRepos: [],
    autoRemoveOnMerge: false,
    projectDir: "/repo",
    mainBranch: "main",
    ...overrides,
  };
}

function createWorktree(
  branch: string,
  overrides: Partial<WorktreeInfo> = {},
): WorktreeInfo {
  return {
    branch,
    kind: "linked",
    label: null,
    archived: false,
    agent: "waiting",
    mux: "",
    path: `/repo/__worktrees/${branch}`,
    dir: `/repo/__worktrees/${branch}`,
    dirty: false,
    unpushed: false,
    status: "idle",
    elapsed: "1m",
    profile: null,
    agentName: null,
    agentLabel: null,
    agentTerminalStale: false,
    services: [],
    paneCount: 1,
    prs: [],
    creating: false,
    creationPhase: null,
    source: "ui",
    oneshot: null,
    tabs: [],
    activeTabId: null,
    ...overrides,
  };
}

function createConversationResponse(
  worktree: WorktreeInfo,
): AgentsUiWorktreeConversationResponse {
  return {
    worktree: {
      branch: worktree.branch,
      path: worktree.path,
      archived: worktree.archived,
      profile: worktree.profile,
      agentName: worktree.agentName,
      agentLabel: worktree.agentLabel,
      agentTerminalStale: worktree.agentTerminalStale,
      mux: worktree.mux === "✓",
      status: worktree.status,
      dirty: worktree.dirty,
      unpushed: worktree.unpushed,
      services: worktree.services,
      prs: worktree.prs,
      creating: worktree.creating,
      creationPhase: worktree.creationPhase,
      conversation: null,
    },
    conversation: {
      provider: worktree.agentName === "codex" ? "codexAppServer" : "claudeCode",
      conversationId: worktree.agentName === "codex" ? "thread-1" : "session-1",
      cwd: worktree.path,
      running: false,
      activeTurnId: null,
      messages: [],
    },
  };
}

function createAppNotification(
  overrides: Partial<AppNotification> = {},
): AppNotification {
  return {
    id: 1,
    branch: "feature/toast",
    type: "runtime_error",
    message: "Notification text",
    url: "https://example.com/notifications/1",
    timestamp: Date.UTC(2026, 3, 9, 11, 30, 0),
    ...overrides,
  };
}

function resetStore(): void {
  useStore.setState({
    config: createConfig(),
    worktrees: [],
    hasLoadedWorktrees: false,
    availableBranches: [],
    baseBranches: [],
    selectedBranch: null,
    searchQuery: "",
    showArchivedWorktrees: false,
    includeRemoteBranches: false,
    useWebChatUi: false,
    notifications: [],
    uiToasts: [],
    notificationHistory: [],
    unreadCount: 0,
    dialog: { kind: "none" },
  });
}

function setupBrowserMocks(): void {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
  Object.defineProperty(globalThis, "Notification", {
    configurable: true,
    writable: true,
    value: MockNotification,
  });
  Object.defineProperty(globalThis, "WebSocket", {
    configurable: true,
    writable: true,
    value: MockWebSocket,
  });
  Object.defineProperty(globalThis, "ResizeObserver", {
    configurable: true,
    writable: true,
    value: MockResizeObserver,
  });
  Object.defineProperty(globalThis, "requestAnimationFrame", {
    configurable: true,
    writable: true,
    value: (callback: FrameRequestCallback) => {
      callback(0);
      return 0;
    },
  });
  HTMLDialogElement.prototype.showModal = vi.fn(function (this: HTMLDialogElement): void {
    this.open = true;
  });
  HTMLDialogElement.prototype.close = vi.fn(function (this: HTMLDialogElement): void {
    this.open = false;
  });
}

function restoreBrowserMocks(): void {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: originalMatchMedia,
  });
  Object.defineProperty(globalThis, "Notification", {
    configurable: true,
    writable: true,
    value: originalNotification,
  });
  Object.defineProperty(globalThis, "WebSocket", {
    configurable: true,
    writable: true,
    value: originalWebSocket,
  });
  Object.defineProperty(globalThis, "ResizeObserver", {
    configurable: true,
    writable: true,
    value: originalResizeObserver,
  });
  Object.defineProperty(globalThis, "requestAnimationFrame", {
    configurable: true,
    writable: true,
    value: originalRequestAnimationFrame,
  });
  HTMLDialogElement.prototype.showModal = originalDialogShowModal;
  HTMLDialogElement.prototype.close = originalDialogClose;
}

async function openCreateDialogAndSubmit(branch: string): Promise<void> {
  fireEvent.click(screen.getByTitle("New Worktree (Cmd+K)"));
  await screen.findByText("New Worktree");
  fireEvent.change(screen.getByLabelText(/Branch name/i), {
    target: { value: branch },
  });
  fireEvent.click(screen.getByRole("button", { name: "Create" }));
}

async function openCreateDialogWithBaseAndSubmit(branch: string, baseBranch: string): Promise<void> {
  fireEvent.click(screen.getByTitle("New Worktree (Cmd+K)"));
  await screen.findByText("New Worktree");
  fireEvent.change(screen.getByLabelText(/Branch name/i), {
    target: { value: branch },
  });
  fireEvent.click(screen.getByRole("button", { name: "Base branch" }));
  fireEvent.click(await screen.findByRole("button", { name: baseBranch }));
  fireEvent.click(screen.getByRole("button", { name: "Create" }));
}

describe("App create selection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    MockTerminal.instances = [];
    MockFitAddon.instances = [];
    MockWebSocket.instances = [];
    cleanup();
    localStorage.clear();
    resetStore();
    setupBrowserMocks();

    vi.mocked(api.fetchConfig).mockResolvedValue(createConfig());
    vi.mocked(fetchWorktrees).mockResolvedValue([]);
    vi.mocked(api.fetchAvailableBranches).mockResolvedValue({ branches: [] });
    vi.mocked(api.fetchBaseBranches).mockResolvedValue({ branches: [] });
    vi.mocked(api.fetchWorktreeDiff).mockResolvedValue({
      uncommitted: "",
      uncommittedTruncated: false,
      gitStatus: "",
      unpushedCommits: [],
    });
    vi.mocked(subscribeNotifications).mockReturnValue(() => {});
    vi.mocked(api.openWorktree).mockResolvedValue({ ok: true });
    vi.mocked(api.closeWorktree).mockResolvedValue({ ok: true });
    vi.mocked(api.removeWorktree).mockResolvedValue({ ok: true });
    vi.mocked(api.setWorktreeArchived).mockResolvedValue({ ok: true, archived: true });
    vi.mocked(api.mergeWorktree).mockResolvedValue({ ok: true });
    vi.mocked(api.pullMain).mockResolvedValue({ status: "updated" });
    vi.mocked(api.dismissNotification).mockResolvedValue({ ok: true });
    vi.mocked(api.fetchCiLogs).mockResolvedValue({ logs: "" });
    vi.mocked(api.sendWorktreePrompt).mockResolvedValue({ ok: true });
    vi.mocked(connectWorktreeConversationStream).mockReturnValue(() => {});
    vi.mocked(refreshWorktreeAgentTerminal).mockResolvedValue(undefined);
    vi.mocked(setWorktreeLabel).mockResolvedValue(null);
  });

  afterEach(() => {
    cleanup();
    restoreBrowserMocks();
  });

  it("keeps the current selection when a new worktree is created from an existing selection", async () => {
    const existingWorktree = createWorktree("main");
    const creatingWorktree = createWorktree("feature/new", {
      creating: true,
      creationPhase: "creating_worktree",
    });
    const newWorktree = createWorktree("feature/new");
    const createResult = deferred<{ primaryBranch: string; branches: string[] }>();

    vi.mocked(fetchWorktrees)
      .mockResolvedValueOnce([existingWorktree])
      .mockResolvedValueOnce([existingWorktree, creatingWorktree])
      .mockResolvedValueOnce([existingWorktree, newWorktree])
      .mockResolvedValue([existingWorktree, newWorktree]);
    vi.mocked(api.createWorktree).mockReturnValueOnce(createResult.promise);

    render(<App />);

    await screen.findByTitle("main");

    await openCreateDialogAndSubmit("feature/new");

    await waitFor(() => {
      expect(fetchWorktrees).toHaveBeenCalledTimes(2);
    });
    expect(screen.getByRole("button", { name: /^feature\/new\b/i })).toBeInTheDocument();
    expect(screen.getByTitle("main")).toBeInTheDocument();
    expect(screen.queryByTitle("feature/new")).not.toBeInTheDocument();

    createResult.resolve({ primaryBranch: "feature/new", branches: ["feature/new"] });

    await waitFor(() => {
      expect(fetchWorktrees).toHaveBeenCalledTimes(3);
    });
    expect(screen.getByTitle("main")).toBeInTheDocument();
    expect(screen.queryByTitle("feature/new")).not.toBeInTheDocument();
  });

  it("selects the new worktree when nothing was selected before creation", async () => {
    const creatingWorktree = createWorktree("feature/new", {
      creating: true,
      creationPhase: "creating_worktree",
    });
    const newWorktree = createWorktree("feature/new");
    const createResult = deferred<{ primaryBranch: string; branches: string[] }>();

    vi.mocked(fetchWorktrees)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([creatingWorktree])
      .mockResolvedValueOnce([newWorktree])
      .mockResolvedValue([newWorktree]);
    vi.mocked(api.createWorktree).mockReturnValueOnce(createResult.promise);

    render(<App />);

    await screen.findByText("Select a worktree");

    await openCreateDialogAndSubmit("feature/new");
    createResult.resolve({ primaryBranch: "feature/new", branches: ["feature/new"] });

    await waitFor(() => {
      expect(fetchWorktrees).toHaveBeenCalledTimes(3);
    });
    expect(screen.getByTitle("feature/new")).toBeInTheDocument();
  });

  it("shows an error toast when worktree creation fails", async () => {
    vi.mocked(fetchWorktrees).mockResolvedValue([]);
    vi.mocked(api.createWorktree).mockRejectedValueOnce(new Error("branch exists"));

    render(<App />);

    await screen.findByText("Select a worktree");
    await openCreateDialogAndSubmit("feature/new");

    const toast = await screen.findByRole("alert");
    expect(toast).toHaveTextContent("Failed to create: branch exists");
  });

  it("dismisses notification toasts through the notification API", async () => {
    let onNotification: ((notification: AppNotification) => void) | undefined;

    vi.mocked(fetchWorktrees).mockResolvedValue([]);
    vi.mocked(subscribeNotifications).mockImplementation((handleNotification) => {
      onNotification = handleNotification;
      return () => {};
    });

    render(<App />);

    await screen.findByText("Select a worktree");
    act(() => {
      onNotification?.(createAppNotification({ id: 42, message: "Background error" }));
    });

    const toast = await screen.findByRole("alert");
    const dismissButton = Array.from(toast.querySelectorAll("button")).find(
      (button) => button.textContent === "×",
    );

    expect(dismissButton).toBeDefined();
    fireEvent.click(dismissButton!);

    expect(api.dismissNotification).toHaveBeenCalledWith({ params: { id: 42 } });
  });

  it("shows a success toast when pulling main succeeds", async () => {
    // Pull now lives in the repo row's actions menu, not a sidebar footer strip.
    vi.mocked(api.fetchConfig).mockResolvedValue(createConfig({
      projectDir: "/repo",
      mainBranch: "main",
    }));
    vi.mocked(fetchWorktrees).mockResolvedValue([createWorktree("main", { kind: "main" })]);
    vi.mocked(api.pullMain).mockResolvedValueOnce({ status: "updated" });

    render(<App />);

    fireEvent.click(await screen.findByLabelText("Actions for main"));
    fireEvent.click(screen.getByRole("button", { name: "Pull" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Pull" }));

    expect(api.pullMain).toHaveBeenCalledWith({ body: {} });
    expect(await screen.findByRole("alert")).toHaveTextContent('Pulled latest "main" from remote');
  });

  it("omits merge, archive, remove and sub-worktree from the repo row menu", async () => {
    // These are refused server-side for the main checkout, so they must not be
    // offered at all — omitted, not merely disabled.
    vi.mocked(fetchWorktrees).mockResolvedValue([
      createWorktree("main", { kind: "main" }),
      createWorktree("feature/x"),
    ]);

    render(<App />);
    fireEvent.click(await screen.findByLabelText("Actions for main"));

    const menu = within(document.querySelector("[data-worktree-row-menu]") as HTMLElement);
    expect(menu.queryByRole("button", { name: "Merge" })).toBeNull();
    expect(menu.queryByRole("button", { name: "Archive" })).toBeNull();
    expect(menu.queryByRole("button", { name: "Remove" })).toBeNull();
    expect(menu.queryByRole("button", { name: "Create sub-worktree" })).toBeNull();
    // Close and Pull remain.
    expect(menu.getByRole("button", { name: "Close" })).toBeInTheDocument();
    expect(menu.getByRole("button", { name: "Pull" })).toBeInTheDocument();
  });

  it("still offers the full action menu for a linked worktree", async () => {
    // Guards against over-hiding: the trimming must be scoped to kind === "main".
    vi.mocked(fetchWorktrees).mockResolvedValue([
      createWorktree("main", { kind: "main" }),
      createWorktree("feature/x"),
    ]);

    render(<App />);
    fireEvent.click(await screen.findByLabelText("Actions for feature/x"));
    const menu = within(document.querySelector("[data-worktree-row-menu]") as HTMLElement);
    for (const name of ["Merge", "Archive", "Remove", "Create sub-worktree"]) {
      expect(menu.getByRole("button", { name })).toBeInTheDocument();
    }
    expect(menu.queryByRole("button", { name: "Pull" })).toBeNull();
  });

  it("renders linked repos as footer pull strips, separately from the repo row", async () => {
    // Linked repos are separate repositories with no worktree/session, so they
    // keep the SidebarRepoRow treatment the main branch no longer uses.
    vi.mocked(api.fetchConfig).mockResolvedValue(createConfig({
      projectDir: "/repo",
      mainBranch: "main",
      linkedRepos: [{ alias: "docs", dir: "/repo-docs" }],
    }));
    vi.mocked(fetchWorktrees).mockResolvedValue([createWorktree("main", { kind: "main" })]);

    render(<App />);
    await screen.findByLabelText("Actions for main");
    expect(await screen.findByText("docs")).toBeInTheDocument();
  });

  it("selects the primary paired worktree when Both is created without a prior selection", async () => {
    const creatingClaude = createWorktree("claude-feature/new", {
      creating: true,
      creationPhase: "creating_worktree",
    });
    const creatingCodex = createWorktree("codex-feature/new", {
      creating: true,
      creationPhase: "creating_worktree",
    });
    const createdClaude = createWorktree("claude-feature/new");
    const createdCodex = createWorktree("codex-feature/new");
    const createResult = deferred<{ primaryBranch: string; branches: string[] }>();

    vi.mocked(fetchWorktrees)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([creatingClaude, creatingCodex])
      .mockResolvedValueOnce([createdClaude, createdCodex])
      .mockResolvedValue([createdClaude, createdCodex]);
    vi.mocked(api.createWorktree).mockReturnValueOnce(createResult.promise);

    render(<App />);

    await screen.findByText("Select a worktree");

    fireEvent.click(screen.getByTitle("New Worktree (Cmd+K)"));
    await screen.findByText("New Worktree");
    fireEvent.click(screen.getByRole("switch", { name: /enable multiple agent selection/i }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Codex" }));
    fireEvent.change(screen.getByLabelText(/Branch name/i), {
      target: { value: "feature/new" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create" }));

    createResult.resolve({
      primaryBranch: "claude-feature/new",
      branches: ["claude-feature/new", "codex-feature/new"],
    });

    await waitFor(() => {
      expect(fetchWorktrees).toHaveBeenCalledTimes(3);
    });
    expect(screen.getByTitle("claude-feature/new")).toBeInTheDocument();
  });

  it("hides archived worktrees until the archived toggle is enabled", async () => {
    vi.mocked(fetchWorktrees).mockResolvedValue([
      createWorktree("feature/active"),
      createWorktree("feature/archived", { archived: true }),
    ]);

    render(<App />);

    await screen.findByRole("button", { name: /^feature\/active\b/i });
    expect(screen.queryByRole("button", { name: /feature\/archived/i })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("switch", { name: /show archived worktrees/i }));

    expect(
      await screen.findByRole("button", { name: /^feature\/archived\b/i }),
    ).toBeInTheDocument();
  });

  it("keeps the current selection while filtering the worktree list", async () => {
    vi.mocked(fetchWorktrees).mockResolvedValue([
      createWorktree("main"),
      createWorktree("feature/alpha"),
      createWorktree("feature/beta"),
    ]);

    render(<App />);

    const searchInput = await screen.findByRole("searchbox", { name: /search worktrees/i });
    await screen.findByTitle("main");

    fireEvent.focus(searchInput);
    fireEvent.change(searchInput, { target: { value: "feature" } });

    expect(screen.getByTitle("main")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^main\b/i })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^feature\/alpha\b/i })).toBeInTheDocument();
  });

  it("clears the worktree search from the trailing clear button", async () => {
    vi.mocked(fetchWorktrees).mockResolvedValue([
      createWorktree("feature/alpha"),
      createWorktree("feature/beta"),
    ]);

    render(<App />);

    const searchInput = await screen.findByRole("searchbox", { name: /search worktrees/i });
    fireEvent.change(searchInput, { target: { value: "alpha" } });
    expect(searchInput).toHaveValue("alpha");

    fireEvent.click(screen.getByRole("button", { name: /clear worktree search/i }));

    expect(searchInput).toHaveValue("");
  });

  it("archives the selected worktree through the API", async () => {
    vi.mocked(fetchWorktrees)
      .mockResolvedValueOnce([createWorktree("feature/active")])
      .mockResolvedValueOnce([createWorktree("feature/active", { archived: true })])
      .mockResolvedValue([createWorktree("feature/active", { archived: true })]);

    render(<App />);

    await screen.findByTitle("feature/active");
    fireEvent.click(screen.getByRole("button", { name: "Archive" }));

    await waitFor(() => {
      expect(api.setWorktreeArchived).toHaveBeenCalledWith({
        params: { name: "feature/active" },
        body: { archived: true },
      });
    });
  });

  it("reconnects the visible terminal after refreshing a stale terminal", async () => {
    useStore.setState({ selectedBranch: "feature/active" });
    localStorage.setItem("wt-last-selected-worktree", "feature/active");
    const staleWorktree = createWorktree("feature/active", {
      mux: "✓",
      agentName: "codex",
      agentLabel: "Codex",
      agentTerminalStale: true,
    });
    const refreshedWorktree = createWorktree("feature/active", {
      mux: "✓",
      agentName: "codex",
      agentLabel: "Codex",
      agentTerminalStale: false,
    });

    vi.mocked(fetchWorktrees)
      .mockResolvedValueOnce([staleWorktree])
      .mockResolvedValueOnce([refreshedWorktree])
      .mockResolvedValue([refreshedWorktree]);

    render(<App />);

    await screen.findByText("Terminal stale");
    await waitFor(() => {
      expect(MockWebSocket.instances).toHaveLength(1);
    });

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    await waitFor(() => {
      expect(refreshWorktreeAgentTerminal).toHaveBeenCalledWith("feature/active");
    });
    await waitFor(() => {
      expect(MockWebSocket.instances).toHaveLength(2);
    });
    expect(MockWebSocket.instances[0]?.readyState).toBe(MockWebSocket.CLOSED);
  });

  it("edits the selected worktree label from the header", async () => {
    vi.mocked(fetchWorktrees).mockResolvedValue([createWorktree("feature/active")]);
    vi.mocked(setWorktreeLabel).mockResolvedValue("Search ranking");

    render(<App />);

    await screen.findByTitle("feature/active");
    fireEvent.click(screen.getByRole("button", { name: "Edit workspace label" }));
    fireEvent.change(screen.getByLabelText("Label"), {
      target: { value: "Search ranking" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(setWorktreeLabel).toHaveBeenCalledWith("feature/active", "Search ranking");
    });
    expect(screen.getAllByText("Search ranking").length).toBeGreaterThan(0);
  });

  it("shows the web chat UI on desktop when the local setting is enabled", async () => {
    const worktree = createWorktree("feature/chat", {
      mux: "✓",
      agentName: "claude",
      agentLabel: "Claude",
    });
    useStore.setState({ useWebChatUi: true });
    vi.mocked(fetchWorktrees).mockResolvedValue([worktree]);
    vi.mocked(attachWorktreeConversation).mockResolvedValue(createConversationResponse(worktree));

    render(<App />);

    expect(await screen.findByRole("textbox", { name: "Message" })).toBeInTheDocument();
    expect(attachWorktreeConversation).toHaveBeenCalledWith("feature/chat");
  });

  it("does not show the stale terminal banner in the web chat UI", async () => {
    const worktree = createWorktree("feature/chat-stale-terminal", {
      mux: "✓",
      agentName: "codex",
      agentLabel: "Codex",
      agentTerminalStale: true,
    });
    useStore.setState({ useWebChatUi: true });
    vi.mocked(fetchWorktrees).mockResolvedValue([worktree]);
    vi.mocked(attachWorktreeConversation).mockResolvedValue(createConversationResponse(worktree));

    render(<App />);

    expect(await screen.findByRole("textbox", { name: "Message" })).toBeInTheDocument();
    expect(screen.queryByText("Terminal stale")).not.toBeInTheDocument();
  });

  it("shows prefixed branch previews when multiple agents are selected", async () => {
    vi.mocked(fetchWorktrees).mockResolvedValue([]);

    render(<App />);

    fireEvent.click(screen.getByTitle("New Worktree (Cmd+K)"));
    await screen.findByText("New Worktree");

    fireEvent.click(screen.getByRole("switch", { name: /enable multiple agent selection/i }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Codex" }));
    fireEvent.change(screen.getByLabelText(/Branch name/i), {
      target: { value: "feature/new" },
    });

    expect(screen.getByText("claude-feature/new")).toBeInTheDocument();
    expect(screen.getByText("codex-feature/new")).toBeInTheDocument();
  });

  it("submits multi-agent worktree creation when multiple agents are selected", async () => {
    vi.mocked(fetchWorktrees).mockResolvedValue([]);
    vi.mocked(api.createWorktree).mockResolvedValue({
      primaryBranch: "claude-feature/new",
      branches: ["claude-feature/new", "codex-feature/new"],
    });

    render(<App />);

    fireEvent.click(screen.getByTitle("New Worktree (Cmd+K)"));
    await screen.findByText("New Worktree");

    fireEvent.click(screen.getByRole("switch", { name: /enable multiple agent selection/i }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Codex" }));
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Use existing branch" })).not.toBeInTheDocument();
    });

    fireEvent.change(screen.getByLabelText(/Branch name/i), {
      target: { value: "feature/new" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create" }));

    await waitFor(() => {
      expect(api.createWorktree).toHaveBeenCalledWith({
        body: {
          mode: "new",
          branch: "feature/new",
          profile: "default",
          agents: ["claude", "codex"],
        },
      });
    });
  });

  it("submits an explicit base branch when provided", async () => {
    vi.mocked(fetchWorktrees).mockResolvedValue([]);
    vi.mocked(api.fetchBaseBranches).mockResolvedValue({ branches: [{ name: "release/base" }] });
    vi.mocked(api.createWorktree).mockResolvedValue({
      primaryBranch: "feature/from-release",
      branches: ["feature/from-release"],
    });

    render(<App />);

    await openCreateDialogWithBaseAndSubmit("feature/from-release", "release/base");

    await waitFor(() => {
      expect(api.createWorktree).toHaveBeenCalledWith({
        body: {
          mode: "new",
          branch: "feature/from-release",
          baseBranch: "release/base",
          profile: "default",
          agents: ["claude"],
        },
      });
    });
  });

  it("caches branch lists across dialog openings and only fetches each mode once", async () => {
    vi.mocked(fetchWorktrees).mockResolvedValue([]);
    vi.mocked(api.fetchAvailableBranches)
      .mockResolvedValueOnce({ branches: [{ name: "feature/local-only" }] })
      .mockResolvedValueOnce({ branches: [{ name: "feature/local-only" }, { name: "feature/remote-only" }] });
    vi.mocked(api.fetchBaseBranches).mockResolvedValue({ branches: [{ name: "main" }] });

    render(<App />);

    fireEvent.click(screen.getByTitle("New Worktree (Cmd+K)"));
    await screen.findByText("New Worktree");

    await waitFor(() => {
      expect(api.fetchAvailableBranches).toHaveBeenCalledTimes(1);
      expect(api.fetchAvailableBranches).toHaveBeenCalledWith({ query: { includeRemote: false } });
      expect(api.fetchBaseBranches).toHaveBeenCalledTimes(1);
    });

    fireEvent.click(screen.getByRole("button", { name: "Use existing branch" }));
    fireEvent.click(await screen.findByRole("switch", { name: /include remote branches/i }));

    await waitFor(() => {
      expect(api.fetchAvailableBranches).toHaveBeenCalledTimes(2);
      expect(api.fetchAvailableBranches).toHaveBeenLastCalledWith({ query: { includeRemote: true } });
    });

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    fireEvent.click(screen.getByTitle("New Worktree (Cmd+K)"));
    await screen.findByText("New Worktree");

    await waitFor(() => {
      expect(api.fetchAvailableBranches).toHaveBeenCalledTimes(2);
      expect(api.fetchBaseBranches).toHaveBeenCalledTimes(1);
    });

    fireEvent.click(screen.getByRole("button", { name: "Use existing branch" }));
    fireEvent.click(await screen.findByRole("switch", { name: /include remote branches/i }));

    await waitFor(() => {
      expect(api.fetchAvailableBranches).toHaveBeenCalledTimes(2);
      expect(api.fetchBaseBranches).toHaveBeenCalledTimes(1);
    });
  });

  it("keeps the current branch list visible while remote branches are loading", async () => {
    const remoteBranches = deferred<Array<{ name: string }>>();

    vi.mocked(fetchWorktrees).mockResolvedValue([]);
    vi.mocked(api.fetchAvailableBranches)
      .mockResolvedValueOnce({ branches: [{ name: "feature/local-only" }] })
      .mockReturnValueOnce(remoteBranches.promise.then((branches) => ({ branches })));

    render(<App />);

    fireEvent.click(screen.getByTitle("New Worktree (Cmd+K)"));
    await screen.findByText("New Worktree");
    fireEvent.click(screen.getByRole("button", { name: "Use existing branch" }));

    expect(await screen.findByRole("button", { name: "feature/local-only" })).toBeInTheDocument();

    fireEvent.click(await screen.findByRole("switch", { name: /include remote branches/i }));

    expect(screen.getByRole("button", { name: "feature/local-only" })).toBeInTheDocument();
    expect(screen.getByText("Updating...")).toBeInTheDocument();

    remoteBranches.resolve([{ name: "feature/local-only" }, { name: "feature/remote-only" }]);

    expect(await screen.findByRole("button", { name: "feature/remote-only" })).toBeInTheDocument();
  });
  it("starts a fresh provider session from the tab bar", async () => {
    // A Claude worktree can start a Codex session against the same branch.
    const open = createWorktree("feature/x", {
      mux: "\u2713",
      agent: "idle",
      agentName: "claude",
      agentLabel: "Claude",
      tabs: [
        {
          tabId: "root",
          kind: "root",
          label: "Root",
          seq: null,
          sessionId: null,
          agent: "claude",
          createdAt: "2026-01-01T00:00:00Z",
        },
      ],
      activeTabId: "root",
    });
    vi.mocked(fetchWorktrees).mockResolvedValue([open]);
    vi.mocked(createWorktreeAgentTab).mockResolvedValue({
      tabId: "agent-codex-1",
      kind: "agent",
      label: "Codex",
      seq: 1,
      sessionId: null,
      agent: "codex",
      createdAt: "2026-01-01T00:00:00Z",
    });

    render(<App />);
    await screen.findByTitle("feature/x");

    fireEvent.click(screen.getByLabelText("New tab"));
    const menu = within(document.querySelector(".tab-add-container") as HTMLElement);
    fireEvent.click(menu.getByRole("button", { name: /New session/ }));
    fireEvent.click(menu.getByRole("button", { name: "Codex" }));

    await waitFor(() => {
      expect(createWorktreeAgentTab).toHaveBeenCalledWith("feature/x", "codex");
    });
  });

  it("offers New session but not Fork for a custom-agent worktree", async () => {
    const open = createWorktree("feature/goose", {
      mux: "\u2713",
      agent: "idle",
      agentName: "goose",
      agentLabel: "Goose",
      tabs: [
        {
          tabId: "root",
          kind: "root",
          label: "Root",
          seq: null,
          sessionId: null,
          agent: "goose",
          createdAt: "2026-01-01T00:00:00Z",
        },
      ],
      activeTabId: "root",
    });
    vi.mocked(fetchWorktrees).mockResolvedValue([open]);
    vi.mocked(api.fetchConfig).mockResolvedValue(
      createConfig({
        agents: [
          ...createConfig().agents,
          {
            id: "goose",
            label: "Goose",
            kind: "custom",
            capabilities: {
              terminal: true,
              inAppChat: false,
              conversationHistory: false,
              interrupt: false,
              resume: false,
              fork: false,
      pinnableSessionId: false,
      permissionInterception: false,
    },
          },
        ],
      }),
    );

    render(<App />);
    await screen.findByTitle("feature/goose");

    fireEvent.click(screen.getByLabelText("New tab"));
    const menu = within(document.querySelector(".tab-add-container") as HTMLElement);
    // Goose cannot fork, but a fresh session and a terminal are still offered.
    expect(menu.queryByRole("button", { name: "Fork" })).toBeNull();
    expect(menu.getByRole("button", { name: "Terminal" })).toBeInTheDocument();
    expect(menu.getByRole("button", { name: /New session/ })).toBeInTheDocument();
  });

  it("shows an error toast when starting a provider session fails", async () => {
    const open = createWorktree("feature/x", {
      mux: "\u2713",
      agent: "idle",
      agentName: "claude",
      agentLabel: "Claude",
      tabs: [
        {
          tabId: "root",
          kind: "root",
          label: "Root",
          seq: null,
          sessionId: null,
          agent: "claude",
          createdAt: "2026-01-01T00:00:00Z",
        },
      ],
      activeTabId: "root",
    });
    vi.mocked(fetchWorktrees).mockResolvedValue([open]);
    vi.mocked(createWorktreeAgentTab).mockRejectedValueOnce(new Error("agent is not configured"));

    render(<App />);
    await screen.findByTitle("feature/x");

    fireEvent.click(screen.getByLabelText("New tab"));
    const menu = within(document.querySelector(".tab-add-container") as HTMLElement);
    fireEvent.click(menu.getByRole("button", { name: /New session/ }));
    fireEvent.click(menu.getByRole("button", { name: "Codex" }));

    expect(await screen.findByText(/agent is not configured/)).toBeInTheDocument();
  });
});
