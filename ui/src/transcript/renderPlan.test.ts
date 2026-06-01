import { describe, expect, it } from "vitest";
import {
  buildTranscriptRenderPlan,
  validateRenderPlan,
} from "./renderPlan";
import { createInitialMachine, dispatchTranscriptEvent } from "./machine";
import { userMsg } from "../test/fixtures/transcript";

describe("renderPlan validation", () => {
  it("detects duplicate tool ids across segments", () => {
    const plan = buildTranscriptRenderPlan(createInitialMachine(), { mode: "main" });
    const bad = [
      ...plan,
      {
        kind: "segment" as const,
        segmentId: "a:0",
        segmentIndex: 0,
        variant: "committed" as const,
        assistantId: "a1",
        toolIds: ["t1"],
      },
      {
        kind: "segment" as const,
        segmentId: "a:1",
        segmentIndex: 1,
        variant: "committed" as const,
        assistantId: "a2",
        toolIds: ["t1"],
      },
    ];
    expect(validateRenderPlan(bad).some((e) => e.includes("multiple segment groups"))).toBe(true);
  });

  it("includes question node after committed pause segment", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, {
      type: "STREAM_CHUNK",
      messageId: "a1",
      delta: "choose",
      kind: "text",
    });
    m = dispatchTranscriptEvent(m, { type: "ASK_USER_QUESTION" });
    const plan = buildTranscriptRenderPlan(m, { mode: "main", includeQuestion: true });
    expect(plan.some((n) => n.kind === "question")).toBe(true);
    expect(validateRenderPlan(plan)).toEqual([]);
  });

  it("appends question when pause segment not yet in committed list (fallback path)", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1") });
    m = dispatchTranscriptEvent(m, {
      type: "STREAM_CHUNK",
      messageId: "a1",
      delta: "x",
      kind: "text",
    });
    const turn = m.context.turns[0];
    turn.pauseAfterSegmentId = "u1:99";
    m = { ...m, phase: "pausedForQuestion" };
    const plan = buildTranscriptRenderPlan(m, { mode: "main", includeQuestion: true });
    expect(plan.some((n) => n.kind === "question" && n.afterSegmentId === "u1:99")).toBe(true);
  });
});
