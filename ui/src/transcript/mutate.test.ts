import { describe, expect, it } from "vitest";
import { createInitialMachine, dispatchTranscriptEvent } from "./machine";
import { userMsg } from "../test/fixtures/transcript";

describe("structural sharing on hot paths", () => {
  it("STREAM_CHUNK keeps committed turns referentially equal", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, {
      type: "STREAM_CHUNK",
      messageId: "a1",
      delta: "hello",
      kind: "text",
    });
    m = dispatchTranscriptEvent(m, { type: "SEGMENT_COMPLETE", segmentIndex: 0 });
    const turnsBefore = m.context.turns;
    const segBefore = m.context.turns[0].segments[0];

    m = dispatchTranscriptEvent(m, {
      type: "STREAM_CHUNK",
      messageId: "a1",
      delta: " world",
      kind: "text",
    });

    expect(m.context.turns).toBe(turnsBefore);
    expect(m.context.turns[0].segments[0]).toBe(segBefore);
    expect(m.context.openSegment?.assistant.contentBlocks[0]?.text).toBe(" world");
  });

  it("TOOL input_delta on open segment keeps turns referentially equal", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "start",
      toolCallId: "t1",
      toolName: "Read",
    });
    const turnsBefore = m.context.turns;
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "input_delta",
      toolCallId: "t1",
      delta: '{"path":',
    });

    expect(m.context.turns).toBe(turnsBefore);
    expect(m.context.openSegment?.tools[0]?.unparsedInput).toBe('{"path":');
  });
});
