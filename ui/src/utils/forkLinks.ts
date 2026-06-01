import type { ForkRunState, UIMessage } from "../hooks/useAgent";

export interface ForkLink {
  forkRunId: string;
  reportContent: string;
}

export function stripSubAgentReportPrefix(content: string): string {
  return content.replace(/^\[子 Agent 完成:\s*\w+\]\s*\n?/, "").trim();
}

/** Pair ForkSubAgent tool rows with subAgentReport flat rows for hydrate; UI renders via SubAgentForkCard. */
export function buildForkSubAgentLinks(messages: UIMessage[]): Map<string, ForkLink> {
  const links = new Map<string, ForkLink>();
  const forkTools = messages.filter((m) => m.role === "tool" && m.toolName === "ForkSubAgent");
  const reports = messages.filter((m) => m.role === "subAgentReport");

  for (let i = 0; i < forkTools.length; i++) {
    const tool = forkTools[i];
    const report = reports[i];
    if (!report?.forkRunId) continue;
    const fullText = report.contentBlocks.map((b) => b.text ?? "").join("\n");
    links.set(tool.id, {
      forkRunId: report.forkRunId,
      reportContent: stripSubAgentReportPrefix(fullText),
    });
  }
  return links;
}

export function forkSubAgentToolMessages(messages: UIMessage[]): UIMessage[] {
  return messages.filter((m) => m.role === "tool" && m.toolName === "ForkSubAgent");
}

export function toolPathForkRunIds(forkRuns: Map<string, ForkRunState>): string[] {
  return [...forkRuns.entries()]
    .filter(([, r]) => r.source === "tool")
    .map(([id]) => id);
}

/** Resolve forkRunId for a ForkSubAgent tool row (hydrate links or live start order). */
export function resolveForkRunIdForTool(
  toolMsgId: string,
  messages: UIMessage[],
  links: Map<string, ForkLink>,
  forkRuns: Map<string, ForkRunState>,
): string | undefined {
  const linked = links.get(toolMsgId);
  if (linked) return linked.forkRunId;

  const toolMsgs = forkSubAgentToolMessages(messages);
  const idx = toolMsgs.findIndex((m) => m.id === toolMsgId);
  if (idx < 0) return undefined;
  const runIds = toolPathForkRunIds(forkRuns);
  return runIds[idx];
}

export function listHookForkRuns(forkRuns: Map<string, ForkRunState>): ForkRunState[] {
  return [...forkRuns.values()].filter((r) => r.source === "hook");
}

export function agentLabelFromType(agentType: string): string {
  const t = agentType.toLowerCase();
  if (t.includes("knowledgeauditor")) return "KnowledgeAuditor";
  if (t.includes("chaptercraft")) return "ChapterCraftAnalyzer";
  if (t.includes("general")) return "GeneralPurpose";
  return agentType || "Subagent";
}
