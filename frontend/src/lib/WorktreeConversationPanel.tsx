import { useEffect, useRef, type ChangeEvent, type KeyboardEvent } from "react";
import AskUserQuestionCard from "./AskUserQuestionCard";
import { ASK_USER_QUESTION_TOOL_NAME, parseAskUserQuestion } from "./ask-user-question";
import type {
  AgentsUiConversationMessage,
  AgentsUiConversationState,
  AskUserQuestionInput,
  WorktreeInfo,
} from "./types";

interface Props {
  worktree: WorktreeInfo;
  /** Whether this worktree's agent declares the in-app chat capability. Passed in
   *  because the panel has no access to the agent registry; the caller reads
   *  `capabilities.inAppChat`. Never inferred from the agent id. */
  supportsChat: boolean;
  conversation: AgentsUiConversationState | null;
  conversationError: string | null;
  conversationLoading: boolean;
  composerText: string;
  isSending: boolean;
  onAttach: () => void;
  onComposerInput: (value: string) => void;
  onInterrupt: () => void;
  onRefresh: () => void;
  onSend: () => void;
  onAnswerQuestion: (text: string) => void;
}

type TranscriptItem =
  | { type: "message"; key: string; message: AgentsUiConversationMessage }
  | {
      type: "question";
      key: string;
      tool: AgentsUiConversationMessage;
      input: AskUserQuestionInput;
      answered: boolean;
    }
  | {
      type: "tool";
      key: string;
      tool: AgentsUiConversationMessage;
      result: AgentsUiConversationMessage | null;
    };

function messageKind(message: AgentsUiConversationMessage): NonNullable<AgentsUiConversationMessage["kind"]> {
  return message.kind ?? "text";
}

function isVisibleTranscriptMessage(message: AgentsUiConversationMessage): boolean {
  const kind = messageKind(message);
  if ((kind === "text" || kind === "thinking") && message.text.trim().length === 0) {
    return false;
  }
  return true;
}

function toolStatusLabel(message: AgentsUiConversationMessage): string {
  if (message.status === "inProgress") return "Running";
  if (message.status === "failed") return "Failed";
  return "Completed";
}

function exitCodeLabel(message: AgentsUiConversationMessage): string | null {
  return message.exitCode === null || message.exitCode === undefined ? null : `exit ${message.exitCode}`;
}

function formatDuration(durationMs: number | null | undefined): string | null {
  if (durationMs === null || durationMs === undefined) return null;
  if (durationMs < 1000) return `${durationMs}ms`;
  return `${(durationMs / 1000).toFixed(1)}s`;
}

function showToolInputFade(text: string): boolean {
  return text.split("\n").length > 2 || text.length > 160;
}

function buildTranscriptItems(messages: AgentsUiConversationMessage[]): TranscriptItem[] {
  const toolUseCallIds = new Set(
    messages
      .filter((message) => messageKind(message) === "toolUse" && message.toolCallId)
      .map((message) => message.toolCallId as string),
  );
  const resultByCallId = new Map<string, AgentsUiConversationMessage>();

  for (const message of messages) {
    if (messageKind(message) === "toolResult" && message.toolCallId && !resultByCallId.has(message.toolCallId)) {
      resultByCallId.set(message.toolCallId, message);
    }
  }

  return messages.flatMap((message): TranscriptItem[] => {
    const kind = messageKind(message);
    if (kind === "toolUse") {
      if (message.toolName === ASK_USER_QUESTION_TOOL_NAME) {
        const input = parseAskUserQuestion(message.text);
        if (input) {
          const answered = messages.some((other) =>
            other.role === "user" && messageKind(other) === "text" && other.order > message.order
          );
          return [{ type: "question", key: message.id, tool: message, input, answered }];
        }
      }
      return [{
        type: "tool",
        key: message.id,
        tool: message,
        result: message.toolCallId ? resultByCallId.get(message.toolCallId) ?? null : null,
      }];
    }

    if (kind === "toolResult" && message.toolCallId && toolUseCallIds.has(message.toolCallId)) {
      return [];
    }

    return [{ type: "message", key: message.id, message }];
  });
}

function SendIcon() {
  return (
    <svg
      aria-hidden="true"
      xmlns="http://www.w3.org/2000/svg"
      width="22"
      height="22"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="m9 10-4 4 4 4" />
      <path d="M5 14h11a4 4 0 0 0 4-4V6" />
    </svg>
  );
}

function StopIcon() {
  return (
    <svg
      aria-hidden="true"
      xmlns="http://www.w3.org/2000/svg"
      width="22"
      height="22"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <rect x="7" y="7" width="10" height="10" rx="1.5" />
    </svg>
  );
}

export default function WorktreeConversationPanel({
  worktree,
  supportsChat,
  conversation,
  conversationError,
  conversationLoading,
  composerText,
  isSending,
  onAttach,
  onComposerInput,
  onInterrupt,
  onRefresh,
  onSend,
  onAnswerQuestion,
}: Props) {
  // Fall back to the agent's own id, never to a guessed builtin name: labelling an
  // unknown agent "Codex" is worse than showing its id.
  const agentLabel = worktree.agentLabel ?? worktree.agentName ?? "Agent";
  const supportsAgentChat = supportsChat;
  const chatAvailable = supportsAgentChat && worktree.mux === "✓";
  const showInterrupt = chatAvailable && (conversation?.running ?? false);
  const showComposerInterrupt = showInterrupt && !conversationError;
  const showProcessingIndicator = isSending || showComposerInterrupt;
  const transcriptItems = buildTranscriptItems((conversation?.messages ?? []).filter(isVisibleTranscriptMessage));
  const canSend =
    chatAvailable
    && conversation !== null
    && !conversationLoading
    && composerText.trim().length > 0
    && !isSending
    && !(conversation?.running ?? false);

  const transcriptViewportRef = useRef<HTMLDivElement>(null);

  const conversationId = conversation?.conversationId ?? null;
  const messageCount = conversation?.messages.length ?? 0;
  const lastMessageId = messageCount > 0 ? conversation?.messages[messageCount - 1]?.id ?? null : null;
  const lastMessageTextLength = messageCount > 0 ? conversation?.messages[messageCount - 1]?.text.length ?? 0 : 0;

  useEffect(() => {
    if (!conversationId) return;
    const viewport = transcriptViewportRef.current;
    if (!viewport) return;
    viewport.scrollTo({
      top: viewport.scrollHeight,
      behavior: "auto",
    });
  }, [conversationId, messageCount, lastMessageId, lastMessageTextLength]);

  function handleComposerInput(event: ChangeEvent<HTMLTextAreaElement>): void {
    onComposerInput(event.currentTarget.value);
  }

  function handleComposerKeydown(event: KeyboardEvent<HTMLTextAreaElement>): void {
    if (event.key !== "Enter" || event.shiftKey) return;
    event.preventDefault();
    if (canSend) {
      onSend();
    }
  }

  function interruptButton() {
    return (
      <button
        type="button"
        className="rounded-md border border-danger px-3 py-1.5 text-xs font-medium text-danger hover:bg-danger/10"
        onClick={onInterrupt}
      >
        Interrupt
      </button>
    );
  }

  function processingIndicator() {
    return (
      <div className="flex max-w-[88%] items-center gap-2 self-start rounded-md border border-edge bg-topbar px-3 py-2 text-xs text-muted">
        <span className="spinner"></span>
        {agentLabel} is processing
      </div>
    );
  }

  function renderItem(item: TranscriptItem) {
    if (item.type === "message") {
      const message = item.message;
      if (messageKind(message) === "thinking") {
        return (
          <div
            key={item.key}
            className="self-start max-w-[88%] min-w-0 rounded-md border border-edge bg-topbar/60 px-3 py-2 text-xs text-muted"
          >
            <div className="mb-1 uppercase tracking-[0.12em]">Thinking</div>
            <div className="whitespace-pre-wrap break-words text-primary/85">{message.text}</div>
            {message.status === "inProgress" && (
              <div className="mt-2 uppercase tracking-[0.12em]">working</div>
            )}
          </div>
        );
      }
      return (
        <div
          key={item.key}
          className={`max-w-[88%] min-w-0 rounded-2xl px-4 py-3 text-sm ${
            message.role === "user"
              ? "self-end bg-accent text-white"
              : "self-start border border-edge bg-topbar text-primary"
          }`}
        >
          <div className="whitespace-pre-wrap break-words">{message.text}</div>
        </div>
      );
    }

    if (item.type === "question") {
      return (
        <AskUserQuestionCard
          key={item.key}
          input={item.input}
          disabled={item.answered}
          onSubmit={onAnswerQuestion}
        />
      );
    }

    const message = item.tool;
    const result = item.result;
    return (
      <details
        key={item.key}
        className={`group self-start max-w-[94%] min-w-0 rounded-md border text-xs ${
          message.status === "failed" || result?.status === "failed"
            ? "border-danger/30 bg-danger/10 text-primary"
            : "border-edge/70 bg-topbar/40 text-primary"
        }`}
        open={message.status === "failed" || result?.status === "failed"}
      >
        <summary className="cursor-pointer px-3 py-2 text-muted">
          <div className="inline-flex flex-wrap items-center gap-x-2 gap-y-1 text-[10px] uppercase tracking-[0.12em]">
            <span>{toolStatusLabel(message)} {message.toolName ?? "tool"}</span>
            {exitCodeLabel(message) && <span>{exitCodeLabel(message)}</span>}
            {formatDuration(message.durationMs) && <span>{formatDuration(message.durationMs)}</span>}
          </div>
          <div className="group-open:hidden relative mt-1 max-h-[2.05rem] overflow-hidden">
            <pre className="whitespace-pre-wrap break-words font-mono text-[11px] leading-[1.35] text-primary/75">{message.text}</pre>
            {showToolInputFade(message.text) && (
              <div className="pointer-events-none absolute inset-x-0 bottom-0 h-5 bg-gradient-to-b from-transparent to-topbar"></div>
            )}
          </div>
        </summary>

        <div className="border-t border-edge/60 px-3 py-2">
          <pre className="whitespace-pre-wrap break-words font-mono text-[11px] leading-relaxed text-primary">{message.text}</pre>
          {message.status === "inProgress" && (
            <div className="mt-2 text-[10px] uppercase tracking-[0.12em] text-muted">running</div>
          )}
          {result && (
            <div className="mt-3 border-t border-edge/60 pt-2">
              <div className="mb-1 text-[10px] uppercase tracking-[0.12em] text-muted">Output</div>
              <pre className="max-h-[18rem] overflow-auto whitespace-pre-wrap break-words font-mono text-[11px] leading-relaxed text-primary">{result.text}</pre>
            </div>
          )}
        </div>
      </details>
    );
  }

  if (!supportsAgentChat) {
    return (
      <div className="flex flex-1 items-center justify-center px-6 text-center text-sm text-muted">
        Chat is not available for this worktree yet.
      </div>
    );
  }

  if (!chatAvailable) {
    return (
      <div className="flex flex-1 items-center justify-center px-6 text-center text-sm text-muted">
        Open this worktree first to use chat.
      </div>
    );
  }

  return (
    <section className="flex min-h-0 flex-1 flex-col overflow-hidden bg-surface">
      {conversationError && (
        <div className="mx-4 mt-4 rounded-md border border-danger/40 bg-danger/10 px-4 py-3 text-sm text-primary">
          <div>{conversationError}</div>
          <div className="mt-3 flex items-center gap-2">
            <button
              type="button"
              className="rounded-md border border-edge bg-surface px-3 py-1.5 text-xs font-medium text-primary hover:bg-hover"
              onClick={conversation ? onRefresh : onAttach}
              disabled={conversationLoading || isSending}
            >
              {conversation ? "Reconnect" : "Attach"}
            </button>
            {showInterrupt && interruptButton()}
          </div>
        </div>
      )}

      <div className="flex min-h-0 flex-1 flex-col px-4 pt-4">
        <div
          ref={transcriptViewportRef}
          className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto overflow-x-hidden pb-4 pr-1"
        >
          {conversationLoading && !conversation ? (
            <div className="rounded-md border border-edge bg-topbar px-4 py-5 text-sm text-muted">
              Connecting to the {agentLabel} session...
            </div>
          ) : !conversation ? (
            <div className="rounded-md border border-edge bg-topbar px-4 py-5 text-sm text-muted">
              No messages yet. Send the first prompt to start this chat.
            </div>
          ) : conversation.messages.length === 0 ? (
            showProcessingIndicator ? (
              processingIndicator()
            ) : (
              <div className="rounded-md border border-edge bg-topbar px-4 py-5 text-sm text-muted">
                No messages yet. Send the first prompt to start this chat.
              </div>
            )
          ) : (
            <>
              {transcriptItems.map((item) => renderItem(item))}
              {showProcessingIndicator && processingIndicator()}
            </>
          )}
        </div>
      </div>

      <div
        className="border-t border-edge bg-topbar px-4 pb-4 pt-4"
        style={{ paddingBottom: "max(1rem, env(safe-area-inset-bottom, 0px))" }}
      >
        <div className="relative">
          <textarea
            id="conversation-composer"
            aria-label="Message"
            className="block min-h-[5.25rem] w-full max-w-full resize-none rounded-2xl border border-edge bg-surface py-3 pl-4 pr-14 text-sm text-primary outline-none transition placeholder:text-muted/70 focus:border-accent"
            placeholder="ask anything"
            value={composerText}
            onChange={handleComposerInput}
            onKeyDown={handleComposerKeydown}
            disabled={isSending}
          ></textarea>

          {showComposerInterrupt ? (
            <button
              type="button"
              aria-label="Interrupt"
              className="absolute right-3 top-1/2 flex size-8 -translate-y-1/2 items-center justify-center rounded-md text-muted transition hover:bg-hover hover:text-primary"
              onClick={onInterrupt}
            >
              <StopIcon />
            </button>
          ) : (
            <button
              type="button"
              aria-label="Send"
              className="absolute right-3 top-1/2 flex size-8 -translate-y-1/2 items-center justify-center rounded-md text-muted transition enabled:hover:bg-hover enabled:hover:text-primary disabled:cursor-not-allowed disabled:opacity-45"
              onClick={onSend}
              disabled={!canSend}
            >
              <SendIcon />
            </button>
          )}
        </div>
      </div>
    </section>
  );
}
