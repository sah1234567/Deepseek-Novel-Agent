import type { ContentBlock, ToolCall, UIMessage } from "../hooks/useAgent";

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
  | { type: "HYDRATE"; flatMessages: UIMessage[] }
  | { type: "PATCH_TOOL"; toolCallId: string; patch: Partial<ToolCall> };

export const SYNTHETIC_USER_ID = "__synthetic_user__";
export const CONTEXT_REFRESH_PREFIX = "[上下文刷新]";

export function isContextRefreshUser(user: UIMessage): boolean {
  if (user.messageKind === "contextRefresh") return true;
  const text = user.contentBlocks.find((b) => b.kind === "text")?.text ?? "";
  return text.startsWith(CONTEXT_REFRESH_PREFIX);
}

export function parseContextRefreshSections(text: string): { skill: string; summary: string } {
  const body = text.startsWith(CONTEXT_REFRESH_PREFIX)
    ? text.slice(CONTEXT_REFRESH_PREFIX.length).trimStart()
    : text;
  const skillMarker = "## 激活 Skill";
  const summaryMarker = "## 会话历史摘要";
  const summaryIdx = body.indexOf(summaryMarker);
  let skill = "";
  let summary = "";
  if (summaryIdx >= 0) {
    summary = body.slice(summaryIdx + summaryMarker.length).trimStart();
    const skillSection = body.slice(0, summaryIdx);
    if (skillSection.includes(skillMarker)) {
      skill = skillSection.slice(skillSection.indexOf(skillMarker) + skillMarker.length).trim();
    }
  } else if (body.includes(skillMarker)) {
    skill = body.slice(body.indexOf(skillMarker) + skillMarker.length).trim();
  } else {
    summary = body.trim();
  }
  return { skill, summary };
}

export function isSyntheticUser(user: UIMessage): boolean {
  return user.id === SYNTHETIC_USER_ID;
}

export function segmentMessageId(baseId: string, segmentIndex: number): string {
  return segmentIndex === 0 ? baseId : `${baseId}-seg-${segmentIndex}`;
}
