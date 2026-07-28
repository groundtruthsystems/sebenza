import type { z } from "zod";
import type { AgentCapabilitiesSchema } from "./schemas";

type AgentCapabilities = z.infer<typeof AgentCapabilitiesSchema>;

/**
 * Agent capabilities for tests, defaulting to the least-capable shape.
 *
 * Centralised so adding a capability to `AgentCapabilitiesSchema` does not require
 * editing every fixture — override only what the test cares about:
 *
 *   capabilities: agentCapabilities({ inAppChat: true, fork: true })
 */
export function agentCapabilities(
  overrides: Partial<AgentCapabilities> = {},
): AgentCapabilities {
  return {
    terminal: true,
    inAppChat: false,
    conversationHistory: false,
    interrupt: false,
    resume: false,
    fork: false,
    pinnableSessionId: false,
    permissionInterception: false,
    ...overrides,
  };
}

/** Capabilities of a fully-featured built-in agent (claude/codex today). */
export function builtinAgentCapabilities(
  overrides: Partial<AgentCapabilities> = {},
): AgentCapabilities {
  return agentCapabilities({
    inAppChat: true,
    conversationHistory: true,
    interrupt: true,
    resume: true,
    fork: true,
    pinnableSessionId: true,
    ...overrides,
  });
}
