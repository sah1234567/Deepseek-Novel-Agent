import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { apiMessagesToUi, type ApiUiMessage } from "../utils/messages";
import {
  createInitialMachine,
  dispatchTranscriptEvent,
  flatMessagesFromMachine,
  hasPendingApproval,
  isTurnInProgress as transcriptTurnInProgress,
  mapSegmentComplete,
  mapStreamChunk,
  mapToolCallRequest,
} from "../transcript";
import {
  applyForkDbToMap,
  dispatchForkEvent,
  emptyForkMachine,
  mergeForkRunOnOpen,
} from "../fork";
import type { TranscriptMachine } from "../transcript/types";
import { shouldShowTurnError } from "../constants/interrupt";

export interface ArchivedEpoch {
  epoch: number;
  flatMessages: UIMessage[];
}

interface SessionTranscriptResponse {
  archives: Array<{ epoch: number; messages: ApiUiMessage[] }>;
  active: ApiUiMessage[];
}

export interface ContentBlock {
  blockIndex: number;
  kind: string;
  text: string;
}

export interface UIMessage {
  id: string;
  role: "user" | "assistant" | "tool" | "subAgentReport";
  contentBlocks: ContentBlock[];
  toolName?: string;
  toolInput?: unknown;
  toolStatus?: ToolCall["status"];
  needsApproval?: boolean;
  forkRunId?: string;
  messageKind?: string;
}

export interface ToolCall {
  id: string;
  name: string;
  input: unknown;
  status: "streaming-args" | "pending" | "running" | "done" | "denied";
  needsApproval: boolean;
  result?: string;
  progressDescription?: string;
  unparsedInput?: string;
  parsedInput?: unknown;
}

export interface AskQuestionOption {
  id: string;
  label: string;
}

export interface PendingQuestion {
  toolCallId: string;
  questions: Array<{
    id: string;
    prompt: string;
    options: AskQuestionOption[];
    allowMultiple?: boolean;
    allowCustom?: boolean;
  }>;
}

/** Live + persisted transcript for one fork instance (tool or PostToolUse hook path). */
export interface ForkRunState {
  forkRunId: string;
  agentType: string;
  taskPreview: string;
  source: "tool" | "hook";
  /** Main-session ForkSubAgent `tool_call_id` (tool path only). */
  parentToolCallId?: string;
  machine: TranscriptMachine;
  status: "running" | "complete";
  reportOutput?: string;
}

interface StreamChunk {
  messageId: string;
  blockIndex: number;
  delta: string;
  kind: string;
}

interface ToolCallRequest {
  toolCallId?: string;
  toolName?: string;
  input?: unknown;
  needsApproval?: boolean;
  phase?: string;
  delta?: string;
  content?: string;
  status?: string;
  description?: string;
}

interface TurnComplete {
  cacheHitTokens?: number;
  cacheMissTokens?: number;
  completionTokens?: number;
  turnHitTokens?: number;
  turnMissTokens?: number;
  turnCompTokens?: number;
  phase?: string;
  message?: string;
  wasInterrupted?: boolean;
}

interface AppStatusSnapshot {
  hasInterruptibleToolInProgress?: boolean;
  pendingUserQuestion?: boolean;
}

export function useAgent(onTurnComplete?: () => void) {
  const onTurnCompleteRef = useRef(onTurnComplete);
  onTurnCompleteRef.current = onTurnComplete;

  const [transcriptMachine, setTranscriptMachine] = useState<TranscriptMachine>(
    createInitialMachine(),
  );
  const [archivedEpochs, setArchivedEpochs] = useState<ArchivedEpoch[]>([]);
  const [isStreaming, setIsStreaming] = useState(false);
  // Turn-level streaming flag from Tauri events (turn-complete / ask-user-question), not FSM phase.
  const [pendingQuestion, setPendingQuestion] = useState<PendingQuestion | null>(null);
  const [questionSelections, setQuestionSelections] = useState<Record<string, string[]>>({});
  const [questionCustomText, setQuestionCustomText] = useState<Record<string, string>>({});
  const [questionError, setQuestionError] = useState<string | null>(null);
  const [hasInterruptibleToolInProgress, setHasInterruptibleToolInProgress] = useState(false);
  const [forkRuns, setForkRuns] = useState<Map<string, ForkRunState>>(new Map());
  const [openForkRunId, setOpenForkRunId] = useState<string | null>(null);
  const [model, setModel] = useState<string>("deepseek-v4-pro");
  const modelRef = useRef(model);
  modelRef.current = model;

  const isStreamingRef = useRef(false);
  const messageQueueRef = useRef<string[]>([]);
  const transcriptMachineRef = useRef(transcriptMachine);
  transcriptMachineRef.current = transcriptMachine;

  const pendingQuestionRef = useRef(pendingQuestion);
  pendingQuestionRef.current = pendingQuestion;

  const flatMessages = useMemo(
    () => flatMessagesFromMachine(transcriptMachine),
    [transcriptMachine],
  );

  const dispatchMain = useCallback((event: Parameters<typeof dispatchTranscriptEvent>[1]) => {
    setTranscriptMachine((m) => dispatchTranscriptEvent(m, event));
  }, []);

  const hydrateMessages = useCallback(async (sessionId?: string, keepPendingQuestion = false) => {
    try {
      const transcript = await invoke<SessionTranscriptResponse>("get_session_transcript", {
        sessionId: sessionId ?? null,
      });
      const archives = transcript.archives.map((arch) => ({
        epoch: arch.epoch,
        flatMessages: apiMessagesToUi(arch.messages),
      }));
      setArchivedEpochs(archives);
      const activeUi = apiMessagesToUi(transcript.active);
      setTranscriptMachine((m) =>
        dispatchTranscriptEvent(m, { type: "HYDRATE", flatMessages: activeUi }),
      );
      if (!keepPendingQuestion) {
        setPendingQuestion(null);
      }
    } catch (e) {
      setQuestionError(String(e));
    }
  }, []);

  isStreamingRef.current = isStreaming;

  const refreshInterruptibleStatus = useCallback(async () => {
    if (!isStreamingRef.current) {
      setHasInterruptibleToolInProgress(false);
      return;
    }
    try {
      const s = await invoke<AppStatusSnapshot>("get_app_status");
      setHasInterruptibleToolInProgress(!!s.hasInterruptibleToolInProgress);
    } catch {
      setHasInterruptibleToolInProgress(false);
    }
  }, []);

  const drainMessageQueue = useCallback(() => {
    const next = messageQueueRef.current.shift();
    if (next) {
      void invoke<string>("send_message", { content: next, model: modelRef.current }).catch((e) => {
        setQuestionError(String(e));
        setIsStreaming(false);
      });
      setIsStreaming(true);
    }
  }, []);

  useEffect(() => {
    if (!isStreaming) {
      setHasInterruptibleToolInProgress(false);
      return;
    }
    void refreshInterruptibleStatus();
    const interval = setInterval(() => void refreshInterruptibleStatus(), 500);
    return () => clearInterval(interval);
  }, [isStreaming, refreshInterruptibleStatus]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || pendingQuestionRef.current) return;
      if (!isStreamingRef.current) return;
      e.preventDefault();
      void (async () => {
        dispatchMain({ type: "INTERRUPT" });
        setIsStreaming(false);
        try {
          await invoke("interrupt", { reason: "user-cancel" });
        } catch (err) {
          setQuestionError(String(err));
        }
      })();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [dispatchMain]);

  useEffect(() => {
    const unlisteners: Promise<UnlistenFn>[] = [];

    unlisteners.push(
      listen<{ segmentIndex: number; forkRunId?: string }>("assistant-segment-complete", (event) => {
        const { segmentIndex, forkRunId } = event.payload;
        if (forkRunId) {
          setForkRuns((prev) => {
            const run = prev.get(forkRunId);
            if (!run) return prev;
            const next = new Map(prev);
            next.set(forkRunId, {
              ...run,
              machine: dispatchForkEvent(run.machine, mapSegmentComplete(segmentIndex ?? 0)),
            });
            return next;
          });
          return;
        }
        dispatchMain(mapSegmentComplete(segmentIndex ?? 0));
      }),
    );

    unlisteners.push(
      listen<StreamChunk>("stream-chunk", (event) => {
        dispatchMain(mapStreamChunk(event.payload));
      }),
    );

    unlisteners.push(
      listen<ToolCallRequest>("tool-call-request", (event) => {
        const mapped = mapToolCallRequest(event.payload);
        if (!mapped) return;
        dispatchMain(mapped);
        void refreshInterruptibleStatus();
      }),
    );

    unlisteners.push(
      listen<PendingQuestion>("ask-user-question", (event) => {
        dispatchMain({ type: "ASK_USER_QUESTION" });
        setPendingQuestion(event.payload);
        setQuestionSelections({});
        setQuestionCustomText({});
        setQuestionError(null);
        setIsStreaming(false);
      }),
    );

    unlisteners.push(
      listen<TurnComplete>("turn-complete", (event) => {
        const p = event.payload;
        if (p.phase === "error") {
          dispatchMain({ type: "INTERRUPT" });
          if (shouldShowTurnError(p)) {
            setQuestionError(p.message ?? "Agent 出错");
          }
          setIsStreaming(false);
          setHasInterruptibleToolInProgress(false);
          drainMessageQueue();
          return;
        }
        if (p.wasInterrupted === true) {
          dispatchMain({ type: "INTERRUPT" });
          setIsStreaming(false);
          setHasInterruptibleToolInProgress(false);
          drainMessageQueue();
          return;
        }
        if (p.phase === "start") return;
        if (p.turnHitTokens !== undefined || p.cacheHitTokens !== undefined) {
          dispatchMain({ type: "TURN_COMPLETE" });
          setIsStreaming(false);
          setHasInterruptibleToolInProgress(false);
          void (async () => {
            if (pendingQuestionRef.current) return;
            if (hasPendingApproval(transcriptMachineRef.current)) return;
            try {
              const s = await invoke<AppStatusSnapshot>("get_app_status");
              if (s.pendingUserQuestion) return;
            } catch {
              // Fall through to normal turn end if status is unavailable.
            }
            onTurnCompleteRef.current?.();
            void hydrateMessages();
            drainMessageQueue();
          })();
        }
      }),
    );

    unlisteners.push(
      listen<{
        forkRunId: string;
        agentType: string;
        taskPreview?: string;
        source?: string;
        parentToolCallId?: string | null;
      }>("sub-agent-started", (event) => {
        const { forkRunId, agentType, taskPreview, source, parentToolCallId } = event.payload;
        setForkRuns((prev) => {
          const next = new Map(prev);
          next.set(forkRunId, {
            forkRunId,
            agentType: agentType ?? "",
            taskPreview: taskPreview ?? "",
            source: source === "hook" ? "hook" : "tool",
            parentToolCallId: parentToolCallId ?? undefined,
            machine: emptyForkMachine(),
            status: "running",
          });
          return next;
        });
      }),
    );

    unlisteners.push(
      listen<{ forkRunId: string; messageId?: string; delta: string; kind: string }>(
        "sub-agent-stream",
        (event) => {
          const { forkRunId, delta, kind, messageId } = event.payload;
          setForkRuns((prev) => {
            const run = prev.get(forkRunId);
            if (!run) return prev;
            const chunk = mapStreamChunk({
              messageId: messageId ?? `fork-${forkRunId}`,
              delta,
              kind,
            });
            const next = new Map(prev);
            next.set(forkRunId, {
              ...run,
              machine: dispatchForkEvent(run.machine, chunk),
            });
            return next;
          });
        },
      ),
    );

    unlisteners.push(
      listen<ToolCallRequest & { forkRunId: string }>("sub-agent-tool", (event) => {
        const { forkRunId, ...p } = event.payload;
        const mapped = mapToolCallRequest(p);
        if (!mapped) return;
        setForkRuns((prev) => {
          const run = prev.get(forkRunId);
          if (!run) return prev;
          const next = new Map(prev);
          next.set(forkRunId, {
            ...run,
            machine: dispatchForkEvent(run.machine, mapped),
          });
          return next;
        });
      }),
    );

    unlisteners.push(
      listen<{ forkRunId: string; agentId?: string; output?: string }>(
        "sub-agent-complete",
        (event) => {
          const { forkRunId, output } = event.payload;
          setForkRuns((prev) => {
            const run = prev.get(forkRunId);
            if (!run) return prev;
            const next = new Map(prev);
            next.set(forkRunId, {
              ...run,
              machine: dispatchForkEvent(run.machine, { type: "TURN_COMPLETE" }),
              status: "complete",
              reportOutput: output ?? run.reportOutput,
            });
            return next;
          });
        },
      ),
    );

    unlisteners.push(
      listen("session-resumed", () => {
        messageQueueRef.current = [];
        setIsStreaming(false);
        setHasInterruptibleToolInProgress(false);
        setPendingQuestion(null);
        setQuestionSelections({});
        setQuestionCustomText({});
        setQuestionError(null);
        setForkRuns(new Map());
        setOpenForkRunId(null);
        setTranscriptMachine(createInitialMachine());
        void (async () => {
          try {
            const s = await invoke<AppStatusSnapshot>("get_app_status");
            void hydrateMessages(undefined, !!s.pendingUserQuestion);
          } catch {
            void hydrateMessages();
          }
        })();
      }),
    );

    return () => {
      void Promise.all(unlisteners).then((fns) => fns.forEach((fn) => fn()));
    };
  }, [dispatchMain, drainMessageQueue, refreshInterruptibleStatus, hydrateMessages]);

  const submitAnswer = useCallback(
    async (selections: Record<string, string[]>, customText: Record<string, string>) => {
      const pq = pendingQuestionRef.current;
      if (!pq) return;
      setQuestionError(null);
      setIsStreaming(true);
      try {
        await invoke("answer_question", {
          toolCallId: pq.toolCallId,
          answers: { selections, customText },
        });
        dispatchMain({ type: "ANSWER_QUESTION" });
        setPendingQuestion(null);
        setQuestionSelections({});
        setQuestionCustomText({});
      } catch (e) {
        setQuestionError(String(e));
        setIsStreaming(false);
      }
    },
    [dispatchMain],
  );

  const sendMessage = useCallback(
    async (content: string) => {
      const trimmed = content.trim();
      if (!trimmed) return;
      const userMsg: UIMessage = {
        id: crypto.randomUUID(),
        role: "user",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: trimmed }],
      };
      dispatchMain({ type: "BEGIN_TURN", user: userMsg });
      setIsStreaming(true);
      setQuestionError(null);
      void invoke<string>("send_message", { content: trimmed, model: modelRef.current }).catch((e) => {
        setQuestionError(String(e));
        setIsStreaming(false);
      });
    },
    [dispatchMain],
  );

  const submitInterrupt = useCallback(
    async (content: string) => {
      const trimmed = content.trim();
      if (!trimmed) return;
      const userMsg: UIMessage = {
        id: crypto.randomUUID(),
        role: "user",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: trimmed }],
      };
      setTranscriptMachine((m) => {
        let next = dispatchTranscriptEvent(m, { type: "INTERRUPT" });
        next = dispatchTranscriptEvent(next, { type: "BEGIN_TURN", user: userMsg });
        return next;
      });
      messageQueueRef.current.push(trimmed);
      setQuestionError(null);
      try {
        await invoke("interrupt", { reason: "interrupt" });
      } catch (e) {
        messageQueueRef.current.pop();
        setQuestionError(String(e));
      }
    },
    [],
  );

  const interrupt = useCallback(async () => {
    dispatchMain({ type: "INTERRUPT" });
    setIsStreaming(false);
    setHasInterruptibleToolInProgress(false);
    try {
      await invoke("interrupt", { reason: "user-cancel" });
    } catch (e) {
      setQuestionError(String(e));
    }
  }, [dispatchMain]);

  const approveTool = useCallback(
    async (toolCallId: string) => {
      setIsStreaming(true);
      dispatchMain({
        type: "PATCH_TOOL",
        toolCallId,
        patch: { status: "running", needsApproval: false },
      });
      await invoke("approve_tool", { toolCallId });
    },
    [dispatchMain],
  );

  const denyTool = useCallback(
    async (toolCallId: string, reason?: string) => {
      dispatchMain({
        type: "PATCH_TOOL",
        toolCallId,
        patch: { status: "denied", needsApproval: false },
      });
      setIsStreaming(true);
      await invoke("deny_tool", { toolCallId, reason });
    },
    [dispatchMain],
  );

  const toggleQuestionOption = useCallback(
    (questionId: string, optionId: string, allowMultiple?: boolean) => {
      setQuestionSelections((prev) => {
        const current = prev[questionId] ?? [];
        if (allowMultiple) {
          const next = current.includes(optionId)
            ? current.filter((id) => id !== optionId)
            : [...current, optionId];
          return { ...prev, [questionId]: next };
        }
        return { ...prev, [questionId]: [optionId] };
      });
    },
    [],
  );

  const answerQuestion = useCallback(async () => {
    await submitAnswer(questionSelections, questionCustomText);
  }, [questionSelections, questionCustomText, submitAnswer]);

  const clearQuestionError = useCallback(() => setQuestionError(null), []);

  const openForkOverlay = useCallback(async (forkRunId: string) => {
    setOpenForkRunId(forkRunId);
    setForkRuns((prev) => mergeForkRunOnOpen(prev, forkRunId));
    try {
      const raw = await invoke<
        Array<{
          id: string;
          role: string;
          contentBlocks: ContentBlock[];
          toolName?: string;
          forkRunId?: string;
        }>
      >("get_fork_messages", { runId: forkRunId });
      const ui = apiMessagesToUi(raw);
      setForkRuns((prev) => applyForkDbToMap(prev, forkRunId, ui));
    } catch (e) {
      setQuestionError(String(e));
    }
  }, []);

  const closeForkOverlay = useCallback(() => {
    setOpenForkRunId(null);
  }, []);

  return {
    transcriptMachine,
    archivedEpochs,
    flatMessages,
    isStreaming,
    forkRuns,
    openForkRunId,
    pendingQuestion,
    questionSelections,
    questionCustomText,
    questionError,
    hasInterruptibleToolInProgress,
    sendMessage,
    submitInterrupt,
    interrupt,
    approveTool,
    denyTool,
    toggleQuestionOption,
    setQuestionCustomText,
    answerQuestion,
    hydrateMessages,
    clearQuestionError,
    openForkOverlay,
    closeForkOverlay,
    model,
    setModel,
    turnInProgress: transcriptTurnInProgress(transcriptMachine, pendingQuestion),
  };
}

export type UseAgentReturn = ReturnType<typeof useAgent>;
