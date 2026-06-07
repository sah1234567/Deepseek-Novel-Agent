import type { PendingQuestion } from "../types/messages";
import type { TranscriptMachine } from "./types";
import { transcriptToFlatMessages } from "./convert";

/** Flat message list derived from FSM context (used for fork linking and hydrate parity). */
export function flatMessagesFromMachine(machine: TranscriptMachine) {
  return transcriptToFlatMessages(machine);
}

function toolBindingToken(tool: {
  id: string;
  status: string;
  unparsedInput?: string;
  result?: string;
}): string {
  return `${tool.id}:${tool.status}:${tool.unparsedInput?.length ?? 0}:${tool.result?.length ?? 0}`;
}

/** Changes when fork cards need fresh tool/report rows; stable during assistant text streaming. */
export function forkBindingSnapshotKey(machine: TranscriptMachine): string {
  const parts: string[] = [machine.phase];
  const ctx = machine.context;
  for (const turn of ctx.turns) {
    for (const report of turn.reports) {
      parts.push(`r:${report.id}:${report.forkRunId ?? ""}`);
    }
    for (const seg of turn.segments) {
      for (const tool of seg.tools) {
        parts.push(toolBindingToken(tool));
      }
    }
  }
  if (ctx.openSegment) {
    for (const tool of ctx.openSegment.tools) {
      parts.push(`o:${toolBindingToken(tool)}`);
    }
  }
  return parts.join("|");
}

export function hasPendingApproval(machine: TranscriptMachine): boolean {
  const ctx = machine.context;
  const checkTools = (tools: { status: string; needsApproval: boolean }[]) =>
    tools.some((t) => t.needsApproval && t.status === "pending");

  if (ctx.openSegment && checkTools(ctx.openSegment.tools)) return true;
  for (const turn of ctx.turns) {
    for (const seg of turn.segments) {
      if (checkTools(seg.tools)) return true;
    }
  }
  return false;
}

export function isTurnInProgress(machine: TranscriptMachine, pendingQuestion: PendingQuestion | null): boolean {
  if (pendingQuestion) return true;
  if (machine.phase !== "idle") return true;
  return hasPendingApproval(machine);
}

export function pauseSegmentId(machine: TranscriptMachine): string | undefined {
  const turns = machine.context.turns;
  const turn = turns.length > 0 ? turns[turns.length - 1] : undefined;
  return turn?.pauseAfterSegmentId;
}

export function isStreamingPhase(machine: TranscriptMachine): boolean {
  return machine.phase === "segmentStreaming";
}
