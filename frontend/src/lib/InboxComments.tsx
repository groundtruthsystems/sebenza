import { useState } from "react";
import type { InboxComment, InboxCommentGroups } from "./api-contract";
import { authorLabel, worktreeLabel, type WorktreeRef } from "./inbox-collab";
import { renderCommentMarkdown } from "./inboxMarkdown";
import Btn from "./Btn";
import PhiNotice from "./PhiNotice";

const authorTone: Record<InboxComment["author"], string> = {
  operator: "border-accent text-accent",
  worktree_agent: "border-edge text-primary",
  system_agent: "border-success text-success",
};

const keyOf = (w: WorktreeRef) => `${w.project}\u0000${w.branch}`;

export function WarningBadges({ warnings }: { warnings: string[] }) {
  if (warnings.length === 0) return null;
  return (
    <>
      {warnings.map((w) => (
        <span
          key={w}
          className="shrink-0 px-1 rounded border border-warning text-warning text-[10px]"
          title="Scan hit: stored unchanged. Redact it if it should not be here."
        >
          {w}
        </span>
      ))}
    </>
  );
}

function CommentRow({
  comment,
  onredact,
}: {
  comment: InboxComment;
  onredact: (eventId: string) => Promise<void>;
}) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const redact = async () => {
    setBusy(true);
    setError("");
    try {
      await onredact(comment.eventId);
      setConfirming(false);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <li className="py-2 border-b border-edge last:border-b-0 flex flex-col gap-1">
      <div className="flex flex-wrap items-center gap-1.5 text-[10px] text-muted">
        <span className={`px-1 rounded border ${authorTone[comment.author]}`}>
          {authorLabel(comment.author)}
        </span>
        {comment.caller && (
          <span title="Self-declared by the caller; not authenticated">
            claims {comment.caller}
          </span>
        )}
        {comment.kind === "advice" ? (
          <span
            className="px-1 rounded border border-edge"
            title="Advice is for you; it is never pasted into a worktree"
          >
            advice · not delivered
          </span>
        ) : (
          comment.kind !== "note" && (
            <span className="px-1 rounded border border-edge">{comment.kind}</span>
          )
        )}
        <WarningBadges warnings={comment.warnings} />
        <span className="ml-auto">{comment.ts.replace("T", " ").slice(0, 16)}</span>
        {!comment.redacted && !confirming && (
          <button
            type="button"
            className="text-muted hover:text-danger cursor-pointer"
            onClick={() => setConfirming(true)}
          >
            Redact
          </button>
        )}
      </div>
      {comment.title && (
        <p className="m-0 text-xs font-semibold">{comment.title}</p>
      )}
      {comment.redacted ? (
        <p className="m-0 text-xs italic text-muted">Redacted</p>
      ) : (
        <div
          className="inbox-comment md-body text-xs"
          dangerouslySetInnerHTML={{ __html: renderCommentMarkdown(comment.body) }}
        />
      )}
      {confirming && (
        <div className="flex items-center gap-2 text-[11px]">
          <span className="text-muted">Mask this body everywhere it is read?</span>
          <Btn
            small
            variant="danger-outline"
            disabled={busy}
            onClick={() => void redact()}
          >
            Confirm redact
          </Btn>
          <Btn small disabled={busy} onClick={() => setConfirming(false)}>
            Cancel
          </Btn>
        </div>
      )}
      {error && <p className="m-0 text-[11px] text-danger">{error}</p>}
    </li>
  );
}

function Thread({
  label,
  title,
  comments,
  onredact,
}: {
  label: string;
  title?: string;
  comments: InboxComment[];
  onredact: (eventId: string) => Promise<void>;
}) {
  return (
    <section aria-label={`${label} thread`} className="mb-3">
      <h3
        className="m-0 mb-1 text-[11px] font-semibold uppercase tracking-wide text-muted"
        title={title}
      >
        {label}
      </h3>
      {comments.length === 0 ? (
        <p className="m-0 text-[11px] text-muted">No comments.</p>
      ) : (
        <ul className="list-none m-0 p-0">
          {comments.map((c) => (
            <CommentRow key={c.eventId} comment={c} onredact={onredact} />
          ))}
        </ul>
      )}
    </section>
  );
}

/**
 * The item's threads: overall first, then one per converted worktree, and a
 * composer that posts as the operator to either.
 *
 * Bodies are untrusted — worktree agents and the system agent write them —
 * so they render through the inbox sanitiser only.
 */
export default function InboxComments({
  groups,
  worktrees,
  onpost,
  onredact,
}: {
  groups: InboxCommentGroups;
  /** Worktrees the composer may post to, beyond those with comments. */
  worktrees: WorktreeRef[];
  onpost: (body: string, worktree?: WorktreeRef) => Promise<InboxComment>;
  onredact: (eventId: string) => Promise<void>;
}) {
  const [text, setText] = useState("");
  const [target, setTarget] = useState("");
  const [posting, setPosting] = useState(false);
  const [error, setError] = useState("");
  const [flagged, setFlagged] = useState<string[]>([]);

  const targets = new Map<string, WorktreeRef>();
  for (const w of [...groups.worktrees, ...worktrees]) {
    targets.set(keyOf(w), { project: w.project, branch: w.branch });
  }

  const post = async () => {
    const body = text.trim();
    if (!body) return;
    setPosting(true);
    setError("");
    setFlagged([]);
    try {
      const posted = await onpost(body, target ? targets.get(target) : undefined);
      setText("");
      setFlagged(posted.warnings);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setPosting(false);
    }
  };

  return (
    <div className="flex flex-col min-h-0 h-full">
      <div className="flex-1 min-h-0 overflow-y-auto p-3">
        <Thread label="Overall" comments={groups.overall} onredact={onredact} />
        {groups.worktrees.map((w) => (
          <Thread
            key={keyOf(w)}
            label={worktreeLabel(w)}
            title={`${w.project} · ${w.branch}`}
            comments={w.comments}
            onredact={onredact}
          />
        ))}
      </div>
      <form
        className="border-t border-edge p-3 flex flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          void post();
        }}
      >
        <PhiNotice />
        <select
          aria-label="Comment thread"
          className="h-7 rounded-md border border-edge bg-surface px-2 text-xs text-primary focus:outline-none focus:border-accent"
          value={target}
          onChange={(e) => setTarget(e.currentTarget.value)}
          disabled={posting}
        >
          <option value="">Overall</option>
          {[...targets.entries()].map(([key, w]) => (
            <option key={key} value={key}>
              {worktreeLabel(w)}
            </option>
          ))}
        </select>
        <textarea
          aria-label="Comment"
          rows={3}
          placeholder="Comment as operator (markdown)"
          className="w-full rounded-md border border-edge bg-surface px-2 py-1.5 text-xs text-primary placeholder:text-muted focus:outline-none focus:border-accent resize-y"
          value={text}
          onChange={(e) => setText(e.currentTarget.value)}
          disabled={posting}
        />
        {error && <p className="m-0 text-[11px] text-danger">{error}</p>}
        {flagged.length > 0 && (
          <p className="m-0 text-[11px] text-warning" role="status">
            Posted, but the scan flagged: {flagged.join(", ")}. It is stored
            unchanged; redact it if it should not be there.
          </p>
        )}
        <div className="flex justify-end">
          <Btn
            type="submit"
            small
            variant="accent-outline"
            disabled={posting || !text.trim()}
          >
            Post comment
          </Btn>
        </div>
      </form>
    </div>
  );
}
