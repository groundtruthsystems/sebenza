import { z } from "zod";

const BooleanLikeSchema = z.union([
  z.boolean(),
  z.literal("true").transform(() => true),
  z.literal("false").transform(() => false),
]);

export const ErrorResponseSchema = z.object({
  error: z.string(),
});

export const OkResponseSchema = z.object({
  ok: z.literal(true),
});

export const EnabledResponseSchema = z.object({
  ok: z.literal(true),
  enabled: z.boolean(),
});

export const BuiltInAgentIdSchema = z.enum([
  "claude",
  "grok",
  "codex",
  "opencode",
]);
export const AgentIdSchema = z.string().trim().min(1);
export const AgentKindSchema = BuiltInAgentIdSchema;
export const WorktreeCreateModeSchema = z.enum(["new", "existing"]);

/** Oneshot watch config carried on create/open requests. When present, the server-side
 *  oneshot watcher will auto-close the session once the agent finishes. Any
 *  browser-originated interaction with the session disarms the watcher. */
export const OneshotConfigSchema = z.object({
  autoCloseOnDone: z.boolean().optional(),
});

export const AgentCapabilitiesSchema = z.object({
  terminal: z.literal(true),
  inAppChat: z.boolean(),
  conversationHistory: z.boolean(),
  interrupt: z.boolean(),
  resume: z.boolean(),
  /** Can branch a new session off an existing one, keeping its history. */
  fork: z.boolean(),
  /** Sebenza can choose the session id at launch, so it never has to poll for it. */
  pinnableSessionId: z.boolean(),
  /** The agent's hooks can gate (deny/allow) a tool call, not merely observe it.
   *  False for every current built-in. */
  permissionInterception: z.boolean(),
});

export const AgentSummarySchema = z.object({
  id: AgentIdSchema,
  label: z.string(),
  kind: z.enum(["builtin", "custom"]),
  capabilities: AgentCapabilitiesSchema,
});

export const AgentDetailsSchema = z.object({
  id: AgentIdSchema,
  label: z.string(),
  kind: z.enum(["builtin", "custom"]),
  capabilities: AgentCapabilitiesSchema,
  startCommand: z.string().nullable(),
  resumeCommand: z.string().nullable(),
});

export const AgentListResponseSchema = z.object({
  agents: z.array(AgentDetailsSchema),
});

export const UpsertCustomAgentRequestSchema = z.object({
  label: z.string().trim().min(1),
  startCommand: z.string().trim().min(1),
  resumeCommand: z.string().trim().optional(),
});

export const AgentResponseSchema = z.object({
  agent: AgentDetailsSchema,
});

export const ValidateCustomAgentResponseSchema = z.object({
  normalizedId: AgentIdSchema,
  warnings: z.array(z.string()),
});
export const WorktreeCreationPhaseSchema = z.enum([
  "creating_worktree",
  "preparing_runtime",
  "running_post_create_hook",
  "starting_session",
  "reconciling",
]);

export const AvailableBranchSchema = z.object({
  name: z.string(),
});

export const AvailableBranchesQuerySchema = z.object({
  includeRemote: BooleanLikeSchema.optional(),
});

const NumberLikePathParamSchema = z.union([
  z.number().int().nonnegative(),
  z
    .string()
    .regex(/^\d+$/)
    .transform((value) => Number(value)),
]);

export const BranchListResponseSchema = z.object({
  branches: z.array(AvailableBranchSchema),
});

export const WorktreeSourceSchema = z.enum(["ui", "oneshot"]);

export const CreateWorktreeRequestSchema = z.object({
  mode: WorktreeCreateModeSchema.optional(),
  branch: z.string().optional(),
  baseBranch: z.string().optional(),
  profile: z.string().optional(),
  agent: AgentIdSchema.optional(),
  agents: z.array(AgentIdSchema).min(1).optional(),
  prompt: z.string().optional(),
  envOverrides: z.record(z.string()).optional(),
  source: WorktreeSourceSchema.optional(),
  oneshot: OneshotConfigSchema.optional(),
});

export const OpenWorktreeRequestSchema = z.object({
  prompt: z.string().optional(),
  oneshot: OneshotConfigSchema.optional(),
});

/** Which configured launcher (editor/tool) to run against a worktree. */
export const LaunchWorktreeRequestSchema = z.object({
  launcherId: z.string(),
});

export const CreateWorktreeResponseSchema = z.object({
  primaryBranch: z.string(),
  branches: z.array(z.string()),
});

export const SetWorktreeArchivedRequestSchema = z.object({
  archived: z.boolean(),
});

export const SetWorktreeArchivedResponseSchema = z.object({
  ok: z.literal(true),
  archived: z.boolean(),
});

export const SetWorktreeLabelRequestSchema = z.object({
  label: z.string().trim().max(80).nullable(),
});

export const SetWorktreeLabelResponseSchema = z.object({
  ok: z.literal(true),
  label: z.string().nullable(),
});

export const ToggleEnabledRequestSchema = z.object({
  enabled: z.boolean(),
});

export const SendWorktreePromptRequestSchema = z.object({
  text: z.string().min(1),
  preamble: z.string().optional(),
});

export const AgentsSendMessageRequestSchema = z.object({
  text: z.string().trim().min(1),
});

export const PullMainRequestSchema = z.object({
  force: z.boolean().optional(),
  repo: z.string().optional(),
});

export const PullMainStatusSchema = z.enum([
  "updated",
  "already_up_to_date",
  "fetch_failed",
  "merge_failed",
  /** The checkout is on some other branch — pulling would move the wrong one. */
  "skipped_wrong_branch",
]);

export const PullMainResponseSchema = z.object({
  status: PullMainStatusSchema,
  from: z.string().optional(),
  to: z.string().optional(),
  error: z.string().optional(),
});

export const ServiceStatusSchema = z.object({
  name: z.string(),
  port: z.number().nullable(),
  running: z.boolean(),
  url: z.string().nullable().optional(),
});

export const PrCommentSchema = z.object({
  type: z.enum(["comment", "inline"]),
  author: z.string(),
  body: z.string(),
  createdAt: z.string(),
  path: z.string().optional(),
  line: z.number().nullable().optional(),
  diffHunk: z.string().optional(),
  isReply: z.boolean().optional(),
});

export const CiCheckSchema = z.object({
  name: z.string(),
  status: z.enum(["pending", "success", "failed", "skipped"]),
  url: z.string().nullable(),
  runId: z.number().nullable(),
});

export const PrEntrySchema = z.object({
  repo: z.string(),
  number: z.number(),
  state: z.enum(["open", "closed", "merged"]),
  url: z.string(),
  updatedAt: z.string(),
  ciStatus: z.enum(["none", "pending", "success", "failed"]),
  ciChecks: z.array(CiCheckSchema),
  comments: z.array(PrCommentSchema),
});

export const AutoNameProviderSchema = z.enum([
  "claude",
  "grok",
  "codex",
  "opencode",
]);

export const AutoNameConfigResponseSchema = z.object({
  autoName: z
    .object({
      provider: AutoNameProviderSchema,
      model: z.string().optional(),
      systemPrompt: z.string().optional(),
    })
    .nullable(),
});

export const WorktreeCreationStateSchema = z.object({
  phase: WorktreeCreationPhaseSchema,
});

export const AppNotificationSchema = z.object({
  id: z.number(),
  branch: z.string(),
  type: z.enum([
    "agent_stopped",
    "pr_opened",
    "runtime_error",
    "worktree_auto_removed",
  ]),
  message: z.string(),
  url: z.string().optional(),
  timestamp: z.number(),
});

export const WorktreeTabSchema = z.object({
  tabId: z.string(),
  /** `agent` is a fresh session of any configured provider, unlike `fork` which
   *  continues the root conversation with the worktree's own agent. */
  kind: z.enum(["root", "fork", "shell", "agent"]),
  label: z.string(),
  seq: z.number().nullable(),
  sessionId: z.string().nullable(),
  paneId: z.string().optional(),
  /** Agent owning this tab's pane; absent on shell tabs and on tabs written
   *  before per-tab agents existed. */
  agent: AgentIdSchema.optional(),
  createdAt: z.string(),
});

export const CreateAgentTabRequestSchema = z.object({
  agent: AgentIdSchema,
});

/** Whether a row is the repository's own checkout or a linked worktree. */
export const WorktreeKindSchema = z.enum(["main", "linked"]);

/** Whether a worktree is waiting on an explicit human response.
 *
 *  `user_question` is reserved: no agent adapter can observe a free-text question
 *  today, so the server never sends it. Clients must still handle it rather than
 *  assume only the first two ever arrive. */
export const WorktreeFeedbackStateSchema = z.enum([
  "none",
  "permission_request",
  "user_question",
]);

export const ProjectWorktreeSnapshotSchema = z.object({
  branch: z.string(),
  /** Defaults to "linked" so an older server (which sends no kind) still parses. */
  kind: WorktreeKindSchema.default("linked"),
  label: z.string().nullable(),
  baseBranch: z.string().optional(),
  path: z.string(),
  dir: z.string(),
  archived: z.boolean(),
  profile: z.string().nullable(),
  agentName: AgentIdSchema.nullable(),
  agentLabel: z.string().nullable(),
  agentTerminalStale: z.boolean(),
  mux: z.boolean(),
  dirty: z.boolean(),
  unpushed: z.boolean(),
  paneCount: z.number(),
  status: z.string(),
  /** Whether a human is being waited on. Separate from `status` because an agent
   *  asking a free-text question is still `running`, so the lifecycle cannot express
   *  it. A typed enum rather than `z.string()` (as `status` is) so an unknown state
   *  fails here instead of reaching the UI unhandled. Default keeps older servers,
   *  which send no such field, valid. */
  feedbackState: WorktreeFeedbackStateSchema.default("none"),
  elapsed: z.string(),
  services: z.array(ServiceStatusSchema),
  prs: z.array(PrEntrySchema),
  creation: WorktreeCreationStateSchema.nullable(),
  source: WorktreeSourceSchema,
  /** Present when the server-side oneshot watcher is armed for this worktree.
   *  Cleared by `disarmOneshot` on the first browser-originated interaction.
   *  CLI clients read this to detect "user took over" mid-run. */
  oneshot: OneshotConfigSchema.nullable(),
  /** Agent-pane tabs (`tabs[0]` is the root). Default keeps older servers valid. */
  tabs: z.array(WorktreeTabSchema).default([]),
  activeTabId: z.string().nullable().default(null),
});

export const ProjectSnapshotSchema = z.object({
  project: z.object({
    name: z.string(),
    mainBranch: z.string(),
  }),
  worktrees: z.array(ProjectWorktreeSnapshotSchema),
  notifications: z.array(AppNotificationSchema),
});

// Mirrors the Rust `WorktreeConversationProvider`. `codexAppServer`/`claudeCode` name a
// transport, from when more than one existed per agent; `grok` and `opencode` need no such
// distinction, so they use the bare agent id. opencode was missing here even though the
// server has emitted it since that agent shipped.
export const WorktreeConversationProviderSchema = z.enum([
  "codexAppServer",
  "claudeCode",
  "grok",
  "opencode",
]);

export const CodexWorktreeConversationRefSchema = z.object({
  provider: z.literal("codexAppServer"),
  conversationId: z.string(),
  cwd: z.string(),
  lastSeenAt: z.string(),
  threadId: z.string(),
});

export const ClaudeWorktreeConversationRefSchema = z.object({
  provider: z.literal("claudeCode"),
  conversationId: z.string(),
  cwd: z.string(),
  lastSeenAt: z.string(),
  sessionId: z.string(),
});

export const GrokWorktreeConversationRefSchema = z.object({
  provider: z.literal("grok"),
  conversationId: z.string(),
  cwd: z.string(),
  lastSeenAt: z.string(),
  sessionId: z.string(),
});

export const OpencodeWorktreeConversationRefSchema = z.object({
  provider: z.literal("opencode"),
  conversationId: z.string(),
  cwd: z.string(),
  lastSeenAt: z.string(),
  sessionId: z.string(),
});

export const WorktreeConversationRefSchema = z.discriminatedUnion("provider", [
  CodexWorktreeConversationRefSchema,
  ClaudeWorktreeConversationRefSchema,
  GrokWorktreeConversationRefSchema,
  OpencodeWorktreeConversationRefSchema,
]);

export const AgentsUiWorktreeSummarySchema = z.object({
  branch: z.string(),
  baseBranch: z.string().optional(),
  path: z.string(),
  archived: z.boolean(),
  profile: z.string().nullable(),
  agentName: AgentIdSchema.nullable(),
  agentLabel: z.string().nullable(),
  agentTerminalStale: z.boolean(),
  mux: z.boolean(),
  status: z.string(),
  dirty: z.boolean(),
  unpushed: z.boolean(),
  services: z.array(ServiceStatusSchema),
  prs: z.array(PrEntrySchema),
  creating: z.boolean(),
  creationPhase: WorktreeCreationPhaseSchema.nullable(),
  conversation: WorktreeConversationRefSchema.nullable(),
});

export const AgentsUiConversationMessageRoleSchema = z.enum([
  "user",
  "assistant",
]);
export const AgentsUiConversationMessageStatusSchema = z.enum([
  "completed",
  "inProgress",
  "failed",
]);
export const AgentsUiConversationMessageKindSchema = z.enum([
  "text",
  "thinking",
  "toolUse",
  "toolResult",
]);

export const AgentsUiConversationMessageSchema = z.object({
  id: z.string(),
  turnId: z.string(),
  order: z.number().int().nonnegative(),
  role: AgentsUiConversationMessageRoleSchema,
  text: z.string(),
  status: AgentsUiConversationMessageStatusSchema,
  createdAt: z.string().nullable(),
  kind: AgentsUiConversationMessageKindSchema,
  phase: z.string().optional(),
  toolName: z.string().optional(),
  toolCallId: z.string().optional(),
  command: z.string().optional(),
  cwd: z.string().optional(),
  exitCode: z.number().nullable().optional(),
  durationMs: z.number().nullable().optional(),
});

export const AgentsUiConversationStateSchema = z.object({
  provider: WorktreeConversationProviderSchema,
  conversationId: z.string(),
  cwd: z.string(),
  running: z.boolean(),
  activeTurnId: z.string().nullable(),
  messages: z.array(AgentsUiConversationMessageSchema),
});

export const AgentsUiWorktreeConversationResponseSchema = z.object({
  worktree: AgentsUiWorktreeSummarySchema,
  conversation: AgentsUiConversationStateSchema,
});

export const AgentsUiSendMessageResponseSchema = z.object({
  conversationId: z.string(),
  turnId: z.string(),
  running: z.literal(true),
  streaming: z.boolean(),
});

export const AgentsUiInterruptResponseSchema = z.object({
  conversationId: z.string(),
  turnId: z.string(),
  interrupted: z.literal(true),
  streaming: z.boolean(),
});

export const AgentsUiConversationMessageDeltaEventSchema = z.object({
  type: z.literal("messageDelta"),
  revision: z.number().int().nonnegative(),
  conversationId: z.string(),
  turnId: z.string(),
  itemId: z.string(),
  order: z.number().int().nonnegative(),
  delta: z.string(),
});

export const AgentsUiConversationMessageUpsertEventSchema = z.object({
  type: z.literal("messageUpsert"),
  revision: z.number().int().nonnegative(),
  conversationId: z.string(),
  message: AgentsUiConversationMessageSchema,
});

export const AgentsUiConversationStatusEventSchema = z.object({
  type: z.literal("conversationStatus"),
  revision: z.number().int().nonnegative(),
  conversationId: z.string(),
  running: z.boolean(),
  activeTurnId: z.string().nullable(),
});

export const AgentsUiConversationErrorEventSchema = z.object({
  type: z.literal("error"),
  message: z.string(),
});

export const AgentsUiConversationEventSchema = z.discriminatedUnion("type", [
  AgentsUiConversationMessageDeltaEventSchema,
  AgentsUiConversationMessageUpsertEventSchema,
  AgentsUiConversationStatusEventSchema,
  AgentsUiConversationErrorEventSchema,
]);

export const WorktreeListResponseSchema = z.object({
  worktrees: z.array(ProjectWorktreeSnapshotSchema),
});

export const UnpushedCommitSchema = z.object({
  hash: z.string(),
  message: z.string(),
});

export const WorktreeDiffResponseSchema = z.object({
  uncommitted: z.string(),
  uncommittedTruncated: z.boolean(),
  gitStatus: z.string(),
  unpushedCommits: z.array(UnpushedCommitSchema),
});

// ── Sebenza tracks (kanban) ──────────────────────────────────────────────────
// These model external `sebenza-tracks-v1` files written by the sebenza plugin
// into `.ai/sebenza/` (snake_case, may evolve), so the schemas are lenient:
// `.passthrough()` keeps unknown keys and unexpected statuses fall back rather
// than failing response validation.
export const TrackStatusSchema = z
  .enum(["backlog", "doing", "blocked", "unblocked", "done"])
  .catch("backlog");

export const PhaseSummarySchema = z
  .object({
    id: z.string(),
    name: z.string(),
    status: TrackStatusSchema,
    blocked_reason: z.string().nullable().optional(),
  })
  .passthrough();

export const TrackProgressSchema = z
  .object({
    total_tasks: z.number(),
    completed_tasks: z.number(),
    percentage: z.number(),
  })
  .passthrough();

export const TrackSchema = z
  .object({
    track_id: z.string(),
    type: z.string().optional(),
    description: z.string(),
    status: TrackStatusSchema,
    blocked_reason: z.string().nullable().optional(),
    created_at: z.string().optional(),
    updated_at: z.string().optional(),
    design_path: z.string().optional(),
    spec_path: z.string().optional(),
    /// `test-plan.md`, from tracks.json `test_plan_path`. The server also fills
    /// this in when the file sits beside spec.md / design.md and the field was omitted.
    test_plan_path: z.string().optional(),
    plan_path: z.string().optional(),
    phases_summary: z.array(PhaseSummarySchema).default([]),
    progress: TrackProgressSchema,
  })
  .passthrough();

export const TracksSchema = z
  .object({
    project: z.object({ name: z.string() }).passthrough().optional(),
    tracks: z.array(TrackSchema).default([]),
  })
  .passthrough();

export const TrackFileQuerySchema = z.object({ path: z.string() });
export const TrackFileResponseSchema = z.object({
  path: z.string(),
  content: z.string(),
});

// ── Sebenza registry portfolio (`~/.ai/sebenza/registry.json`) ───────────────
// Per the plugin's daemon spec a project whose path or tracks.json has gone
// missing is reported, not dropped — so `tracks` is nullable and `status`
// carries the reason.
export const RegistryProjectStatusSchema = z
  .enum(["ok", "missing_path", "missing_tracks", "invalid_tracks"])
  .catch("ok");

export const RegistryProjectSchema = z
  .object({
    name: z.string(),
    path: z.string(),
    tracks_file: z.string(),
    registered_at: z.string().optional(),
    last_synced: z.string().optional(),
    status: RegistryProjectStatusSchema,
    tracks: TracksSchema.nullable().default(null),
    error: z.string().nullable().optional(),
  })
  .passthrough();

export const PortfolioSchema = z.object({
  registry_path: z.string(),
  exists: z.boolean(),
  error: z.string().nullable().optional(),
  projects: z.array(RegistryProjectSchema).default([]),
});

export const RegistryFileQuerySchema = z.object({
  project: z.string(),
  path: z.string(),
});

export const ServiceConfigSchema = z.object({
  name: z.string(),
  portEnv: z.string(),
});

export const ProfileConfigSchema = z.object({
  name: z.string(),
  systemPrompt: z.string().optional(),
});

export const LinkedRepoInfoSchema = z.object({
  alias: z.string(),
  dir: z.string().optional(),
});

/** A configured external launcher (editor/tool) shown in the "Open in…" menu. */
export const LauncherViewSchema = z.object({
  id: z.string(),
  label: z.string(),
});

export const AppConfigSchema = z.object({
  name: z.string(),
  services: z.array(ServiceConfigSchema),
  profiles: z.array(ProfileConfigSchema),
  agents: z.array(AgentSummarySchema),
  launchers: z.array(LauncherViewSchema),
  defaultProfileName: z.string(),
  defaultAgentId: BuiltInAgentIdSchema,
  autoName: z.boolean(),
  startupEnvs: z.record(z.union([z.string(), z.boolean()])),
  linkedRepos: z.array(LinkedRepoInfoSchema),
  autoRemoveOnMerge: z.boolean(),
  projectDir: z.string(),
  mainBranch: z.string(),
});

export const CiLogsResponseSchema = z.object({
  logs: z.string(),
});

export const WorktreeNameParamsSchema = z.object({
  name: z.string(),
});

export const WorktreeTabParamsSchema = z.object({
  name: z.string(),
  tabId: z.string(),
});

export const CreateTabResponseSchema = z.object({
  tab: WorktreeTabSchema,
});

export const NotificationIdParamsSchema = z.object({
  id: NumberLikePathParamSchema,
});

export const AgentIdParamsSchema = z.object({
  id: AgentIdSchema,
});

export const RunIdParamsSchema = z.object({
  runId: NumberLikePathParamSchema,
});

/** Another Sebenza server running on this machine (migration sensor). Surfaced
 *  by `/api/instances` so the dashboard can prompt the user to consolidate
 *  leftover single-project instances with `sebenza-cli project migrate`. */
export const InstanceSummarySchema = z.object({
  port: z.number(),
  projectDir: z.string(),
});

export const InstancesResponseSchema = z.object({
  instances: z.array(InstanceSummarySchema),
});

export type InstanceSummary = z.infer<typeof InstanceSummarySchema>;
export type InstancesResponse = z.infer<typeof InstancesResponseSchema>;

export const ProjectSummarySchema = z.object({
  prefix: z.string(),
  name: z.string(),
  path: z.string(),
  /** True while at least one client has a terminal/agent WebSocket open on this
   *  project (i.e. it is currently being viewed). */
  active: z.boolean(),
});

/** One project's slice of the cross-project ticker feed.
 *
 *  `worktrees` reuses the per-project snapshot shape so the same `deriveTickerItems`
 *  runs over it — eligibility is defined once, on the client, and cannot drift between
 *  the single-project and cross-project paths. */
export const ActiveWorktreeProjectSchema = z.object({
  prefix: z.string(),
  name: z.string(),
  worktrees: z.array(ProjectWorktreeSnapshotSchema).default([]),
});

export const ActiveWorktreesResponseSchema = z.object({
  projects: z.array(ActiveWorktreeProjectSchema).default([]),
});

export const ProjectsResponseSchema = z.object({
  projects: z.array(ProjectSummarySchema),
});

export const AddProjectRequestSchema = z.object({
  path: z.string().min(1),
});

/** Adding a repo that has no .ai/sebenza.yaml kicks off an async setup job
 *  (scaffold config → analyze with Claude → register); the response says the
 *  job started and the client polls `projectInits`. When the repo already has
 *  config it's registered immediately and `project` is returned. */
export const AddProjectResponseSchema = z.object({
  initializing: z.boolean(),
  path: z.string(),
  project: ProjectSummarySchema.nullable(),
});

/** Phases of the on-add project setup, surfaced so the UI and CLI can show
 *  progress: scaffold the .ai/sebenza.yaml → analyze the repo with Claude → ready
 *  (registered). `failed` means setup errored before the project was usable. */
export const ProjectInitPhaseSchema = z.enum([
  "creating_config",
  "analyzing",
  "ready",
  "failed",
]);

export const ProjectInitStateSchema = z.object({
  path: z.string(),
  phase: ProjectInitPhaseSchema,
  /** Set once the project is registered (phase "ready") so the client can open it. */
  prefix: z.string().nullable(),
  name: z.string().nullable(),
  /** Set when phase is "failed". */
  error: z.string().nullable(),
});

export const ProjectInitsResponseSchema = z.object({
  inits: z.array(ProjectInitStateSchema),
});

export const ProjectPrefixParamsSchema = z.object({
  prefix: z.string(),
});

/** Fold the repos served by leftover single-project instances into this server.
 *  The CLI sends each other instance's projectDir; the server adds + persists
 *  them so this one dashboard serves them going forward. */
export const MigrateProjectsRequestSchema = z.object({
  paths: z.array(z.string().min(1)),
});

export const MigrateProjectsResponseSchema = z.object({
  migrated: z.array(ProjectSummarySchema),
  failed: z.array(z.object({ path: z.string(), error: z.string() })),
});

export type ProjectSummary = z.infer<typeof ProjectSummarySchema>;
export type ProjectsResponse = z.infer<typeof ProjectsResponseSchema>;
export type AddProjectRequest = z.infer<typeof AddProjectRequestSchema>;
export type AddProjectResponse = z.infer<typeof AddProjectResponseSchema>;
export type ProjectInitPhase = z.infer<typeof ProjectInitPhaseSchema>;
export type ProjectInitState = z.infer<typeof ProjectInitStateSchema>;
export type ProjectInitsResponse = z.infer<typeof ProjectInitsResponseSchema>;
export type MigrateProjectsRequest = z.infer<
  typeof MigrateProjectsRequestSchema
>;
export type MigrateProjectsResponse = z.infer<
  typeof MigrateProjectsResponseSchema
>;

export type BuiltInAgentId = z.infer<typeof BuiltInAgentIdSchema>;
export type AgentId = z.infer<typeof AgentIdSchema>;
export type AgentKind = z.infer<typeof AgentKindSchema>;
export type AgentCapabilities = z.infer<typeof AgentCapabilitiesSchema>;
export type AgentSummary = z.infer<typeof AgentSummarySchema>;
export type AgentDetails = z.infer<typeof AgentDetailsSchema>;
export type AgentListResponse = z.infer<typeof AgentListResponseSchema>;
export type UpsertCustomAgentRequest = z.infer<
  typeof UpsertCustomAgentRequestSchema
>;
export type AgentResponse = z.infer<typeof AgentResponseSchema>;
export type ValidateCustomAgentResponse = z.infer<
  typeof ValidateCustomAgentResponseSchema
>;
export type WorktreeCreateMode = z.infer<typeof WorktreeCreateModeSchema>;
export type OneshotConfig = z.infer<typeof OneshotConfigSchema>;
export type WorktreeCreationPhase = z.infer<typeof WorktreeCreationPhaseSchema>;
export type AvailableBranch = z.infer<typeof AvailableBranchSchema>;
// Keep this manual so frontend callers pass booleans instead of raw `"true"`/`"false"` query literals.
export type AvailableBranchesQuery = { includeRemote?: boolean };
export type BranchListResponse = z.infer<typeof BranchListResponseSchema>;
export type CreateWorktreeRequest = z.infer<typeof CreateWorktreeRequestSchema>;
export type OpenWorktreeRequest = z.infer<typeof OpenWorktreeRequestSchema>;
export type LaunchWorktreeRequest = z.infer<typeof LaunchWorktreeRequestSchema>;
export type WorktreeSource = z.infer<typeof WorktreeSourceSchema>;
export type WorktreeKind = z.infer<typeof WorktreeKindSchema>;
export type WorktreeFeedbackState = z.infer<typeof WorktreeFeedbackStateSchema>;
export type ActiveWorktreeProject = z.infer<typeof ActiveWorktreeProjectSchema>;
export type ActiveWorktreesResponse = z.infer<
  typeof ActiveWorktreesResponseSchema
>;
export type CreateWorktreeResponse = z.infer<
  typeof CreateWorktreeResponseSchema
>;
export type SetWorktreeArchivedRequest = z.infer<
  typeof SetWorktreeArchivedRequestSchema
>;
export type SetWorktreeArchivedResponse = z.infer<
  typeof SetWorktreeArchivedResponseSchema
>;
export type SetWorktreeLabelRequest = z.infer<
  typeof SetWorktreeLabelRequestSchema
>;
export type SetWorktreeLabelResponse = z.infer<
  typeof SetWorktreeLabelResponseSchema
>;
export type ToggleEnabledRequest = z.infer<typeof ToggleEnabledRequestSchema>;
export type SendWorktreePromptRequest = z.infer<
  typeof SendWorktreePromptRequestSchema
>;
export type AgentsSendMessageRequest = z.infer<
  typeof AgentsSendMessageRequestSchema
>;
export type PullMainRequest = z.infer<typeof PullMainRequestSchema>;
export type PullMainResult = z.infer<typeof PullMainResponseSchema>;
export type ServiceStatus = z.infer<typeof ServiceStatusSchema>;
export type PrComment = z.infer<typeof PrCommentSchema>;
export type CiCheck = z.infer<typeof CiCheckSchema>;
export type PrEntry = z.infer<typeof PrEntrySchema>;
export type AutoNameConfigResponse = z.infer<
  typeof AutoNameConfigResponseSchema
>;
export type WorktreeCreationState = z.infer<typeof WorktreeCreationStateSchema>;
export type AppNotification = z.infer<typeof AppNotificationSchema>;
export type ProjectWorktreeSnapshot = z.infer<
  typeof ProjectWorktreeSnapshotSchema
>;
export type WorktreeTab = z.infer<typeof WorktreeTabSchema>;
export type WorktreeTabParams = z.infer<typeof WorktreeTabParamsSchema>;
export type CreateTabResponse = z.infer<typeof CreateTabResponseSchema>;
export type ProjectSnapshot = z.infer<typeof ProjectSnapshotSchema>;
export type WorktreeConversationProvider = z.infer<
  typeof WorktreeConversationProviderSchema
>;
export type CodexWorktreeConversationRef = z.infer<
  typeof CodexWorktreeConversationRefSchema
>;
export type ClaudeWorktreeConversationRef = z.infer<
  typeof ClaudeWorktreeConversationRefSchema
>;
export type WorktreeConversationRef = z.infer<
  typeof WorktreeConversationRefSchema
>;
export type AgentsUiWorktreeSummary = z.infer<
  typeof AgentsUiWorktreeSummarySchema
>;
export type AgentsUiConversationMessageRole = z.infer<
  typeof AgentsUiConversationMessageRoleSchema
>;
export type AgentsUiConversationMessageStatus = z.infer<
  typeof AgentsUiConversationMessageStatusSchema
>;
export type AgentsUiConversationMessageKind = z.infer<
  typeof AgentsUiConversationMessageKindSchema
>;
export type AgentsUiConversationMessage = z.infer<
  typeof AgentsUiConversationMessageSchema
>;
export type AgentsUiConversationState = z.infer<
  typeof AgentsUiConversationStateSchema
>;
export type AgentsUiWorktreeConversationResponse = z.infer<
  typeof AgentsUiWorktreeConversationResponseSchema
>;
export type AgentsUiSendMessageResponse = z.infer<
  typeof AgentsUiSendMessageResponseSchema
>;
export type AgentsUiInterruptResponse = z.infer<
  typeof AgentsUiInterruptResponseSchema
>;
export type AgentsUiConversationMessageDeltaEvent = z.infer<
  typeof AgentsUiConversationMessageDeltaEventSchema
>;
export type AgentsUiConversationMessageUpsertEvent = z.infer<
  typeof AgentsUiConversationMessageUpsertEventSchema
>;
export type AgentsUiConversationStatusEvent = z.infer<
  typeof AgentsUiConversationStatusEventSchema
>;
export type AgentsUiConversationErrorEvent = z.infer<
  typeof AgentsUiConversationErrorEventSchema
>;
export type AgentsUiConversationEvent = z.infer<
  typeof AgentsUiConversationEventSchema
>;
export type WorktreeListResponse = z.infer<typeof WorktreeListResponseSchema>;
export type UnpushedCommit = z.infer<typeof UnpushedCommitSchema>;
export type WorktreeDiffResponse = z.infer<typeof WorktreeDiffResponseSchema>;
export type TrackStatus = z.infer<typeof TrackStatusSchema>;
export type PhaseSummary = z.infer<typeof PhaseSummarySchema>;
export type Track = z.infer<typeof TrackSchema>;
export type Tracks = z.infer<typeof TracksSchema>;
export type TrackFileResponse = z.infer<typeof TrackFileResponseSchema>;
export type RegistryProjectStatus = z.infer<typeof RegistryProjectStatusSchema>;
export type RegistryProject = z.infer<typeof RegistryProjectSchema>;
export type Portfolio = z.infer<typeof PortfolioSchema>;
export type ServiceConfig = z.infer<typeof ServiceConfigSchema>;
export type ProfileConfig = z.infer<typeof ProfileConfigSchema>;
export type LinkedRepoInfo = z.infer<typeof LinkedRepoInfoSchema>;
export type LauncherView = z.infer<typeof LauncherViewSchema>;
export type AppConfig = z.infer<typeof AppConfigSchema>;
export type CiLogsResponse = z.infer<typeof CiLogsResponseSchema>;
export type ErrorResponse = z.infer<typeof ErrorResponseSchema>;
export type OkResponse = z.infer<typeof OkResponseSchema>;
export type EnabledResponse = z.infer<typeof EnabledResponseSchema>;

/** A draft's project link. `resolved` is false when the stored path no longer
 *  matches a registered project — the draft still works, the link just cannot
 *  be followed. */
export const InboxProjectLinkSchema = z.object({
  path: z.string(),
  name: z.string().nullable(),
  resolved: z.boolean(),
});

export const DraftStatusSchema = z.enum(["Draft", "Promoted", "Dropped"]);

/** `P0` is most urgent; the inbox lists by priority, then newest. */
export const InboxPrioritySchema = z.enum(["P0", "P1", "P2", "P3"]);
/** `operator` while an override stands; the system agent may not move it. */
export const InboxPrioritySourceSchema = z.enum(["agent", "operator"]);

export const InboxDraftSummarySchema = z.object({
  id: z.string(),
  title: z.string(),
  status: DraftStatusSchema,
  /** Empty for an unparseable draft. The list orders by priority, then this. */
  createdAt: z.string().default(""),
  updatedAt: z.string(),
  project: InboxProjectLinkSchema.nullable(),
  priority: InboxPrioritySchema.default("P2"),
  prioritySource: InboxPrioritySourceSchema.default("agent"),
  /** A request needs the operator: triage or delivery failed. */
  flagged: z.boolean().default(false),
  isRaw: z.boolean(),
});

export const InboxDraftListSchema = z.object({
  drafts: z.array(InboxDraftSummarySchema),
});

export const ConversionOutcomeSchema = z.object({
  projectPath: z.string(),
  branch: z.string(),
  baseBranch: z.string().optional(),
  agentId: z.string().nullish(),
  prompt: z.string(),
  /** The system instruction the launch carried, when there was one. */
  systemInstruction: z.string().optional(),
  /** Launched architect-first; false for a direct or operator-only launch. */
  architectFirst: z.boolean().default(false),
  outcome: z.string(),
  worktreePath: z.string().optional(),
  error: z.string().optional(),
  at: z.string(),
});

/** `bodyHash` is what a save must echo back; it is the only thing that gates a
 *  write, so a conversion job touching frontmatter cannot provoke a conflict. */
export const InboxDraftSchema = z.object({
  id: z.string(),
  title: z.string(),
  status: DraftStatusSchema,
  createdAt: z.string(),
  updatedAt: z.string(),
  body: z.string(),
  bodyHash: z.string(),
  project: InboxProjectLinkSchema.nullable(),
  priority: InboxPrioritySchema.default("P2"),
  prioritySource: InboxPrioritySourceSchema.default("agent"),
  conversions: z.array(ConversionOutcomeSchema).default([]),
  raw: z.object({ text: z.string(), error: z.string() }).nullable(),
});

export const CreateInboxDraftRequestSchema = z.object({ title: z.string() });

export const SaveInboxDraftBodyRequestSchema = z.object({
  expectedHash: z.string(),
  body: z.string(),
});

export const PatchInboxDraftRequestSchema = z.object({
  title: z.string().optional(),
  projectPath: z.string().nullable().optional(),
  status: DraftStatusSchema.optional(),
});

/** `null` clears the override and hands priority back to the agent. The key
 *  is required: an empty body is refused rather than read as a clear. */
export const SetInboxPriorityRequestSchema = z.object({
  priority: InboxPrioritySchema.nullable(),
});

export const InboxWorktreeKeySchema = z.object({
  project: z.string(),
  branch: z.string(),
});

export const InboxAuthorKindSchema = z.enum([
  "operator",
  "worktree_agent",
  "system_agent",
]);

/** One thread row. Bodies are untrusted text: render as plain text or
 *  sanitised markdown only. `caller` is self-declared, never authenticated. */
export const InboxCommentSchema = z.object({
  eventId: z.string(),
  ts: z.string(),
  author: InboxAuthorKindSchema,
  caller: z.string().nullable(),
  kind: z.enum(["note", "request", "advice", "proposal", "resolution"]),
  body: z.string(),
  title: z.string().nullable(),
  requestId: z.string().nullable(),
  parentEventId: z.string().nullable(),
  warnings: z.array(z.string()).default([]),
  redacted: z.boolean(),
});

export const InboxCommentGroupsSchema = z.object({
  overall: z.array(InboxCommentSchema),
  worktrees: z.array(
    InboxWorktreeKeySchema.extend({ comments: z.array(InboxCommentSchema) }),
  ),
});

/** `worktree` absent posts to the overall thread. */
export const PostInboxCommentRequestSchema = z.object({
  body: z.string(),
  worktree: InboxWorktreeKeySchema.optional(),
});

export const PostInboxCommentResponseSchema = z.object({
  comment: InboxCommentSchema,
});

export const InboxRequestStatusSchema = z.enum([
  "open",
  "proposed",
  "confirmed",
  "resolved",
  "delivery_failed",
]);

export const InboxRequestSchema = z.object({
  requestId: z.string(),
  worktree: InboxWorktreeKeySchema,
  title: z.string(),
  body: z.string(),
  status: InboxRequestStatusSchema,
  flagged: z.boolean(),
  proposalId: z.string().nullable(),
  proposal: z.string().nullable(),
  /** `content_hash` of `proposal`: what confirming it must quote. */
  proposalHash: z.string().nullable().default(null),
  /** Secret/PHI scan hits on the proposal; shown before confirm. */
  proposalWarnings: z.array(z.string()).default([]),
  contentHash: z.string().nullable(),
  confirmedText: z.string().nullable(),
  lastReason: z.string().nullable(),
  lastError: z.string().nullable(),
  attempts: z.number(),
  warnings: z.array(z.string()).default([]),
  openedAt: z.string(),
});

export type InboxComment = z.infer<typeof InboxCommentSchema>;
export type InboxCommentGroups = z.infer<typeof InboxCommentGroupsSchema>;
export type InboxRequest = z.infer<typeof InboxRequestSchema>;

export const InboxRequestListSchema = z.object({
  requests: z.array(InboxRequestSchema),
});

/** `contentHash` is the hash of the text the operator was shown: the
 *  proposal, or `body` itself when there is no proposal (an authored
 *  resolution). `body` alongside a proposal is an edit. A stale hash is a
 *  409 and nothing is delivered. */
export const ConfirmInboxRequestSchema = z.object({
  contentHash: z.string(),
  body: z.string().optional(),
});

export const RejectInboxRequestSchema = z.object({
  reason: z.string(),
});

export const InboxRequestResponseSchema = z.object({
  request: InboxRequestSchema,
});

export const RetryInboxTriageResponseSchema = z.object({
  jobId: z.string(),
});

/** A system-agent job (triage, draft-help, convert). `output` is the
 *  validated result, tagged by `jobKind`; `error` never holds agent text. */
export const InboxAgentJobSchema = z.object({
  jobId: z.string(),
  draftId: z.string(),
  kind: z.enum(["triage", "draft_help", "convert"]),
  requestId: z.string().nullable(),
  attempt: z.number(),
  status: z.enum(["queued", "running", "succeeded", "failed"]),
  output: z.record(z.string(), z.unknown()).nullable(),
  error: z.string().nullable(),
  reseeded: z.boolean(),
  enqueuedAt: z.string(),
  startedAt: z.string().nullable(),
  finishedAt: z.string().nullable(),
});

export type InboxAgentJob = z.infer<typeof InboxAgentJobSchema>;

/** One frame of `/api/inbox/:id/agent/stream`. */
export const InboxJobEventSchema = z.object({
  type: z.literal("inbox.job"),
  job: InboxAgentJobSchema,
});

/** `POST /api/inbox/:id/agent/draft-help`. `instruction` optionally steers
 *  the proposal. */
export const InboxDraftHelpRequestSchema = z.object({
  instruction: z.string().optional(),
});

/** The queued job. Its result arrives on the job (GET or the `inbox.job`
 *  stream) as {@link InboxDraftHelpOutputSchema}; it is never written to the
 *  draft. Applying it is the hash-gated `PUT …/body`, a 409 when stale. */
export const InboxDraftHelpResponseSchema = z.object({
  jobId: z.string(),
});

/** A succeeded draft-help job's `output`. */
export const InboxDraftHelpOutputSchema = z.object({
  jobKind: z.literal("draft_help"),
  proposed_body: z.string(),
  summary: z.string(),
});

export type InboxDraftHelpOutput = z.infer<typeof InboxDraftHelpOutputSchema>;

export const RedactInboxCommentResponseSchema = z.object({
  eventId: z.string(),
  targetEventId: z.string(),
});

export const AdvisorySchema = z.object({
  kind: z.string(),
  message: z.string(),
});

/** `systemInstruction` is the reviewed (perhaps edited) instruction from
 *  `convert/instructions`; omit it to launch with `prompt` alone (UC-07a).
 *  `architectFirst` defaults to true and applies only when the project has a
 *  Sebenza workspace; false asks for a direct instruction (UC-07b). */
export const ConversionTargetSchema = z.object({
  projectPath: z.string(),
  branch: z.string(),
  baseBranch: z.string().nullish(),
  agentId: z.string().nullish(),
  prompt: z.string(),
  systemInstruction: z.string().nullish(),
  architectFirst: z.boolean().optional(),
});

/** `POST /api/inbox/:id/convert/instructions`. `project` is the registered
 *  project's path; `prompt` may still be empty. */
export const ConvertInstructionsRequestSchema = z.object({
  targets: z.array(
    z.object({
      project: z.string(),
      branch: z.string(),
      prompt: z.string().optional(),
    }),
  ),
});

export const ConvertInstructionTargetSchema = z.object({
  project: z.string(),
  branch: z.string(),
  /** How the agent named this target (`<project dir>/<branch>`); the key in
   *  a convert job's `output.targets[].project`. */
  key: z.string(),
  /** Null when no instruction is coming: convert with the prompt alone. */
  systemInstruction: z.string().nullable(),
  /** `.ai/sebenza/index.md` exists, so an architect-first launch can run. */
  sebenzaWorkspace: z.boolean(),
});

/** The route waits for the job (bounded by the agent timeout plus slack).
 *  `succeeded`: instructions filled in. `unavailable` / `failed`: `fallback`
 *  is true, every instruction is null; convert with the operator prompt.
 *  `pending`: the wait elapsed first; follow `jobId` on the agent job route
 *  or stream. Always a 200 apart from validation and guard errors. */
export const ConvertInstructionsResponseSchema = z.object({
  jobId: z.string().nullable(),
  status: z.enum(["succeeded", "failed", "unavailable", "pending"]),
  fallback: z.boolean(),
  error: z.string().nullable(),
  targets: z.array(ConvertInstructionTargetSchema),
  advisories: z.array(AdvisorySchema).default([]),
});

export type ConvertInstructionsResponse = z.infer<
  typeof ConvertInstructionsResponseSchema
>;

export const ConvertDraftRequestSchema = z.object({
  targets: z.array(ConversionTargetSchema),
});

/** Advisories are warnings, never refusals — the fan-out has already started
 *  by the time they are read. */
export const ConvertDraftResponseSchema = z.object({
  jobId: z.string(),
  advisories: z.array(AdvisorySchema).default([]),
});

export const ConversionJobSchema = z.object({
  id: z.string(),
  draftId: z.string(),
  total: z.number(),
  outcomes: z.array(ConversionOutcomeSchema),
  finished: z.boolean(),
  error: z.string().optional(),
});
