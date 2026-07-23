import { useEffect, useRef, useState } from "react";
import {
  attachWorktreeConversation,
  connectWorktreeConversationStream,
  fetchWorktreeConversationHistory,
  interruptWorktreeConversation,
  sendWorktreeConversationMessage,
} from "./api";
import {
  applyConversationMessageDelta,
  applyConversationMessageUpsert,
  applyConversationStatus,
  buildConversationProgressSignature,
  markConversationTurnStarted,
  mergeConversationSnapshot,
} from "./worktree-conversation";
import type {
  AgentsUiConversationEvent,
  AgentsUiConversationState,
  AgentsUiWorktreeConversationResponse,
  WorktreeInfo,
} from "./types";
import WorktreeConversationPanel from "./WorktreeConversationPanel";

interface Props {
  worktree: WorktreeInfo;
  onConversationMessageSent?: () => void;
}

interface RefreshPollingState {
  token: number;
  baselineSignature: string | null;
  lastSignature: string | null;
  sawProgress: boolean;
  unchangedTicks: number;
  stopWhenIdle: boolean;
}

interface StreamConnection {
  conversationId: string;
  disconnect: () => void;
}

const REFRESH_POLL_INTERVAL_MS = 1000;
const REFRESH_POLL_SETTLE_TICKS = 3;

export default function MobileChatSurface({
  worktree,
  onConversationMessageSent = () => {},
}: Props) {
  const [conversation, setConversationState] = useState<AgentsUiConversationState | null>(null);
  const [conversationError, setConversationError] = useState<string | null>(null);
  const [conversationLoading, setConversationLoading] = useState(false);
  const [composerText, setComposerText] = useState("");
  const [isSending, setIsSendingState] = useState(false);
  const [refreshPollingState, setRefreshPollingStateValue] = useState<RefreshPollingState | null>(null);

  // Refs mirror the mutable values that logic callbacks read synchronously, so
  // long-lived stream/interval closures always see the latest value.
  const conversationRef = useRef<AgentsUiConversationState | null>(null);
  const isSendingRef = useRef(false);
  const isAnsweringQuestionRef = useRef(false);
  const refreshPollingStateRef = useRef<RefreshPollingState | null>(null);
  const streamConnectionRef = useRef<StreamConnection | null>(null);
  const nextRefreshPollingTokenRef = useRef(1);
  const lastStreamRevisionRef = useRef(0);

  function setConversation(next: AgentsUiConversationState | null): void {
    conversationRef.current = next;
    setConversationState(next);
  }

  function setIsSending(next: boolean): void {
    isSendingRef.current = next;
    setIsSendingState(next);
  }

  function setRefreshPollingState(next: RefreshPollingState | null): void {
    refreshPollingStateRef.current = next;
    setRefreshPollingStateValue(next);
  }

  function closeConversationStream(): void {
    streamConnectionRef.current?.disconnect();
    streamConnectionRef.current = null;
    lastStreamRevisionRef.current = 0;
  }

  function supportsStreaming(nextConversation: AgentsUiConversationState | null): boolean {
    return nextConversation?.provider === "codexAppServer" || nextConversation?.provider === "claudeCode";
  }

  function hasActiveConversationStream(conversationId: string): boolean {
    return streamConnectionRef.current?.conversationId === conversationId;
  }

  function applyConversationResponse(response: AgentsUiWorktreeConversationResponse): void {
    setConversation(mergeConversationSnapshot(conversationRef.current, response.conversation));
    setConversationError(null);
    syncConversationStream();
  }

  function handleConversationStreamFailure(conversationId: string, message: string): void {
    if (!hasActiveConversationStream(conversationId) || !streamConnectionRef.current) return;
    const currentConnection = streamConnectionRef.current;
    streamConnectionRef.current = null;
    currentConnection.disconnect();
    setConversationError(message);
  }

  function handleConversationStreamEvent(conversationId: string, event: AgentsUiConversationEvent): void {
    if (!hasActiveConversationStream(conversationId)) return;
    if (event.type !== "error") {
      if (event.revision <= lastStreamRevisionRef.current) return;
      lastStreamRevisionRef.current = event.revision;
    }

    switch (event.type) {
      case "messageDelta":
        setConversation(applyConversationMessageDelta(conversationRef.current, event));
        break;
      case "messageUpsert":
        setConversation(applyConversationMessageUpsert(conversationRef.current, event));
        break;
      case "conversationStatus":
        setConversation(applyConversationStatus(conversationRef.current, event));
        syncConversationStream();
        break;
      case "error":
        setConversationError(event.message);
        break;
    }
  }

  function syncConversationStream(force = false): void {
    const currentConversation = conversationRef.current;
    const conversationId = supportsStreaming(currentConversation) ? currentConversation?.conversationId ?? null : null;

    // Keep one stream open across turns (close only on conversation change) so the
    // server-side message ordering isn't reseeded per turn, which interleaves turns.
    if (streamConnectionRef.current && streamConnectionRef.current.conversationId !== conversationId) {
      closeConversationStream();
    }

    if (!conversationId || hasActiveConversationStream(conversationId)) {
      return;
    }

    // Not connected yet: open on a send (force) or when a run is already active.
    if (!force && currentConversation?.running !== true) {
      return;
    }

    lastStreamRevisionRef.current = 0;
    const disconnect = connectWorktreeConversationStream(worktree.branch, {
      onEvent: (event) => {
        handleConversationStreamEvent(conversationId, event);
      },
      onError: (message) => {
        handleConversationStreamFailure(conversationId, message);
      },
      onClose: () => {
        handleConversationStreamFailure(conversationId, "Conversation stream disconnected");
      },
    });
    streamConnectionRef.current = { conversationId, disconnect };
  }

  function requestConversation(mode: "attach" | "history"): Promise<AgentsUiWorktreeConversationResponse> {
    return mode === "attach"
      ? attachWorktreeConversation(worktree.branch)
      : fetchWorktreeConversationHistory(worktree.branch);
  }

  async function loadConversation(mode: "attach" | "history"): Promise<void> {
    setConversationLoading(true);
    setConversationError(null);

    try {
      const response = await requestConversation(mode);
      applyConversationResponse(response);
    } catch (error) {
      setConversationError(error instanceof Error ? error.message : String(error));
    } finally {
      setConversationLoading(false);
    }
  }

  function startRefreshPolling(
    baselineConversation: AgentsUiConversationState | null = conversationRef.current,
    stopWhenIdle = false,
  ): void {
    const baselineSignature = buildConversationProgressSignature(baselineConversation);
    setRefreshPollingState({
      token: nextRefreshPollingTokenRef.current,
      baselineSignature,
      lastSignature: baselineSignature,
      sawProgress: false,
      unchangedTicks: 0,
      stopWhenIdle,
    });
    nextRefreshPollingTokenRef.current += 1;
  }

  function updateRefreshPollingState(
    token: number,
    nextConversation: AgentsUiConversationState,
  ): void {
    const currentState = refreshPollingStateRef.current;
    if (!currentState || currentState.token !== token) return;

    // Terminal-owned turns settle when the worktree agent goes idle (handled by the
    // busy-poll effect below), not via the message-progress heuristic used for sends.
    if (currentState.stopWhenIdle) return;

    const nextSignature = buildConversationProgressSignature(nextConversation);
    const sawProgress = currentState.sawProgress || nextSignature !== currentState.baselineSignature;
    const unchangedTicks = nextSignature === currentState.lastSignature
      ? currentState.unchangedTicks + 1
      : 0;

    if (sawProgress && unchangedTicks >= REFRESH_POLL_SETTLE_TICKS) {
      setRefreshPollingState(null);
      return;
    }

    setRefreshPollingState({
      ...currentState,
      lastSignature: nextSignature,
      sawProgress,
      unchangedTicks,
    });
  }

  async function sendConversationText(text: string): Promise<boolean> {
    const currentConversation = conversationRef.current;
    if (!currentConversation) return false;
    const baselineConversation = currentConversation;
    const trimmed = text.trim();
    if (trimmed.length === 0) return false;

    setIsSending(true);
    setConversationError(null);
    try {
      syncConversationStream(true);
      const response = await sendWorktreeConversationMessage(worktree.branch, { text: trimmed });
      const base = conversationRef.current ?? currentConversation;
      const withConversationId = base.conversationId !== response.conversationId
        ? { ...base, conversationId: response.conversationId }
        : base;
      setConversation(markConversationTurnStarted(withConversationId, response.turnId, trimmed));
      if (response.streaming) {
        syncConversationStream();
      } else {
        closeConversationStream();
        startRefreshPolling(baselineConversation);
      }
      onConversationMessageSent();
      return true;
    } catch (error) {
      setConversationError(error instanceof Error ? error.message : String(error));
      return false;
    } finally {
      setIsSending(false);
    }
  }

  async function sendSelectedConversationMessage(): Promise<void> {
    if (composerText.trim().length === 0) return;
    const sent = await sendConversationText(composerText);
    if (sent) setComposerText("");
  }

  async function interruptSelectedConversation(): Promise<void> {
    const baselineConversation = conversationRef.current;
    setConversationError(null);
    try {
      const response = await interruptWorktreeConversation(worktree.branch);
      if (response.streaming) {
        syncConversationStream();
      } else {
        closeConversationStream();
        startRefreshPolling(baselineConversation);
      }
    } catch (error) {
      setConversationError(error instanceof Error ? error.message : String(error));
    }
  }

  // Answering an AskUserQuestion is a new turn, so the run that asked it must end
  // first. In headless `claude -p` the question is auto-dismissed and the turn
  // keeps going, so interrupt the active run before sending the answer.
  async function answerConversationQuestion(text: string): Promise<void> {
    if (!conversationRef.current || isSendingRef.current || isAnsweringQuestionRef.current) return;
    isAnsweringQuestionRef.current = true;
    try {
      if (conversationRef.current.running) {
        await interruptSelectedConversation();
      }
      await sendConversationText(text);
    } finally {
      isAnsweringQuestionRef.current = false;
    }
  }

  useEffect(() => {
    void loadConversation("attach");
    return () => {
      closeConversationStream();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    // A Claude turn started in the terminal (the initial worktree prompt, or anything
    // typed in the pane) is not a backend-owned run, so there is no stream to subscribe
    // to and the snapshot reports running:false. While the worktree agent is busy, poll
    // history so the terminal claude's flushed messages appear live; stop once it idles.
    const agentBusy = worktree.agent === "working";
    const isTerminalOwnedClaudeTurn =
      conversation?.provider === "claudeCode" && conversation.running !== true;

    if (agentBusy && isTerminalOwnedClaudeTurn) {
      if (refreshPollingStateRef.current === null) {
        startRefreshPolling(conversation, true);
      }
      return;
    }

    if (refreshPollingStateRef.current?.stopWhenIdle === true) {
      setRefreshPollingState(null);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [worktree.agent, conversation, refreshPollingState]);

  useEffect(() => {
    const pollingState = refreshPollingState;
    if (!pollingState) return;

    const token = pollingState.token;
    let requestInFlight = false;

    // Polling is only for conversation providers that do not publish live stream events.
    const interval = window.setInterval(() => {
      if (!refreshPollingStateRef.current || refreshPollingStateRef.current.token !== token || requestInFlight) return;
      requestInFlight = true;
      void (async () => {
        try {
          const response = await requestConversation("history");
          applyConversationResponse(response);
          updateRefreshPollingState(token, response.conversation);
        } catch (error) {
          setConversationError(error instanceof Error ? error.message : String(error));
        } finally {
          requestInFlight = false;
        }
      })();
    }, REFRESH_POLL_INTERVAL_MS);

    return () => {
      window.clearInterval(interval);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [refreshPollingState]);

  return (
    <WorktreeConversationPanel
      worktree={worktree}
      conversation={conversation}
      conversationError={conversationError}
      conversationLoading={conversationLoading}
      composerText={composerText}
      isSending={isSending}
      onAttach={() => void loadConversation("attach")}
      onComposerInput={(value) => {
        setComposerText(value);
      }}
      onInterrupt={() => void interruptSelectedConversation()}
      onRefresh={() => void loadConversation("history")}
      onSend={() => void sendSelectedConversationMessage()}
      onAnswerQuestion={(text) => void answerConversationQuestion(text)}
    />
  );
}
