import { describe, expect, it } from "vitest";
import type { ForkRunState } from "../hooks/useAgent";
import { createInitialMachine } from "../transcript";
import {
  reportContentByForkRunId,
  resolveForkRunIdForToolCard,
  stripSubAgentReportPrefix,
} from "./binding";

describe("fork/binding", () => {
  it("stripSubAgentReportPrefix removes completion header", () => {
    expect(stripSubAgentReportPrefix("[子 Agent 完成: GeneralPurpose]\nbody")).toBe("body");
  });

  it("reportContentByForkRunId keys by forkRunId", () => {
    const map = reportContentByForkRunId([
      {
        id: "r1",
        role: "subAgentReport",
        forkRunId: "run-a",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "[子 Agent 完成: X]\nreport" }],
      },
    ]);
    expect(map.get("run-a")).toBe("report");
  });

  it("resolveForkRunIdForToolCard uses parentToolCallId", () => {
    const forkRuns = new Map<string, ForkRunState>([
      [
        "live-run",
        {
          forkRunId: "live-run",
          agentType: "GeneralPurpose",
          taskPreview: "",
          source: "tool",
          parentToolCallId: "tc-1",
          machine: createInitialMachine(),
          status: "running",
        },
      ],
    ]);
    expect(resolveForkRunIdForToolCard("tool-tc-1", forkRuns, [])).toBe("live-run");
  });

  it("resolveForkRunIdForToolCard returns undefined without binding", () => {
    expect(resolveForkRunIdForToolCard("tool-missing", new Map(), [])).toBeUndefined();
  });
});
