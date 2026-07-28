import type { AgentSummary } from "./types";

type CapabilityKey = "fork" | "inAppChat" | "conversationHistory" | "interrupt" | "resume";

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
  claude: { fork: true, inAppChat: true, conversationHistory: true, interrupt: true, resume: true },
  codex: { fork: true, inAppChat: true, conversationHistory: true, interrupt: true, resume: true },
};

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
