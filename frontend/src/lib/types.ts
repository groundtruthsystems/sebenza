import type {
  AgentId,
  BuiltInAgentId,
  OneshotConfig,
  PrEntry,
  ServiceStatus,
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
  ConductorStatus,
  ConductorPhaseSummary,
  ConductorTrack,
  ConductorTracks,
  ConductorFileResponse,
} from "./api-contract";
export type { AgentsSendMessageRequest as AgentsUiSendMessageRequest } from "./api-contract";

// Parsed `plan.json` (via a track's plan_path) — fetched as raw text through the
// conductor-file endpoint and JSON.parsed client-side, so it's a frontend shape.
export interface ConductorPlanSubtask {
  id: string;
  name: string;
  status: string;
  blocked_reason?: string | null;
}
export interface ConductorPlanTask {
  id: string;
  name: string;
  description?: string;
  status: string;
  blocked_reason?: string | null;
  commit?: string;
  notes?: string;
  subtasks?: ConductorPlanSubtask[];
}
export interface ConductorPlanPhase {
  id: string;
  name: string;
  status: string;
  blocked_reason?: string | null;
  tasks?: ConductorPlanTask[];
}
export interface ConductorPlan {
  track_id?: string;
  phases: ConductorPlanPhase[];
}

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
