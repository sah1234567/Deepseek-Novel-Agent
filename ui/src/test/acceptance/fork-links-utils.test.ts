import { describe, expect, it } from "vitest";
import type { ForkRunState } from "../../hooks/useAgent";
import {
  agentLabelFromType,
  buildForkSubAgentLinks,
  forkSubAgentToolMessages,
  listHookForkRuns,
  stripSubAgentReportPrefix,
  toolPathForkRunIds,
} from "../../utils/forkLinks";
import { createInitialMachine } from "../../transcript";

describe("forkLinks utilities", () => {
  it("stripSubAgentReportPrefix removes completion header", () => {
    expect(stripSubAgentReportPrefix("[子 Agent 完成: GeneralPurpose]\nbody")).toBe("body");
  });

  it("buildForkSubAgentLinks skips report without forkRunId", () => {
    const links = buildForkSubAgentLinks([
      {
        id: "tool-f1",
        role: "tool",
        toolName: "ForkSubAgent",
        contentBlocks: [],
        toolInput: {},
      },
      {
        id: "report-1",
        role: "subAgentReport",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "no id" }],
      },
    ]);
    expect(links.size).toBe(0);
  });

  it("forkSubAgentToolMessages filters ForkSubAgent tools only", () => {
    const msgs = forkSubAgentToolMessages([
      { id: "t1", role: "tool", toolName: "Read", contentBlocks: [] },
      { id: "t2", role: "tool", toolName: "ForkSubAgent", contentBlocks: [] },
    ]);
    expect(msgs).toHaveLength(1);
    expect(msgs[0].id).toBe("t2");
  });

  it("toolPathForkRunIds excludes hook-source runs", () => {
    const runs = new Map<string, ForkRunState>([
      [
        "tool-run",
        {
          forkRunId: "tool-run",
          agentType: "GeneralPurpose",
          taskPreview: "",
          source: "tool",
          machine: createInitialMachine(),
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
          machine: createInitialMachine(),
          status: "running",
        },
      ],
    ]);
    expect(toolPathForkRunIds(runs)).toEqual(["tool-run"]);
    expect(listHookForkRuns(runs)).toHaveLength(1);
    expect(listHookForkRuns(runs)[0].forkRunId).toBe("hook-run");
  });

  it("agentLabelFromType maps known agent types", () => {
    expect(agentLabelFromType("KnowledgeAuditorSubagent")).toBe("KnowledgeAuditor");
    expect(agentLabelFromType("chaptercraft-analyzer")).toBe("ChapterCraftAnalyzer");
    expect(agentLabelFromType("general-purpose")).toBe("GeneralPurpose");
    expect(agentLabelFromType("")).toBe("Subagent");
  });
});
