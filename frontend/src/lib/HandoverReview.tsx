// One project's handover under review, in the normal card or full screen
// (FR-11, FR-15, FR-16). Full screen is a mode of the same review: both modes
// read and write the same review store, so the instruction edit, the reject
// form and the reading position carry across. Leaving full screen is a
// visible button or Esc (after any open dialog).
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useEscLayer } from "./escStack";
import { renderDraftMarkdown } from "./inboxMarkdown";
import ReviewControls, { focusRing } from "./ReviewControls";
import { useHandoverReview } from "./reviewStore";
import { ApiRefusal, type HandoverCard, type SolutionApi } from "./solutionApi";

function message(e: unknown): string {
  if (e instanceof ApiRefusal) return e.code ? `${e.message} (${e.code})` : e.message;
  return (e as Error).message;
}

export default function HandoverReview({
  draftId,
  handover: h,
  api,
  designApprovalValid,
  onchanged,
}: {
  draftId: string;
  handover: HandoverCard;
  api: SolutionApi;
  designApprovalValid: boolean;
  /** Re-read the solution after an action. */
  onchanged: () => void | Promise<void>;
}) {
  const [review, patch] = useHandoverReview(draftId, h.slug);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const full = review.fullscreen;
  useEscLayer("fullscreen", full, () => patch({ fullscreen: false }));

  /** Load the instruction. A pending edit is kept unless `fresh` (after a
   *  save, the saved text is the new baseline). */
  const load = async (fresh = false) => {
    try {
      const d = await api.doc(draftId, `handover/${h.slug}.md`);
      patch({
        doc: { content: d.content, hash: d.hash },
        draft: fresh ? d.content : (review.draft ?? d.content),
      });
    } catch (e) {
      setError(message(e));
    }
  };

  useEffect(() => {
    if (full && !review.doc) void load();
    // Load once on entering full screen; `load` reads the latest state.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [full]);

  const act = async (fn: () => Promise<unknown>, after?: () => void) => {
    setBusy(true);
    setError("");
    try {
      await fn();
      after?.();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
    await onchanged();
  };

  const controls = (
    <ReviewControls
      draftId={draftId}
      handover={h}
      designApprovalValid={designApprovalValid}
      busy={busy}
      onapprove={() => void act(() => api.approveHandover(draftId, h.slug, h.instructionHash))}
      ondefer={() => void act(() => api.deferHandover(draftId, h.slug))}
      onreject={(reason, comment) =>
        void act(
          () => api.rejectHandover(draftId, h.slug, reason, comment),
          () => patch({ rejectOpen: false, comment: "" }),
        )
      }
    />
  );

  const fullButton = (
    <button
      type="button"
      className={`px-2 py-0.5 rounded border border-edge ${focusRing}`}
      onClick={() => patch({ fullscreen: !full })}
    >
      {full ? "Exit full screen" : "Full screen"}
    </button>
  );

  const status = (
    <>
      <span className="font-semibold">{h.slug}</span>
      <span>{h.status}</span>
      {h.copyStale && <span className="text-warning">worktree copy is stale</span>}
      {h.rejection && (
        <span className="text-danger">
          Rejected ({h.rejection.reason}): {h.rejection.comment}
        </span>
      )}
    </>
  );

  const body = review.doc && (
    <Reader
      draftId={draftId}
      slug={h.slug}
      tall={full}
      busy={busy}
      onsave={(content, hash) =>
        void act(async () => {
          await api.editHandover(draftId, h.slug, content, hash);
          await load(true);
        })
      }
    />
  );

  return (
    <>
      <div aria-label={`Handover ${h.slug}`} className="border border-edge rounded-md p-2 flex flex-col gap-2">
        <div className="flex flex-wrap items-center gap-2">
          {status}
          <div className="ml-auto flex flex-wrap gap-2">
            {!full && (
              <>
                <button
                  type="button"
                  className={`px-2 py-0.5 rounded border border-edge ${focusRing}`}
                  onClick={() => void load()}
                >
                  Review
                </button>
                {fullButton}
                {controls}
              </>
            )}
            {full && <span className="text-muted">Open full screen</span>}
          </div>
        </div>
        {!full && error && <p role="alert" className="m-0 text-danger">{error}</p>}
        {!full && body}
      </div>
      {full && (
        <div
          role="region"
          aria-label={`Full-screen handover ${h.slug}`}
          className="fixed inset-0 z-50 bg-surface text-primary flex flex-col gap-3 p-4 overflow-hidden"
        >
          <div className="flex flex-wrap items-center gap-2">
            {status}
            <div className="ml-auto flex flex-wrap gap-2">
              {fullButton}
              {controls}
            </div>
          </div>
          {error && <p role="alert" className="m-0 text-danger">{error}</p>}
          <div className="flex-1 min-h-0 flex flex-col">{body}</div>
        </div>
      )}
    </>
  );
}

/** The instruction, rendered and editable, in a scroller whose position is
 *  kept in the review store (restored on every mount, i.e. on mode change). */
function Reader({
  draftId,
  slug,
  tall,
  busy,
  onsave,
}: {
  draftId: string;
  slug: string;
  tall: boolean;
  busy: boolean;
  onsave: (content: string, hash: string) => void;
}) {
  const [review, patch] = useHandoverReview(draftId, slug);
  const ref = useRef<HTMLDivElement>(null);
  const [html, setHtml] = useState("");
  const content = review.draft ?? review.doc?.content ?? "";

  useLayoutEffect(() => {
    if (ref.current) ref.current.scrollTop = review.scrollTop;
    // Restore once per mount; later scrolls write the store.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    let live = true;
    void renderDraftMarkdown(review.doc?.content ?? "").then((x) => live && setHtml(x));
    return () => {
      live = false;
    };
  }, [review.doc?.content]);

  if (!review.doc) return null;
  return (
    <div
      ref={ref}
      data-testid={`handover-reader-${slug}`}
      onScroll={(e) => patch({ scrollTop: e.currentTarget.scrollTop })}
      className={`flex flex-col gap-2 overflow-y-auto ${tall ? "flex-1 min-h-0" : "max-h-[24rem]"}`}
    >
      <div
        className="md-body text-sm"
        // Sanitized by renderDraftMarkdown (DOMPurify + strict mermaid).
        dangerouslySetInnerHTML={{ __html: html }}
      />
      <textarea
        aria-label={`Instruction for ${slug}`}
        className={`w-full rounded-md border border-edge bg-surface p-2 ${tall ? "min-h-[16rem]" : "min-h-[8rem]"}`}
        value={content}
        onChange={(e) => patch({ draft: e.currentTarget.value })}
      />
      <button
        type="button"
        className={`self-start px-2 py-0.5 rounded border border-edge ${focusRing}`}
        disabled={busy}
        onClick={() => onsave(content, review.doc!.hash)}
      >
        Save instruction
      </button>
    </div>
  );
}
