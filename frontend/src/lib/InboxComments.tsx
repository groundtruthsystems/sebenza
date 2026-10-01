import type { JSX } from "react";
import type { InboxComment, InboxCommentGroups } from "./api-contract";
import type { WorktreeRef } from "./inbox-collab";

export default function InboxComments(_props: {
  groups: InboxCommentGroups;
  /** Worktrees the composer may post to, beyond those with comments. */
  worktrees: WorktreeRef[];
  onpost: (body: string, worktree?: WorktreeRef) => Promise<InboxComment>;
  onredact: (eventId: string) => Promise<void>;
}): JSX.Element {
  throw new Error("todo");
}
