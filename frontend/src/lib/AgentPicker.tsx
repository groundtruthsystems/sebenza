import type { AgentProfile, FactoryAgent } from "./solutionApi";

const LABEL: Record<string, string> = { claude: "Claude", codex: "Codex", grok: "Grok" };

/** Why an agent cannot be chosen for this role, or null. */
export function refusal(agent: FactoryAgent | undefined, reviewer: boolean): string | null {
  if (!agent) return "Unknown agent.";
  if (!agent.installed) return `${LABEL[agent.agent] ?? agent.agent} is not installed on this machine.`;
  if (reviewer && !agent.canReview)
    return `${LABEL[agent.agent] ?? agent.agent} cannot review: its read-only enforcement is unverified, so it may only coordinate or design.`;
  return null;
}

/**
 * Pick the agent (and optionally the model) for one factory role. Every
 * known agent is listed: uninstalled ones are disabled, an unverified one is
 * badged "read-only unverified", and for a reviewer role it is refused with
 * the reason (FR-23, TS-03, TS-56).
 */
export default function AgentPicker({
  label,
  agents,
  value,
  reviewer = false,
  onChange,
}: {
  label: string;
  agents: FactoryAgent[];
  value: AgentProfile | null | undefined;
  reviewer?: boolean;
  onChange: (next: AgentProfile | null) => void;
}) {
  const current = agents.find((a) => a.agent === value?.agent);
  const problem = value ? refusal(current, reviewer) : null;
  return (
    <fieldset className="flex flex-col gap-1 text-xs border-0 p-0 m-0">
      <legend className="text-muted mb-1">{label}</legend>
      <div className="flex flex-wrap items-center gap-2">
        <select
          aria-label={`${label} agent`}
          className="h-7 rounded-md border border-edge bg-surface px-2"
          value={value?.agent ?? ""}
          onChange={(e) => {
            const agent = e.currentTarget.value;
            onChange(agent ? { agent, model: value?.model ?? null } : null);
          }}
        >
          <option value="">Default</option>
          {agents.map((a) => {
            const why = refusal(a, reviewer);
            return (
              <option key={a.agent} value={a.agent} disabled={why !== null} title={why ?? undefined}>
                {LABEL[a.agent] ?? a.agent}
                {a.enforcement === "unverified" ? " (read-only unverified)" : ""}
              </option>
            );
          })}
        </select>
        <input
          aria-label={`${label} model`}
          className="h-7 w-48 rounded-md border border-edge bg-surface px-2"
          placeholder="model (optional)"
          value={value?.model ?? ""}
          disabled={!value}
          onChange={(e) =>
            value && onChange({ agent: value.agent, model: e.currentTarget.value || null })
          }
        />
        {agents
          .filter((a) => a.enforcement === "unverified")
          .map((a) => (
            <span
              key={a.agent}
              className="px-1 rounded border border-warning text-warning"
              title={`${LABEL[a.agent] ?? a.agent}'s read-only enforcement has not passed its conformance test.`}
            >
              {LABEL[a.agent] ?? a.agent}: read-only unverified
            </span>
          ))}
      </div>
      {problem && (
        <p role="alert" className="m-0 text-danger">
          {problem}
        </p>
      )}
    </fieldset>
  );
}
