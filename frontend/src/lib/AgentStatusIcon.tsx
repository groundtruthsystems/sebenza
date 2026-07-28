// Whether the icon renders a visible mark for this status — kept beside the
// component so callers can avoid laying out an empty slot.
export function agentIconVisible(status: string, unread: boolean): boolean {
  return (
    status === "working" ||
    status === "waiting" ||
    status === "awaiting-permission" ||
    status === "error" ||
    (status === "done" && unread)
  );
}

function pillClass(s: string): string {
  if (s === "working") return "bg-success/15 text-success";
  if (s === "waiting" || s === "awaiting-permission") return "bg-warning/15 text-warning";
  if (s === "done") return "bg-success/15 text-success";
  if (s === "error") return "bg-danger/15 text-danger";
  return "bg-hover text-muted";
}

function Icon({ status, size, unread }: { status: string; size: number; unread: boolean }) {
  if (status === "working") {
    return (
      <svg
        className="text-success working-dots"
        xmlns="http://www.w3.org/2000/svg"
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="currentColor"
        stroke="none"
      >
        <circle cx="3" cy="12" r="2.5" />
        <circle cx="12" cy="12" r="2.5" />
        <circle cx="21" cy="12" r="2.5" />
      </svg>
    );
  }
  if (status === "waiting" || status === "awaiting-permission") {
    return (
      <svg
        className="text-warning"
        xmlns="http://www.w3.org/2000/svg"
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
        <path d="M12 7v2" />
        <path d="M12 13h.01" />
      </svg>
    );
  }
  if (status === "done") {
    if (!unread) return null;
    return (
      <svg
        className="text-accent"
        xmlns="http://www.w3.org/2000/svg"
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="currentColor"
        stroke="none"
      >
        <circle cx="12" cy="12" r="6" />
      </svg>
    );
  }
  if (status === "error") {
    return (
      <svg
        className="text-danger"
        xmlns="http://www.w3.org/2000/svg"
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="3"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <line x1="18" y1="6" x2="6" y2="18" />
        <line x1="6" y1="6" x2="18" y2="18" />
      </svg>
    );
  }
  return null;
}

export default function AgentStatusIcon({
  status,
  size = 10,
  pill = false,
  unread = false,
}: {
  status: string;
  size?: number;
  pill?: boolean;
  unread?: boolean;
}) {
  if (!pill) return <Icon status={status} size={size} unread={unread} />;
  return (
    <span className={`text-xs px-2 py-0.5 rounded-xl flex items-center gap-1 ${pillClass(status)}`}>
      <Icon status={status} size={size} unread={unread} />
      {status === "awaiting-permission" ? "needs approval" : status || "idle"}
    </span>
  );
}
