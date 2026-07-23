import { useEffect, useMemo, useState } from "react";
import type { PrEntry, PrComment } from "./types";
import { api } from "./api";
import { normalizeTextForPrompt } from "./promptUtils";
import { prLabel, errorMessage } from "./utils";
import { useStore } from "../store";
import BaseDialog from "./BaseDialog";
import Btn from "./Btn";
import LinkBtn from "./LinkBtn";

export default function CommentReviewDialog({
  pr,
  branch,
  onclose,
  onsendsuccess,
}: {
  pr: PrEntry;
  branch: string;
  onclose: () => void;
  onsendsuccess: () => void;
}) {
  const [sending, setSending] = useState(false);
  const [sendError, setSendError] = useState("");
  const success = useStore((s) => s.success);

  const [selected, setSelected] = useState<Set<number>>(new Set());

  useEffect(() => {
    const len = pr.comments.length;
    const next = new Set<number>();
    for (let i = 0; i < len; i++) next.add(i);
    setSelected(next);
  }, [pr.comments.length]);

  const label = useMemo(() => prLabel(pr), [pr]);
  const sortedComments = useMemo(
    () =>
      pr.comments
        .map((comment, i) => ({ comment, originalIndex: i }))
        .sort((a, b) => b.comment.createdAt.localeCompare(a.comment.createdAt)),
    [pr.comments],
  );
  const allSelected = selected.size === pr.comments.length;
  const noneSelected = selected.size === 0;

  function toggleAll(): void {
    if (allSelected) {
      setSelected(new Set());
    } else {
      const next = new Set<number>();
      for (let i = 0; i < pr.comments.length; i++) next.add(i);
      setSelected(next);
    }
  }

  function toggleOne(index: number): void {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(index)) {
        next.delete(index);
      } else {
        next.add(index);
      }
      return next;
    });
  }

  function formatComment(c: PrComment, idx: number): string {
    if (c.type === "inline") {
      const loc = c.line ? `${c.path}:${c.line}` : c.path;
      const hunk = c.diffHunk ? `\n\`\`\`diff\n${c.diffHunk}\n\`\`\`\n` : "\n";
      return `[${idx}] @${c.author} (${c.createdAt.slice(0, 10)}) on ${loc}:${hunk}${c.body}`;
    }
    return `[${idx}] @${c.author} (${c.createdAt.slice(0, 10)}):\n${c.body}`;
  }

  async function handleSend(): Promise<void> {
    if (!branch || noneSelected) return;
    setSending(true);
    setSendError("");
    const preamble =
      [
        "Review these comments and elaborate a plan to address the ones you find relevant.",
        `PR: ${label}`,
        "",
        "Comments:",
      ].join("\n") + "\n";
    const content = pr.comments
      .filter((_, i) => selected.has(i))
      .map((c, i) => formatComment(c, i + 1))
      .join("\n\n");
    try {
      await api.sendWorktreePrompt({
        params: { name: branch },
        body: {
          text: normalizeTextForPrompt(content, 20000),
          preamble,
        },
      });
      success(`Sent ${selected.size} comment${selected.size === 1 ? "" : "s"} to agent`);
      onsendsuccess();
    } catch (err) {
      setSendError(errorMessage(err));
    } finally {
      setSending(false);
    }
  }

  return (
    <BaseDialog onclose={onclose} wide>
      <h2 className="text-base mb-4">PR Comments &mdash; {label}</h2>

      <div className="flex items-center justify-between mb-3">
        <LinkBtn onClick={toggleAll}>{allSelected ? "Deselect all" : "Select all"}</LinkBtn>
        <span className="text-[11px] text-muted">
          {selected.size} of {pr.comments.length} selected
        </span>
      </div>

      <ul className="list-none p-0 m-0 flex flex-col gap-2 mb-4 max-h-[400px] overflow-y-auto">
        {sortedComments.map(({ comment, originalIndex }) => (
          <li key={originalIndex} className="rounded-md border border-edge bg-surface p-3">
            <label className="flex items-start gap-2 cursor-pointer">
              <input
                type="checkbox"
                checked={selected.has(originalIndex)}
                onChange={() => toggleOne(originalIndex)}
                className="mt-0.5 accent-accent"
              />
              <div className="flex-1 min-w-0">
                {comment.type === "inline" && (
                  <div className="text-[10px] font-mono text-accent mb-1 truncate" title={comment.path}>
                    {comment.path}
                    {comment.line ? `:${comment.line}` : ""}
                    {comment.isReply && <span className="text-muted ml-1">(reply)</span>}
                  </div>
                )}
                <div className="text-[12px] text-muted mb-1">
                  <span className="font-medium text-primary">@{comment.author}</span>
                  &middot; {comment.createdAt.slice(0, 10)}
                  {comment.type === "inline" && <span className="text-accent/60 ml-1">review</span>}
                </div>
                <pre className="text-[11px] font-mono whitespace-pre-wrap m-0 text-primary/80">
                  {comment.body}
                </pre>
              </div>
            </label>
          </li>
        ))}
      </ul>

      {sendError && <div className="text-[12px] text-danger mb-3">{sendError}</div>}

      <div className="flex justify-end gap-2">
        <Btn type="button" onClick={onclose}>
          Cancel
        </Btn>
        <Btn variant="cta" small disabled={noneSelected || sending} onClick={handleSend}>
          {sending ? "Sending..." : `Send ${selected.size} to agent`}
        </Btn>
      </div>
    </BaseDialog>
  );
}
