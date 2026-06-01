import { describe, expect, it } from "vitest";
import type { ForkRunState, UIMessage } from "../../hooks/useAgent";
import { createInitialMachine } from "../../transcript";
import { reportContentByForkRunId, resolveForkRunIdForToolCard } from "../../fork";

describe("fork binding — ForkSubAgent acceptance", () => {
  it("report map and resolve by parentToolCallId", () => {
    const messages: UIMessage[] = [
      { id: "u1", role: "user", contentBlocks: [{ blockIndex: 0, kind: "text", text: "go" }] },
      {
        id: "tool-f1",
        role: "tool",
        toolName: "ForkSubAgent",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "" }],
        toolInput: { agent_type: "GeneralPurpose" },
      },
      {
        id: "report-1",
        role: "subAgentReport",
        forkRunId: "run-abc",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "[子 Agent 完成: GeneralPurpose]\nfull report" }],
      },
    ];
    const reports = reportContentByForkRunId(messages);
    expect(reports.get("run-abc")).toContain("full report");

    const forkRuns = new Map<string, ForkRunState>([
      [
        "run-abc",
        {
          forkRunId: "run-abc",
          agentType: "GeneralPurpose",
          taskPreview: "task",
          source: "tool",
          parentToolCallId: "f1",
          machine: createInitialMachine(),
          status: "complete",
        },
      ],
    ]);
    expect(resolveForkRunIdForToolCard("tool-f1", forkRuns, messages)).toBe("run-abc");
  });

  it("hydrate order fallback when forkRuns empty", () => {
    const messages: UIMessage[] = [
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
        forkRunId: "run-hydrate",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "done" }],
      },
    ];
    expect(resolveForkRunIdForToolCard("tool-f1", new Map(), messages)).toBe("run-hydrate");
  });
});
