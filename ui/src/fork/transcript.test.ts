import { describe, expect, it } from "vitest";
import { dispatchTranscriptEvent } from "../transcript";
import { dispatchForkEvent, hydrateForkMachine } from "./transcript";

describe("fork/transcript", () => {
  it("dispatchForkEvent accepts stream after hydrate-idle", () => {
    const hydrated = hydrateForkMachine([
      {
        id: "u1",
        role: "user",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "task" }],
      },
      {
        id: "a1",
        role: "assistant",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "partial" }],
      },
    ]);
    const next = dispatchForkEvent(hydrated, {
      type: "STREAM_CHUNK",
      messageId: "fork-run-1",
      delta: " more",
      kind: "text",
    });
    const turn = next.context.turns[next.context.turns.length - 1];
    const text =
      next.context.openSegment?.assistant.contentBlocks.find((b) => b.kind === "text")
        ?.text ??
      turn.segments[turn.segments.length - 1]?.assistant.contentBlocks.find(
        (b) => b.kind === "text",
      )?.text;
    expect(text).toContain("more");
  });

  it("dispatchForkEvent begins turn from idle without hydrate", () => {
    let machine = dispatchTranscriptEvent(
      { phase: "idle", context: { turns: [], openSegment: null, streamingMessageId: null } },
      { type: "INTERRUPT" },
    );
    machine = dispatchForkEvent(machine, {
      type: "STREAM_CHUNK",
      messageId: "fork-1",
      delta: "x",
      kind: "text",
    });
    expect(machine.phase).not.toBe("idle");
    expect(
      machine.context.openSegment?.assistant.contentBlocks.some((b) => b.text === "x"),
    ).toBe(true);
  });
});
