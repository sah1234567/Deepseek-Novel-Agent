import type { UIMessage } from "../../hooks/useAgent";
import { createInitialMachine, dispatchTranscriptEvent } from "../../transcript/machine";
import { SYNTHETIC_USER_ID } from "../../transcript/types";
import type { TranscriptEvent, TranscriptMachine } from "../../transcript/types";
import { mapSegmentComplete, mapStreamChunk, mapToolCallRequest } from "../../transcript/mapEvents";

export const userMsg = (id: string, text = "hello"): UIMessage => ({
  id,
  role: "user",
  contentBlocks: [{ blockIndex: 0, kind: "text", text }],
});

export function applyEvents(
  machine: TranscriptMachine,
  events: TranscriptEvent[],
): TranscriptMachine {
  return events.reduce((m, e) => dispatchTranscriptEvent(m, e), machine);
}

export function scenario(events: TranscriptEvent[]): TranscriptMachine {
  return applyEvents(createInitialMachine(), events);
}

export function streamText(messageId: string, delta: string) {
  return mapStreamChunk({ messageId, delta, kind: "text" });
}

export function streamThinking(messageId: string, delta: string) {
  return mapStreamChunk({ messageId, delta, kind: "thinking" });
}

export function toolStart(id: string, name: string) {
  return mapToolCallRequest({ toolCallId: id, toolName: name, phase: "start" })!;
}

export function toolDelta(id: string, delta: string) {
  return mapToolCallRequest({ toolCallId: id, delta, phase: "input_delta" })!;
}

export function toolComplete(
  id: string,
  name: string,
  input?: unknown,
  needsApproval?: boolean,
) {
  return mapToolCallRequest({
    toolCallId: id,
    toolName: name,
    input: input ?? {},
    needsApproval,
    phase: "input_complete",
  })!;
}

export function toolResult(id: string, content: string, toolName?: string) {
  return mapToolCallRequest({
    toolCallId: id,
    content,
    toolName,
    phase: "result",
  })!;
}

export function segmentComplete(index: number) {
  return mapSegmentComplete(index);
}

export function beginTurn(user: UIMessage): TranscriptEvent {
  return { type: "BEGIN_TURN", user };
}

/** Keep in sync with `useAgent.ts` `forkInitialMachine()` (synthetic user + BEGIN_TURN). */
export function forkInitialMachine(): TranscriptMachine {
  return dispatchTranscriptEvent(createInitialMachine(), {
    type: "BEGIN_TURN",
    user: { id: SYNTHETIC_USER_ID, role: "user", contentBlocks: [] },
  });
}

/** Test helper: dispatch fork transcript events (mirrors useAgent fork listeners). */
export function dispatchForkEvent(
  machine: TranscriptMachine,
  event: TranscriptEvent,
): TranscriptMachine {
  return dispatchTranscriptEvent(machine, event);
}
