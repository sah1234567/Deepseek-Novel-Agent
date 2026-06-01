/** Display label for fork agent type strings from backend. */
export function agentLabelFromType(agentType: string): string {
  const t = agentType.toLowerCase();
  if (t.includes("knowledgeauditor")) return "KnowledgeAuditor";
  if (t.includes("chaptercraft")) return "ChapterCraftAnalyzer";
  if (t.includes("general")) return "GeneralPurpose";
  return agentType || "Subagent";
}
