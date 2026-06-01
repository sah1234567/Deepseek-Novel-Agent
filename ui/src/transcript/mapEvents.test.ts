import { describe, expect, it } from "vitest";
import { mapStreamChunk, mapToolCallRequest } from "./mapEvents";

describe("mapToolCallRequest", () => {
  it("maps explicit phases", () => {
    expect(mapToolCallRequest({ toolCallId: "t1", phase: "start", toolName: "Read" })).toEqual({
      type: "TOOL",
      phase: "start",
      toolCallId: "t1",
      toolName: "Read",
      input: undefined,
      needsApproval: undefined,
      delta: undefined,
      content: undefined,
      status: undefined,
      description: undefined,
    });
  });

  it("infers result from content without phase", () => {
    const e = mapToolCallRequest({ toolCallId: "t1", content: "done" });
    expect(e?.type).toBe("TOOL");
    if (e?.type === "TOOL") expect(e.phase).toBe("result");
  });

  it("infers input_delta from delta without phase", () => {
    const e = mapToolCallRequest({ toolCallId: "t1", delta: "{" });
    expect(e?.type).toBe("TOOL");
    if (e?.type === "TOOL") expect(e.phase).toBe("input_delta");
  });

  it("infers input_complete from toolName without phase", () => {
    const e = mapToolCallRequest({
      toolCallId: "t1",
      toolName: "Write",
      input: { path: "a" },
      needsApproval: true,
    });
    expect(e?.type).toBe("TOOL");
    if (e?.type === "TOOL") {
      expect(e.phase).toBe("input_complete");
      expect(e.needsApproval).toBe(true);
    }
  });

  it("returns null without toolCallId", () => {
    expect(mapToolCallRequest({ toolName: "Read" })).toBeNull();
  });
});

describe("mapStreamChunk", () => {
  it("maps stream chunks", () => {
    expect(mapStreamChunk({ messageId: "m1", delta: "hi", kind: "text" })).toEqual({
      type: "STREAM_CHUNK",
      messageId: "m1",
      delta: "hi",
      kind: "text",
    });
  });
});
