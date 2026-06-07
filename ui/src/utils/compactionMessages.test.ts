import { describe, expect, it } from "vitest";
import { compactionProgressLabel, compactionProgressVariant } from "./compactionMessages";

describe("compactionMessages", () => {
  it("maps generating-summary", () => {
    expect(compactionProgressLabel({ action: "generating-summary" })).toContain("摘要");
  });

  it("maps rebuilding-session", () => {
    expect(compactionProgressLabel({ action: "rebuilding-session" })).toContain("重建");
  });

  it("maps done with token counts", () => {
    const label = compactionProgressLabel({
      action: "done",
      tokensBefore: 12000,
      tokensAfter: 4000,
    });
    expect(label).toContain("12,000");
    expect(label).toContain("4,000");
  });

  it("maps done with retained turn range", () => {
    const label = compactionProgressLabel({
      action: "done",
      retainedMinTurn: 46,
      retainedMaxTurn: 50,
    });
    expect(label).toContain("保留 turn 46–50");
  });

  it("maps failed with reason", () => {
    expect(
      compactionProgressLabel({ action: "failed", reason: "timeout" }),
    ).toContain("timeout");
    expect(compactionProgressVariant("failed")).toBe("warn");
  });
});
