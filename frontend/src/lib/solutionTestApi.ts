// A fake SolutionApi for component tests: every method is a vi.fn with a
// sensible default, overridable per test.
import { vi } from "vitest";
import type { SolutionApi, SolutionView } from "./solutionApi";

export function view(over: Partial<SolutionView> = {}, state: Partial<SolutionView["state"]> = {}): SolutionView {
  return {
    kind: "solution",
    roundOf: 3,
    documents: {},
    designHash: "dh",
    designApprovalValid: false,
    handovers: [],
    job: { running: false },
    profiles: null,
    ...over,
    state: {
      phase: "requirements",
      cycle: 1,
      round: 0,
      rounds: [],
      problems: [],
      coordinatorNotes: {},
      designApproval: null,
      ...state,
    },
  };
}

export function fakeApi(over: Partial<Record<keyof SolutionApi, unknown>> = {}): SolutionApi {
  const base = {
    view: vi.fn().mockResolvedValue(view()),
    transcript: vi.fn().mockResolvedValue({ entries: [], job: { running: false } }),
    say: vi.fn().mockResolvedValue({ turnId: "T2" }),
    apply: vi.fn().mockResolvedValue({ noteId: "n2" }),
    startDesign: vi.fn().mockResolvedValue(view()),
    review: vi.fn().mockResolvedValue(view()),
    doc: vi.fn().mockResolvedValue({ name: "requirements.md", content: "# R", hash: "h1" }),
    saveDoc: vi.fn().mockResolvedValue({ hash: "h2" }),
    resolve: vi.fn().mockResolvedValue({}),
    approveDesign: vi.fn().mockResolvedValue({}),
    draftHandovers: vi.fn().mockResolvedValue({}),
    editHandover: vi.fn().mockResolvedValue({ instructionHash: "ih2" }),
    approveHandover: vi.fn().mockResolvedValue({}),
    deferHandover: vi.fn().mockResolvedValue({}),
    agents: vi.fn().mockResolvedValue({
      agents: [
        { agent: "claude", installed: true, enforcement: "verified", canReview: true },
        { agent: "codex", installed: true, enforcement: "verified", canReview: true },
        { agent: "grok", installed: true, enforcement: "unverified", canReview: false },
      ],
    }),
    config: vi.fn().mockResolvedValue({ config: {}, effective: null }),
    saveConfig: vi.fn().mockResolvedValue({}),
    itemAgents: vi.fn().mockResolvedValue({ overrides: {}, effective: null }),
    saveItemAgents: vi.fn().mockResolvedValue({ effective: null }),
  };
  return { ...base, ...over } as unknown as SolutionApi;
}
