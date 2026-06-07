import { describe, expect, it } from "vitest";
import { extractToolPath, formatToolInput, formatToolSummary } from "./tools";

describe("extractToolPath", () => {
  it("reads file_path from input", () => {
    expect(extractToolPath({ file_path: "chapters/ch01.md" })).toBe("chapters/ch01.md");
  });

  it("returns null for empty input", () => {
    expect(extractToolPath(null)).toBeNull();
  });
});

describe("formatToolSummary", () => {
  it("includes file_path for Read tool", () => {
    expect(formatToolSummary("Read", { file_path: "knowledge/INDEX.md" })).toBe(
      "Read: knowledge/INDEX.md",
    );
  });

  it("includes skill_id for InvokeSkill", () => {
    expect(formatToolSummary("InvokeSkill", { skill_id: "write-chapter" })).toBe("write-chapter");
  });

  it("includes question count for AskUserQuestion", () => {
    expect(formatToolSummary("AskUserQuestion", { questions: [{ id: "q1" }, { id: "q2" }] })).toBe(
      "2 题",
    );
  });

  it("includes agent_type for ForkSubAgent", () => {
    expect(formatToolSummary("ForkSubAgent", { agent_type: "KnowledgeAuditor" })).toBe(
      "KnowledgeAuditor",
    );
  });
});

describe("formatToolInput", () => {
  it("formats Read with offset and limit", () => {
    expect(formatToolInput("Read", { file_path: "a.md", offset: 10, limit: 20 })).toContain("a.md");
    expect(formatToolInput("Read", { file_path: "a.md", offset: 10, limit: 20 })).toContain("L10");
  });

  it("formats ForkSubAgent with agent and task preview", () => {
    expect(formatToolInput("ForkSubAgent", { agent_type: "GeneralPurpose", task: " audit " })).toBe(
      "启动 GeneralPurpose: audit",
    );
    expect(formatToolInput("ForkSubAgent", { agent_type: "KnowledgeAuditor" })).toBe(
      "启动 KnowledgeAuditor",
    );
  });
});
