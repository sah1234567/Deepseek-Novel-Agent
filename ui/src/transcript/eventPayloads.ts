/** Canonical Tauri event payload shapes for transcript mapping. */
export interface StreamChunkPayload {
  messageId: string;
  blockIndex?: number;
  delta: string;
  kind: string;
}

export interface ToolCallRequestPayload {
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

export interface TurnCompletePayload {
  cacheHitTokens?: number;
  cacheMissTokens?: number;
  completionTokens?: number;
  turnHitTokens?: number;
  turnMissTokens?: number;
  turnCompTokens?: number;
  phase?: string;
  message?: string;
  wasInterrupted?: boolean;
}
