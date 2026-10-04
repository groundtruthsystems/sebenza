import { useCallback, useEffect, useRef, useState } from "react";
import {
  confirmInboxRequest,
  fetchInboxComments,
  fetchInboxRequests,
  postInboxComment,
  redactInboxComment,
  redeliverInboxRequest,
  rejectInboxRequest,
  retryInboxTriage,
} from "./api";
import type {
  InboxAgentJob,
  InboxCommentGroups,
  InboxRequest,
} from "./api-contract";
import { isStale, type WorktreeRef } from "./inbox-collab";
import InboxComments from "./InboxComments";
import InboxRequestCard from "./InboxRequestCard";

type Tab = "requests" | "comments";

const EMPTY: InboxCommentGroups = { overall: [], worktrees: [] };

/**
 * The open item's requests and comment threads, side by side with the
 * editor. Owns their loading and the operator's decisions; the agent stream
 * lives in the view and arrives here as `lastJob`.
 */
export default function InboxActivity({
  draftId,
  worktrees,
  lastJob,
  onchanged,
}: {
  draftId: string;
  worktrees: WorktreeRef[];
  /** The newest `inbox.job` frame for this item, if any. */
  lastJob: InboxAgentJob | null;
  /** A decision or a finished triage may move priority or the list flag. */
  onchanged: () => void;
}) {
  const [tab, setTab] = useState<Tab>("requests");
  const [groups, setGroups] = useState<InboxCommentGroups>(EMPTY);
  const [requests, setRequests] = useState<InboxRequest[]>([]);
  const [error, setError] = useState("");
  /** Requests with a triage job queued or running. */
  const [triaging, setTriaging] = useState<Set<string>>(new Set());
  // Held in a ref so a new callback identity never replays a stream frame.
  const changed = useRef(onchanged);
  changed.current = onchanged;

  const load = useCallback(async () => {
    const [c, r] = await Promise.allSettled([
      fetchInboxComments(draftId),
      fetchInboxRequests(draftId),
    ]);
    if (c.status === "fulfilled") setGroups(c.value);
    if (r.status === "fulfilled") setRequests(r.value.requests);
    const failed = [c, r].find((x) => x.status === "rejected");
    setError(failed ? ((failed as PromiseRejectedResult).reason as Error).message : "");
  }, [draftId]);

  useEffect(() => {
    setGroups(EMPTY);
    setRequests([]);
    setTriaging(new Set());
    void load();
  }, [load]);

  useEffect(() => {
    if (!lastJob || lastJob.draftId !== draftId || lastJob.kind !== "triage") return;
    const rid = lastJob.requestId;
    const live = lastJob.status === "queued" || lastJob.status === "running";
    if (rid) {
      setTriaging((prev) => {
        const next = new Set(prev);
        if (live) next.add(rid);
        else next.delete(rid);
        return next;
      });
    }
    if (!live) {
      void load();
      changed.current();
    }
  }, [lastJob, draftId, load]);

  /** Run a decision, then re-read; a stale one re-reads too, so the card
   *  shows what changed before the operator tries again. */
  const decide = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
    } catch (err) {
      if (isStale(err)) void load();
      throw err;
    }
    await load();
    changed.current();
  };

  const flagged = requests.some((r) => r.flagged);
  const commentCount =
    groups.overall.length +
    groups.worktrees.reduce((n, w) => n + w.comments.length, 0);

  const tabClass = (t: Tab) =>
    `flex-1 h-9 text-xs cursor-pointer border-b-2 ${
      tab === t
        ? "border-accent text-primary"
        : "border-transparent text-muted hover:text-primary"
    }`;

  return (
    <aside className="w-[360px] shrink-0 border-l border-edge bg-sidebar flex flex-col min-h-0">
      <div role="tablist" className="flex border-b border-edge">
        <button
          type="button"
          role="tab"
          aria-selected={tab === "requests"}
          className={tabClass("requests")}
          onClick={() => setTab("requests")}
        >
          Requests <span className="text-muted">{requests.length}</span>
          {flagged && (
            <span
              className="inline-block ml-1 h-1.5 w-1.5 rounded-full bg-danger align-middle"
              title="A request needs you"
            />
          )}
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={tab === "comments"}
          className={tabClass("comments")}
          onClick={() => setTab("comments")}
        >
          Comments <span className="text-muted">{commentCount}</span>
        </button>
      </div>
      {error && (
        <p className="m-0 px-3 py-2 text-[11px] text-danger border-b border-edge" role="alert">
          {error}
        </p>
      )}
      {tab === "requests" ? (
        <div className="flex-1 min-h-0 overflow-y-auto p-3 flex flex-col gap-2">
          {requests.length === 0 && (
            <p className="m-0 text-[11px] text-muted">
              No requests. Worktree agents raise them with{" "}
              <code>sebenza-agentctl request</code>.
            </p>
          )}
          {requests.map((r) => (
            <InboxRequestCard
              key={r.requestId}
              request={r}
              triaging={triaging.has(r.requestId)}
              onconfirm={(c) =>
                decide(() => confirmInboxRequest(draftId, r.requestId, c))
              }
              onreject={(reason) =>
                decide(() => rejectInboxRequest(draftId, r.requestId, reason))
              }
              onredeliver={() =>
                decide(() => redeliverInboxRequest(draftId, r.requestId))
              }
              onretry={() =>
                decide(async () => {
                  await retryInboxTriage(draftId, r.requestId);
                  setTriaging((prev) => new Set(prev).add(r.requestId));
                })
              }
            />
          ))}
        </div>
      ) : (
        <div className="flex-1 min-h-0">
          <InboxComments
            groups={groups}
            worktrees={worktrees}
            onpost={async (body, worktree) => {
              const { comment } = await postInboxComment(draftId, body, worktree);
              await load();
              return comment;
            }}
            onredact={async (eventId) => {
              await redactInboxComment(draftId, eventId);
              await load();
            }}
          />
        </div>
      )}
    </aside>
  );
}
