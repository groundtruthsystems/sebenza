import type { TrackStatus } from "./types";

/** Kanban columns, in board order. */
export const TRACK_COLUMNS: { key: TrackStatus; label: string }[] = [
  { key: "backlog", label: "Backlog" },
  { key: "doing", label: "Doing" },
  { key: "blocked", label: "Blocked" },
  { key: "unblocked", label: "Unblocked" },
  { key: "done", label: "Done" },
];

/** Tailwind `bg-*` token for a status dot/marker. */
export function statusDotClass(status: string): string {
  switch (status) {
    case "done":
      return "bg-success";
    case "doing":
      return "bg-accent";
    case "blocked":
      return "bg-danger";
    case "unblocked":
      return "bg-warning";
    default:
      return "bg-muted";
  }
}

/** Tailwind `text-*` token for status text. */
export function statusTextClass(status: string): string {
  switch (status) {
    case "done":
      return "text-success";
    case "doing":
      return "text-accent";
    case "blocked":
      return "text-danger";
    case "unblocked":
      return "text-warning";
    default:
      return "text-muted";
  }
}

export function statusLabel(status: string): string {
  return status ? status.charAt(0).toUpperCase() + status.slice(1) : status;
}
