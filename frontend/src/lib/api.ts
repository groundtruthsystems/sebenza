import {
  AgentsUiConversationEventSchema,
  InboxJobEventSchema,
  apiPaths,
  createApi,
} from "./api-contract";
import type { InboxAgentJob } from "./api-contract";
import type {
  ActiveProjectWorktrees,
  AgentDetails,
  AgentResponse,
  AgentsUiConversationEvent,
  AgentsUiInterruptResponse,
  AgentsUiSendMessageRequest,
  AgentsUiSendMessageResponse,
  AgentsUiWorktreeConversationResponse,
  AppNotification,
  Tracks,
  TrackFileResponse,
  Portfolio,
  FileUploadResult,
  InstanceSummary,
  ProjectInitPhase,
  ProjectInitState,
  ProjectSummary,
  ProjectWorktreeSnapshot,
  UpsertCustomAgentRequest,
  ValidateCustomAgentResponse,
  WorktreeInfo,
  WorktreeTab,
} from "./types";

/** The active project's URL prefix, taken from the first path segment (the
 *  server serves each project under `/<prefix>/...` on the shared port). Empty
 *  when at the root before the bootstrap redirect picks a project. */
export const activePrefix: string =
  window.location.pathname.split("/")[1] ?? "";

/** Base path for the active project's API + WebSocket calls. */
export const apiBase: string = activePrefix ? `/${activePrefix}` : "";

/** Per-project client — every worktree/agent/config call is scoped to the
 *  active project. */
export const api = createApi(apiBase);

/** Hub client — project list/add/remove + the migration sensor are global (no prefix). */
const hubApi = createApi("");

function mapAgentStatus(status: string): string {
  switch (status) {
    case "creating":
    case "running":
    case "starting":
      return "working";
    case "idle":
      return "waiting";
    // Blocked on a permission prompt in the agent's own UI. Shares `waiting`'s visual
    // treatment (it needs you either way) but keeps its own value so the label can say
    // WHAT kind of attention it needs. Sebenza cannot answer the prompt for you.
    case "awaiting_permission":
      return "awaiting-permission";
    case "stopped":
      return "done";
    case "error":
      return "error";
    default:
      return "idle";
  }
}

function mapWorktree(snapshot: ProjectWorktreeSnapshot): WorktreeInfo {
  return {
    branch: snapshot.branch,
    kind: snapshot.kind,
    label: snapshot.label,
    ...(snapshot.baseBranch ? { baseBranch: snapshot.baseBranch } : {}),
    archived: snapshot.archived,
    agent: mapAgentStatus(snapshot.status),
    mux: snapshot.mux ? "✓" : "",
    path: snapshot.path,
    dir: snapshot.dir,
    dirty: snapshot.dirty,
    unpushed: snapshot.unpushed,
    status: snapshot.status,
    feedbackState: snapshot.feedbackState,
    elapsed: snapshot.elapsed,
    profile: snapshot.profile,
    agentName: snapshot.agentName,
    agentLabel: snapshot.agentLabel,
    agentTerminalStale: snapshot.agentTerminalStale,
    services: snapshot.services,
    paneCount: snapshot.paneCount,
    prs: snapshot.prs,
    creating: snapshot.creation !== null,
    creationPhase: snapshot.creation?.phase ?? null,
    source: snapshot.source,
    oneshot: snapshot.oneshot,
    tabs: snapshot.tabs,
    activeTabId: snapshot.activeTabId,
  };
}

export async function createWorktreeTab(branch: string): Promise<WorktreeTab> {
  const response = await api.createWorktreeTab({ params: { name: branch } });
  return response.tab;
}

export async function createWorktreeShellTab(
  branch: string,
): Promise<WorktreeTab> {
  const response = await api.createWorktreeShellTab({
    params: { name: branch },
  });
  return response.tab;
}

/** Start a fresh session of `agentId` as a new tab in `branch`'s worktree. */
export async function createWorktreeAgentTab(
  branch: string,
  agentId: string,
): Promise<WorktreeTab> {
  const response = await api.createWorktreeAgentTab({
    params: { name: branch },
    body: { agent: agentId },
  });
  return response.tab;
}

export function selectWorktreeTab(
  branch: string,
  tabId: string,
): Promise<void> {
  return api
    .selectWorktreeTab({ params: { name: branch, tabId } })
    .then(() => undefined);
}

export function deleteWorktreeTab(
  branch: string,
  tabId: string,
): Promise<void> {
  return api
    .deleteWorktreeTab({ params: { name: branch, tabId } })
    .then(() => undefined);
}

export async function fetchWorktrees(): Promise<WorktreeInfo[]> {
  const response = await api.fetchWorktrees();
  return response.worktrees.map((worktree) => mapWorktree(worktree));
}

/** Every loaded project's worktrees, for the cross-project ticker.
 *
 *  Hub-scoped, so it deliberately bypasses `apiBase`: the point is to see past the
 *  active project. Returns the raw snapshots — the caller runs the same
 *  `deriveTickerItems` used for the single-project ticker, so eligibility stays defined
 *  in one place. */
export async function fetchActiveWorktrees(): Promise<
  ActiveProjectWorktrees[]
> {
  const response = await hubApi.fetchActiveWorktrees();
  return response.projects.map((project) => ({
    prefix: project.prefix,
    name: project.name,
    worktrees: project.worktrees.map((worktree) => mapWorktree(worktree)),
  }));
}

export async function setWorktreeLabel(
  branch: string,
  label: string | null,
): Promise<string | null> {
  const response = await api.setWorktreeLabel({
    params: { name: branch },
    body: { label },
  });
  return response.label;
}

export function attachWorktreeConversation(
  branch: string,
): Promise<AgentsUiWorktreeConversationResponse> {
  return api.attachAgentsWorktreeConversation({
    params: { name: branch },
  });
}

export function fetchWorktreeConversationHistory(
  branch: string,
): Promise<AgentsUiWorktreeConversationResponse> {
  return api.fetchAgentsWorktreeConversationHistory({
    params: { name: branch },
  });
}

/** Sebenza track registry for a worktree (`.ai/sebenza/tracks.json`), or null
 *  when the worktree has no Sebenza workspace. */
export function fetchTracks(branch: string): Promise<Tracks | null> {
  return api.fetchTracks({ params: { name: branch } });
}

/** A single file under a worktree's `.ai/sebenza` dir
 *  (plan.json / spec.md / design.md / test-plan.md). */
export function fetchTrackFile(
  branch: string,
  path: string,
): Promise<TrackFileResponse> {
  return api.fetchTrackFile({ params: { name: branch }, query: { path } });
}

/** The user-scoped Sebenza registry (`~/.ai/sebenza/registry.json`) with each
 *  registered project's tracks resolved — the cross-project portfolio view. */
export function fetchRegistry(): Promise<Portfolio> {
  return hubApi.fetchRegistry();
}

/** A track file belonging to a *registered* project, addressed by its absolute
 *  registry `path` rather than by worktree. */
export function fetchRegistryFile(
  project: string,
  path: string,
): Promise<TrackFileResponse> {
  return hubApi.fetchRegistryFile({ query: { project, path } });
}

export function sendWorktreeConversationMessage(
  branch: string,
  body: AgentsUiSendMessageRequest,
): Promise<AgentsUiSendMessageResponse> {
  return api.sendAgentsWorktreeConversationMessage({
    params: { name: branch },
    body,
  });
}

export function interruptWorktreeConversation(
  branch: string,
): Promise<AgentsUiInterruptResponse> {
  return api.interruptAgentsWorktreeConversation({
    params: { name: branch },
  });
}

export function refreshWorktreeAgentTerminal(branch: string): Promise<void> {
  return api
    .refreshWorktreeAgentTerminal({
      params: { name: branch },
    })
    .then(() => undefined);
}

export function launchWorktree(
  branch: string,
  launcherId: string,
): Promise<void> {
  return api
    .launchWorktree({
      params: { name: branch },
      body: { launcherId },
    })
    .then(() => undefined);
}

function withWorktreeName(path: string, branch: string): string {
  return path.replace(":name", encodeURIComponent(branch));
}

export function connectWorktreeConversationStream(
  branch: string,
  callbacks: {
    onEvent: (event: AgentsUiConversationEvent) => void;
    onError: (message: string) => void;
    onClose?: () => void;
  },
): () => void {
  const socket = new WebSocket(
    `${window.location.protocol === "https:" ? "wss" : "ws"}://${window.location.host}${apiBase}${withWorktreeName(
      apiPaths.streamAgentsWorktreeConversation,
      branch,
    )}`,
  );
  let closedByClient = false;

  socket.addEventListener("message", (event) => {
    if (typeof event.data !== "string") return;
    try {
      callbacks.onEvent(
        AgentsUiConversationEventSchema.parse(JSON.parse(event.data)),
      );
    } catch {
      callbacks.onError("Received malformed conversation stream data");
    }
  });

  socket.addEventListener("error", () => {
    callbacks.onError("Conversation stream connection failed");
  });

  socket.addEventListener("close", () => {
    if (!closedByClient) {
      callbacks.onClose?.();
    }
  });

  return () => {
    closedByClient = true;
    socket.close();
  };
}

export function fetchAgents(): Promise<AgentDetails[]> {
  return api.fetchAgents().then((response) => response.agents);
}

export function createAgent(
  body: UpsertCustomAgentRequest,
): Promise<AgentResponse> {
  return api.createAgent({ body });
}

export function updateAgent(
  id: string,
  body: UpsertCustomAgentRequest,
): Promise<AgentResponse> {
  return api.updateAgent({ params: { id }, body });
}

export function deleteAgent(id: string): Promise<void> {
  return api.deleteAgent({ params: { id } }).then(() => undefined);
}

export function validateAgent(
  body: UpsertCustomAgentRequest,
): Promise<ValidateCustomAgentResponse> {
  return api.validateAgent({ body });
}

/** Other Sebenza servers running on this machine (migration sensor) — drives the
 *  banner that prompts the user to consolidate them with `sebenza-cli project migrate`. */
export async function fetchInstances(): Promise<InstanceSummary[]> {
  const response = await hubApi.fetchInstances();
  return response.instances;
}

export async function fetchProjects(): Promise<ProjectSummary[]> {
  const response = await hubApi.fetchProjects();
  return response.projects;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

const SETUP_POLL_INTERVAL_MS = 600;
const SETUP_TIMEOUT_MS = 5 * 60_000;

/** Add a project and, when the repo has no `.ai/sebenza.yaml`, drive its setup
 *  (scaffold → analyze with Claude → register) to completion, reporting each
 *  phase via `onPhase`. Resolves with the project's prefix once it's ready. */
export async function setUpProject(
  path: string,
  onPhase?: (phase: ProjectInitPhase) => void,
): Promise<{ prefix: string }> {
  const res = await hubApi.addProject({ body: { path } });
  if (!res.initializing) {
    if (!res.project)
      throw new Error(
        "Server accepted the project but returned nothing to open.",
      );
    return { prefix: res.project.prefix };
  }

  const deadline = Date.now() + SETUP_TIMEOUT_MS;
  let lastPhase: ProjectInitPhase | null = null;
  while (Date.now() < deadline) {
    // A transient poll failure shouldn't fail the flow — the backend job keeps
    // running, so swallow it and retry until the deadline.
    const inits = await hubApi
      .projectInits()
      .then((r) => r.inits)
      .catch((): ProjectInitState[] => []);
    const state = inits.find((entry) => entry.path === res.path);
    if (state) {
      if (state.phase !== lastPhase) {
        lastPhase = state.phase;
        onPhase?.(state.phase);
      }
      if (state.phase === "ready" && state.prefix)
        return { prefix: state.prefix };
      if (state.phase === "failed")
        throw new Error(state.error ?? "Project setup failed.");
    }
    await delay(SETUP_POLL_INTERVAL_MS);
  }
  throw new Error("Project setup timed out.");
}

export async function removeProject(prefix: string): Promise<void> {
  await hubApi.removeProject({ params: { prefix } });
}

export type ProjectBootstrap =
  "ready" | "redirecting" | "no-projects" | "registry" | "inbox";

/** Decide what to mount before the app loads, based on the URL prefix and the
 *  known projects:
 *  - `registry`     — `/registry`, the user-scoped portfolio; not a project, so
 *                     it must short-circuit before the redirect below.
 *  - `inbox`        — `/inbox`, the global draft store; likewise not a project,
 *                     and reachable with no projects registered at all.
 *  - `ready`        — the URL points at a real project; mount the dashboard.
 *  - `redirecting`  — the URL has no/unknown prefix but projects exist; a
 *                     redirect to the first project is in flight, mount nothing.
 *  - `no-projects`  — nothing is registered; mount the empty state so the
 *                     dashboard doesn't boot into 404-ing per-project calls. */
export async function ensureProjectPrefix(): Promise<ProjectBootstrap> {
  // `registry` and `inbox` are reserved prefixes server-side, so neither can
  // ever be a project.
  if (activePrefix === "registry") return "registry";
  if (activePrefix === "inbox") return "inbox";
  const projects = await fetchProjects().catch((): ProjectSummary[] => []);
  if (projects.some((project) => project.prefix === activePrefix))
    return "ready";
  const target = projects[0]?.prefix;
  if (!target) return "no-projects";
  window.location.replace(`/${target}/`);
  return "redirecting";
}

export function subscribeNotifications(
  onNotification: (n: AppNotification) => void,
  onDismiss: (id: number) => void,
  onInitial?: (n: AppNotification) => void,
): () => void {
  const es = new EventSource(`${apiBase}/api/notifications/stream`);

  es.addEventListener("initial", (e: MessageEvent) => {
    try {
      const n = JSON.parse(e.data as string) as AppNotification;
      onInitial?.(n);
    } catch {
      /* ignore malformed SSE data */
    }
  });

  es.addEventListener("notification", (e: MessageEvent) => {
    try {
      const n = JSON.parse(e.data as string) as AppNotification;
      onNotification(n);
    } catch {
      /* ignore malformed SSE data */
    }
  });

  es.addEventListener("dismiss", (e: MessageEvent) => {
    try {
      const { id } = JSON.parse(e.data as string) as { id: number };
      onDismiss(id);
    } catch {
      /* ignore malformed SSE data */
    }
  });

  return () => es.close();
}

export async function uploadFiles(
  worktree: string,
  files: File[],
): Promise<FileUploadResult> {
  const form = new FormData();
  for (const file of files) {
    form.append("files", file);
  }
  const res = await fetch(
    `${apiBase}/api/worktrees/${encodeURIComponent(worktree)}/upload`,
    {
      method: "POST",
      body: form,
    },
  );
  const data = await res.json();
  if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`);
  return data as FileUploadResult;
}

// ---------------------------------------------------------------------------
// Inbox — global drafts. These live on the hub client, not the per-project one:
// a draft may exist before it belongs to any project.
//
// Mutating calls carry the control token, which the server requires alongside a
// same-origin check. `window.__SEBENZA_CONTROL_TOKEN__` is injected into the
// page at load; a cross-origin page cannot read it.
// ---------------------------------------------------------------------------

declare global {
  interface Window {
    __SEBENZA_CONTROL_TOKEN__?: string;
  }
}

function inboxAuthHeaders(): Record<string, string> {
  const token = window.__SEBENZA_CONTROL_TOKEN__;
  return token ? { authorization: `Bearer ${token}` } : {};
}

/** Fetch the control token once at startup and cache it on `window`.
 *
 *  Served over GET rather than injected into the HTML so it works the same in
 *  dev (Vite proxy) and prod (SPA embedded in the binary). A cross-origin page
 *  can issue this request but cannot read the response - the server mounts no
 *  CORS layer, and the route additionally refuses a foreign `Origin`. */
export async function loadInboxControlToken(): Promise<void> {
  if (window.__SEBENZA_CONTROL_TOKEN__) return;
  try {
    const res = await fetch("/api/inbox/session", {
      credentials: "same-origin",
    });
    if (!res.ok) return;
    const data = (await res.json()) as { token?: string };
    if (data.token) window.__SEBENZA_CONTROL_TOKEN__ = data.token;
  } catch {
    // Leave it unset: mutating inbox calls will 401 and the UI reports it,
    // which is the honest failure rather than a silent half-working inbox.
  }
}

export async function fetchInboxDrafts(params?: {
  search?: string;
  includeDropped?: boolean;
}) {
  return hubApi.fetchInboxDrafts({ query: params ?? {} });
}

export async function fetchInboxDraft(id: string) {
  return hubApi.fetchInboxDraft({ params: { id } });
}

export async function createInboxDraft(title: string) {
  return hubApi.createInboxDraft({
    body: { title },
    extraHeaders: inboxAuthHeaders(),
  });
}

/** Save the body. `expectedHash` is the `bodyHash` last read; a stale one comes
 *  back 409 and nothing is written. */
export async function saveInboxDraftBody(
  id: string,
  expectedHash: string,
  body: string,
) {
  return hubApi.saveInboxDraftBody({
    params: { id },
    body: { expectedHash, body },
    extraHeaders: inboxAuthHeaders(),
  });
}

/** Rename, link/unlink a project (`projectPath: null` unlinks), or drop. */
export async function patchInboxDraft(
  id: string,
  patch: { title?: string; projectPath?: string | null; status?: "Dropped" },
) {
  return hubApi.patchInboxDraft({
    params: { id },
    body: patch,
    extraHeaders: inboxAuthHeaders(),
  });
}

/** Delete. A promoted draft needs `confirmed`, else the server answers 409. */
export async function deleteInboxDraft(id: string, confirmed = false) {
  return hubApi.deleteInboxDraft({
    params: { id },
    query: { confirmed },
    body: {},
    extraHeaders: inboxAuthHeaders(),
  });
}

/** Start a fan-out. Returns a job id immediately; the wave runs in the
 *  background because git plus tmux is multiple seconds per target. */
export async function convertInboxDraft(
  id: string,
  targets: {
    projectPath: string;
    branch: string;
    prompt: string;
    baseBranch?: string | null;
    agentId?: string | null;
    systemInstruction?: string | null;
    architectFirst?: boolean;
  }[],
) {
  return hubApi.convertInboxDraft({
    params: { id },
    body: { targets },
    extraHeaders: inboxAuthHeaders(),
  });
}

/** Ask the system agent for a `systemInstruction` per target, for the
 *  convert dialog to show and the operator to edit. Waits (bounded) for the
 *  job; on `fallback` every instruction is null and the conversion goes
 *  ahead with the operator prompt alone. */
export async function requestConvertInstructions(
  id: string,
  targets: { project: string; branch: string; prompt?: string }[],
) {
  return hubApi.requestConvertInstructions({
    params: { id },
    body: { targets },
    extraHeaders: inboxAuthHeaders(),
  });
}

/** Poll a conversion's progress. Unknown ids are a 404 — job ids are ULIDs,
 *  so that is the access check. */
export async function fetchConversionJob(jobId: string) {
  return hubApi.fetchConversionJob({ params: { id: jobId } });
}

/** Set (`P0`-`P3`) or clear (`null`) the operator priority override. While
 *  an override stands the system agent cannot move it. */
export async function setInboxPriority(
  id: string,
  priority: "P0" | "P1" | "P2" | "P3" | null,
) {
  return hubApi.setInboxPriority({
    params: { id },
    body: { priority },
    extraHeaders: inboxAuthHeaders(),
  });
}

/** The overall thread plus one thread per converted worktree. */
export async function fetchInboxComments(id: string) {
  return hubApi.fetchInboxComments({ params: { id } });
}

/** Post an operator comment, to a worktree's thread or (no `worktree`) the
 *  overall one. */
export async function postInboxComment(
  id: string,
  body: string,
  worktree?: { project: string; branch: string },
) {
  return hubApi.postInboxComment({
    params: { id },
    body: worktree ? { body, worktree } : { body },
    extraHeaders: inboxAuthHeaders(),
  });
}

/** Every request worktree agents raised against the item, with its state. */
export async function fetchInboxRequests(id: string) {
  return hubApi.fetchInboxRequests({ params: { id } });
}

/** Confirm a request's resolution and deliver it to its worktree.
 *  `contentHash` must be the hash of the text the operator was shown (the
 *  proposal, or `body` when authoring one); `body` with a proposal is an
 *  edit. A stale hash is a 409 and nothing is delivered. */
export async function confirmInboxRequest(
  id: string,
  requestId: string,
  confirm: { contentHash: string; body?: string },
) {
  return hubApi.confirmInboxRequest({
    params: { id, rid: requestId },
    body: confirm,
    extraHeaders: inboxAuthHeaders(),
  });
}

/** Reject a proposal (or a request) with a reason; it reopens. */
export async function rejectInboxRequest(
  id: string,
  requestId: string,
  reason: string,
) {
  return hubApi.rejectInboxRequest({
    params: { id, rid: requestId },
    body: { reason },
    extraHeaders: inboxAuthHeaders(),
  });
}

/** Retry a failed delivery as the next attempt. */
export async function redeliverInboxRequest(id: string, requestId: string) {
  return hubApi.redeliverInboxRequest({
    params: { id, rid: requestId },
    body: {},
    extraHeaders: inboxAuthHeaders(),
  });
}

/** Re-run triage on an open (typically flagged) request. */
export async function retryInboxTriage(id: string, requestId: string) {
  return hubApi.retryInboxTriage({
    params: { id, rid: requestId },
    body: {},
    extraHeaders: inboxAuthHeaders(),
  });
}

/** One system-agent job's state and validated result. */
export async function fetchInboxAgentJob(id: string, jobId: string) {
  return hubApi.fetchInboxAgentJob({ params: { id, jobId } });
}

/** Ask the system agent for a proposed body. Returns the job id; the
 *  proposal arrives on the job (`fetchInboxAgentJob` or the agent stream) and
 *  is never written to the draft. Apply it with `saveInboxDraftBody` and the
 *  hash the editor holds: a 409 means the body changed and needs a merge.
 *  A 503 means the system agent is disabled. */
export async function requestInboxDraftHelp(id: string, instruction?: string) {
  return hubApi.requestInboxDraftHelp({
    params: { id },
    body: instruction ? { instruction } : {},
    extraHeaders: inboxAuthHeaders(),
  });
}

/** Tombstone a comment, request, proposal or advice body; reads mask it. */
export async function redactInboxComment(id: string, eventId: string) {
  return hubApi.redactInboxComment({
    params: { id, eventId },
    body: {},
    extraHeaders: inboxAuthHeaders(),
  });
}

/** Follow an item's system-agent jobs: each known job, then every change,
 *  as `inbox.job` events. Returns a function that closes the stream. */
export function connectInboxAgentStream(
  id: string,
  callbacks: {
    onJob: (job: InboxAgentJob) => void;
    onError: (message: string) => void;
    onClose?: () => void;
  },
): () => void {
  const socket = new WebSocket(
    `${window.location.protocol === "https:" ? "wss" : "ws"}://${window.location.host}${apiPaths.streamInboxAgent.replace(
      ":id",
      encodeURIComponent(id),
    )}`,
  );
  let closedByClient = false;
  socket.addEventListener("message", (event) => {
    if (typeof event.data !== "string") return;
    try {
      callbacks.onJob(InboxJobEventSchema.parse(JSON.parse(event.data)).job);
    } catch {
      callbacks.onError("Received malformed agent job data");
    }
  });
  socket.addEventListener("error", () => {
    callbacks.onError("Agent job stream connection failed");
  });
  socket.addEventListener("close", () => {
    if (!closedByClient) callbacks.onClose?.();
  });
  return () => {
    closedByClient = true;
    socket.close();
  };
}

/** Base branches for an arbitrary project, by URL prefix.
 *
 *  The conversion dialog is global but each row targets one project, so it
 *  cannot use the page's own per-project client. This builds one per prefix
 *  rather than adding a hub route that would duplicate an existing one. */
export async function fetchBaseBranchesFor(prefix: string): Promise<string[]> {
  const client = createApi(`/${prefix}`);
  const { branches } = await client.fetchBaseBranches();
  return branches.map((b) => b.name);
}
