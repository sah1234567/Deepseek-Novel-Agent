import type { UIMessage } from "../types/messages";

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

/** Skill ids from `### {id}` headings in the activated-skill block (omits reference paths). */
export function extractActivatedSkillLabels(skillSection: string): string[] {
  if (!skillSection.trim()) return [];
  const labels: string[] = [];
  const seen = new Set<string>();
  for (const line of skillSection.split("\n")) {
    const m = line.match(/^###\s+(.+?)\s*$/);
    if (!m) continue;
    const id = m[1];
    if (id.includes("/references/")) continue;
    if (seen.has(id)) continue;
    seen.add(id);
    labels.push(id);
  }
  return labels;
}

/** One-line preview for collapsed context-refresh bubble. */
export function summaryPreview(summary: string, maxLen = 96): string {
  const flat = summary.replace(/\s+/g, " ").trim();
  if (!flat) return "";
  if (flat.length <= maxLen) return flat;
  return `${flat.slice(0, maxLen)}…`;
}
