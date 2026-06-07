import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { IPC_COMMANDS } from "../ipc/commands";
import type { AppStatusSnapshot } from "../transcript/eventPayloads";
import type {
  ContentBlock,
  ForkRunState,
  PendingQuestion,
  UIMessage,
} from "../types/messages";
import { useAgentTauriListeners } from "./useAgentTauriListeners";

export type {
  AskQuestionOption,
  ContentBlock,
  ForkRunState,
  PendingQuestion,
  ToolCall,
  UIMessage,
} from "../types/messages";
import { apiMessagesToUi } from "../utils/messages";
import {
  createInitialMachine,
  dispatchTranscriptEvent,
  flatMessagesFromMachine,
  forkBindingSnapshotKey,
  isTurnInProgress as transcriptTurnInProgress,
} from "../transcript";
import {
  applyForkDbToMap,
  dispatchForkEvent,
  mergeForkRunOnOpen,
} from "../fork";
import type { TranscriptEvent, TranscriptMachine } from "../transcript/types";
import { createRafBatcher } from "../utils/rafDispatch";

export function useAgent(onTurnComplete?: () => void) {
  const onTurnCompleteRef = useRef(onTurnComplete);
  onTurnCompleteRef.current = onTurnComplete;

  const [transcriptMachine, setTranscriptMachine] = useState<TranscriptMachine>(
    createInitialMachine(),
  );
  const [isStreaming, setIsStreaming] = useState(false);
  // Turn-level streaming flag from Tauri events (turn-complete / ask-user-question), not FSM phase.
  const [pendingQuestion, setPendingQuestion] = useState<PendingQuestion | null>(null);
  const [questionSelections, setQuestionSelections] = useState<Record<string, string[]>>({});
  const [questionCustomText, setQuestionCustomText] = useState<Record<string, string>>({});
  const [questionError, setQuestionError] = useState<string | null>(null);
  const [hasInterruptibleToolInProgress, setHasInterruptibleToolInProgress] = useState(false);
  const [forkRuns, setForkRuns] = useState<Map<string, ForkRunState>>(new Map());
  const [openForkRunId, setOpenForkRunId] = useState<string | null>(null);
  const openForkRunIdRef = useRef<string | null>(null);
  openForkRunIdRef.current = openForkRunId;
  const [model, setModel] = useState<string>("deepseek-v4-pro");
  const modelRef = useRef(model);
  modelRef.current = model;

  const isStreamingRef = useRef(false);
  const messageQueueRef = useRef<string[]>([]);
  const transcriptMachineRef = useRef(transcriptMachine);
  transcriptMachineRef.current = transcriptMachine;

  const pendingQuestionRef = useRef(pendingQuestion);
  pendingQuestionRef.current = pendingQuestion;

  const forkBindingKey = forkBindingSnapshotKey(transcriptMachine);
  const forkBindingMessages = useMemo(
    () => flatMessagesFromMachine(transcriptMachine),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed recompute avoids per-chunk scans
    [forkBindingKey],
  );

  const streamChunkBatchRef = useRef(
    createRafBatcher<TranscriptEvent>((events) => {
      setTranscriptMachine((m) =>
        events.reduce((acc, ev) => dispatchTranscriptEvent(acc, ev), m),
      );
    }),
  );
  const forkStreamBatchRef = useRef(
    createRafBatcher<{ forkRunId: string; event: TranscriptEvent }>((items) => {
      setForkRuns((prev) => {
        let next: Map<string, ForkRunState> | null = null;
        for (const { forkRunId, event } of items) {
          const source: Map<string, ForkRunState> = next ?? prev;
          const run = source.get(forkRunId);
          if (!run) continue;
          if (!next) next = new Map(source);
          next.set(forkRunId, {
            ...run,
            machine: dispatchForkEvent(run.machine, event),
          });
        }
        return next ?? prev;
      });
    }),
  );

  const flushStreamBatches = useCallback(() => {
    streamChunkBatchRef.current.flushNow();
    forkStreamBatchRef.current.flushNow();
  }, []);

  const dispatchMain = useCallback((event: TranscriptEvent) => {
    streamChunkBatchRef.current.flushNow();
    setTranscriptMachine((m) => dispatchTranscriptEvent(m, event));
  }, []);

  const reloadActiveTailRef = useRef<(() => Promise<void>) | undefined>(undefined);

  /** Register lazy-loader tail reload; returns cleanup. Called from ChatPanel. */
  const registerReloadActiveTail = useCallback((reloadActiveTail: () => Promise<void>) => {
    reloadActiveTailRef.current = reloadActiveTail;
    return () => {
      if (reloadActiveTailRef.current === reloadActiveTail) {
        reloadActiveTailRef.current = undefined;
      }
    };
  }, []);

  isStreamingRef.current = isStreaming;

  const refreshInterruptibleStatus = useCallback(async () => {
    if (!isStreamingRef.current) {
      setHasInterruptibleToolInProgress(false);
      return;
    }
    try {
      const s = await invoke<AppStatusSnapshot>(IPC_COMMANDS.getAppStatus);
      setHasInterruptibleToolInProgress(!!s.hasInterruptibleToolInProgress);
    } catch {
      setHasInterruptibleToolInProgress(false);
    }
  }, []);

  const drainMessageQueue = useCallback(() => {
    const next = messageQueueRef.current.shift();
    if (next) {
      void invoke<string>(IPC_COMMANDS.sendMessage, { content: next, model: modelRef.current }).catch((e) => {
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
          await invoke(IPC_COMMANDS.interrupt, { reason: "user-cancel" });
        } catch (err) {
          setQuestionError(String(err));
        }
      })();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [dispatchMain]);

  useAgentTauriListeners({
    dispatchMain,
    drainMessageQueue,
    refreshInterruptibleStatus,
    flushStreamBatches,
    streamChunkBatchRef,
    forkStreamBatchRef,
    pendingQuestionRef,
    transcriptMachineRef,
    openForkRunIdRef,
    messageQueueRef,
    reloadActiveTailRef,
    onTurnCompleteRef,
    setForkRuns,
    setPendingQuestion,
    setQuestionSelections,
    setQuestionCustomText,
    setQuestionError,
    setIsStreaming,
    setHasInterruptibleToolInProgress,
    setOpenForkRunId,
  });

  const submitAnswer = useCallback(
    async (selections: Record<string, string[]>, customText: Record<string, string>) => {
      const pq = pendingQuestionRef.current;
      if (!pq) return;
      setQuestionError(null);
      setIsStreaming(true);
      try {
        await invoke(IPC_COMMANDS.answerQuestion, {
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
      void invoke<string>(IPC_COMMANDS.sendMessage, { content: trimmed, model: modelRef.current }).catch((e) => {
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
        await invoke(IPC_COMMANDS.interrupt, { reason: "interrupt" });
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
      await invoke(IPC_COMMANDS.interrupt, { reason: "user-cancel" });
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
      try {
        await invoke(IPC_COMMANDS.approveTool, { toolCallId });
      } catch (e) {
        setQuestionError(String(e));
        setIsStreaming(false);
      }
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
      try {
        await invoke(IPC_COMMANDS.denyTool, { toolCallId, reason });
      } catch (e) {
        setQuestionError(String(e));
        setIsStreaming(false);
      }
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
      >(IPC_COMMANDS.getForkMessages, { runId: forkRunId });
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
    forkBindingMessages,
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
    dispatchTranscript: dispatchMain,
    registerReloadActiveTail,
    clearQuestionError,
    openForkOverlay,
    closeForkOverlay,
    model,
    setModel,
    turnInProgress: transcriptTurnInProgress(transcriptMachine, pendingQuestion),
  };
}

export type UseAgentReturn = ReturnType<typeof useAgent>;
