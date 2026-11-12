// Client for the software factory's solution routes. Reads go out plain
// (the server allows same-origin GETs); every action that runs an agent,
// edits a document or decides something carries the operator token
// (./operator), never the shared control token.

import { operatorToken } from "./operator";

export type AgentProfile = { agent: string; model?: string | null };

export interface FactoryAgent {
  agent: string;
  installed: boolean;
  enforcement: "verified" | "unverified";
  canReview: boolean;
}

export interface EffectiveProfiles {
  coordinator: AgentProfile;
  designer: AgentProfile;
  solutionReviewer: AgentProfile | null;
  projectReviewer: AgentProfile | null;
}

export interface FactoryOverrides {
  coordinator?: AgentProfile;
  designer?: AgentProfile;
  reviewer?: { solution?: AgentProfile; project?: AgentProfile };
}

export interface FactoryConfig {
  solution?: { coordinator?: AgentProfile; designer?: AgentProfile; reviewer?: AgentProfile };
  project?: { reviewer?: AgentProfile };
}

export interface RoundView {
  round: { level: string; scope: string; cycle: number; round: number };
  verdict: "agreed" | "changes";
  reviewer: AgentProfile;
  problemIds: string[];
  at: string;
}

export interface ProblemView {
  problemId: string;
  requestId: string;
  title: string;
  status: "open" | "resolved";
}

export interface HandoverCard {
  project: string;
  slug: string;
  status: "drafted" | "approved" | "deferred" | "converted";
  instructionHash: string;
  approvalValid: boolean;
  copyStale: boolean;
}

export interface SolutionView {
  kind: string;
  state: {
    phase: string;
    cycle: number;
    round: number;
    rounds: RoundView[];
    problems: ProblemView[];
    coordinatorNotes: Record<string, string>;
    designApproval: { docHashes: Record<string, string> } | null;
  };
  roundOf: number;
  documents: Record<string, string>;
  designHash: string;
  designApprovalValid: boolean;
  handovers: HandoverCard[];
  job: { running: boolean; step?: string | null; errorCode?: string | null; error?: string | null };
  profiles: EffectiveProfiles | null;
}

export interface TranscriptEntry {
  turnId?: string;
  kind: "operator" | "coordinator" | "report" | "review" | "error" | "applied";
  text?: string;
  agent?: string;
  model?: string | null;
  warnings?: string[];
  noteProposals?: { note_id: string | null; type: string; title: string; body: string }[] | null;
  problemResolutions?: { problem_id: string; text: string }[] | null;
  index?: number;
  code?: string;
  error?: string;
  round?: number;
  verdict?: string;
}

/** An API refusal: the HTTP status, the server's `code` and message. */
export class ApiRefusal extends Error {
  constructor(
    public status: number,
    public code: string | undefined,
    message: string,
  ) {
    super(message);
  }
}

async function call<T>(method: string, path: string, body?: unknown, operator = false): Promise<T> {
  const headers: Record<string, string> = {};
  if (body !== undefined) headers["content-type"] = "application/json";
  if (operator) {
    const token = await operatorToken();
    if (token) headers.authorization = `Bearer ${token}`;
  }
  const res = await fetch(path, {
    method,
    credentials: "same-origin",
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const data = (await res.json().catch(() => ({}))) as Record<string, unknown>;
  if (!res.ok) {
    throw new ApiRefusal(
      res.status,
      typeof data.code === "string" ? data.code : undefined,
      typeof data.error === "string" ? data.error : `HTTP ${res.status}`,
    );
  }
  return data as T;
}

const item = (id: string) => `/api/inbox/${encodeURIComponent(id)}`;

export const solutionApi = {
  view: (id: string) => call<SolutionView>("GET", `${item(id)}/solution`),
  transcript: (id: string) =>
    call<{ entries: TranscriptEntry[]; job: SolutionView["job"] }>("GET", `${item(id)}/coordinator/transcript`),
  say: (id: string, message: string, confirmWarnings = false) =>
    call<{ turnId: string }>("POST", `${item(id)}/coordinator/turn`, { message, confirmWarnings }, true),
  apply: (id: string, turnId: string, index: number) =>
    call<{ noteId: string }>("POST", `${item(id)}/coordinator/proposals/${encodeURIComponent(turnId)}/${index}/apply`, {}, true),
  startDesign: (id: string) => call<SolutionView>("POST", `${item(id)}/solution/start-design`, {}, true),
  review: (id: string, newCycle = false) =>
    call<SolutionView>("POST", `${item(id)}/solution/review`, { newCycle }, true),
  doc: (id: string, name: string) =>
    call<{ name: string; content: string; hash: string }>("GET", `${item(id)}/solution/docs/${name}`),
  saveDoc: (id: string, name: string, content: string, expectedHash: string) =>
    call<{ hash: string }>("PUT", `${item(id)}/solution/docs/${name}`, { content, expectedHash }, true),
  resolve: (id: string, problemId: string, note: string) =>
    call("POST", `${item(id)}/solution/problems/${encodeURIComponent(problemId)}/resolve`, { note }, true),
  approveDesign: (id: string) => call("POST", `${item(id)}/solution/approve-design`, {}, true),
  draftHandovers: (id: string) => call("POST", `${item(id)}/handover/draft`, {}, true),
  editHandover: (id: string, slug: string, content: string, expectedHash: string) =>
    call<{ instructionHash: string }>("PUT", `${item(id)}/handover/${slug}`, { content, expectedHash }, true),
  approveHandover: (id: string, slug: string, instructionHash: string) =>
    call("POST", `${item(id)}/handover/${slug}/approve`, { instructionHash }, true),
  deferHandover: (id: string, slug: string) => call("POST", `${item(id)}/handover/${slug}/defer`, {}, true),
  agents: () => call<{ agents: FactoryAgent[] }>("GET", "/api/factory/agents"),
  config: () => call<{ config: FactoryConfig; effective: EffectiveProfiles }>("GET", "/api/factory/config"),
  saveConfig: (config: FactoryConfig) => call("PUT", "/api/factory/config", config, true),
  itemAgents: (id: string) =>
    call<{ overrides: FactoryOverrides; effective: EffectiveProfiles }>("GET", `${item(id)}/factory/agents`),
  saveItemAgents: (id: string, overrides: FactoryOverrides) =>
    call<{ effective: EffectiveProfiles }>("PUT", `${item(id)}/factory/agents`, overrides, true),
};

export type SolutionApi = typeof solutionApi;
