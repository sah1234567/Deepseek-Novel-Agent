import { describe, expect, it } from "vitest";
import { userMsg } from "../test/fixtures/transcript";
import {
  extractActivatedSkillLabels,
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
    });
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
