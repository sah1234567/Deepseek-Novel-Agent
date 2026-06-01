import { describe, expect, it } from "vitest";
import {
  hasPendingApproval,
  isStreamingPhase,
  isTurnInProgress,
  pauseSegmentId,
} from "./selectors";
import { createInitialMachine, dispatchTranscriptEvent } from "./machine";
import { userMsg } from "../test/fixtures/transcript";

describe("selectors", () => {
  it("hasPendingApproval finds pending Write in openSegment", () => {
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
    expect(hasPendingApproval(m)).toBe(true);
  });

  it("hasPendingApproval finds pending tool in committed segment", () => {
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
    m = dispatchTranscriptEvent(m, { type: "SEGMENT_COMPLETE", segmentIndex: 0 });
    expect(hasPendingApproval(m)).toBe(true);
  });

  it("hasPendingApproval false after approve patch", () => {
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
    expect(hasPendingApproval(m)).toBe(false);
  });

  it("isTurnInProgress true while streaming", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    expect(isTurnInProgress(m, null)).toBe(true);
  });

  it("isTurnInProgress true when pendingQuestion set even if phase idle", () => {
    const m = createInitialMachine();
    expect(
      isTurnInProgress(m, {
        toolCallId: "ask1",
        questions: [{ id: "q1", prompt: "?", options: [{ id: "o1", label: "A" }] }],
      }),
    ).toBe(true);
  });

  it("isTurnInProgress true at idle with pending approval (blocks model switch)", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "input_complete",
      toolCallId: "w1",
      toolName: "Write",
      needsApproval: true,
      input: {},
    });
    m = dispatchTranscriptEvent(m, { type: "TURN_COMPLETE" });
    expect(m.phase).toBe("idle");
    expect(isTurnInProgress(m, null)).toBe(true);
  });

  it("isTurnInProgress false when idle without pending question or approval", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, { type: "TURN_COMPLETE" });
    expect(isTurnInProgress(m, null)).toBe(false);
  });

  it("pauseSegmentId returns pauseAfterSegmentId on current turn", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, { type: "ASK_USER_QUESTION" });
    expect(pauseSegmentId(m)).toBeDefined();
    expect(pauseSegmentId(m)).toBe(m.context.turns[0].pauseAfterSegmentId);
  });

  it("isStreamingPhase only true in segmentStreaming", () => {
    let m = createInitialMachine();
    expect(isStreamingPhase(m)).toBe(false);
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    expect(isStreamingPhase(m)).toBe(true);
    m = dispatchTranscriptEvent(m, { type: "SEGMENT_COMPLETE", segmentIndex: 0 });
    expect(isStreamingPhase(m)).toBe(false);
  });
});
