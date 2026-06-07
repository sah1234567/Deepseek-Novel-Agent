import { describe, expect, it } from "vitest";
import type { ForkRunState } from "../types/messages";
import { applyForkDbSnapshot } from "./overlay";
import { forkRunAcceptsDbSnapshot } from "./transcript";
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
  it("forkRunAcceptsDbSnapshot is false while running", () => {
    expect(forkRunAcceptsDbSnapshot("running")).toBe(false);
    expect(forkRunAcceptsDbSnapshot("complete")).toBe(true);
  });

  it("applyForkDbSnapshot skips DB while running", () => {
    const run = sampleRun("running");
    const next = applyForkDbSnapshot(run, [
      {
        id: "a1",
        role: "assistant",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "db" }],
      },
    ]);
    expect(next.machine).toBe(run.machine);
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
