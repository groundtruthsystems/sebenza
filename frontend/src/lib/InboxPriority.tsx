import { PRIORITIES, type Priority, type PrioritySource } from "./inbox-collab";

const tone: Record<Priority, string> = {
  P0: "border-danger text-danger",
  P1: "border-warning text-warning",
  P2: "border-edge text-muted",
  P3: "border-edge text-muted opacity-70",
};

function describe(priority: Priority, source: PrioritySource) {
  return source === "operator"
    ? `Priority ${priority}, operator override`
    : `Priority ${priority}, set by the system agent`;
}

/** `P0`–`P3`; an operator override carries a trailing dot. */
export function PriorityBadge({
  priority,
  source,
}: {
  priority: Priority;
  source: PrioritySource;
}) {
  return (
    <span
      className={`shrink-0 px-1 py-0.5 rounded border font-mono text-[10px] leading-none ${tone[priority]} ${
        source === "operator" ? "border-solid" : "border-dashed"
      }`}
      title={describe(priority, source)}
    >
      {priority}
    </span>
  );
}

/**
 * Set or clear the operator override. Picking a value pins it — the system
 * agent cannot move it until the override is cleared.
 */
export default function PriorityControl({
  priority,
  source,
  disabled = false,
  onchange,
}: {
  priority: Priority;
  source: PrioritySource;
  disabled?: boolean;
  onchange: (priority: Priority | null) => void;
}) {
  return (
    <div className="flex items-center gap-1.5 text-[11px] text-muted">
      <select
        aria-label="Priority"
        title={describe(priority, source)}
        className={`h-7 rounded-md border bg-surface px-1.5 text-xs font-mono focus:outline-none focus:border-accent ${tone[priority]}`}
        value={priority}
        disabled={disabled}
        onChange={(e) => onchange(e.currentTarget.value as Priority)}
      >
        {PRIORITIES.map((p) => (
          <option key={p} value={p}>
            {p}
          </option>
        ))}
      </select>
      {source === "operator" ? (
        <>
          <span>operator override</span>
          <button
            type="button"
            className="text-accent hover:underline cursor-pointer disabled:opacity-50"
            disabled={disabled}
            onClick={() => onchange(null)}
            title="Hand priority back to the system agent"
          >
            Clear override
          </button>
        </>
      ) : (
        <span>set by agent</span>
      )}
    </div>
  );
}
