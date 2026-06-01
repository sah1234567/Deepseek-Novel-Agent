import { describe, expect, it } from "vitest";
import type { ForkRunState, UIMessage } from "../../hooks/useAgent";
import { createInitialMachine } from "../../transcript";
import {
  buildForkSubAgentLinks,
  resolveForkRunIdForTool,
  toolPathForkRunIds,
} from "../../utils/forkLinks";

describe("forkLinks — ForkSubAgent acceptance", () => {
  it("#9 pairs ForkSubAgent tool rows with subAgentReport by order", () => {
    const messages: UIMessage[] = [
      { id: "u1", role: "user", contentBlocks: [{ blockIndex: 0, kind: "text", text: "go" }] },
      { id: "a1", role: "assistant", contentBlocks: [{ blockIndex: 0, kind: "text", text: "forking" }] },
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
    const links = buildForkSubAgentLinks(messages);
    expect(links.get("tool-f1")?.forkRunId).toBe("run-abc");
    expect(links.get("tool-f1")?.reportContent).toContain("full report");
  });

  it("#9 resolveForkRunIdForTool falls back to live forkRuns order", () => {
    const messages: UIMessage[] = [
      {
        id: "tool-f1",
        role: "tool",
        toolName: "ForkSubAgent",
        contentBlocks: [],
        toolInput: {},
      },
    ];
    const forkRuns = new Map<string, ForkRunState>([
      [
        "live-run-1",
        {
          forkRunId: "live-run-1",
          agentType: "GeneralPurpose",
          taskPreview: "task",
          source: "tool",
          machine: createInitialMachine(),
          status: "running",
        },
      ],
    ]);
    const id = resolveForkRunIdForTool("tool-f1", messages, new Map(), forkRuns);
    expect(id).toBe("live-run-1");
    expect(toolPathForkRunIds(forkRuns)).toEqual(["live-run-1"]);
  });
});
