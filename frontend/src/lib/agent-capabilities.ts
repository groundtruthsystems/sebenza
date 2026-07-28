import type { AgentSummary } from "./types";

export type CapabilityKey =
  | "fork"
  | "inAppChat"
  | "conversationHistory"
  | "interrupt"
  | "resume"
  | "pinnableSessionId"
  | "permissionInterception";

/**
 * What the built-in agents can do, used when the server's response doesn't say.
 *
 * Needed because API responses are not validated at runtime, so a Zod
 * `.default()` on a new capability field would claim a value an older server
 * never sent. Falling back on the agent id instead keeps one table of literals
 * here rather than scattering `agentName === "claude" || ...` checks through the
 * components.
 */
const BUILTIN_FALLBACK: Record<string, Partial<Record<CapabilityKey, boolean>>> = {
  claude: {
    fork: true,
    inAppChat: true,
    conversationHistory: true,
    interrupt: true,
    resume: true,
    // Accepts `--session-id`, so Sebenza can choose the id at launch.
    pinnableSessionId: true,
  },
  codex: {
    fork: true,
    inAppChat: true,
    conversationHistory: true,
    interrupt: true,
    resume: true,
    // Codex assigns its own session id; it must be discovered by polling.
    pinnableSessionId: false,
  },
  // opencode belongs here too, or it would fall back to all-false while its siblings
  // fall back to something sensible. Chat/history/interrupt are deliberately absent:
  // in-app chat needs a StreamProvider that does not exist yet.
  opencode: {
    fork: true,
    resume: true,
    conversationHistory: true,
    pinnableSessionId: true,
  },
};

// No agent can gate a tool call: claude's and codex's hooks observe only, and opencode's
// `permission.ask` hook was verified not to fire on 1.18.9. Left out of the fallback table
// entirely so it resolves false for everyone.

/**
 * Whether `agentId` supports `key`, per the server's advertised capabilities.
 *
 * Unknown agents and unknown capabilities are `false` — an affordance the server
 * would reject should not be offered.
 */
export function agentCan(
  agents: AgentSummary[],
  agentId: string | null | undefined,
  key: CapabilityKey,
): boolean {
  if (!agentId) return false;
  const advertised = agents.find((agent) => agent.id === agentId)?.capabilities[key];
  return advertised ?? BUILTIN_FALLBACK[agentId]?.[key] ?? false;
}
