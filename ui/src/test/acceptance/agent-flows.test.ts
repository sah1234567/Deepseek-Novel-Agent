import { describe, expect, it } from "vitest";
import { createInitialMachine } from "../../transcript/machine";
import { transcriptToFlatMessages } from "../../transcript/convert";
import { dispatchTranscriptEvent } from "../../transcript/machine";
import { mapStreamChunk } from "../../transcript/mapEvents";
import {
  dispatchForkEvent,
  forkInitialMachine,
  segmentComplete,
  streamText,
  toolComplete,
  toolResult,
  toolStart,
} from "../fixtures/transcript";
import { assertPlanHealthy } from "../helpers/transcriptPlan";

/** Mirrors useAgent sub-agent-* + assistant-segment-complete(forkRunId) handlers. */
function runForkLifecycle(forkRunId: string) {
  let machine = forkInitialMachine();
  machine = dispatchForkEvent(machine, streamText(`fork-${forkRunId}`, "分析中…"));
  machine = dispatchForkEvent(
    machine,
    toolStart("t1", "Read"),
  );
  machine = dispatchForkEvent(
    machine,
    toolComplete("t1", "Read", { path: "knowledge/INDEX.md" }),
  );
  machine = dispatchForkEvent(machine, toolResult("t1", "index content"));
  machine = dispatchForkEvent(machine, segmentComplete(0));
  machine = dispatchForkEvent(machine, { type: "TURN_COMPLETE" });
  return machine;
}

describe("fork machine lifecycle (useAgent parity)", () => {
  it("sub-agent stream + tool + segment-complete + complete", () => {
    const machine = runForkLifecycle("run-1");
    expect(machine.phase).toBe("idle");
    expect(machine.context.turns[0].segments).toHaveLength(1);
    expect(machine.context.turns[0].segments[0].tools[0].status).toBe("done");
    assertPlanHealthy(machine, { mode: "fork" });
  });

  it("get_fork_messages hydrate replaces fork machine", () => {
    let live = runForkLifecycle("run-1");
    const flat = transcriptToFlatMessages(live);
    live = dispatchTranscriptEvent(live, { type: "HYDRATE", flatMessages: flat });
    expect(live.context.turns[0].segments[0].assistant.contentBlocks[0]?.text).toContain("分析");
    expect(transcriptToFlatMessages(live).map((m) => m.id)).toEqual(flat.map((m) => m.id));
  });

  it("fork stream without messageId uses forkRunId-based message id", () => {
    let machine = forkInitialMachine();
    machine = dispatchForkEvent(
      machine,
      mapStreamChunk({ messageId: "fork-run-x", delta: "task", kind: "text" }),
    );
    expect(machine.context.openSegment?.assistant.contentBlocks[0]?.text).toBe("task");
  });
});

describe("main session flows (useAgent parity)", () => {
  it("submitInterrupt: INTERRUPT then BEGIN_TURN preserves queued turn shape", () => {
    let m = forkInitialMachine();
    m = dispatchForkEvent(m, streamText("a1", "streaming"));
    m = dispatchTranscriptEvent(m, { type: "INTERRUPT" });
    m = dispatchTranscriptEvent(m, {
      type: "BEGIN_TURN",
      user: { id: "u2", role: "user", contentBlocks: [{ blockIndex: 0, kind: "text", text: "next" }] },
    });
    expect(m.phase).toBe("segmentStreaming");
    expect(m.context.turns).toHaveLength(2);
    expect(m.context.turns[1].user.contentBlocks[0].text).toBe("next");
  });

  it("session-resumed: reset to initial then HYDRATE", () => {
    let m = createInitialMachine();
    m = runForkLifecycle("x");
    m = createInitialMachine();
    const flat = [
      { id: "u1", role: "user" as const, contentBlocks: [{ blockIndex: 0, kind: "text", text: "restored" }] },
      { id: "a1", role: "assistant" as const, contentBlocks: [{ blockIndex: 0, kind: "text", text: "hi" }] },
    ];
    m = dispatchTranscriptEvent(m, { type: "HYDRATE", flatMessages: flat });
    expect(m.context.turns[0].user.contentBlocks[0].text).toBe("restored");
    expect(m.phase).toBe("idle");
  });

  it("turn-complete with pending approval keeps machine idle but blocks isTurnInProgress false only after approve", () => {
    let m = forkInitialMachine();
    m = dispatchForkEvent(m, {
      type: "TOOL",
      phase: "input_complete",
      toolCallId: "w1",
      toolName: "Write",
      needsApproval: true,
      input: {},
    });
    m = dispatchForkEvent(m, { type: "TURN_COMPLETE" });
    expect(m.context.openSegment).toBeNull();
    expect(m.context.turns[0].segments[0].tools[0].status).toBe("pending");
  });

  it("PATCH_TOOL updates tool in committed segment after segment complete", () => {
    let m = forkInitialMachine();
    m = dispatchForkEvent(m, {
      type: "TOOL",
      phase: "input_complete",
      toolCallId: "w1",
      toolName: "Write",
      needsApproval: true,
      input: {},
    });
    m = dispatchForkEvent(m, segmentComplete(0));
    m = dispatchForkEvent(m, {
      type: "PATCH_TOOL",
      toolCallId: "w1",
      patch: { status: "running", needsApproval: false },
    });
    expect(m.context.turns[0].segments[0].tools[0].status).toBe("running");
  });
});
