// The handover review controls, shared by the normal and the full-screen view
// (FR-11): Approve, Defer and Reject. Stateless itself: the reject form lives
// in the review store, so it survives a mode change. Reject opens as a dialog
// layer, closed first by Esc (FR-16).
import { useEscLayer } from "./escStack";
import { useHandoverReview, type RejectReason } from "./reviewStore";
import type { HandoverCard } from "./solutionApi";

/** Native buttons with an accessible name and a visible focus ring (NFR-04). */
export const focusRing =
  "focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent";

export default function ReviewControls({
  draftId,
  handover: h,
  designApprovalValid,
  busy,
  onapprove,
  ondefer,
  onreject,
}: {
  draftId: string;
  handover: HandoverCard;
  designApprovalValid: boolean;
  busy: boolean;
  onapprove: () => void;
  ondefer: () => void;
  onreject: (reason: RejectReason, comment: string) => void;
}) {
  const [review, patch] = useHandoverReview(draftId, h.slug);
  const awaiting = h.status === "drafted" || h.status === "deferred";
  const decidable = h.status !== "converted" && h.status !== "rejected";
  useEscLayer("dialog", review.rejectOpen, () => patch({ rejectOpen: false }));
  const btn = `px-2 py-0.5 rounded border ${focusRing}`;

  return (
    <>
      {decidable && (
        <>
          <button
            type="button"
            className={`${btn} border-accent text-accent`}
            disabled={!designApprovalValid || busy}
            onClick={onapprove}
          >
            Approve and create worktree
          </button>
          <button type="button" className={`${btn} border-edge`} disabled={busy} onClick={ondefer}>
            Defer
          </button>
        </>
      )}
      {awaiting && (
        <button
          type="button"
          className={`${btn} border-danger text-danger`}
          aria-expanded={review.rejectOpen}
          disabled={busy}
          onClick={() => patch({ rejectOpen: true })}
        >
          Reject
        </button>
      )}
      {awaiting && review.rejectOpen && (
        <div
          role="dialog"
          aria-label={`Reject handover ${h.slug}`}
          className="basis-full flex flex-col gap-2 border border-danger rounded-md p-2 bg-surface"
        >
          <fieldset className="flex gap-3 border-0 p-0 m-0">
            <legend className="text-[11px] text-muted mb-1">Reason</legend>
            {(
              [
                ["wrong_project", "Wrong project"],
                ["other", "Other"],
              ] as [RejectReason, string][]
            ).map(([value, label]) => (
              <label key={value} className="flex items-center gap-1">
                <input
                  type="radio"
                  name={`reject-reason-${h.slug}`}
                  value={value}
                  checked={review.reason === value}
                  onChange={() => patch({ reason: value })}
                />
                {label}
              </label>
            ))}
          </fieldset>
          {review.reason === "wrong_project" && (
            <p className="m-0 text-[11px] text-muted">
              The solution design goes back to revising, naming this project. Nothing is removed automatically.
            </p>
          )}
          <textarea
            aria-label="Comment"
            placeholder="Why is this handover rejected? (required)"
            className="w-full min-h-[4rem] rounded-md border border-edge bg-surface p-2"
            value={review.comment}
            onChange={(e) => patch({ comment: e.currentTarget.value })}
          />
          <div className="flex gap-2 justify-end">
            <button type="button" className={`${btn} border-edge`} onClick={() => patch({ rejectOpen: false })}>
              Cancel
            </button>
            <button
              type="button"
              className={`${btn} border-danger text-danger`}
              disabled={busy || !review.comment.trim()}
              onClick={() => onreject(review.reason, review.comment.trim())}
            >
              Confirm reject
            </button>
          </div>
        </div>
      )}
    </>
  );
}
