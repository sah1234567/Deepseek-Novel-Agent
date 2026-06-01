import type { ForkRunState, UIMessage } from "../hooks/useAgent";

export function stripSubAgentReportPrefix(content: string): string {
  return content.replace(/^\[子 Agent 完成:\s*\w+\]\s*\n?/, "").trim();
}

/** `forkRunId` → report body (hydrate / completed runs). */
export function reportContentByForkRunId(messages: UIMessage[]): Map<string, string> {
  const out = new Map<string, string>();
  for (const m of messages) {
    if (m.role !== "subAgentReport" || !m.forkRunId) continue;
    const fullText = m.contentBlocks.map((b) => b.text ?? "").join("\n");
    out.set(m.forkRunId, stripSubAgentReportPrefix(fullText));
  }
  return out;
}

export function forkRunIdByToolCallId(
  toolCallId: string,
  forkRuns: Map<string, ForkRunState>,
): string | undefined {
  for (const run of forkRuns.values()) {
    if (run.parentToolCallId === toolCallId) return run.forkRunId;
  }
  return undefined;
}

function hydrateForkRunIdByToolOrder(
  toolMsgId: string,
  messages: UIMessage[],
): string | undefined {
  const tools = messages.filter((m) => m.role === "tool" && m.toolName === "ForkSubAgent");
  const reports = messages.filter((m) => m.role === "subAgentReport" && m.forkRunId);
  const idx = tools.findIndex((m) => m.id === toolMsgId);
  if (idx < 0 || idx >= reports.length) return undefined;
  return reports[idx].forkRunId;
}

/**
 * Resolve fork run for a ForkSubAgent tool row (`toolMsgId` = `tool-{toolCallId}`).
 * Live: `parentToolCallId` on fork run. Hydrate: fall back to tool/report order.
 */
export function resolveForkRunIdForToolCard(
  toolMsgId: string,
  forkRuns: Map<string, ForkRunState>,
  messages: UIMessage[] = [],
): string | undefined {
  const toolCallId = toolMsgId.replace(/^tool-/, "");
  const live = forkRunIdByToolCallId(toolCallId, forkRuns);
  if (live) return live;
  return hydrateForkRunIdByToolOrder(toolMsgId, messages);
}

export function listHookForkRuns(forkRuns: Map<string, ForkRunState>): ForkRunState[] {
  return [...forkRuns.values()].filter((r) => r.source === "hook");
}
