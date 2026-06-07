import type { TranscriptMachine } from "../transcript/types";

export interface ContentBlock {
  blockIndex: number;
  kind: string;
  text: string;
}

export interface ToolCall {
  id: string;
  name: string;
  input: unknown;
  status: "streaming-args" | "pending" | "running" | "done" | "denied";
  needsApproval: boolean;
  result?: string;
  progressDescription?: string;
  unparsedInput?: string;
  parsedInput?: unknown;
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
  source: "tool" | "hook";
  /** Main-session ForkSubAgent `tool_call_id` (tool path only). */
  parentToolCallId?: string;
  machine: TranscriptMachine;
  status: "running" | "complete";
  reportOutput?: string;
}
