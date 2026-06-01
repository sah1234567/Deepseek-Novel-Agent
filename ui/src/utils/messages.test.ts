import { describe, expect, it } from "vitest";
import { flatMessagesToTranscript } from "../transcript/convert";
import { apiMessagesToUi } from "./messages";

describe("apiMessagesToUi", () => {
  it("filters to user, assistant, tool, subAgentReport", () => {
    const out = apiMessagesToUi([
      { id: "1", role: "system", contentBlocks: [{ blockIndex: 0, kind: "text", text: "x" }] },
      { id: "2", role: "user", contentBlocks: [{ blockIndex: 0, kind: "text", text: "hi" }] },
      { id: "3", role: "assistant", contentBlocks: [{ blockIndex: 0, kind: "text", text: "yo" }] },
      {
        id: "4",
        role: "tool",
        toolName: "Read",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "out" }],
      },
      {
        id: "5",
        role: "subAgentReport",
        forkRunId: "fork-1",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: "report" }],
      },
    ]);
    expect(out.map((m) => m.role)).toEqual(["user", "assistant", "tool", "subAgentReport"]);
    expect(out[3].forkRunId).toBe("fork-1");
  });
});

describe("api hydrate path", () => {
  it("apiMessagesToUi + flatMessagesToTranscript yields idle machine with segments", () => {
    const m = flatMessagesToTranscript(
      apiMessagesToUi([
        { id: "u1", role: "user", contentBlocks: [{ blockIndex: 0, kind: "text", text: "hi" }] },
        { id: "a1", role: "assistant", contentBlocks: [{ blockIndex: 0, kind: "text", text: "reply" }] },
        {
          id: "tool-t1",
          role: "tool",
          toolName: "Read",
          contentBlocks: [{ blockIndex: 0, kind: "text", text: "file" }],
        },
      ]),
    );
    expect(m.phase).toBe("idle");
    expect(m.context.turns[0].segments[0].tools[0].name).toBe("Read");
  });
});
