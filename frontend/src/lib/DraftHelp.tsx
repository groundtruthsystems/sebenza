import type { JSX } from "react";
import type { InboxDraftHelpOutput } from "./api-contract";

export function DraftHelpPanel(_props: {
  current: string;
  pending: boolean;
  error: string;
  proposal: InboxDraftHelpOutput | null;
  onrequest: (instruction: string) => void;
  onaccept: () => void;
  ondiscard: () => void;
  onclose: () => void;
}): JSX.Element {
  throw new Error("todo");
}

export function DraftMergeView(_props: {
  theirs: string;
  theirsLabel: string;
  mine: string;
  saving: boolean;
  onsave: (merged: string) => void;
  oncancel: () => void;
}): JSX.Element {
  throw new Error("todo");
}
