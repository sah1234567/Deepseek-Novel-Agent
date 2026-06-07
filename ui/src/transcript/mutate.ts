import type { LlmSegment, TranscriptContext, TranscriptMachine, Turn } from "./types";

/** Deep-clone a single segment (assistant blocks + tools). */
export function cloneSegment(seg: LlmSegment): LlmSegment {
  return {
    ...seg,
    assistant: {
      ...seg.assistant,
      contentBlocks: [...seg.assistant.contentBlocks],
    },
    tools: seg.tools.map((t) => ({ ...t })),
  };
}

/** Shallow context patch; `turns` keeps prior reference unless overridden. */
export function patchContext(
  ctx: TranscriptContext,
  patch: Partial<TranscriptContext>,
): TranscriptContext {
  return {
    turns: patch.turns ?? ctx.turns,
    openSegment: patch.openSegment !== undefined ? patch.openSegment : ctx.openSegment,
    streamingMessageId:
      patch.streamingMessageId !== undefined ? patch.streamingMessageId : ctx.streamingMessageId,
  };
}

export type ToolSegmentLocation =
  | { kind: "open" }
  | { kind: "committed"; turnIndex: number; segmentIndex: number };

/** Replace only `openSegment`; committed turns stay referentially equal. */
export function withOpenSegment(
  machine: TranscriptMachine,
  phase: TranscriptMachine["phase"],
  openSegment: LlmSegment,
  streamingMessageId?: string | null,
): TranscriptMachine {
  return {
    phase,
    context: patchContext(machine.context, {
      openSegment,
      streamingMessageId:
        streamingMessageId !== undefined ? streamingMessageId : machine.context.streamingMessageId,
    }),
  };
}

/** Replace one committed segment; other turns/segments keep prior references. */
export function withCommittedSegment(
  machine: TranscriptMachine,
  turnIndex: number,
  segmentIndex: number,
  segment: LlmSegment,
): TranscriptMachine {
  const ctx = machine.context;
  const turns = ctx.turns.map((turn, ti): Turn => {
    if (ti !== turnIndex) return turn;
    return {
      ...turn,
      segments: turn.segments.map((seg, si) => (si === segmentIndex ? segment : seg)),
    };
  });
  return {
    phase: machine.phase,
    context: patchContext(ctx, { turns }),
  };
}

export function updateOpenSegment(
  machine: TranscriptMachine,
  phase: TranscriptMachine["phase"],
  updater: (seg: LlmSegment) => LlmSegment,
  streamingMessageId?: string | null,
): TranscriptMachine {
  const open = machine.context.openSegment;
  if (!open) return machine;
  return withOpenSegment(machine, phase, updater(cloneSegment(open)), streamingMessageId);
}

export function updateToolSegment(
  machine: TranscriptMachine,
  location: ToolSegmentLocation,
  updater: (seg: LlmSegment) => LlmSegment,
): TranscriptMachine {
  if (location.kind === "open") {
    return updateOpenSegment(machine, machine.phase, updater);
  }
  const seg = machine.context.turns[location.turnIndex]?.segments[location.segmentIndex];
  if (!seg) return machine;
  return withCommittedSegment(
    machine,
    location.turnIndex,
    location.segmentIndex,
    updater(cloneSegment(seg)),
  );
}
