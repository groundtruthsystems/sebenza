import type { AppNotification } from "./types";

export default function NotificationItem({
  notification,
  showTimestamp = false,
  large = false,
  wrap = false,
}: {
  notification: AppNotification;
  showTimestamp?: boolean;
  large?: boolean;
  wrap?: boolean;
}) {
  const isSuccess =
    notification.type === "agent_stopped" || notification.type === "worktree_auto_removed";
  return (
    <>
      <span className={`shrink-0 ${large ? "text-base" : "text-sm"}`}>
        {isSuccess ? (
          <span className="text-success">&#10003;</span>
        ) : (
          <span className="text-accent">&#9741;</span>
        )}
      </span>
      <span className="flex flex-col gap-0.5 min-w-0">
        <span
          className={`${large ? "text-sm" : "text-xs"} text-primary ${
            wrap ? "whitespace-normal break-words" : "truncate"
          }`}
        >
          {notification.message}
        </span>
        {showTimestamp ? (
          <span className="text-[10px] text-muted">
            {new Date(notification.timestamp).toLocaleTimeString()}
          </span>
        ) : notification.url ? (
          <span
            className={`text-xs text-accent ${wrap ? "whitespace-normal break-all" : "truncate"}`}
          >
            {notification.url}
          </span>
        ) : null}
      </span>
    </>
  );
}
