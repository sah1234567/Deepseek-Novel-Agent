import { useEffect, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { IPC_COMMANDS } from "../ipc/commands";
import { IPC_EVENTS } from "../ipc/events";
import type {
  AppStatusSnapshot,
  StreamChunkPayload,
  ToolCallRequestPayload,
  TurnCompletePayload,
} from "../transcript/eventPayloads";
import type { ForkRunState, PendingQuestion } from "../types/messages";
import { mountTauriListeners } from "../utils/tauriEvents";
import {
  dispatchForkEvent,
  emptyForkMachine,
} from "../fork";
import {
  hasPendingApproval,
  mapSegmentComplete,
  mapStreamChunk,
  mapToolCallRequest,
} from "../transcript";
import type { TranscriptEvent, TranscriptMachine } from "../transcript/types";
import { shouldShowTurnError } from "../constants/interrupt";
import type { createRafBatcher } from "../utils/rafDispatch";

type RafBatcher<T> = ReturnType<typeof createRafBatcher<T>>;

export type AgentListenerDeps = {
  dispatchMain: (event: TranscriptEvent) => void;
  drainMessageQueue: () => void;
  refreshInterruptibleStatus: () => Promise<void>;
  flushStreamBatches: () => void;
  streamChunkBatchRef: MutableRefObject<RafBatcher<TranscriptEvent>>;
  forkStreamBatchRef: MutableRefObject<
    RafBatcher<{ forkRunId: string; event: TranscriptEvent }>
  >;
  pendingQuestionRef: MutableRefObject<PendingQuestion | null>;
  transcriptMachineRef: MutableRefObject<TranscriptMachine>;
  openForkRunIdRef: MutableRefObject<string | null>;
  messageQueueRef: MutableRefObject<string[]>;
  reloadActiveTailRef: MutableRefObject<(() => Promise<void>) | undefined>;
  onTurnCompleteRef: MutableRefObject<(() => void) | undefined>;
  setForkRuns: Dispatch<SetStateAction<Map<string, ForkRunState>>>;
  setPendingQuestion: Dispatch<SetStateAction<PendingQuestion | null>>;
  setQuestionSelections: Dispatch<SetStateAction<Record<string, string[]>>>;
  setQuestionCustomText: Dispatch<SetStateAction<Record<string, string>>>;
  setQuestionError: Dispatch<SetStateAction<string | null>>;
  setIsStreaming: Dispatch<SetStateAction<boolean>>;
  setHasInterruptibleToolInProgress: Dispatch<SetStateAction<boolean>>;
  setOpenForkRunId: Dispatch<SetStateAction<string | null>>;
};

export function useAgentTauriListeners(deps: AgentListenerDeps) {
  const {
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
  } = deps;

  useEffect(() => {
    const cleanupListeners = mountTauriListeners([
      () =>
        listen<{ segmentIndex: number; forkRunId?: string }>(
          IPC_EVENTS.assistantSegmentComplete,
          (event) => {
            const { segmentIndex, forkRunId } = event.payload;
            if (forkRunId) {
              forkStreamBatchRef.current.flushNow();
              setForkRuns((prev) => {
                const run = prev.get(forkRunId);
                if (!run) return prev;
                const next = new Map(prev);
                next.set(forkRunId, {
                  ...run,
                  machine: dispatchForkEvent(
                    run.machine,
                    mapSegmentComplete(segmentIndex ?? 0),
                  ),
                });
                return next;
              });
              return;
            }
            dispatchMain(mapSegmentComplete(segmentIndex ?? 0));
          },
        ),
      () =>
        listen<StreamChunkPayload>(IPC_EVENTS.streamChunk, (event) => {
          streamChunkBatchRef.current.push(mapStreamChunk(event.payload));
        }),
      () =>
        listen<ToolCallRequestPayload>(IPC_EVENTS.toolCallRequest, (event) => {
          const mapped = mapToolCallRequest(event.payload);
          if (!mapped) return;
          dispatchMain(mapped);
          void refreshInterruptibleStatus();
        }),
      () =>
        listen<PendingQuestion>(IPC_EVENTS.askUserQuestion, (event) => {
          dispatchMain({ type: "ASK_USER_QUESTION" });
          setPendingQuestion(event.payload);
          setQuestionSelections({});
          setQuestionCustomText({});
          setQuestionError(null);
          setIsStreaming(false);
        }),
      () =>
        listen<TurnCompletePayload>(IPC_EVENTS.turnComplete, (event) => {
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
            // Keep optimistic BEGIN_TURN from submitInterrupt when a queued follow-up exists.
            if (messageQueueRef.current.length === 0) {
              dispatchMain({ type: "INTERRUPT" });
            }
            setIsStreaming(false);
            setHasInterruptibleToolInProgress(false);
            drainMessageQueue();
            return;
          }
          if (p.phase === "start") return;
          if (p.turnHitTokens !== undefined || p.cacheHitTokens !== undefined) {
            // Guard: keep FSM alive when tools are pending approval.
            // TURN_COMPLETE would set phase=idle, discarding subsequent TOOL events.
            if (hasPendingApproval(transcriptMachineRef.current)) return;
            dispatchMain({ type: "TURN_COMPLETE" });
            setIsStreaming(false);
            setHasInterruptibleToolInProgress(false);
            void (async () => {
              if (pendingQuestionRef.current) return;
              try {
                const s = await invoke<AppStatusSnapshot>(IPC_COMMANDS.getAppStatus);
                if (s.pendingUserQuestion) return;
              } catch {
                // Fall through to normal turn end if status is unavailable.
              }
              onTurnCompleteRef.current?.();
              void reloadActiveTailRef.current?.();
              drainMessageQueue();
            })();
          }
        }),
      () =>
        listen<{
          forkRunId: string;
          agentType: string;
          taskPreview?: string;
          source?: string;
          parentToolCallId?: string | null;
        }>(IPC_EVENTS.subAgentStarted, (event) => {
          const { forkRunId, agentType, taskPreview, source, parentToolCallId } =
            event.payload;
          const forkSource =
            source === "hook" || source === "tool"
              ? source
              : parentToolCallId
                ? "tool"
                : "hook";
          setForkRuns((prev) => {
            const next = new Map(prev);
            next.set(forkRunId, {
              forkRunId,
              agentType: agentType ?? "",
              taskPreview: taskPreview ?? "",
              source: forkSource,
              parentToolCallId: parentToolCallId ?? undefined,
              machine: emptyForkMachine(),
              status: "running",
            });
            return next;
          });
        }),
      () =>
        listen<{ forkRunId: string; messageId?: string; delta: string; kind: string }>(
          IPC_EVENTS.subAgentStream,
          (event) => {
            const { forkRunId, delta, kind, messageId } = event.payload;
            forkStreamBatchRef.current.push({
              forkRunId,
              event: mapStreamChunk({
                messageId: messageId ?? `fork-${forkRunId}`,
                delta,
                kind,
              }),
            });
          },
        ),
      () =>
        listen<ToolCallRequestPayload & { forkRunId: string }>(
          IPC_EVENTS.subAgentTool,
          (event) => {
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
          },
        ),
      () =>
        listen<{ forkRunId: string; agentId?: string; output?: string }>(
          IPC_EVENTS.subAgentComplete,
          (event) => {
            const { forkRunId, output } = event.payload;
            setForkRuns((prev) => {
              const run = prev.get(forkRunId);
              if (!run) return prev;
              const next = new Map(prev);
              const completedMachine =
                openForkRunIdRef.current === forkRunId
                  ? dispatchForkEvent(run.machine, { type: "TURN_COMPLETE" })
                  : emptyForkMachine();
              next.set(forkRunId, {
                ...run,
                machine: completedMachine,
                status: "complete",
                reportOutput: output ?? run.reportOutput,
              });
              return next;
            });
          },
        ),
      () =>
        listen(IPC_EVENTS.sessionResumed, () => {
          flushStreamBatches();
          messageQueueRef.current = [];
          setIsStreaming(false);
          setHasInterruptibleToolInProgress(false);
          setPendingQuestion(null);
          setQuestionSelections({});
          setQuestionCustomText({});
          setQuestionError(null);
          setForkRuns(new Map());
          setOpenForkRunId(null);
        }),
    ]);

    return () => {
      flushStreamBatches();
      cleanupListeners();
    };
  }, [
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
  ]);
}
