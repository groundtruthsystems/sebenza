import { useState } from "react";
import type { InboxRequest, InboxRequestHistory } from "./api-contract";
import { isStale, lineDiff, sha1Hex, worktreeLabel } from "./inbox-collab";
import { renderCommentMarkdown } from "./inboxMarkdown";
import { WarningBadges } from "./InboxComments";
import Btn from "./Btn";

const statusTone: Record<InboxRequest["status"], string> = {
  open: "border-accent text-accent",
  proposed: "border-warning text-warning",
  confirmed: "border-edge text-muted",
  resolved: "border-success text-success",
  delivery_failed: "border-danger text-danger",
  dismissed: "border-edge text-muted line-through",
};

/** Visible keyboard focus for the request-management controls (NFR-04). */
const focusRing = "focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent";

const field =
  "w-full rounded-md border border-edge bg-surface px-2 py-1.5 text-xs text-primary placeholder:text-muted focus:outline-none focus:border-accent resize-y font-mono";

/** Removed/added lines of the operator's edit against the proposal. */
export function DiffView({
  before,
  after,
  label,
}: {
  before: string;
  after: string;
  label: string;
}) {
  return (
    <pre
      aria-label={label}
      className="m-0 rounded-md border border-edge bg-surface p-2 text-[11px] font-mono whitespace-pre-wrap overflow-x-auto"
    >
      {lineDiff(before, after).map((line, i) => (
        <div
          key={i}
          data-op={line.op}
          className={
            line.op === "add"
              ? "bg-success/10 text-success before:content-['+_']"
              : line.op === "del"
                ? "bg-danger/10 text-danger line-through before:content-['-_']"
                : "text-muted before:content-['_']"
          }
        >
          {line.text}
        </div>
      ))}
    </pre>
  );
}

/**
 * One worktree request and what the operator can do with it.
 *
 * The proposal is shown verbatim — as plain text, exactly what would be
 * pasted — and confirming quotes its hash, so a proposal that changed after
 * it was shown is a 409 rather than a delivery of something unseen.
 */
export default function InboxRequestCard({
  request,
  triaging = false,
  onconfirm,
  onreject,
  onredeliver,
  onretry,
  ondismiss,
  onarchive,
  onretrynotice,
  onhistory,
}: {
  request: InboxRequest;
  /** A triage job is queued or running for this request. */
  triaging?: boolean;
  onconfirm: (confirm: { contentHash: string; body?: string }) => Promise<void>;
  onreject: (reason: string) => Promise<void>;
  onredeliver: () => Promise<void>;
  onretry: () => Promise<void>;
  /** Dismiss an unanswered request (operator only). */
  ondismiss?: () => Promise<void>;
  /** Archive a confirmed or resolved request; status is unchanged. */
  onarchive?: () => Promise<void>;
  /** Retry a dismissal notice that failed to deliver. */
  onretrynotice?: () => Promise<void>;
  /** Load the request's history for the inline history view. */
  onhistory?: () => Promise<InboxRequestHistory>;
}) {
  const [history, setHistory] = useState<InboxRequestHistory | null>(null);
  const [showHistory, setShowHistory] = useState(false);
  const [editing, setEditing] = useState(false);
  const [edited, setEdited] = useState(request.proposal ?? "");
  const [authored, setAuthored] = useState("");
  const [rejecting, setRejecting] = useState(false);
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const r = request;
  const hasProposal =
    r.status === "proposed" && r.proposal !== null && r.proposalHash !== null;
  const decidable = r.status === "open" || r.status === "proposed";
  const rejectable = decidable || r.status === "delivery_failed";
  const dismissable = decidable || r.status === "delivery_failed";
  const archivable =
    (r.status === "confirmed" || r.status === "resolved") && !r.archivedAt;
  const noticeFailed = r.notice?.state === "failed";

  const toggleHistory = () =>
    act(async () => {
      if (!showHistory && onhistory) setHistory(await onhistory());
      setShowHistory((v) => !v);
    });

  const act = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError("");
    try {
      await fn();
      setEditing(false);
      setRejecting(false);
      setReason("");
    } catch (err) {
      setError(
        isStale(err)
          ? "This request changed since you opened it. Review the refreshed card and decide again."
          : (err as Error).message,
      );
    } finally {
      setBusy(false);
    }
  };

  const confirm = () =>
    act(async () => {
      if (hasProposal) {
        const changed = editing && edited !== r.proposal;
        await onconfirm(
          changed
            ? { contentHash: r.proposalHash!, body: edited }
            : { contentHash: r.proposalHash! },
        );
      } else {
        // UC-06c: no proposal, so the hash is of the text being sent.
        await onconfirm({ contentHash: await sha1Hex(authored), body: authored });
      }
    });

  return (
    <article
      aria-label={`Request: ${r.title}`}
      className={`rounded-md border p-3 flex flex-col gap-2 ${
        r.flagged ? "border-danger" : "border-edge"
      }`}
    >
      <header className="flex flex-wrap items-center gap-1.5 text-[10px] text-muted">
        <span className={`px-1 rounded border ${statusTone[r.status]}`}>
          {r.status}
        </span>
        {r.flagged && (
          <span className="px-1 rounded border border-danger text-danger">flagged</span>
        )}
        {r.archivedAt && (
          <span className="px-1 rounded border border-edge text-muted" title={`Archived ${r.archivedAt}`}>
            archived
          </span>
        )}
        {noticeFailed && (
          <span
            className="px-1 rounded border border-danger text-danger"
            title={r.notice?.lastError ?? undefined}
          >
            notice not delivered
          </span>
        )}
        {triaging && <span className="text-accent">triaging…</span>}
        {r.worktree ? (
          <span title={`${r.worktree.project} · ${r.worktree.branch}`}>
            {worktreeLabel(r.worktree)}
          </span>
        ) : (
          <span>solution problem</span>
        )}
        <WarningBadges warnings={r.warnings} />
        <span className="ml-auto">{r.openedAt.replace("T", " ").slice(0, 16)}</span>
      </header>

      <h4 className="m-0 text-xs font-semibold">{r.title}</h4>
      {r.summary?.request && (
        <p aria-label="Summary" className="m-0 text-[11px] text-muted">
          {r.summary.request}
          {r.summary.result && <> — {r.summary.result}</>}
        </p>
      )}
      <div
        className="inbox-comment md-body text-xs"
        dangerouslySetInnerHTML={{ __html: renderCommentMarkdown(r.body) }}
      />

      {r.lastError && (
        <p className="m-0 text-[11px] text-danger">Failed: {r.lastError}</p>
      )}
      {r.lastReason && (
        <p className="m-0 text-[11px] text-muted">Last rejected: {r.lastReason}</p>
      )}

      {hasProposal && (
        <div className="flex flex-col gap-1">
          <div className="flex items-center gap-1.5 text-[10px] text-muted">
            <span>Proposed resolution (verbatim)</span>
            <WarningBadges warnings={r.proposalWarnings} />
          </div>
          <pre
            aria-label="Proposal"
            className="m-0 rounded-md border border-edge bg-surface p-2 text-[11px] font-mono whitespace-pre-wrap"
          >
            {r.proposal}
          </pre>
          {editing && (
            <>
              <textarea
                aria-label="Edited resolution"
                rows={4}
                className={field}
                value={edited}
                onChange={(e) => setEdited(e.currentTarget.value)}
                disabled={busy}
              />
              {edited !== r.proposal && (
                <DiffView
                  label="Changes to the proposal"
                  before={r.proposal!}
                  after={edited}
                />
              )}
            </>
          )}
        </div>
      )}

      {decidable && !hasProposal && (
        <textarea
          aria-label="Resolution"
          rows={3}
          placeholder={
            triaging
              ? "Triage is running; or write the resolution yourself"
              : "No proposal. Write the resolution to send to the worktree"
          }
          className={field}
          value={authored}
          onChange={(e) => setAuthored(e.currentTarget.value)}
          disabled={busy}
        />
      )}

      {(r.status === "confirmed" ||
        r.status === "resolved" ||
        r.status === "delivery_failed") &&
        r.confirmedText && (
          <div className="flex flex-col gap-1">
            <span className="text-[10px] text-muted">
              {r.status === "resolved" ? "Delivered" : "Confirmed"}
            </span>
            <pre className="m-0 rounded-md border border-edge bg-surface p-2 text-[11px] font-mono whitespace-pre-wrap">
              {r.confirmedText}
            </pre>
          </div>
        )}

      {rejecting && (
        <input
          aria-label="Reason"
          placeholder="Why? The request reopens with this reason"
          className="h-7 rounded-md border border-edge bg-surface px-2 text-xs text-primary placeholder:text-muted focus:outline-none focus:border-accent"
          value={reason}
          onChange={(e) => setReason(e.currentTarget.value)}
          disabled={busy}
        />
      )}

      {showHistory && history && (
        <div className="flex flex-col gap-1 rounded-md border border-edge bg-surface p-2">
          <ol aria-label="Request history" className="m-0 pl-4 text-[11px] flex flex-col gap-0.5">
            {history.events.map((e, i) => (
              <li key={i}>
                <span className="font-semibold">{e.kind.replace("_", " ")}</span>
                {e.from && e.to && e.from !== e.to && (
                  <span className="text-muted"> {e.from} → {e.to}</span>
                )}
                {e.detail && <span className="text-muted"> ({e.detail})</span>}
                <span className="text-muted"> · {e.actor} · {e.at.replace("T", " ").slice(0, 16)}</span>
              </li>
            ))}
          </ol>
          {history.answers.map((a, i) => (
            <div key={i} className="flex flex-col gap-0.5">
              <span className="text-[10px] text-muted">{a.kind}</span>
              <pre className="m-0 text-[11px] font-mono whitespace-pre-wrap">{a.text}</pre>
            </div>
          ))}
        </div>
      )}

      {error && <p className="m-0 text-[11px] text-danger">{error}</p>}

      <div className="flex flex-wrap items-center justify-end gap-2">
        {rejecting ? (
          <>
            <Btn small disabled={busy} onClick={() => setRejecting(false)}>
              Cancel
            </Btn>
            <Btn
              small
              variant="danger-outline"
              disabled={busy || !reason.trim()}
              onClick={() => void act(() => onreject(reason.trim()))}
            >
              Confirm reject
            </Btn>
          </>
        ) : (
          <>
            {onhistory && (
              <Btn
                small
                type="button"
                className={focusRing}
                aria-expanded={showHistory}
                disabled={busy}
                onClick={() => void toggleHistory()}
              >
                History
              </Btn>
            )}
            {noticeFailed && onretrynotice && (
              <Btn
                small
                type="button"
                variant="accent-outline"
                className={focusRing}
                disabled={busy}
                onClick={() => void act(onretrynotice)}
              >
                Retry notice
              </Btn>
            )}
            {onarchive && r.status !== "dismissed" && (
              <Btn
                small
                type="button"
                className={focusRing}
                disabled={busy || !archivable}
                title={
                  archivable
                    ? "Archive: keep it, out of the way"
                    : r.archivedAt
                      ? "Already archived"
                      : "Only a confirmed or resolved request can be archived"
                }
                onClick={() => void act(onarchive)}
              >
                Archive
              </Btn>
            )}
            {dismissable && ondismiss && (
              <Btn
                small
                type="button"
                variant="danger-outline"
                className={focusRing}
                disabled={busy}
                onClick={() => void act(ondismiss)}
              >
                Dismiss
              </Btn>
            )}
            {r.flagged && r.status === "open" && (
              <Btn small disabled={busy || triaging} onClick={() => void act(onretry)}>
                Retry triage
              </Btn>
            )}
            {r.status === "delivery_failed" && (
              <Btn
                small
                variant="accent-outline"
                disabled={busy}
                onClick={() => void act(onredeliver)}
              >
                Redeliver
              </Btn>
            )}
            {rejectable && (
              <Btn small disabled={busy} onClick={() => setRejecting(true)}>
                Reject
              </Btn>
            )}
            {hasProposal && !editing && (
              <Btn
                small
                disabled={busy}
                onClick={() => {
                  setEdited(r.proposal ?? "");
                  setEditing(true);
                }}
              >
                Edit
              </Btn>
            )}
            {decidable && (
              <Btn
                small
                variant="accent-outline"
                disabled={
                  busy ||
                  (hasProposal
                    ? editing && !edited.trim()
                    : !authored.trim())
                }
                onClick={() => void confirm()}
              >
                {hasProposal && editing && edited !== r.proposal
                  ? "Confirm edit"
                  : "Confirm"}
              </Btn>
            )}
          </>
        )}
      </div>
    </article>
  );
}
