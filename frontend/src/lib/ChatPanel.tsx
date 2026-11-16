// The handover chat panel (FR-19–21). Read-only advice about one handover:
// every reply is rendered as a plain text node (never HTML, markdown, links
// or images), the thread says it is not part of the review record, and Esc
// closes it before full screen (FR-16).
import { useState } from "react";
import { useEscLayer } from "./escStack";
import { focusRing } from "./ReviewControls";
import { ApiRefusal, type ChatThread, type SolutionApi } from "./solutionApi";

export default function ChatPanel({
  draftId,
  slug,
  api,
  thread,
  onthread,
  onclose,
}: {
  draftId: string;
  slug: string;
  api: SolutionApi;
  thread: ChatThread;
  onthread: (t: ChatThread) => void;
  onclose: () => void;
}) {
  const [question, setQuestion] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEscLayer("chat", true, onclose);

  const send = async () => {
    const text = question.trim();
    if (!text) return;
    setBusy(true);
    setError("");
    try {
      const { chat } = await api.ask(draftId, slug, text);
      onthread(chat);
      setQuestion("");
    } catch (e) {
      setError(e instanceof ApiRefusal ? e.message : (e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <aside aria-label={`Chat about ${slug}`} className="basis-full flex flex-col gap-2 border border-edge rounded-md p-2 bg-surface">
      <div className="flex items-center gap-2">
        <span className="font-semibold">Chat about {slug}</span>
        <span className="text-[11px] text-muted">{thread.notice}</span>
        <button type="button" className={`ml-auto px-2 py-0.5 rounded border border-edge ${focusRing}`} onClick={onclose}>
          Close chat
        </button>
      </div>
      <ol aria-label="Chat messages" className="m-0 p-0 list-none flex flex-col gap-1 max-h-[20rem] overflow-y-auto">
        {thread.messages.map((m) => (
          <li key={m.messageId} className={m.role === "reviewer" ? "self-end" : "self-start"}>
            <span className="text-[10px] text-muted">{m.role === "agent" ? "agent" : m.author}</span>
            {m.role === "aborted" ? (
              <p className="m-0 text-warning">This turn was stopped by the safety guard; its output was discarded.</p>
            ) : (
              // A text node: React escapes it, so model output never becomes markup.
              <p className="m-0 whitespace-pre-wrap break-words">{m.text}</p>
            )}
          </li>
        ))}
      </ol>
      {error && (
        <p role="alert" className="m-0 text-danger">
          {error}
        </p>
      )}
      <textarea
        aria-label="Question"
        placeholder="Ask about this handover"
        className="w-full min-h-[3rem] rounded-md border border-edge bg-surface p-2"
        value={question}
        disabled={busy}
        onChange={(e) => setQuestion(e.currentTarget.value)}
      />
      <button
        type="button"
        className={`self-end px-2 py-0.5 rounded border border-accent text-accent ${focusRing}`}
        disabled={busy || !question.trim()}
        onClick={() => void send()}
      >
        {busy ? "Asking…" : "Send"}
      </button>
    </aside>
  );
}
