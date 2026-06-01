import type { ContentBlock, ToolCall, UIMessage } from "../hooks/useAgent";
import {
  segmentMessageId,
  SYNTHETIC_USER_ID,
  type LlmSegment,
  type TranscriptContext,
  type TranscriptEvent,
  type TranscriptMachine,
  type Turn,
} from "./types";

export function createInitialMachine(): TranscriptMachine {
  return {
    phase: "idle",
    context: { turns: [], openSegment: null, streamingMessageId: null },
  };
}

function cloneContext(ctx: TranscriptContext): TranscriptContext {
  return {
    turns: ctx.turns.map((t) => ({
      ...t,
      segments: t.segments.map((s) => ({
        ...s,
        assistant: { ...s.assistant, contentBlocks: [...s.assistant.contentBlocks] },
        tools: s.tools.map((tool) => ({ ...tool })),
      })),
      reports: [...t.reports],
    })),
    openSegment: ctx.openSegment
      ? {
          ...ctx.openSegment,
          assistant: {
            ...ctx.openSegment.assistant,
            contentBlocks: [...ctx.openSegment.assistant.contentBlocks],
          },
          tools: ctx.openSegment.tools.map((t) => ({ ...t })),
        }
      : null,
    streamingMessageId: ctx.streamingMessageId,
  };
}

function currentTurn(ctx: TranscriptContext): Turn | undefined {
  return ctx.turns.length > 0 ? ctx.turns[ctx.turns.length - 1] : undefined;
}

function nextSegmentIndex(ctx: TranscriptContext): number {
  const turn = currentTurn(ctx);
  if (!turn) return 0;
  return turn.segments.length + (ctx.openSegment ? 0 : 0);
}

function assistantHasContent(blocks: ContentBlock[]): boolean {
  return blocks.some((b) => (b.text ?? "").length > 0);
}

function ensureOpenSegment(
  ctx: TranscriptContext,
  segmentIndex: number,
  messageId?: string | null,
): LlmSegment {
  if (ctx.openSegment && ctx.openSegment.segmentIndex === segmentIndex) {
    return ctx.openSegment;
  }
  const turn = currentTurn(ctx);
  const turnId = turn?.turnId ?? "orphan";
  const baseId = messageId ?? ctx.streamingMessageId ?? `assistant-${turnId}`;
  const seg: LlmSegment = {
    segmentId: `${turnId}:${segmentIndex}`,
    segmentIndex,
    assistant: {
      id: segmentMessageId(baseId, segmentIndex),
      status: "streaming",
      contentBlocks: [],
    },
    tools: [],
  };
  ctx.openSegment = seg;
  return seg;
}

function commitOpenSegment(ctx: TranscriptContext): string | null {
  const open = ctx.openSegment;
  if (!open) return null;
  const turn = currentTurn(ctx);
  if (!turn) return null;

  const blocks = open.assistant.contentBlocks;
  const status = assistantHasContent(blocks) ? "committed" : "placeholder";
  const committed: LlmSegment = {
    ...open,
    assistant: {
      ...open.assistant,
      status,
      contentBlocks: blocks,
    },
    tools: open.tools.map((t) => ({ ...t })),
  };
  turn.segments.push(committed);
  ctx.openSegment = null;
  return committed.segmentId;
}

function upsertToolInSegment(segment: LlmSegment, toolCallId: string, patch: Partial<ToolCall> & { name?: string }): void {
  const idx = segment.tools.findIndex((t) => t.id === toolCallId);
  if (idx >= 0) {
    segment.tools[idx] = { ...segment.tools[idx], ...patch, id: toolCallId };
  } else if (patch.name) {
    segment.tools.push({
      id: toolCallId,
      name: patch.name,
      input: patch.input ?? {},
      status: patch.status ?? "streaming-args",
      needsApproval: patch.needsApproval ?? false,
      result: patch.result,
      progressDescription: patch.progressDescription,
      unparsedInput: patch.unparsedInput,
      parsedInput: patch.parsedInput,
    });
  }
}

function findToolSegment(ctx: TranscriptContext, toolCallId: string): LlmSegment | null {
  if (ctx.openSegment) {
    const t = ctx.openSegment.tools.find((x) => x.id === toolCallId);
    if (t) return ctx.openSegment;
  }
  const turn = currentTurn(ctx);
  if (!turn) return null;
  for (let i = turn.segments.length - 1; i >= 0; i--) {
    if (turn.segments[i].tools.some((x) => x.id === toolCallId)) {
      return turn.segments[i];
    }
  }
  return null;
}

function handleToolEvent(machine: TranscriptMachine, event: Extract<TranscriptEvent, { type: "TOOL" }>): TranscriptMachine {
  const { phase, toolCallId } = event;

  if (machine.phase === "idle") {
    return machine;
  }

  if (machine.phase === "pausedForQuestion") {
    return machine;
  }

  let ctx = cloneContext(machine.context);
  let phaseNext = machine.phase;

  const ensureStreaming = () => {
    if (machine.phase === "segmentCommitted") {
      const segIdx = nextSegmentIndex(ctx);
      ensureOpenSegment(ctx, segIdx, ctx.streamingMessageId);
      phaseNext = "segmentStreaming";
    } else if (!ctx.openSegment) {
      const segIdx = nextSegmentIndex(ctx);
      ensureOpenSegment(ctx, segIdx, ctx.streamingMessageId);
    }
  };

  if (phase === "start") {
    ensureStreaming();
    if (!ctx.openSegment || !event.toolName) return machine;
    upsertToolInSegment(ctx.openSegment, toolCallId, {
      name: event.toolName,
      input: {},
      status: "streaming-args",
      needsApproval: event.needsApproval ?? false,
      unparsedInput: "",
    });
    return { phase: phaseNext, context: ctx };
  }

  if (phase === "input_delta") {
    ensureStreaming();
    const seg = ctx.openSegment ?? findToolSegment(ctx, toolCallId);
    if (!seg) return machine;
    const existing = seg.tools.find((t) => t.id === toolCallId);
    if (!existing) return machine;
    upsertToolInSegment(seg, toolCallId, {
      unparsedInput: (existing.unparsedInput ?? "") + (event.delta ?? ""),
    });
    return { phase: phaseNext, context: ctx };
  }

  if (phase === "input_complete" || (!phase && event.toolName)) {
    ensureStreaming();
    const seg = ctx.openSegment ?? findToolSegment(ctx, toolCallId);
    if (!seg || !event.toolName) {
      if (ctx.openSegment && event.toolName) {
        upsertToolInSegment(ctx.openSegment, toolCallId, {
          name: event.toolName,
          input: event.input ?? {},
          status: event.needsApproval ? "pending" : "running",
          needsApproval: !!event.needsApproval,
          parsedInput: event.input,
        });
        return { phase: phaseNext, context: ctx };
      }
      return machine;
    }
    upsertToolInSegment(seg, toolCallId, {
      name: event.toolName,
      input: event.input ?? {},
      status: event.needsApproval ? "pending" : "running",
      needsApproval: !!event.needsApproval,
      parsedInput: event.input,
    });
    return { phase: phaseNext, context: ctx };
  }

  if (phase === "progress") {
    const seg = findToolSegment(ctx, toolCallId) ?? ctx.openSegment;
    if (!seg) return machine;
    upsertToolInSegment(seg, toolCallId, {
      progressDescription: event.description ?? event.status,
    });
    return { phase: machine.phase, context: ctx };
  }

  if (phase === "result") {
    if (machine.phase !== "segmentStreaming" && machine.phase !== "segmentCommitted") {
      return machine;
    }
    let seg = ctx.openSegment;
    if (!seg) {
      const turn = currentTurn(ctx);
      if (turn && turn.segments.length > 0) {
        seg = turn.segments[turn.segments.length - 1];
      }
    }
    if (!seg) return machine;
    const existing = seg.tools.find((t) => t.id === toolCallId);
    upsertToolInSegment(seg, toolCallId, {
      name: existing?.name ?? event.toolName ?? "Tool",
      input: event.input ?? existing?.input ?? {},
      status: "done",
      needsApproval: existing?.needsApproval ?? false,
      result: event.content,
    });
    return { phase: machine.phase, context: ctx };
  }

  return machine;
}

function handleStreamChunk(
  machine: TranscriptMachine,
  event: Extract<TranscriptEvent, { type: "STREAM_CHUNK" }>,
): TranscriptMachine {
  if (machine.phase === "idle" || machine.phase === "pausedForQuestion") {
    return machine;
  }

  const ctx = cloneContext(machine.context);
  ctx.streamingMessageId = event.messageId;

  let phase = machine.phase;
  if (machine.phase === "segmentCommitted") {
    const segIdx = nextSegmentIndex(ctx);
    ensureOpenSegment(ctx, segIdx, event.messageId);
    phase = "segmentStreaming";
  } else if (!ctx.openSegment) {
    const segIdx = nextSegmentIndex(ctx);
    ensureOpenSegment(ctx, segIdx, event.messageId);
  }

  const seg = ctx.openSegment!;
  const kind = event.kind === "thinking" ? "thinking" : "text";
  const blocks = [...seg.assistant.contentBlocks];
  const last = blocks[blocks.length - 1];
  if (last && last.kind === kind) {
    blocks[blocks.length - 1] = { ...last, text: last.text + event.delta };
  } else {
    blocks.push({ blockIndex: blocks.length, kind, text: event.delta });
  }
  seg.assistant = { ...seg.assistant, status: "streaming", contentBlocks: blocks };

  return { phase, context: ctx };
}

export function dispatchTranscriptEvent(
  machine: TranscriptMachine,
  event: TranscriptEvent,
): TranscriptMachine {
  switch (event.type) {
    case "BEGIN_TURN": {
      const ctx = cloneContext(machine.context);
      ctx.turns.push({
        turnId: event.user.id,
        user: event.user,
        segments: [],
        reports: [],
      });
      ctx.openSegment = null;
      ctx.streamingMessageId = null;
      const segIdx = 0;
      ensureOpenSegment(ctx, segIdx, null);
      return { phase: "segmentStreaming", context: ctx };
    }

    case "STREAM_CHUNK":
      return handleStreamChunk(machine, event);

    case "TOOL":
      return handleToolEvent(machine, event);

    case "SEGMENT_COMPLETE": {
      if (machine.phase !== "segmentStreaming" && machine.phase !== "segmentCommitted") {
        return machine;
      }
      const ctx = cloneContext(machine.context);
      if (ctx.openSegment) {
        commitOpenSegment(ctx);
      }
      return { phase: "segmentCommitted", context: ctx };
    }

    case "ASK_USER_QUESTION": {
      if (machine.phase !== "segmentStreaming" && machine.phase !== "segmentCommitted") {
        return machine;
      }
      const ctx = cloneContext(machine.context);
      const lastTurn = ctx.turns.length > 0 ? ctx.turns[ctx.turns.length - 1] : undefined;
      const segId = ctx.openSegment
        ? commitOpenSegment(ctx)
        : lastTurn && lastTurn.segments.length > 0
          ? lastTurn.segments[lastTurn.segments.length - 1].segmentId
          : undefined;
      const turn = currentTurn(ctx);
      if (turn && segId) {
        turn.pauseAfterSegmentId = segId;
      }
      return { phase: "pausedForQuestion", context: ctx };
    }

    case "ANSWER_QUESTION": {
      if (machine.phase !== "pausedForQuestion") {
        return machine;
      }
      const ctx = cloneContext(machine.context);
      const turn = currentTurn(ctx);
      if (turn) {
        turn.pauseAfterSegmentId = undefined;
      }
      return { phase: "segmentCommitted", context: ctx };
    }

    case "TURN_COMPLETE":
    case "INTERRUPT": {
      const ctx = cloneContext(machine.context);
      if (ctx.openSegment) {
        commitOpenSegment(ctx);
      }
      ctx.openSegment = null;
      ctx.streamingMessageId = null;
      const turn = currentTurn(ctx);
      if (turn) {
        turn.pauseAfterSegmentId = undefined;
      }
      return { phase: "idle", context: ctx };
    }

    case "HYDRATE": {
      const { machine: hydrated } = flatMessagesToMachine(event.flatMessages);
      return hydrated;
    }

    case "PATCH_TOOL": {
      if (machine.phase === "idle") return machine;
      const ctx = cloneContext(machine.context);
      const seg = findToolSegment(ctx, event.toolCallId);
      if (!seg) return machine;
      upsertToolInSegment(seg, event.toolCallId, event.patch);
      return { ...machine, context: ctx };
    }

    default:
      return machine;
  }
}

export function flatMessagesToMachine(flatMessages: UIMessage[]): {
  machine: TranscriptMachine;
} {
  const ctx: TranscriptContext = {
    turns: [],
    openSegment: null,
    streamingMessageId: null,
  };

  let currentTurn: Turn | null = null;
  let currentSegment: LlmSegment | null = null;
  let segmentIndex = 0;

  for (const msg of flatMessages) {
    if (msg.role === "user") {
      currentTurn = {
        turnId: msg.id,
        user: msg,
        segments: [],
        reports: [],
      };
      ctx.turns.push(currentTurn);
      currentSegment = null;
      segmentIndex = 0;
      continue;
    }
    if (msg.role === "subAgentReport") {
      if (currentTurn) currentTurn.reports.push(msg);
      continue;
    }
    if (msg.role === "assistant") {
      if (!currentTurn) {
        currentTurn = {
          turnId: SYNTHETIC_USER_ID,
          user: { id: SYNTHETIC_USER_ID, role: "user", contentBlocks: [] },
          segments: [],
          reports: [],
        };
        ctx.turns.push(currentTurn);
      }
      const hasContent = assistantHasContent(msg.contentBlocks);
      currentSegment = {
        segmentId: `${currentTurn.turnId}:${segmentIndex}`,
        segmentIndex,
        assistant: {
          id: msg.id,
          status: hasContent ? "committed" : "placeholder",
          contentBlocks: msg.contentBlocks,
        },
        tools: [],
      };
      currentTurn.segments.push(currentSegment);
      segmentIndex++;
      continue;
    }
    if (msg.role === "tool") {
      if (!currentTurn) {
        currentTurn = {
          turnId: SYNTHETIC_USER_ID,
          user: { id: SYNTHETIC_USER_ID, role: "user", contentBlocks: [] },
          segments: [],
          reports: [],
        };
        ctx.turns.push(currentTurn);
      }
      if (!currentSegment) {
        currentSegment = {
          segmentId: `${currentTurn.turnId}:${segmentIndex}`,
          segmentIndex,
          assistant: {
            id: `placeholder-${segmentIndex}`,
            status: "placeholder",
            contentBlocks: [],
          },
          tools: [],
        };
        currentTurn.segments.push(currentSegment);
        segmentIndex++;
      }
      const toolId = msg.id.replace(/^tool-/, "");
      currentSegment.tools.push({
        id: toolId,
        name: msg.toolName ?? "Tool",
        input: msg.toolInput ?? {},
        status: (msg.toolStatus as ToolCall["status"]) ?? "done",
        needsApproval: !!msg.needsApproval,
        result: msg.contentBlocks[0]?.text,
      });
    }
  }

  return { machine: { phase: "idle", context: ctx } };
}
