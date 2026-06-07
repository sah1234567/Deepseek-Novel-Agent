import type { ContentBlock, ToolCall } from "../types/messages";
import {
  cloneSegment,
  updateToolSegment,
  withOpenSegment,
  type ToolSegmentLocation,
} from "./mutate";
import { isLiveOrphanTurn, reconcileOrphanLiveTurns } from "./liveTail";
import { evictTurnsFromMachine, mergeTurnsIntoMachine } from "./merge";
import {
  segmentMessageId,
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

/** Prefer live orphan (optimistic tail); must match findLiveTailTurn / merge sort order. */
function currentTurn(ctx: TranscriptContext): Turn | undefined {
  for (let i = ctx.turns.length - 1; i >= 0; i--) {
    if (isLiveOrphanTurn(ctx.turns[i])) {
      return ctx.turns[i];
    }
  }
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
  const loc = findToolSegmentLocation(ctx, toolCallId);
  if (!loc) return null;
  if (loc.kind === "open") return ctx.openSegment;
  return ctx.turns[loc.turnIndex]?.segments[loc.segmentIndex] ?? null;
}

function findToolSegmentLocation(
  ctx: TranscriptContext,
  toolCallId: string,
): ToolSegmentLocation | null {
  if (ctx.openSegment?.tools.some((x) => x.id === toolCallId)) {
    return { kind: "open" };
  }
  for (let ti = ctx.turns.length - 1; ti >= 0; ti--) {
    const turn = ctx.turns[ti];
    for (let si = turn.segments.length - 1; si >= 0; si--) {
      if (turn.segments[si].tools.some((x) => x.id === toolCallId)) {
        return { kind: "committed", turnIndex: ti, segmentIndex: si };
      }
    }
  }
  return null;
}

function createOpenSegmentImmutable(
  ctx: TranscriptContext,
  segmentIndex: number,
  messageId?: string | null,
): LlmSegment {
  const turn = currentTurn(ctx);
  const turnId = turn?.turnId ?? "orphan";
  const baseId = messageId ?? ctx.streamingMessageId ?? `assistant-${turnId}`;
  return {
    segmentId: `${turnId}:${segmentIndex}`,
    segmentIndex,
    assistant: {
      id: segmentMessageId(baseId, segmentIndex),
      status: "streaming",
      contentBlocks: [],
    },
    tools: [],
  };
}

function resolveToolSegment(ctx: TranscriptContext, toolCallId: string): LlmSegment | null {
  return findToolSegment(ctx, toolCallId) ?? ctx.openSegment;
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
    if (findToolSegment(ctx, toolCallId)) {
      return machine;
    }
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
    let loc = findToolSegmentLocation(machine.context, toolCallId);
    if (!loc) {
      ensureStreaming();
      loc = findToolSegmentLocation(ctx, toolCallId);
      if (!loc) {
        const seg = resolveToolSegment(ctx, toolCallId);
        if (!seg) return machine;
        const existing = seg.tools.find((t) => t.id === toolCallId);
        if (!existing) return machine;
        upsertToolInSegment(seg, toolCallId, {
          unparsedInput: (existing.unparsedInput ?? "") + (event.delta ?? ""),
        });
        return { phase: phaseNext, context: ctx };
      }
    } else {
      const seg = findToolSegment(machine.context, toolCallId);
      if (!seg) return machine;
      const existing = seg.tools.find((t) => t.id === toolCallId);
      if (!existing) return machine;
      const delta = (existing.unparsedInput ?? "") + (event.delta ?? "");
      return updateToolSegment(machine, loc, (s) => {
        const copy = cloneSegment(s);
        upsertToolInSegment(copy, toolCallId, { unparsedInput: delta });
        return copy;
      });
    }
    const seg = resolveToolSegment(ctx, toolCallId);
    if (!seg) return machine;
    const existing = seg.tools.find((t) => t.id === toolCallId);
    if (!existing) return machine;
    upsertToolInSegment(seg, toolCallId, {
      unparsedInput: (existing.unparsedInput ?? "") + (event.delta ?? ""),
    });
    return { phase: phaseNext, context: ctx };
  }

  if (phase === "input_complete" || (!phase && event.toolName)) {
    if (!findToolSegment(ctx, toolCallId)) {
      ensureStreaming();
    }
    const seg = resolveToolSegment(ctx, toolCallId);
    if (!seg || !event.toolName) {
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
    const seg = resolveToolSegment(ctx, toolCallId);
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
    let seg = findToolSegment(ctx, toolCallId);
    // Orphan result (no prior start): attach to last committed segment only when no live openSegment.
    if (!seg && !ctx.openSegment) {
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

  let phase = machine.phase;
  const ctx = machine.context;
  let openSegment = ctx.openSegment;

  if (phase === "segmentCommitted" || !openSegment) {
    const segIdx = nextSegmentIndex(ctx);
    openSegment = createOpenSegmentImmutable(
      { ...ctx, streamingMessageId: event.messageId },
      segIdx,
      event.messageId,
    );
    if (phase === "segmentCommitted") {
      phase = "segmentStreaming";
    }
  }

  const kind = event.kind === "thinking" ? "thinking" : "text";
  const seg = cloneSegment(openSegment);
  const blocks = [...seg.assistant.contentBlocks];
  const last = blocks[blocks.length - 1];
  if (last && last.kind === kind) {
    blocks[blocks.length - 1] = { ...last, text: last.text + event.delta };
  } else {
    blocks.push({ blockIndex: blocks.length, kind, text: event.delta });
  }
  seg.assistant = { ...seg.assistant, status: "streaming", contentBlocks: blocks };

  return withOpenSegment(machine, phase, seg, event.messageId);
}

export function dispatchTranscriptEvent(
  machine: TranscriptMachine,
  event: TranscriptEvent,
): TranscriptMachine {
  switch (event.type) {
    case "BEGIN_TURN": {
      const ctx = cloneContext(machine.context);
      ctx.turns = reconcileOrphanLiveTurns(ctx.turns, machine.phase);
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

    case "RESET_TRANSCRIPT":
      return createInitialMachine();

    case "MERGE_TURNS": {
      const merged = mergeTurnsIntoMachine(machine, event.bundles, event.archiveEpoch);
      return {
        ...merged,
        context: {
          ...merged.context,
          turns: reconcileOrphanLiveTurns(merged.context.turns, merged.phase),
        },
      };
    }

    case "EVICT_TURNS":
      return evictTurnsFromMachine(machine, event.turns);

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
