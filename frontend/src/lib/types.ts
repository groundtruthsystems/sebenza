import type {
  AgentId,
  BuiltInAgentId,
  OneshotConfig,
  PrEntry,
  ServiceStatus,
  TrackFileResponse,
  WorktreeCreationPhase,
  WorktreeSource,
  WorktreeTab,
} from "./api-contract";

export type {
  AgentsUiConversationEvent,
  AgentsUiConversationMessage,
  AgentsUiConversationMessageDeltaEvent,
  AgentsUiConversationMessageUpsertEvent,
  AgentsUiConversationStatusEvent,
  AgentsUiConversationState,
  AgentsUiInterruptResponse,
  AgentsUiSendMessageResponse,
  AgentsUiWorktreeConversationResponse,
  AgentCapabilities,
  AgentDetails,
  AgentId,
  AgentKind,
  BuiltInAgentId,
  AgentListResponse,
  AgentResponse,
  AgentSummary,
  ValidateCustomAgentResponse,
  AppConfig,
  AppNotification,
  AvailableBranch,
  AvailableBranchesQuery,
  BranchListResponse,
  CiCheck,
  CreateWorktreeRequest,
  CreateWorktreeResponse,
  LinkedRepoInfo,
  OneshotConfig,
  InstanceSummary,
  PrComment,
  PrEntry,
  ProfileConfig,
  ProjectInitPhase,
  ProjectInitState,
  ProjectSnapshot,
  ProjectSummary,
  ProjectWorktreeSnapshot,
  PullMainResult,
  ServiceConfig,
  UpsertCustomAgentRequest,
  ServiceStatus,
  SetWorktreeArchivedRequest,
  SetWorktreeArchivedResponse,
  SetWorktreeLabelRequest,
  SetWorktreeLabelResponse,
  UnpushedCommit,
  WorktreeCreationPhase,
  WorktreeCreationState,
  WorktreeCreateMode,
  WorktreeDiffResponse,
  WorktreeListResponse,
  WorktreeSource,
  WorktreeTab,
  TrackStatus,
  PhaseSummary,
  Track,
  Tracks,
  TrackFileResponse,
  RegistryProjectStatus,
  RegistryProject,
  Portfolio,
} from "./api-contract";
export type { AgentsSendMessageRequest as AgentsUiSendMessageRequest } from "./api-contract";

// Parsed `plan.json` (via a track's plan_path) — fetched as raw text through the
// track-file endpoint and JSON.parsed client-side, so it's a frontend shape.
// Field names follow the plugin's `sebenza-plan-v1` schema.
export interface TrackPlanSubtask {
  id: string;
  name: string;
  status: string;
  blocked_reason?: string | null;
}
export interface TrackPlanTask {
  id: string;
  name: string;
  description?: string;
  status: string;
  blocked_reason?: string | null;
  /** Short SHA recorded by `sebenza-implement` when the task is completed. */
  commit_sha?: string;
  notes?: string;
  subtasks?: TrackPlanSubtask[];
}
export interface TrackPlanPhase {
  id: string;
  name: string;
  status: string;
  blocked_reason?: string | null;
  /** Short SHA of the phase checkpoint commit. */
  checkpoint_sha?: string;
  tasks?: TrackPlanTask[];
}
export interface TrackPlan {
  track_id?: string;
  phases: TrackPlanPhase[];
}

/** Reads a path relative to a Sebenza workspace root. Lets the track detail
 *  views work against either a worktree (`fetchTrackFile`) or a registered
 *  project (`fetchRegistryFile`). Callers must keep the reference stable. */
export type TrackFileFetcher = (path: string) => Promise<TrackFileResponse>;

export interface FileUploadResult {
  files: Array<{ path: string }>;
}

export interface AskUserQuestionOption {
  label: string;
  description?: string;
}

export interface AskUserQuestionItem {
  question: string;
  header: string;
  multiSelect?: boolean;
  options: AskUserQuestionOption[];
}

export interface AskUserQuestionInput {
  questions: AskUserQuestionItem[];
}

export interface DiffDialogProps {
  branch: string;
  onclose: () => void;
}

export interface WorktreeInfo {
  branch: string;
  label: string | null;
  baseBranch?: string;
  archived: boolean;
  agent: string;
  mux: string;
  path: string;
  dir: string | null;
  dirty: boolean;
  unpushed: boolean;
  status: string;
  elapsed: string;
  profile: string | null;
  agentName: AgentId | null;
  agentLabel: string | null;
  agentTerminalStale: boolean;
  services: ServiceStatus[];
  paneCount: number;
  prs: PrEntry[];
  creating: boolean;
  creationPhase: WorktreeCreationPhase | null;
  source: WorktreeSource;
  oneshot: OneshotConfig | null;
  tabs: WorktreeTab[];
  activeTabId: string | null;
}

export interface WorktreeListRow {
  worktree: WorktreeInfo;
  depth: number;
}

export type ToastTone = "info" | "success" | "error";

export interface ToastInput {
  tone: ToastTone;
  message: string;
  detail?: string;
}

export interface UiToastItem extends ToastInput {
  id: string;
  source: "ui";
}

export interface NotificationToastItem extends ToastInput {
  id: string;
  source: "notification";
  notificationId: number;
  branch: string;
}

export type ToastItem = UiToastItem | NotificationToastItem;
