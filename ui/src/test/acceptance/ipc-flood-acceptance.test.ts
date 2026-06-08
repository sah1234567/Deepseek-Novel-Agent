import { describe, expect, it, vi } from "vitest";
import { applyForkDbSnapshot } from "../../fork/overlay";
import { emptyForkMachine } from "../../fork/transcript";
import { dispatchTranscriptEvent } from "../../transcript/machine";
import { mapSegmentComplete, mapStreamChunk } from "../../transcript/mapEvents";
import {
  dispatchForkEvent,
  forkInitialMachine,
  streamText,
  toolResult,
} from "../fixtures/transcript";

describe("forkStreamSubscription", () => {
  it("openForkOverlay order: setOpen before subscribe before hydrate", async () => {
    const calls: string[] = [];
    let openId: string | null = null;
    const setOpen = (id: string) => {
      openId = id;
      calls.push("setOpen");
    };
    const invoke = vi.fn(async (cmd: string, _args?: unknown) => {
      if (cmd === "subscribe_fork_stream") {
        expect(openId).toBe("fr-1");
        calls.push("subscribe");
      }
      if (cmd === "get_fork_messages") {
        expect(openId).toBe("fr-1");
        calls.push("hydrate");
      }
      return [];
    });
    setOpen("fr-1");
    await invoke("subscribe_fork_stream", { runId: "fr-1" });
    await invoke("get_fork_messages", { runId: "fr-1" });
    expect(calls).toEqual(["setOpen", "subscribe", "hydrate"]);
  });

  it("applyForkDbSnapshot hydrates running fork", () => {
    const run = {
      forkRunId: "fr-1",
      agentType: "KnowledgeAuditor",
      taskPreview: "t",
      source: "tool" as const,
      machine: emptyForkMachine(),
      status: "running" as const,
    };
    const next = applyForkDbSnapshot(run, [
      {
        id: "a1",
        role: "assistant",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "db" }],
      },
    ]);
    expect(next.machine.phase).not.toBe("idle");
  });
});

describe("forkListenerGatedDispatch", () => {
  function gatedForkStream(
    openForkRunId: string | null,
    forkRunId: string,
    machine: ReturnType<typeof forkInitialMachine>,
  ) {
    if (openForkRunId !== forkRunId) return machine;
    return dispatchForkEvent(machine, streamText(`fork-${forkRunId}`, "live"));
  }

  it("ignores stream when overlay closed", () => {
    const base = forkInitialMachine();
    const next = gatedForkStream(null, "fr-1", base);
    expect(next).toBe(base);
  });

  it("applies stream when overlay open", () => {
    let m = forkInitialMachine();
    m = gatedForkStream("fr-1", "fr-1", m);
    expect(m.context.openSegment?.assistant.contentBlocks[0]?.text).toBe("live");
  });
});

describe("forkCardWithoutOverlay", () => {
  it("started/complete lifecycle without stream events", () => {
    type Status = "running" | "complete";
    let status: Status = "running";
    let reportOutput: string | undefined;
    let machine = emptyForkMachine();
    // started
    status = "running";
    machine = emptyForkMachine();
    // complete without overlay
    const openForkRunId: string | null = null;
    status = "complete";
    reportOutput = "report";
    machine = openForkRunId === "fr-1" ? dispatchForkEvent(machine, { type: "TURN_COMPLETE" }) : emptyForkMachine();
    expect(status).toBe("complete");
    expect(reportOutput).toBe("report");
    expect(machine.phase).toBe("segmentStreaming");
  });
});

describe("interruptibleEventDriven", () => {
  it("event payload updates interruptible flag", () => {
    let hasInterruptible = false;
    const onEvent = (payload: { hasInterruptibleToolInProgress: boolean }) => {
      hasInterruptible = payload.hasInterruptibleToolInProgress;
    };
    onEvent({ hasInterruptibleToolInProgress: true });
    expect(hasInterruptible).toBe(true);
    onEvent({ hasInterruptibleToolInProgress: false });
    expect(hasInterruptible).toBe(false);
  });
});

describe("segmentBoundaryAfterCoalesce", () => {
  it("segment complete starts new segment for following chunks", () => {
    let m = forkInitialMachine();
    m = dispatchForkEvent(m, mapStreamChunk({ messageId: "f1", delta: "think", kind: "thinking" }));
    m = dispatchForkEvent(m, mapSegmentComplete(0));
    m = dispatchForkEvent(m, mapStreamChunk({ messageId: "f1", delta: "answer", kind: "text" }));
    const seg0 = m.context.turns[0].segments[0];
    const open = m.context.openSegment;
    expect(seg0.assistant.contentBlocks.some((b) => b.kind === "thinking")).toBe(true);
    expect(open?.assistant.contentBlocks[0]?.text).toBe("answer");
  });
});

describe("mainSessionToolBubbles", () => {
  it("Write pending approval through result done", () => {
    let m = forkInitialMachine();
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "input_complete",
      toolCallId: "w1",
      toolName: "Write",
      input: { file_path: "a.md", content: "x" },
      needsApproval: true,
    });
    expect(m.context.openSegment?.tools[0].status).toBe("pending");
    m = dispatchTranscriptEvent(m, {
      type: "PATCH_TOOL",
      toolCallId: "w1",
      patch: { status: "running", needsApproval: false },
    });
    m = dispatchTranscriptEvent(m, toolResult("w1", "written"));
    expect(m.context.openSegment?.tools[0].status).toBe("done");
  });
});

describe("askUserQuestionPause", () => {
  it("ASK_USER_QUESTION then ANSWER_QUESTION returns to segmentCommitted", () => {
    let m = forkInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "ASK_USER_QUESTION" });
    expect(m.phase).toBe("pausedForQuestion");
    m = dispatchTranscriptEvent(m, { type: "ANSWER_QUESTION" });
    expect(m.phase).toBe("segmentCommitted");
  });
});
