export type CompactionAction =
  | "started"
  | "generating-summary"
  | "rebuilding-session"
  | "done"
  | "failed";

export interface CompactionProgressPayload {
  action: CompactionAction;
  attempt?: number;
  tokensBefore?: number;
  tokensAfter?: number;
  epoch?: number;
  retainedMinTurn?: number;
  retainedMaxTurn?: number;
  reason?: string;
}

export function compactionProgressLabel(payload: CompactionProgressPayload): string {
  switch (payload.action) {
    case "started":
      return "正在压缩上下文…";
    case "generating-summary":
      return "正在生成会话摘要…";
    case "rebuilding-session":
      return "正在重建上下文…";
    case "done": {
      const retained =
        payload.retainedMinTurn !== undefined && payload.retainedMaxTurn !== undefined
          ? payload.retainedMinTurn === payload.retainedMaxTurn
            ? `，保留 turn ${payload.retainedMinTurn}`
            : `，保留 turn ${payload.retainedMinTurn}–${payload.retainedMaxTurn}`
          : "";
      const tokens =
        payload.tokensBefore !== undefined && payload.tokensAfter !== undefined
          ? `（${payload.tokensBefore.toLocaleString()} → ${payload.tokensAfter.toLocaleString()} tokens）`
          : "";
      return `上下文已压缩${tokens}${retained}`;
    }
    case "failed":
      return payload.reason ? `压缩失败：${payload.reason}` : "压缩失败";
    default:
      return "正在压缩上下文…";
  }
}

export function compactionProgressVariant(
  action: CompactionAction,
): "info" | "success" | "warn" {
  if (action === "failed") return "warn";
  if (action === "done") return "success";
  return "info";
}
