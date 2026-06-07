import type { UIMessage } from "../types/messages";

export const CONTEXT_REFRESH_PREFIX = "[上下文刷新]";
const AUDIT_STATUS_MARKER = "## 审计状态";

export function isContextRefreshUser(user: UIMessage): boolean {
  if (user.messageKind === "contextRefresh") return true;
  const text = user.contentBlocks.find((b) => b.kind === "text")?.text ?? "";
  return text.startsWith(CONTEXT_REFRESH_PREFIX);
}

export function parseContextRefreshSections(text: string): {
  skill: string;
  summary: string;
  auditStatus: string;
} {
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
  const { audit, restSummary } = extractAuditStatusSection(summary);
  return { skill, summary: restSummary, auditStatus: audit };
}

/** Pull `## 审计状态` block out of compaction summary markdown. */
export function extractAuditStatusSection(summary: string): {
  audit: string;
  restSummary: string;
} {
  const idx = summary.indexOf(AUDIT_STATUS_MARKER);
  if (idx < 0) {
    return { audit: "", restSummary: summary };
  }
  const afterMarker = summary.slice(idx + AUDIT_STATUS_MARKER.length);
  const nextH2 = afterMarker.search(/\n## /);
  const audit = (nextH2 >= 0 ? afterMarker.slice(0, nextH2) : afterMarker).trim();
  const before = summary.slice(0, idx).trimEnd();
  const after = nextH2 >= 0 ? afterMarker.slice(nextH2).trimStart() : "";
  const restSummary = [before, after].filter(Boolean).join("\n\n");
  return { audit, restSummary };
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

/** Collapsed preview: prefer audit status line when present. */
export function contextRefreshPreview(auditStatus: string, summary: string, maxLen = 96): string {
  const auditLine = summaryPreview(auditStatus, maxLen);
  if (auditLine) return `审计：${auditLine}`;
  return summaryPreview(summary, maxLen);
}
