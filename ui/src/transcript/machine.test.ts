import { describe, expect, it } from "vitest";
import type { UIMessage } from "../types/messages";
import { flatMessagesToTranscript, transcriptToFlatMessages } from "./convert";
import { createInitialMachine, dispatchTranscriptEvent } from "./machine";
import { flatMessagesToMachine } from "./flatParse";
import { hydrateAllForTest } from "./testHelpers";
import { SYNTHETIC_USER_ID } from "./types";
import { userMsg } from "../test/fixtures/transcript";

describe("dispatchTranscriptEvent", () => {
  it("T1: tool result before SEGMENT_COMPLETE stays in openSegment", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, {
      type: "STREAM_CHUNK",
      messageId: "a1",
      delta: "Hi",
      kind: "text",
    });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "start",
      toolCallId: "t1",
      toolName: "Read",
    });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "result",
      toolCallId: "t1",
      content: "file contents",
    });
    expect(m.context.openSegment?.tools[0]?.status).toBe("done");
    expect(m.phase).toBe("segmentStreaming");
  });

  it("T2: multi-segment ReAct creates separate segments", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, {
      type: "STREAM_CHUNK",
      messageId: "a1",
      delta: "seg0",
      kind: "text",
    });
    m = dispatchTranscriptEvent(m, { type: "SEGMENT_COMPLETE", segmentIndex: 0 });
    expect(m.phase).toBe("segmentCommitted");
    m = dispatchTranscriptEvent(m, {
      type: "STREAM_CHUNK",
      messageId: "a1",
      delta: "seg1",
      kind: "text",
    });
    expect(m.phase).toBe("segmentStreaming");
    m = dispatchTranscriptEvent(m, { type: "SEGMENT_COMPLETE", segmentIndex: 1 });
    m = dispatchTranscriptEvent(m, {
      type: "STREAM_CHUNK",
      messageId: "a1",
      delta: "seg2",
      kind: "text",
    });
    m = dispatchTranscriptEvent(m, { type: "TURN_COMPLETE" });
    expect(m.context.turns[0].segments).toHaveLength(3);
  });

  it("T3: tool-only segment gets placeholder assistant", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "start",
      toolCallId: "t1",
      toolName: "Read",
    });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "input_complete",
      toolCallId: "t1",
      toolName: "Read",
      input: { path: "x" },
    });
    m = dispatchTranscriptEvent(m, { type: "SEGMENT_COMPLETE", segmentIndex: 0 });
    const seg = m.context.turns[0].segments[0];
    expect(seg.assistant.status).toBe("placeholder");
    expect(seg.tools).toHaveLength(1);
  });

  it("T4: AskUserQuestion sets pauseAfterSegmentId", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, {
      type: "STREAM_CHUNK",
      messageId: "a1",
      delta: "Q?",
      kind: "text",
    });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "start",
      toolCallId: "ask1",
      toolName: "AskUserQuestion",
    });
    m = dispatchTranscriptEvent(m, { type: "ASK_USER_QUESTION" });
    expect(m.phase).toBe("pausedForQuestion");
    expect(m.context.turns[0].pauseAfterSegmentId).toBeDefined();
  });

  it("T5: pending approval tool in openSegment", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "input_complete",
      toolCallId: "w1",
      toolName: "Write",
      input: {},
      needsApproval: true,
    });
    expect(m.context.openSegment?.tools[0].status).toBe("pending");
  });

  it("T7: deny tool via PATCH_TOOL", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "input_complete",
      toolCallId: "w1",
      toolName: "Write",
      input: {},
      needsApproval: true,
    });
    m = dispatchTranscriptEvent(m, {
      type: "PATCH_TOOL",
      toolCallId: "w1",
      patch: { status: "denied", needsApproval: false },
    });
    expect(m.context.openSegment?.tools[0].status).toBe("denied");
  });

  it("T6: approve via PATCH_TOOL then result", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "input_complete",
      toolCallId: "w1",
      toolName: "Write",
      input: {},
      needsApproval: true,
    });
    m = dispatchTranscriptEvent(m, {
      type: "PATCH_TOOL",
      toolCallId: "w1",
      patch: { status: "running", needsApproval: false },
    });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "result",
      toolCallId: "w1",
      content: "ok",
    });
    expect(m.context.openSegment?.tools[0].status).toBe("done");
  });

  it("T10: INTERRUPT commits partial assistant", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, {
      type: "STREAM_CHUNK",
      messageId: "a1",
      delta: "partial",
      kind: "text",
    });
    m = dispatchTranscriptEvent(m, { type: "INTERRUPT" });
    expect(m.phase).toBe("idle");
    expect(m.context.turns[0].segments[0].assistant.contentBlocks[0].text).toBe("partial");
  });

  it("T11: TURN_COMPLETE commits openSegment", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, {
      type: "STREAM_CHUNK",
      messageId: "a1",
      delta: "done",
      kind: "text",
    });
    m = dispatchTranscriptEvent(m, { type: "TURN_COMPLETE" });
    expect(m.phase).toBe("idle");
    expect(m.context.openSegment).toBeNull();
    expect(m.context.turns[0].segments).toHaveLength(1);
  });

  // Defensive invariant only — production path has ordered ToolCallResult before TurnComplete.
  it("T12: idle + late TOOL result is no-op", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, { type: "TURN_COMPLETE" });
    const before = JSON.stringify(m);
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "result",
      toolCallId: "late",
      content: "x",
    });
    expect(JSON.stringify(m)).toBe(before);
  });

  it("T13: mid-stream TOOL survives when no idle reset (compaction-mid-turn guard)", () => {
    // Simulates the fixed path: compaction fires mid-stream, but the guard
    // (isStreamingRef / turnInProgressRef) skips RESET_TRANSCRIPT. FSM stays
    // in segmentStreaming and continues processing events.
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL", phase: "start", toolCallId: "t1", toolName: "Read",
    });
    expect(m.phase).toBe("segmentStreaming");
    // Compaction banner fires here — but guard skips RESET_TRANSCRIPT.
    // Subsequent tool events must still be processed:
    m = dispatchTranscriptEvent(m, {
      type: "TOOL", phase: "result", toolCallId: "t1", content: "done",
    });
    expect(m.phase).not.toBe("idle");
    expect(m.phase).toBe("segmentStreaming");
    // Tool result applied in openSegment (not yet committed):
    const tool = m.context.openSegment?.tools.find(
      (t: { id: string }) => t.id === "t1",
    );
    expect(tool?.status).toBe("done");
    expect(tool?.result).toBe("done");
  });

  it("pausedForQuestion blocks STREAM_CHUNK", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, { type: "ASK_USER_QUESTION" });
    const before = JSON.stringify(m);
    m = dispatchTranscriptEvent(m, {
      type: "STREAM_CHUNK",
      messageId: "a1",
      delta: "x",
      kind: "text",
    });
    expect(JSON.stringify(m)).toBe(before);
  });

  it("ANSWER_QUESTION resumes to segmentCommitted", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, { type: "ASK_USER_QUESTION" });
    m = dispatchTranscriptEvent(m, { type: "ANSWER_QUESTION" });
    expect(m.phase).toBe("segmentCommitted");
    expect(m.context.turns[0].pauseAfterSegmentId).toBeUndefined();
  });

  it("segmentCommitted + TOOL start opens new segment", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, { type: "SEGMENT_COMPLETE", segmentIndex: 0 });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "start",
      toolCallId: "t2",
      toolName: "Read",
    });
    expect(m.phase).toBe("segmentStreaming");
    expect(m.context.openSegment?.segmentIndex).toBe(1);
  });

  it("input_complete after segment-complete updates committed segment not new openSegment", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "start",
      toolCallId: "t1",
      toolName: "Read",
    });
    m = dispatchTranscriptEvent(m, { type: "SEGMENT_COMPLETE", segmentIndex: 0 });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "input_complete",
      toolCallId: "t1",
      toolName: "Read",
      input: { path: "a.md" },
    });
    expect(m.context.turns[0].segments[0].tools[0].status).toBe("running");
    expect(m.context.openSegment?.tools.length ?? 0).toBe(0);
  });

  it("tool result with openSegment for next API updates prior segment tool", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "start",
      toolCallId: "t1",
      toolName: "Read",
    });
    m = dispatchTranscriptEvent(m, { type: "SEGMENT_COMPLETE", segmentIndex: 0 });
    m = dispatchTranscriptEvent(m, {
      type: "STREAM_CHUNK",
      messageId: "a2",
      delta: "next",
      kind: "text",
    });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "result",
      toolCallId: "t1",
      content: "done",
    });
    expect(m.context.turns[0].segments[0].tools[0].result).toBe("done");
    expect(m.context.openSegment?.tools.length ?? 0).toBe(0);
  });

  it("STREAM_CHUNK in idle is no-op", () => {
    const m = createInitialMachine();
    const before = JSON.stringify(m);
    const after = dispatchTranscriptEvent(m, {
      type: "STREAM_CHUNK",
      messageId: "a1",
      delta: "x",
      kind: "text",
    });
    expect(JSON.stringify(after)).toBe(before);
  });

  it("PATCH_TOOL in idle is no-op", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, { type: "TURN_COMPLETE" });
    const before = JSON.stringify(m);
    m = dispatchTranscriptEvent(m, {
      type: "PATCH_TOOL",
      toolCallId: "w1",
      patch: { status: "running" },
    });
    expect(JSON.stringify(m)).toBe(before);
  });

  it("TOOL progress without existing tool is no-op", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    const before = JSON.stringify(m.context);
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "progress",
      toolCallId: "missing",
      description: "working",
    });
    expect(JSON.stringify(m.context)).toBe(before);
  });

  it("MERGE_TURNS via hydrateAllForTest replaces entire machine state", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = hydrateAllForTest(m, [
      userMsg("hydrated"),
      { id: "a1", role: "assistant", contentBlocks: [{ blockIndex: 0, kind: "text", text: "x" }] },
    ]);
    expect(m.context.turns[0].user.id).toBe("hydrated");
    expect(m.phase).toBe("idle");
  });
});

describe("flatMessagesToTranscript", () => {
  it("flatMessagesToMachine deduplicates tool rows by toolCallId in one segment", () => {
    const flat: UIMessage[] = [
      userMsg("u1"),
      { id: "a1", role: "assistant", contentBlocks: [{ blockIndex: 0, kind: "text", text: "hi" }] },
      {
        id: "tool-t1",
        role: "tool",
        toolName: "Read",
        toolStatus: "done",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "first" }],
        toolInput: {},
      },
      {
        id: "tool-t1",
        role: "tool",
        toolName: "Read",
        toolStatus: "done",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "second" }],
        toolInput: {},
      },
    ];
    const { machine } = flatMessagesToMachine(flat);
    expect(machine.context.turns[0].segments[0].tools).toHaveLength(1);
    expect(machine.context.turns[0].segments[0].tools[0].result).toBe("second");
  });

  it("T8: hydrateAllForTest round-trip preserves order", () => {
    const flat: UIMessage[] = [
      userMsg("u1"),
      { id: "a1", role: "assistant", contentBlocks: [{ blockIndex: 0, kind: "text", text: "hi" }] },
      {
        id: "tool-t1",
        role: "tool",
        toolName: "Read",
        toolStatus: "done",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "out" }],
        toolInput: {},
      },
    ];
    const m = hydrateAllForTest(createInitialMachine(), flat);
    const back = transcriptToFlatMessages(m);
    expect(back.map((x) => x.id)).toEqual(flat.map((x) => x.id));
  });

  it("T9: synthetic user for fork flat without user", () => {
    const flat: UIMessage[] = [
      { id: "a1", role: "assistant", contentBlocks: [{ blockIndex: 0, kind: "text", text: "task" }] },
    ];
    const m = flatMessagesToTranscript(flat);
    expect(m.context.turns[0].user.id).toBe(SYNTHETIC_USER_ID);
    const back = transcriptToFlatMessages(m);
    expect(back.some((x) => x.role === "user")).toBe(false);
  });
});
