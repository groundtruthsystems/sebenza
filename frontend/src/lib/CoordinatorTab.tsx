import { useCallback, useEffect, useState } from "react";
import { renderCommentMarkdown } from "./inboxMarkdown";
import NotesPanel from "./NotesPanel";
import type { InboxNote } from "./api";
import { ApiRefusal, solutionApi, type SolutionApi, type TranscriptEntry } from "./solutionApi";

const POLL_MS = 1500;

/**
 * The Coordinator tab of a solution item: the notes on the left (badged
 * `coordinator` while the coordinator's write stands), the chat on the right.
 * Replies come from the persisted transcript, so a reload shows them (TS-04).
 * Agent text renders through the sanitizing markdown path only.
 */
export default function CoordinatorTab({
  draftId,
  notes,
  listHash,
  coordinatorNotes,
  onNotesChanged,
  onStale,
  api = solutionApi,
  pollMs = POLL_MS,
}: {
  draftId: string;
  notes: InboxNote[];
  listHash: string;
  coordinatorNotes: Record<string, string>;
  onNotesChanged: (notes: InboxNote[], listHash: string) => void;
  onStale: () => void;
  api?: SolutionApi;
  pollMs?: number;
}) {
  const [entries, setEntries] = useState<TranscriptEntry[]>([]);
  const [running, setRunning] = useState(false);
  const [message, setMessage] = useState("");
  const [warning, setWarning] = useState("");
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    try {
      const t = await api.transcript(draftId);
      setEntries(t.entries);
      setRunning(t.job?.running ?? false);
    } catch (e) {
      setError((e as Error).message);
    }
  }, [api, draftId]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (!running) return;
    const t = setInterval(() => void load(), pollMs);
    return () => clearInterval(t);
  }, [running, load, pollMs]);

  const send = async (confirm: boolean) => {
    setError("");
    try {
      await api.say(draftId, message, confirm);
      setMessage("");
      setWarning("");
      setRunning(true);
      await load();
    } catch (e) {
      if (e instanceof ApiRefusal && e.code === "scan_warning") {
        setWarning(e.message);
        return;
      }
      setError((e as Error).message);
    }
  };

  const applied = new Set(
    entries.filter((e) => e.kind === "applied").map((e) => `${e.turnId}:${e.index}`),
  );

  const apply = async (turnId: string, index: number) => {
    setError("");
    try {
      await api.apply(draftId, turnId, index);
      onStale();
      await load();
    } catch (e) {
      setError((e as Error).message);
    }
  };

  return (
    <div className="flex-1 min-h-0 flex">
      <div className="w-1/2 min-w-0 border-r border-edge flex flex-col">
        <NotesPanel
          draftId={draftId}
          notes={notes}
          listHash={listHash}
          onchange={onNotesChanged}
          onstale={onStale}
          coordinatorNotes={coordinatorNotes}
        />
      </div>
      <section aria-label="Coordinator chat" className="w-1/2 min-w-0 flex flex-col">
        <ol className="flex-1 min-h-0 overflow-y-auto list-none m-0 p-3 flex flex-col gap-2">
          {entries
            .filter((e) => e.kind !== "applied")
            .map((e, i) => (
              <li
                key={`${e.turnId ?? "x"}-${e.kind}-${i}`}
                aria-label={`${e.kind} message`}
                className={`rounded-md border p-2 text-xs ${e.kind === "operator" ? "border-edge self-end bg-surface" : e.kind === "error" ? "border-danger" : "border-edge bg-sidebar"}`}
              >
                <div className="text-[10px] text-muted mb-1">
                  {e.kind === "operator" ? "You" : e.kind === "report" ? `Coordinator report (${e.agent ?? ""})` : e.kind === "review" ? `Review round ${e.round ?? ""}: ${e.verdict ?? ""}` : e.kind === "error" ? `Failed (${e.code ?? "error"})` : `Coordinator (${e.agent ?? ""}${e.model ? ` ${e.model}` : ""})`}
                </div>
                <div
                  className="inbox-comment md-body"
                  // Sanitized by renderCommentMarkdown (DOMPurify).
                  dangerouslySetInnerHTML={{ __html: renderCommentMarkdown(e.text ?? e.error ?? "") }}
                />
                {e.noteProposals?.map((p, idx) => (
                  <div key={idx} className="mt-2 border-t border-edge pt-2" aria-label={`Proposed note: ${p.title}`}>
                    <div className="text-[10px] text-muted">
                      Proposed {p.type} note: {p.title}
                    </div>
                    <pre className="whitespace-pre-wrap m-0 text-[11px]">{p.body}</pre>
                    <button
                      type="button"
                      className="mt-1 px-2 py-0.5 rounded border border-edge hover:border-accent"
                      disabled={applied.has(`${e.turnId}:${idx}`)}
                      onClick={() => void apply(e.turnId ?? "", idx)}
                    >
                      {applied.has(`${e.turnId}:${idx}`) ? "Applied" : "Apply"}
                    </button>
                  </div>
                ))}
              </li>
            ))}
          {running && <li className="text-xs text-accent">The coordinator is working…</li>}
        </ol>
        {warning && (
          <div role="alert" className="px-3 py-2 text-xs border-t border-warning bg-warning/10">
            {warning}{" "}
            <button type="button" className="underline" onClick={() => void send(true)}>
              Send anyway
            </button>
          </div>
        )}
        {error && (
          <p role="status" className="m-0 px-3 py-1 text-xs text-danger">
            {error}
          </p>
        )}
        <form
          className="border-t border-edge p-2 flex gap-2"
          onSubmit={(ev) => {
            ev.preventDefault();
            if (message.trim()) void send(false);
          }}
        >
          <textarea
            aria-label="Message the coordinator"
            className="flex-1 min-h-[3rem] rounded-md border border-edge bg-surface p-2 text-xs"
            value={message}
            onChange={(e) => setMessage(e.currentTarget.value)}
          />
          <button type="submit" className="px-3 rounded-md border border-edge hover:border-accent text-xs" disabled={running}>
            Send
          </button>
        </form>
      </section>
    </div>
  );
}
