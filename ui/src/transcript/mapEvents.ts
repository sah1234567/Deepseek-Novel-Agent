import type { TranscriptEvent } from "./types";
import type {
  StreamChunkPayload,
  ToolCallRequestPayload,
} from "./eventPayloads";

export type { StreamChunkPayload, ToolCallRequestPayload } from "./eventPayloads";

export function mapStreamChunk(payload: StreamChunkPayload): TranscriptEvent {
  return {
    type: "STREAM_CHUNK",
    messageId: payload.messageId,
    delta: payload.delta,
    kind: payload.kind,
  };
}

export function mapToolCallRequest(payload: ToolCallRequestPayload): TranscriptEvent | null {
  if (!payload.toolCallId) return null;
  let phase = payload.phase;
  if (!phase && payload.content !== undefined) {
    phase = "result";
  } else if (!phase && payload.delta) {
    phase = "input_delta";
  } else if (!phase && payload.toolName) {
    phase = "input_complete";
  }
  const delta =
    payload.delta ??
    (phase === "input_delta" ? payload.content : undefined);
  return {
    type: "TOOL",
    phase,
    toolCallId: payload.toolCallId,
    toolName: payload.toolName,
    input: payload.input,
    needsApproval: payload.needsApproval,
    delta,
    content: payload.content,
    status: payload.status,
    description: payload.description,
  };
}

export function mapSegmentComplete(segmentIndex: number): TranscriptEvent {
  return { type: "SEGMENT_COMPLETE", segmentIndex };
}
