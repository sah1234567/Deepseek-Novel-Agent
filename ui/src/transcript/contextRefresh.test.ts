import { describe, expect, it } from "vitest";
import { userMsg } from "../test/fixtures/transcript";
import {
  contextRefreshPreview,
  extractActivatedSkillLabels,
  extractAuditStatusSection,
  isContextRefreshUser,
  parseContextRefreshSections,
  summaryPreview,
} from "./contextRefresh";

describe("contextRefresh", () => {
  it("isContextRefreshUser detects merged refresh bubble", () => {
    const user = userMsg("ctx");
    user.messageKind = "contextRefresh";
    user.contentBlocks = [{ blockIndex: 0, kind: "text", text: "[上下文刷新]\n## 会话历史摘要\nx" }];
    expect(isContextRefreshUser(user)).toBe(true);
  });

  it("parseContextRefreshSections splits skill and summary", () => {
    const text =
      "[上下文刷新]\n## 激活 Skill\nskill body\n\n## 会话历史摘要\nsummary body";
    expect(parseContextRefreshSections(text)).toEqual({
      skill: "skill body",
      summary: "summary body",
      auditStatus: "",
    });
  });

  it("extractAuditStatusSection pulls audit block from summary", () => {
    const summary = "## 创作进度\nwriting\n\n## 审计状态\nCh1 PA 已通过\n\n## 情节与正文\nplot";
    const { audit, restSummary } = extractAuditStatusSection(summary);
    expect(audit).toContain("Ch1 PA");
    expect(restSummary).not.toContain("审计状态");
    expect(restSummary).toContain("创作进度");
  });

  it("contextRefreshPreview prefers audit line", () => {
    expect(contextRefreshPreview("Ch5 待审", "long summary")).toMatch(/^审计：/);
  });

  it("extractActivatedSkillLabels reads skill headings", () => {
    const skill = "### write-chapter\nbody\n### revision/references/foo.md\nref";
    expect(extractActivatedSkillLabels(skill)).toEqual(["write-chapter"]);
  });

  it("summaryPreview truncates long summary", () => {
    expect(summaryPreview("a".repeat(120), 40)).toMatch(/…$/);
    expect(summaryPreview("short")).toBe("short");
  });
});
