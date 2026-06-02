import { describe, expect, it } from "vitest";
import { isUserInitiatedInterruptMessage, shouldShowTurnError } from "./interrupt";

describe("interrupt constants", () => {
  it("wasInterrupted suppresses turn error", () => {
    expect(shouldShowTurnError({ wasInterrupted: true, phase: "error", message: "x" })).toBe(
      false,
    );
  });

  it("user interrupt message does not show banner", () => {
    expect(shouldShowTurnError({ phase: "error", message: "用户已中断" })).toBe(false);
  });

  it("real errors still show banner", () => {
    expect(shouldShowTurnError({ phase: "error", message: "Agent 出错" })).toBe(true);
  });

  it("matches abort strings", () => {
    expect(isUserInitiatedInterruptMessage("Request was aborted")).toBe(true);
  });
});
