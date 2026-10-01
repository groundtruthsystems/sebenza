import type { JSX } from "react";
import type { InboxRequest } from "./api-contract";

export default function InboxRequestCard(_props: {
  request: InboxRequest;
  /** A triage job is queued or running for this request. */
  triaging?: boolean;
  onconfirm: (confirm: { contentHash: string; body?: string }) => Promise<void>;
  onreject: (reason: string) => Promise<void>;
  onredeliver: () => Promise<void>;
  onretry: () => Promise<void>;
}): JSX.Element {
  throw new Error("todo");
}
