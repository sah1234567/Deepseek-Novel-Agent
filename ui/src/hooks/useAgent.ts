import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { apiMessagesToUi, tryParseJson } from "../utils/messages";

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
  status: "pending" | "running" | "done" | "denied";
  needsApproval: boolean;
  result?: string;
  progressDescription?: string;
}

export interface StreamingToolUse {
  id: string;
  name: string;
  unparsedInput: string;
  parsedInput?: unknown;
  needsApproval?: boolean;
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
  messages: UIMessage[];
  streamingText: string | null;
  streamingThinking: string | null;
  activeTools: Map<string, ToolCall>;
  status: "running" | "complete";
}

export interface HookForkBanner {
  forkRunId: string;
  agentType: string;
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
  wasInterrupted?: boolean;
  phase?: string;
  message?: string;
}

interface AppStatusSnapshot {
  hasInterruptibleToolInProgress?: boolean;
  pendingUserQuestion?: boolean;
}

function segmentMessageId(baseId: string, segmentIndex: number): string {
  return segmentIndex === 0 ? baseId : `${baseId}-seg-${segmentIndex}`;
}

function toolToMessage(tool: ToolCall): UIMessage {
  return {
    id: `tool-${tool.id}`,
    role: "tool",
    toolName: tool.name,
    toolStatus: tool.status,
    needsApproval: tool.needsApproval,
    contentBlocks: [{ blockIndex: 0, kind: "text", text: tool.result ?? "" }],
    toolInput: tool.input,
  };
}

function finalizeForkSegment(run: ForkRunState, segmentIndex = 0): ForkRunState {
  const blocks: ContentBlock[] = [];
  if (run.streamingThinking && run.streamingThinking.length > 0) {
    blocks.push({ blockIndex: 0, kind: "thinking", text: run.streamingThinking });
  }
  if (run.streamingText && run.streamingText.length > 0) {
    blocks.push({ blockIndex: blocks.length, kind: "text", text: run.streamingText });
  }
  if (blocks.length === 0) return run;
  return {
    ...run,
    messages: [
      ...run.messages,
      {
        id: `fork-${run.forkRunId}-assistant-${run.messages.length}-seg-${segmentIndex}`,
        role: "assistant" as const,
        contentBlocks: blocks,
      },
    ],
    streamingText: null,
    streamingThinking: null,
  };
}

function flushForkStreaming(run: ForkRunState): ForkRunState {
  let runWithSegment = finalizeForkSegment(run, run.messages.length);
  let messages = [...runWithSegment.messages];
  for (const tool of runWithSegment.activeTools.values()) {
    if (tool.status === "done" || tool.status === "denied") {
      messages = [...messages, toolToMessage(tool)];
    }
  }
  return {
    ...runWithSegment,
    messages,
    streamingText: null,
    streamingThinking: null,
    activeTools: new Map(),
    status: "complete",
  };
}

function applyForkToolUpdate(run: ForkRunState, p: ToolCallRequest): ForkRunState {
  if (p.phase === "start" && p.toolCallId && p.toolName) {
    const activeTools = new Map(run.activeTools);
    activeTools.set(p.toolCallId, {
      id: p.toolCallId,
      name: p.toolName,
      input: {},
      status: "running",
      needsApproval: !!p.needsApproval,
    });
    return { ...run, activeTools };
  }
  if (p.phase === "result" && p.toolCallId) {
    const activeTools = new Map(run.activeTools);
    const existing = activeTools.get(p.toolCallId);
    const tool: ToolCall = existing
      ? { ...existing, status: "done", result: p.content, input: p.input ?? existing.input }
      : {
          id: p.toolCallId,
          name: p.toolName ?? "Tool",
          input: p.input ?? {},
          status: "done",
          needsApproval: false,
          result: p.content,
        };
    activeTools.delete(p.toolCallId);
    return { ...run, activeTools, messages: [...run.messages, toolToMessage(tool)] };
  }
  return run;
}

function isKnowledgeAuditorHook(agentType: string): boolean {
  return agentType.toLowerCase().includes("knowledgeauditor");
}

export function useAgent(onTurnComplete?: () => void) {
  const onTurnCompleteRef = useRef(onTurnComplete);
  onTurnCompleteRef.current = onTurnComplete;

  const [messages, setMessages] = useState<UIMessage[]>([]);
  const [isStreaming, setIsStreaming] = useState(false);
  const [hookRunning, setHookRunning] = useState(false);
  const [activeSubAgent, setActiveSubAgent] = useState<string | null>(null);
  const [streamingText, setStreamingText] = useState<string | null>(null);
  const [streamingThinking, setStreamingThinking] = useState<string | null>(null);
  const [streamingMessageId, setStreamingMessageId] = useState<string | null>(null);
  const [streamingToolUses, setStreamingToolUses] = useState<StreamingToolUse[]>([]);
  const [activeToolCalls, setActiveToolCalls] = useState<Map<string, ToolCall>>(new Map());
  const [pendingQuestion, setPendingQuestion] = useState<PendingQuestion | null>(null);
  /** Messages before this index render above the question panel; streaming/tools render below. */
  const [questionAnchorIndex, setQuestionAnchorIndex] = useState<number | null>(null);
  const [questionSelections, setQuestionSelections] = useState<Record<string, string[]>>({});
  const [questionCustomText, setQuestionCustomText] = useState<Record<string, string>>({});
  const [questionError, setQuestionError] = useState<string | null>(null);
  const [hasInterruptibleToolInProgress, setHasInterruptibleToolInProgress] = useState(false);
  /** Fork transcript overlay state — never merged into main `messages` (LLM isolation). */
  const [forkRuns, setForkRuns] = useState<Map<string, ForkRunState>>(new Map());
  const [openForkRunId, setOpenForkRunId] = useState<string | null>(null);
  const [activeForkCount, setActiveForkCount] = useState(0);
  const [hookForkBanner, setHookForkBanner] = useState<HookForkBanner | null>(null);
  const [model, setModel] = useState<string>("deepseek-v4-pro");
  const modelRef = useRef(model);
  modelRef.current = model;
  const activeForkCountRef = useRef(0);

  const streamingMessageIdRef = useRef<string | null>(null);
  const streamingTextRef = useRef<string | null>(null);
  const streamingThinkingRef = useRef<string | null>(null);
  const isStreamingRef = useRef(false);
  const messageQueueRef = useRef<string[]>([]);
  const activeToolCallsRef = useRef<Map<string, ToolCall>>(new Map());
  const streamingToolUsesRef = useRef<StreamingToolUse[]>([]);
  /** Tool call ids in LLM stream order (ToolUseStarted) for the current segment. */
  const segmentToolOrderRef = useRef<string[]>([]);
  /** Index in `messages` where the current segment's tool placeholders begin. */
  const segmentToolAnchorRef = useRef(0);
  /** False until `finalizeSegment` runs; blocks early tool inserts at index 0. */
  const segmentToolsAnchoredRef = useRef(false);

  streamingMessageIdRef.current = streamingMessageId;
  streamingTextRef.current = streamingText;
  streamingThinkingRef.current = streamingThinking;
  isStreamingRef.current = isStreaming;
  activeToolCallsRef.current = activeToolCalls;
  streamingToolUsesRef.current = streamingToolUses;

  const pendingQuestionRef = useRef(pendingQuestion);
  pendingQuestionRef.current = pendingQuestion;

  const hydrateMessages = useCallback(async (sessionId?: string, keepPendingQuestion = false) => {
    try {
      const raw = await invoke<Array<{ id: string; role: string; contentBlocks: ContentBlock[] }>>(
        "get_session_messages",
        { session_id: sessionId ?? null },
      );
      setMessages(apiMessagesToUi(raw));
      if (!keepPendingQuestion) {
        setQuestionAnchorIndex(null);
        setPendingQuestion(null);
      }
      setActiveToolCalls(new Map());
      setStreamingToolUses([]);
      segmentToolOrderRef.current = [];
      segmentToolAnchorRef.current = 0;
      segmentToolsAnchoredRef.current = false;
    } catch (e) {
      setQuestionError(String(e));
    }
  }, []);

  const toolCallFromRefs = useCallback((id: string): ToolCall | null => {
    const active = activeToolCallsRef.current.get(id);
    if (active) return active;
    const st = streamingToolUsesRef.current.find((t) => t.id === id);
    if (!st) return null;
    let input: unknown = st.parsedInput;
    if (input === undefined && st.unparsedInput.trim()) {
      try {
        input = JSON.parse(st.unparsedInput) as unknown;
      } catch {
        input = {};
      }
    }
    return {
      id: st.id,
      name: st.name,
      input: input ?? {},
      status: "running",
      needsApproval: !!st.needsApproval,
    };
  }, []);

  const upsertToolMessage = useCallback((tool: ToolCall) => {
    const msg = toolToMessage(tool);
    setMessages((prev) => {
      const existingIdx = prev.findIndex((m) => m.id === msg.id);
      if (existingIdx >= 0) {
        const next = [...prev];
        next[existingIdx] = msg;
        return next;
      }
      // Tool results can arrive before assistant-segment-complete; append if anchor not set yet.
      if (!segmentToolsAnchoredRef.current) {
        return [...prev, msg];
      }
      const orderIdx = segmentToolOrderRef.current.indexOf(tool.id);
      if (orderIdx >= 0) {
        const insertAt = Math.min(segmentToolAnchorRef.current + orderIdx, prev.length);
        const next = [...prev];
        next.splice(insertAt, 0, msg);
        return next;
      }
      return [...prev, msg];
    });
  }, []);

  const archiveCompletedActiveTools = useCallback(() => {
    const order = segmentToolOrderRef.current;
    const ids =
      order.length > 0
        ? order
        : Array.from(activeToolCallsRef.current.keys());
    let archived = false;
    for (const id of ids) {
      const t = activeToolCallsRef.current.get(id);
      if (!t || (t.status !== "done" && t.status !== "denied")) continue;
      upsertToolMessage(t);
      archived = true;
    }
    if (!archived) return;
    setActiveToolCalls((prev) => {
      const next = new Map(prev);
      for (const id of ids) {
        const t = next.get(id);
        if (t && (t.status === "done" || t.status === "denied")) next.delete(id);
      }
      return next;
    });
  }, [upsertToolMessage]);

  const archiveAllActiveTools = useCallback(() => {
    const order = segmentToolOrderRef.current;
    const tools =
      order.length > 0
        ? order
            .map((id) => activeToolCallsRef.current.get(id))
            .filter((t): t is ToolCall => !!t)
        : Array.from(activeToolCallsRef.current.values());
    if (tools.length === 0) return;
    for (const t of tools) upsertToolMessage(t);
    setActiveToolCalls(new Map());
  }, [upsertToolMessage]);

  const finalizeSegment = useCallback((segmentIndex = 0) => {
    const order = [...segmentToolOrderRef.current];
    segmentToolOrderRef.current = [];

    const mid = streamingMessageIdRef.current;
    const text = streamingTextRef.current;
    const thinking = streamingThinkingRef.current;
    const blocks: ContentBlock[] = [];
    if (thinking && thinking.length > 0) {
      blocks.push({ blockIndex: blocks.length, kind: "thinking", text: thinking });
    }
    if (text && text.length > 0) {
      blocks.push({ blockIndex: blocks.length, kind: "text", text });
    }
    const msgId = mid ? segmentMessageId(mid, segmentIndex) : null;

    setMessages((prev) => {
      let next = prev;
      const lastUserIdx = next.reduce(
        (acc, m, i) => (m.role === "user" ? i : acc),
        -1,
      );
      const orderSet = new Set(order);
      // Drop tools that were inserted at anchor 0 before segment-complete (fast InvokeSkill, etc.).
      if (lastUserIdx >= 0 && orderSet.size > 0) {
        next = next.filter((m, i) => {
          if (m.role !== "tool") return true;
          const tid = m.id.replace(/^tool-/, "");
          if (!orderSet.has(tid)) return true;
          return i > lastUserIdx;
        });
      }
      if (msgId && blocks.length > 0 && !next.some((m) => m.id === msgId && m.role === "assistant")) {
        next = [...next, { id: msgId, role: "assistant" as const, contentBlocks: blocks }];
      }
      const anchor = next.length;
      segmentToolAnchorRef.current = anchor;
      segmentToolsAnchoredRef.current = true;
      let offset = 0;
      for (const id of order) {
        const msgKey = `tool-${id}`;
        const existingIdx = next.findIndex((m) => m.id === msgKey);
        if (existingIdx >= 0) {
          // Tool arrived early (background poller beat segment-complete).
          // Remove from old position — it will be re-inserted below assistant.
          next = [...next.slice(0, existingIdx), ...next.slice(existingIdx + 1)];
        }
        const tool = toolCallFromRefs(id);
        if (!tool) continue;
        if (tool.needsApproval && tool.status === "pending") continue;
        const insertAt = anchor + offset;
        next = [...next.slice(0, insertAt), toolToMessage(tool), ...next.slice(insertAt)];
        offset += 1;
      }
      return next;
    });

    setStreamingText(null);
    setStreamingThinking(null);
    streamingTextRef.current = null;
    streamingThinkingRef.current = null;
    setStreamingToolUses((prev) => prev.filter((t) => !order.includes(t.id)));
    setActiveToolCalls((prev) => {
      const next = new Map(prev);
      for (const id of order) {
        const t = next.get(id);
        if (t && !(t.needsApproval && t.status === "pending")) next.delete(id);
      }
      return next;
    });
  }, [toolCallFromRefs]);

  const finalizeStreamingAssistant = useCallback(
    (segmentIndex = 0) => {
      finalizeSegment(segmentIndex);
    },
    [finalizeSegment],
  );

  const clearStreamingState = useCallback(() => {
    setStreamingText(null);
    setStreamingThinking(null);
    setStreamingMessageId(null);
    streamingTextRef.current = null;
    streamingThinkingRef.current = null;
    setStreamingToolUses([]);
    segmentToolOrderRef.current = [];
    segmentToolsAnchoredRef.current = false;
  }, []);

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
        clearStreamingState();
        setHookRunning(false);
      });
      setIsStreaming(true);
      setActiveToolCalls(new Map());
      clearStreamingState();
    }
  }, [clearStreamingState]);

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
        finalizeStreamingAssistant();
        archiveAllActiveTools();
        setIsStreaming(false);
        setHookRunning(false);
        try {
          await invoke("interrupt", { reason: "user-cancel" });
        } catch (err) {
          setQuestionError(String(err));
        }
      })();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [finalizeStreamingAssistant, archiveAllActiveTools]);

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
            next.set(forkRunId, finalizeForkSegment(run, segmentIndex ?? 0));
            return next;
          });
          return;
        }
        finalizeSegment(segmentIndex ?? 0);
      }),
    );

    unlisteners.push(
      listen<StreamChunk>("stream-chunk", (event) => {
        const payload = event.payload;
        setStreamingMessageId(payload.messageId);
        if (payload.kind === "thinking") {
          setStreamingThinking((prev) => {
            const next = (prev ?? "") + payload.delta;
            streamingThinkingRef.current = next;
            return next;
          });
          return;
        }
        setStreamingText((prev) => {
          const next = (prev ?? "") + payload.delta;
          streamingTextRef.current = next;
          return next;
        });
      }),
    );

    unlisteners.push(
      listen<ToolCallRequest>("tool-call-request", (event) => {
        const p = event.payload;

        if (p.phase === "start" && p.toolCallId && p.toolName) {
          if (!segmentToolOrderRef.current.includes(p.toolCallId)) {
            segmentToolOrderRef.current.push(p.toolCallId);
          }
          setStreamingToolUses((prev) => {
            if (prev.some((t) => t.id === p.toolCallId)) return prev;
            return [...prev, { id: p.toolCallId!, name: p.toolName!, unparsedInput: "" }];
          });
          return;
        }

        if (p.phase === "input_delta" && p.toolCallId && p.delta) {
          setStreamingToolUses((prev) =>
            prev.map((t) =>
              t.id === p.toolCallId
                ? { ...t, unparsedInput: t.unparsedInput + p.delta }
                : t,
            ),
          );
          return;
        }

        if (p.phase === "input_complete" && p.toolCallId && p.toolName) {
          setStreamingToolUses((prev) =>
            prev.map((t) =>
              t.id === p.toolCallId
                ? {
                    ...t,
                    parsedInput: p.input,
                    needsApproval: !!p.needsApproval,
                  }
                : t,
            ),
          );
          const built: ToolCall = {
            id: p.toolCallId,
            name: p.toolName,
            input: p.input,
            status: p.needsApproval ? "pending" : "running",
            needsApproval: !!p.needsApproval,
          };
          setActiveToolCalls((prev) => {
            const next = new Map(prev);
            next.set(p.toolCallId!, built);
            return next;
          });
          void refreshInterruptibleStatus();
          return;
        }

        if (p.phase === "progress" && p.toolCallId) {
          setActiveToolCalls((prev) => {
            const existing = prev.get(p.toolCallId!);
            if (!existing) return prev;
            const next = new Map(prev);
            next.set(p.toolCallId!, {
              ...existing,
              progressDescription: p.description ?? p.status,
            });
            return next;
          });
          return;
        }

        if (p.phase === "result" && p.toolCallId) {
          setActiveToolCalls((prev) => {
            const existing = prev.get(p.toolCallId!);
            if (!existing) return prev;
            const done: ToolCall = {
              ...existing,
              status: "done",
              result: p.content,
            };
            upsertToolMessage(done);
            const next = new Map(prev);
            next.delete(p.toolCallId!);
            return next;
          });
          void refreshInterruptibleStatus();
          return;
        }

        if (!p.toolCallId || !p.toolName) return;

        setStreamingToolUses((prev) => prev.filter((t) => t.id !== p.toolCallId));
        setActiveToolCalls((prev) => {
          const next = new Map(prev);
          const existing = next.get(p.toolCallId!);
          if (existing && (existing.status === "running" || existing.status === "done")) {
            if (p.input !== undefined) {
              next.set(p.toolCallId!, { ...existing, input: p.input });
            }
            return next;
          }
          next.set(p.toolCallId!, {
            id: p.toolCallId!,
            name: p.toolName!,
            input: p.input,
            status: p.needsApproval ? "pending" : "running",
            needsApproval: !!p.needsApproval,
          });
          return next;
        });
        void refreshInterruptibleStatus();
      }),
    );

    unlisteners.push(
      listen<PendingQuestion>("ask-user-question", (event) => {
        const p = event.payload;
        finalizeStreamingAssistant();
        archiveCompletedActiveTools();
        setMessages((prev) => {
          setQuestionAnchorIndex(prev.length);
          return prev;
        });
        setPendingQuestion(p);
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
          finalizeStreamingAssistant();
          archiveAllActiveTools();
          setQuestionError(p.message ?? "Agent 出错");
          setIsStreaming(false);
          setHasInterruptibleToolInProgress(false);
          drainMessageQueue();
          return;
        }
        if (p.phase === "start") return;
        if (p.turnHitTokens !== undefined || p.cacheHitTokens !== undefined) {
          finalizeStreamingAssistant();
          archiveAllActiveTools();
          setIsStreaming(false);
          setHookRunning(false);
          setHasInterruptibleToolInProgress(false);
          void (async () => {
            if (pendingQuestionRef.current) return;
            if (
              [...activeToolCallsRef.current.values()].some(
                (t) => t.status === "pending" || t.status === "running",
              )
            ) {
              return;
            }
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
      listen<{ forkRunId: string; agentType: string; taskPreview?: string }>(
        "sub-agent-started",
        (event) => {
          const { forkRunId, agentType, taskPreview } = event.payload;
          activeForkCountRef.current += 1;
          setActiveForkCount(activeForkCountRef.current);
          setActiveSubAgent(agentType ?? "");
          if (isKnowledgeAuditorHook(agentType ?? "")) setHookRunning(true);
          setForkRuns((prev) => {
            const next = new Map(prev);
            next.set(forkRunId, {
              forkRunId,
              agentType: agentType ?? "",
              taskPreview: taskPreview ?? "",
              messages: [],
              streamingText: null,
              streamingThinking: null,
              activeTools: new Map(),
              status: "running",
            });
            return next;
          });
        },
      ),
    );

    unlisteners.push(
      listen<{ forkRunId: string; delta: string; kind: string }>("sub-agent-stream", (event) => {
        const { forkRunId, delta, kind } = event.payload;
        setForkRuns((prev) => {
          const run = prev.get(forkRunId);
          if (!run) return prev;
          const next = new Map(prev);
          if (kind === "thinking") {
            next.set(forkRunId, {
              ...run,
              streamingThinking: (run.streamingThinking ?? "") + delta,
            });
          } else {
            next.set(forkRunId, {
              ...run,
              streamingText: (run.streamingText ?? "") + delta,
            });
          }
          return next;
        });
      }),
    );

    unlisteners.push(
      listen<ToolCallRequest & { forkRunId: string }>("sub-agent-tool", (event) => {
        const { forkRunId, ...p } = event.payload;
        setForkRuns((prev) => {
          const run = prev.get(forkRunId);
          if (!run) return prev;
          const next = new Map(prev);
          next.set(forkRunId, applyForkToolUpdate(run, p));
          return next;
        });
      }),
    );

    unlisteners.push(
      listen<{ forkRunId: string; agentId?: string; output?: string }>(
        "sub-agent-complete",
        (event) => {
          const { forkRunId } = event.payload;
          activeForkCountRef.current = Math.max(0, activeForkCountRef.current - 1);
          setActiveForkCount(activeForkCountRef.current);
          if (activeForkCountRef.current === 0) {
            setActiveSubAgent(null);
          }
          setHookRunning(false);

          setForkRuns((prev) => {
            const run = prev.get(forkRunId);
            if (!run) return prev;
            const next = new Map(prev);
            const finished = flushForkStreaming(run);
            next.set(forkRunId, finished);
            if (isKnowledgeAuditorHook(finished.agentType)) {
              setHookForkBanner({ forkRunId, agentType: finished.agentType });
            }
            return next;
          });
        },
      ),
    );

    unlisteners.push(
      listen("session-resumed", () => {
        setIsStreaming(false);
        setHookRunning(false);
        setHasInterruptibleToolInProgress(false);
        clearStreamingState();
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
  }, [
    finalizeSegment,
    finalizeStreamingAssistant,
    archiveAllActiveTools,
    archiveCompletedActiveTools,
    upsertToolMessage,
    drainMessageQueue,
    refreshInterruptibleStatus,
    hydrateMessages,
    // onTurnComplete intentionally excluded — stored in onTurnCompleteRef to avoid re-registering listeners every render
  ]);

  const submitAnswer = useCallback(
    async (selections: Record<string, string[]>, customText: Record<string, string>) => {
      const pq = pendingQuestionRef.current;
      if (!pq) return;
      setQuestionError(null);
      setIsStreaming(true);
      clearStreamingState();
      try {
        await invoke("answer_question", {
          toolCallId: pq.toolCallId,
          answers: { selections, customText },
        });
        setPendingQuestion(null);
        setQuestionAnchorIndex(null);
        setQuestionSelections({});
        setQuestionCustomText({});
      } catch (e) {
        setQuestionError(String(e));
        setIsStreaming(false);
      }
    },
    [clearStreamingState],
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
      setMessages((prev) => [...prev, userMsg]);
      setIsStreaming(true);
      setActiveToolCalls(new Map());
      setQuestionAnchorIndex(null);
      segmentToolAnchorRef.current = 0;
      segmentToolsAnchoredRef.current = false;
      clearStreamingState();
      setQuestionError(null);
      void invoke<string>("send_message", { content: trimmed, model: modelRef.current }).catch((e) => {
        setQuestionError(String(e));
        setIsStreaming(false);
        clearStreamingState();
        setHookRunning(false);
      });
    },
    [clearStreamingState],
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
      setMessages((prev) => [...prev, userMsg]);
      messageQueueRef.current.push(trimmed);
      finalizeStreamingAssistant();
      archiveAllActiveTools();
      setQuestionError(null);
      try {
        await invoke("interrupt", { reason: "interrupt" });
      } catch (e) {
        messageQueueRef.current.pop();
        setQuestionError(String(e));
      }
    },
    [finalizeStreamingAssistant, archiveAllActiveTools],
  );

  const interrupt = useCallback(async () => {
    finalizeStreamingAssistant();
    archiveAllActiveTools();
    setIsStreaming(false);
    setHookRunning(false);
    setHasInterruptibleToolInProgress(false);
    try {
      await invoke("interrupt", { reason: "user-cancel" });
    } catch (e) {
      setQuestionError(String(e));
    }
  }, [finalizeStreamingAssistant, archiveAllActiveTools]);

  const approveTool = useCallback(async (toolCallId: string) => {
    setIsStreaming(true);
    setStreamingText(null);
    setStreamingThinking(null);
    streamingTextRef.current = null;
    streamingThinkingRef.current = null;
    setStreamingToolUses([]);
    setActiveToolCalls((prev) => {
      const next = new Map(prev);
      const t = next.get(toolCallId);
      if (t) next.set(toolCallId, { ...t, status: "running", needsApproval: false });
      return next;
    });
    await invoke("approve_tool", { toolCallId });
  }, []);

  const denyTool = useCallback(async (toolCallId: string, reason?: string) => {
    setActiveToolCalls((prev) => {
      const next = new Map(prev);
      const t = next.get(toolCallId);
      if (t) next.set(toolCallId, { ...t, status: "denied", needsApproval: false });
      return next;
    });
    setIsStreaming(true);
    await invoke("deny_tool", { toolCallId, reason });
  }, []);

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

  const loadForkMessages = useCallback(async (forkRunId: string) => {
    setForkRuns((prev) => {
      if (prev.has(forkRunId) && prev.get(forkRunId)!.messages.length > 0) return prev;
      const next = new Map(prev);
      next.set(forkRunId, {
        forkRunId,
        agentType: prev.get(forkRunId)?.agentType ?? "加载中…",
        taskPreview: prev.get(forkRunId)?.taskPreview ?? "",
        messages: prev.get(forkRunId)?.messages ?? [],
        streamingText: null,
        streamingThinking: null,
        activeTools: new Map(),
        status: prev.get(forkRunId)?.status ?? "complete",
      });
      return next;
    });
    try {
      const raw = await invoke<
        Array<{
          id: string;
          role: string;
          contentBlocks: ContentBlock[];
          toolName?: string;
          forkRunId?: string;
          messageKind?: string;
        }>
      >("get_fork_messages", { runId: forkRunId });
      const ui = apiMessagesToUi(raw);
      setForkRuns((prev) => {
        const next = new Map(prev);
        next.set(forkRunId, {
          forkRunId,
          agentType: prev.get(forkRunId)?.agentType ?? "Subagent",
          taskPreview: prev.get(forkRunId)?.taskPreview ?? "",
          messages: ui.length > 0 ? ui : (prev.get(forkRunId)?.messages ?? []),
          streamingText: null,
          streamingThinking: null,
          activeTools: new Map(),
          status: "complete",
        });
        return next;
      });
    } catch (e) {
      setQuestionError(String(e));
    }
  }, []);

  const openForkOverlay = useCallback(async (forkRunId: string) => {
    setOpenForkRunId(forkRunId);
    await loadForkMessages(forkRunId);
  }, [loadForkMessages]);

  const closeForkOverlay = useCallback(() => {
    setOpenForkRunId(null);
  }, []);

  const dismissHookForkBanner = useCallback(() => {
    setHookForkBanner(null);
  }, []);

  useEffect(() => {
    if (!hookForkBanner) return;
    const timer = setTimeout(() => setHookForkBanner(null), 12_000);
    return () => clearTimeout(timer);
  }, [hookForkBanner]);

  return {
    messages,
    isStreaming,
    hookRunning,
    activeSubAgent,
    activeForkCount,
    forkRuns,
    openForkRunId,
    hookForkBanner,
    streamingText,
    streamingThinking,
    streamingToolUses,
    activeToolCalls,
    pendingQuestion,
    questionAnchorIndex,
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
    loadForkMessages,
    openForkOverlay,
    closeForkOverlay,
    dismissHookForkBanner,
    model,
    setModel,
    tryParseJson,
  };
}

export type UseAgentReturn = ReturnType<typeof useAgent>;
