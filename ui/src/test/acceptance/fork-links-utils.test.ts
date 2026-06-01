import { describe, expect, it } from "vitest";
import type { ForkRunState } from "../../hooks/useAgent";
import { listHookForkRuns, stripSubAgentReportPrefix } from "../../fork";

describe("fork utilities", () => {
  it("stripSubAgentReportPrefix removes completion header", () => {
    expect(stripSubAgentReportPrefix("[子 Agent 完成: GeneralPurpose]\nbody")).toBe("body");
  });

  it("listHookForkRuns excludes tool-source runs", () => {
    const runs = new Map<string, ForkRunState>([
      [
        "tool-run",
        {
          forkRunId: "tool-run",
          agentType: "GeneralPurpose",
          taskPreview: "",
          source: "tool",
          machine: { phase: "idle", context: { turns: [], openSegment: null, streamingMessageId: null } },
          status: "running",
        },
      ],
      [
        "hook-run",
        {
          forkRunId: "hook-run",
          agentType: "KnowledgeAuditor",
          taskPreview: "",
          source: "hook",
          machine: { phase: "idle", context: { turns: [], openSegment: null, streamingMessageId: null } },
          status: "running",
        },
      ],
    ]);
    expect(listHookForkRuns(runs)).toHaveLength(1);
    expect(listHookForkRuns(runs)[0].forkRunId).toBe("hook-run");
  });
});
