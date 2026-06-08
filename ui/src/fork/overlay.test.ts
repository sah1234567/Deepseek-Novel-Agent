import { describe, expect, it } from "vitest";
import type { ForkRunState } from "../types/messages";
import { applyForkDbSnapshot } from "./overlay";
import { emptyForkMachine } from "./transcript";

function sampleRun(status: ForkRunState["status"]): ForkRunState {
  return {
    forkRunId: "run-1",
    agentType: "KnowledgeAuditor",
    taskPreview: "t",
    source: "tool",
    machine: emptyForkMachine(),
    status,
  };
}

describe("fork/overlay", () => {
  it("applyForkDbSnapshot hydrates while running", () => {
    const run = sampleRun("running");
    const next = applyForkDbSnapshot(run, [
      {
        id: "a1",
        role: "assistant",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "db" }],
      },
    ]);
    expect(next.machine).not.toBe(run.machine);
    expect(next.machine.phase).not.toBe("idle");
  });

  it("applyForkDbSnapshot hydrates when complete", () => {
    const run = sampleRun("complete");
    const next = applyForkDbSnapshot(run, [
      {
        id: "a1",
        role: "assistant",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "db" }],
      },
    ]);
    expect(next.machine).not.toBe(run.machine);
    expect(next.machine.phase).not.toBe("idle");
  });
});
