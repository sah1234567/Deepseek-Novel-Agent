import { describe, expect, it } from "vitest";
import { apiMessagesToUi } from "./messages";

describe("apiMessagesToUi", () => {
  it("filters to user and assistant roles", () => {
    const out = apiMessagesToUi([
      { id: "1", role: "system", contentBlocks: [{ blockIndex: 0, kind: "text", text: "x" }] },
      { id: "2", role: "user", contentBlocks: [{ blockIndex: 0, kind: "text", text: "hi" }] },
      { id: "3", role: "assistant", contentBlocks: [{ blockIndex: 0, kind: "text", text: "yo" }] },
    ]);
    expect(out).toHaveLength(2);
    expect(out[0].role).toBe("user");
  });
});
