import { describe, expect, it } from "vitest";
import type { UIMessage } from "../types/messages";
import { flatMessagesToTranscript, transcriptToFlatMessages } from "./convert";
import { dispatchTranscriptEvent } from "./machine";
import { userMsg } from "../test/fixtures/transcript";

describe("transcriptToFlatMessages", () => {
  it("preserves user → assistant → tool order for simple turn", () => {
    const flat: UIMessage[] = [
      userMsg("u1"),
      { id: "a1", role: "assistant", contentBlocks: [{ blockIndex: 0, kind: "text", text: "hi" }] },
      {
        id: "tool-t1",
        role: "tool",
        toolName: "Read",
        toolStatus: "done",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "out" }],
        toolInput: { path: "x" },
      },
    ];
    const m = flatMessagesToTranscript(flat);
    const back = transcriptToFlatMessages(m);
    expect(back.map((x) => x.role)).toEqual(["user", "assistant", "tool"]);
    expect(back.map((x) => x.id)).toEqual(flat.map((x) => x.id));
  });

  it("round-trips multi-segment turn with multiple tools per segment", () => {
    const flat: UIMessage[] = [
      userMsg("u1"),
      { id: "a1", role: "assistant", contentBlocks: [{ blockIndex: 0, kind: "text", text: "seg0" }] },
      {
        id: "tool-t1",
        role: "tool",
        toolName: "Read",
        toolStatus: "done",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "r1" }],
        toolInput: {},
      },
      {
        id: "tool-t2",
        role: "tool",
        toolName: "Grep",
        toolStatus: "done",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "g1" }],
        toolInput: {},
      },
      { id: "a1-seg-1", role: "assistant", contentBlocks: [{ blockIndex: 0, kind: "text", text: "seg1" }] },
      {
        id: "tool-t3",
        role: "tool",
        toolName: "Write",
        toolStatus: "done",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "w1" }],
        toolInput: {},
      },
    ];
    const m = flatMessagesToTranscript(flat);
    expect(m.context.turns[0].segments).toHaveLength(2);
    expect(m.context.turns[0].segments[0].tools).toHaveLength(2);
    const back = transcriptToFlatMessages(m);
    expect(back.map((x) => x.id)).toEqual(flat.map((x) => x.id));
  });

  it("includes subAgentReport in flat output but not in segment groups", () => {
    const flat: UIMessage[] = [
      userMsg("u1"),
      { id: "a1", role: "assistant", contentBlocks: [{ blockIndex: 0, kind: "text", text: "fork" }] },
      {
        id: "tool-fork",
        role: "tool",
        toolName: "ForkSubAgent",
        toolStatus: "done",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "" }],
        toolInput: { agent_type: "GeneralPurpose" },
      },
      {
        id: "report-1",
        role: "subAgentReport",
        forkRunId: "fork-1",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "[子 Agent 完成: GeneralPurpose]\nreport" }],
      },
    ];
    const m = flatMessagesToTranscript(flat);
    expect(m.context.turns[0].reports).toHaveLength(1);
    const back = transcriptToFlatMessages(m);
    expect(back.some((x) => x.role === "subAgentReport")).toBe(true);
  });

  it("placeholder assistant segment hydrates with tools only", () => {
    const flat: UIMessage[] = [
      userMsg("u1"),
      {
        id: "a1",
        role: "assistant",
        contentBlocks: [],
      },
      {
        id: "tool-t1",
        role: "tool",
        toolName: "Read",
        toolStatus: "done",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "x" }],
        toolInput: {},
      },
    ];
    const m = flatMessagesToTranscript(flat);
    expect(m.context.turns[0].segments[0].assistant.status).toBe("placeholder");
    expect(m.context.turns[0].segments[0].tools).toHaveLength(1);
  });

  it("omits streaming-args tools from flat unless they have result", () => {
    let m = flatMessagesToTranscript([userMsg("u1")]);
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u2") });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "start",
      toolCallId: "t1",
      toolName: "Read",
    });
    const flat = transcriptToFlatMessages(m);
    expect(flat.some((x) => x.id === "tool-t1")).toBe(false);
  });

  it("two user turns hydrate as two turns", () => {
    const flat: UIMessage[] = [
      userMsg("u1", "first"),
      { id: "a1", role: "assistant", contentBlocks: [{ blockIndex: 0, kind: "text", text: "r1" }] },
      userMsg("u2", "second"),
      { id: "a2", role: "assistant", contentBlocks: [{ blockIndex: 0, kind: "text", text: "r2" }] },
    ];
    const m = flatMessagesToTranscript(flat);
    expect(m.context.turns).toHaveLength(2);
    const back = transcriptToFlatMessages(m);
    expect(back.filter((x) => x.role === "user")).toHaveLength(2);
  });
});
