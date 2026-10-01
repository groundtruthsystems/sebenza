import type { JSX } from "react";
import type { Priority, PrioritySource } from "./inbox-collab";

export function PriorityBadge(_props: {
  priority: Priority;
  source: PrioritySource;
}): JSX.Element {
  throw new Error("todo");
}

export default function PriorityControl(_props: {
  priority: Priority;
  source: PrioritySource;
  disabled?: boolean;
  onchange: (priority: Priority | null) => void;
}): JSX.Element {
  throw new Error("todo");
}
