import { describe, expect, it } from "vitest";
import { agentCan } from "./agent-capabilities";
import type { AgentSummary } from "./types";

function agent(
  id: string,
  kind: "builtin" | "custom",
  overrides: Partial<AgentSummary["capabilities"]> = {},
): AgentSummary {
  return {
    id,
    label: id,
    kind,
    capabilities: {
      terminal: true,
      inAppChat: kind === "builtin",
      conversationHistory: kind === "builtin",
      interrupt: kind === "builtin",
      resume: kind === "builtin",
      fork: kind === "builtin",
      ...overrides,
    },
  };
}

describe("agentCan", () => {
  it("reads the capability from the agent summary when advertised", () => {
    const agents = [agent("claude", "builtin"), agent("goose", "custom")];
    expect(agentCan(agents, "claude", "fork")).toBe(true);
    expect(agentCan(agents, "goose", "fork")).toBe(false);
  });

  it("prefers the advertised value over the built-in fallback", () => {
    // A server that has disabled forking for claude must be believed.
    const agents = [agent("claude", "builtin", { fork: false })];
    expect(agentCan(agents, "claude", "fork")).toBe(false);
  });

  it("falls back to the built-in table when an older server omits the field", () => {
    // Responses are not validated at runtime, so `fork` can simply be absent.
    const stale = agent("claude", "builtin");
    delete (stale.capabilities as Record<string, unknown>).fork;
    expect(agentCan([stale], "claude", "fork")).toBe(true);
    expect(agentCan([stale], "codex", "fork")).toBe(true);
  });

  it("returns false for a custom agent missing from the fallback table", () => {
    const stale = agent("goose", "custom");
    delete (stale.capabilities as Record<string, unknown>).fork;
    expect(agentCan([stale], "goose", "fork")).toBe(false);
  });

  it("returns false for an unknown agent or a missing id", () => {
    const agents = [agent("claude", "builtin")];
    expect(agentCan(agents, "nope", "fork")).toBe(false);
    expect(agentCan(agents, null, "fork")).toBe(false);
    expect(agentCan(agents, undefined, "fork")).toBe(false);
  });

  it("resolves inAppChat the same way, so chat and fork share one table", () => {
    const agents = [agent("codex", "builtin"), agent("opencode", "custom")];
    expect(agentCan(agents, "codex", "inAppChat")).toBe(true);
    expect(agentCan(agents, "opencode", "inAppChat")).toBe(false);
  });
});
