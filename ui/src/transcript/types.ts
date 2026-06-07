import type { ContentBlock, ToolCall, UIMessage } from "../types/messages";

export type AssistantStatus = "streaming" | "committed" | "placeholder";

export type TranscriptPhase =
  | "idle"
  | "segmentStreaming"
  | "segmentCommitted"
  | "pausedForQuestion";

export interface SegmentAssistant {
  id: string;
  status: AssistantStatus;
  contentBlocks: ContentBlock[];
}

export interface LlmSegment {
  segmentId: string;
  segmentIndex: number;
  assistant: SegmentAssistant;
  tools: ToolCall[];
}

export interface Turn {
  turnId: string;
  /** DB `turn_number`; omitted on live streaming turn until tail reload. */
  turnNumber?: number;
  /** Set when this turn was loaded from `message_archive` (disambiguates turn numbers). */
  archiveEpoch?: number;
  user: UIMessage;
  segments: LlmSegment[];
  reports: UIMessage[];
  pauseAfterSegmentId?: string;
}

export interface TranscriptContext {
  turns: Turn[];
  openSegment: LlmSegment | null;
  streamingMessageId: string | null;
}

export interface TranscriptMachine {
  phase: TranscriptPhase;
  context: TranscriptContext;
}

export type TranscriptEvent =
  | { type: "BEGIN_TURN"; user: UIMessage }
  | { type: "STREAM_CHUNK"; messageId: string; delta: string; kind: string }
  | {
      type: "TOOL";
      phase?: string;
      toolCallId: string;
      toolName?: string;
      input?: unknown;
      needsApproval?: boolean;
      delta?: string;
      content?: string;
      status?: string;
      description?: string;
    }
  | { type: "SEGMENT_COMPLETE"; segmentIndex: number }
  | { type: "ASK_USER_QUESTION" }
  | { type: "ANSWER_QUESTION" }
  | { type: "TURN_COMPLETE" }
  | { type: "INTERRUPT" }
  | { type: "RESET_TRANSCRIPT" }
  | {
      type: "MERGE_TURNS";
      bundles: import("./service").UiTurnBundle[];
      archiveEpoch?: number;
    }
  | {
      type: "EVICT_TURNS";
      turns: { turnNumber: number; archiveEpoch?: number }[];
    }
  | { type: "PATCH_TOOL"; toolCallId: string; patch: Partial<ToolCall> };

export const SYNTHETIC_USER_ID = "__synthetic_user__";

export function isSyntheticUser(user: UIMessage): boolean {
  return user.id === SYNTHETIC_USER_ID;
}

export function segmentMessageId(baseId: string, segmentIndex: number): string {
  return segmentIndex === 0 ? baseId : `${baseId}-seg-${segmentIndex}`;
}
