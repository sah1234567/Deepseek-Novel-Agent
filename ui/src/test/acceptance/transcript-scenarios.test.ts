import { describe, expect, it } from "vitest";
import { transcriptToFlatMessages } from "../../transcript/convert";
import { dispatchTranscriptEvent } from "../../transcript/machine";
import {
  buildTranscriptRenderPlan,
  questionFollowsSegmentTools,
  segmentGroupsInOrder,
} from "../../transcript/renderPlan";
import { SYNTHETIC_USER_ID } from "../../transcript/types";
import {
  beginTurn,
  scenario,
  segmentComplete,
  streamText,
  streamThinking,
  toolComplete,
  toolDelta,
  toolResult,
  toolStart,
  userMsg,
} from "../fixtures/transcript";
import { assertPlanHealthy } from "../helpers/transcriptPlan";

describe("manual acceptance — structural render plan", () => {
  it("#1 InvokeSkill: async tool result before segment-complete stays under live Agent", () => {
    const m = scenario([
      beginTurn(userMsg("u1")),
      streamText("a1", "正在调用技能…"),
      toolStart("t1", "InvokeSkill"),
      toolDelta("t1", '{"skill":'),
      toolComplete("t1", "InvokeSkill", { skill: "write-chapter" }),
      toolResult("t1", "skill output"),
    ]);
    const plan = assertPlanHealthy(m);
    expect(m.context.openSegment?.tools[0]?.status).toBe("done");
    const live = plan.find((n) => n.kind === "segment" && n.variant === "live");
    expect(live?.kind).toBe("segment");
    if (live?.kind === "segment") {
      expect(live.toolIds).toContain("t1");
      expect(live.assistantId).toBeTruthy();
    }
    expect(plan.filter((n) => n.kind === "segment")).toHaveLength(1);
  });

  it("#2 multi-segment ReAct: segments do not cross-mix tools", () => {
    const m = scenario([
      beginTurn(userMsg("u1")),
      streamText("a1", "seg0"),
      segmentComplete(0),
      streamText("a1", "seg1"),
      toolStart("t1", "Read"),
      toolComplete("t1", "Read", { path: "a" }),
      toolResult("t1", "content-a"),
      segmentComplete(1),
      streamText("a1", "seg2"),
      toolStart("t2", "Write"),
      toolComplete("t2", "Write", { path: "b" }),
      segmentComplete(2),
      { type: "TURN_COMPLETE" },
    ]);
    const plan = assertPlanHealthy(m);
    expect(segmentGroupsInOrder(plan)).toEqual([
      "u1:0:committed",
      "u1:1:committed",
      "u1:2:committed",
    ]);
    expect(m.context.turns[0].segments[0].tools).toHaveLength(0);
    expect(m.context.turns[0].segments[1].tools.map((t) => t.id)).toEqual(["t1"]);
    expect(m.context.turns[0].segments[2].tools.map((t) => t.id)).toEqual(["t2"]);
  });

  it("#3 AskUserQuestion: question follows all tools in triggering segment", () => {
    const m = scenario([
      beginTurn(userMsg("u1")),
      streamText("a1", "请选择"),
      toolStart("ask1", "AskUserQuestion"),
      toolComplete("ask1", "AskUserQuestion", { questions: [] }),
      toolStart("extra", "Read"),
      toolComplete("extra", "Read", {}),
      toolResult("extra", "prefetch"),
      { type: "ASK_USER_QUESTION" },
    ]);
    const pauseId = m.context.turns[0].pauseAfterSegmentId!;
    const plan = assertPlanHealthy(m, { includeQuestion: true });
    expect(m.phase).toBe("pausedForQuestion");
    expect(questionFollowsSegmentTools(plan, pauseId)).toBe(true);
    const seg = plan.find((n) => n.kind === "segment" && n.segmentId === pauseId);
    if (seg?.kind === "segment") {
      expect(seg.toolIds).toEqual(["ask1", "extra"]);
    }
  });

  it("#4 Write pending approval: tool in same segment as Agent", () => {
    const m = scenario([
      beginTurn(userMsg("u1")),
      streamText("a1", "将写入文件"),
      toolComplete("w1", "Write", { path: "ch.md" }, true),
    ]);
    const plan = assertPlanHealthy(m);
    const live = plan.find((n) => n.kind === "segment" && n.variant === "live");
    expect(live?.kind).toBe("segment");
    if (live?.kind === "segment") {
      expect(live.toolIds).toEqual(["w1"]);
    }
    expect(m.context.openSegment?.tools[0].status).toBe("pending");
  });

  it("#5 after approve: new segment streams above new tools", () => {
    let m = scenario([
      beginTurn(userMsg("u1")),
      streamText("a1", "first"),
      toolComplete("w1", "Write", {}, true),
      segmentComplete(0),
    ]);
    m = dispatchTranscriptEvent(m, {
      type: "PATCH_TOOL",
      toolCallId: "w1",
      patch: { status: "running", needsApproval: false },
    });
    m = dispatchTranscriptEvent(m, toolResult("w1", "written"));
    m = dispatchTranscriptEvent(m, segmentComplete(1));
    m = dispatchTranscriptEvent(m, streamText("a1", "continuing"));
    m = dispatchTranscriptEvent(m, toolStart("t2", "Read"));
    const plan = assertPlanHealthy(m);
    const groups = segmentGroupsInOrder(plan);
    expect(groups).toContain("u1:0:committed");
    expect(groups[groups.length - 1]).toMatch(/:live$/);
    const live = plan.find((n) => n.kind === "segment" && n.variant === "live");
    if (live?.kind === "segment") {
      expect(live.toolIds).toEqual(["t2"]);
    }
  });

  it("#6 SubAgent fork: fork plan omits user; synthetic user omitted in main too", () => {
    const m = scenario([
      beginTurn({ id: SYNTHETIC_USER_ID, role: "user", contentBlocks: [] }),
      streamText("a1", "sub task"),
      toolStart("t1", "Read"),
      toolResult("t1", "out"),
    ]);
    expect(buildTranscriptRenderPlan(m, { mode: "fork" }).some((n) => n.kind === "user")).toBe(false);
    expect(buildTranscriptRenderPlan(m, { mode: "main" }).some((n) => n.kind === "user")).toBe(false);

    const main = scenario([beginTurn(userMsg("u1")), streamText("a1", "hi")]);
    expect(buildTranscriptRenderPlan(main, { mode: "main" }).some((n) => n.kind === "user")).toBe(true);
    expect(buildTranscriptRenderPlan(main, { mode: "fork" }).some((n) => n.kind === "user")).toBe(false);
    assertPlanHealthy(m, { mode: "fork" });
  });

  it("#7 Esc interrupt: partial assistant and done tools preserved in order", () => {
    const m = scenario([
      beginTurn(userMsg("u1")),
      streamText("a1", "partial answer"),
      toolStart("t1", "Read"),
      toolResult("t1", "early result"),
      { type: "INTERRUPT" },
    ]);
    const plan = assertPlanHealthy(m);
    expect(m.phase).toBe("idle");
    expect(m.context.turns[0].segments).toHaveLength(1);
    expect(m.context.turns[0].segments[0].assistant.contentBlocks[0]?.text).toBe("partial answer");
    expect(m.context.turns[0].segments[0].tools[0]?.status).toBe("done");
    const seg = plan[1];
    expect(seg?.kind).toBe("segment");
    if (seg?.kind === "segment") {
      expect(seg.toolIds).toEqual(["t1"]);
    }
    const flat = transcriptToFlatMessages(m);
    expect(flat.map((x) => x.role)).toEqual(["user", "assistant", "tool"]);
  });

  it("#8 turn-end hydrate: complex history round-trips flat order", () => {
    let m = scenario([
      beginTurn(userMsg("u1")),
      streamText("a1", "hello"),
      toolStart("t1", "Read"),
      toolResult("t1", "file"),
      segmentComplete(0),
      { type: "TURN_COMPLETE" },
    ]);
    const flat = transcriptToFlatMessages(m);
    m = dispatchTranscriptEvent(m, { type: "HYDRATE", flatMessages: flat });
    const plan = assertPlanHealthy(m);
    expect(plan.filter((n) => n.kind === "segment")).toHaveLength(1);
    expect(transcriptToFlatMessages(m).map((x) => x.id)).toEqual(flat.map((x) => x.id));
  });
});

describe("conversation scenarios — extended", () => {
  it("thinking stream then text stream in same segment", () => {
    const m = scenario([
      beginTurn(userMsg("u1")),
      streamThinking("a1", "reasoning…"),
      streamText("a1", "answer"),
    ]);
    const blocks = m.context.openSegment?.assistant.contentBlocks ?? [];
    expect(blocks.some((b) => b.kind === "thinking")).toBe(true);
    expect(blocks.some((b) => b.kind === "text")).toBe(true);
    assertPlanHealthy(m);
  });

  it("multiple tools in one segment preserve ToolUseStarted order", () => {
    const m = scenario([
      beginTurn(userMsg("u1")),
      streamText("a1", "run tools"),
      toolStart("t1", "Read"),
      toolComplete("t1", "Read", {}),
      toolStart("t2", "Grep"),
      toolComplete("t2", "Grep", {}),
      toolResult("t1", "r1"),
      toolResult("t2", "g1"),
    ]);
    expect(m.context.openSegment?.tools.map((t) => t.id)).toEqual(["t1", "t2"]);
    assertPlanHealthy(m);
  });

  it("T7 deny tool via PATCH_TOOL stays in segment", () => {
    let m = scenario([
      beginTurn(userMsg("u1")),
      toolComplete("w1", "Write", {}, true),
    ]);
    m = dispatchTranscriptEvent(m, {
      type: "PATCH_TOOL",
      toolCallId: "w1",
      patch: { status: "denied", needsApproval: false },
    });
    expect(m.context.openSegment?.tools[0].status).toBe("denied");
    assertPlanHealthy(m);
  });

  it("tool result after segment-complete writes to last committed segment", () => {
    let m = scenario([
      beginTurn(userMsg("u1")),
      streamText("a1", "done streaming"),
      segmentComplete(0),
    ]);
    m = dispatchTranscriptEvent(m, toolResult("t1", "late sync result", "Read"));
    const lastSeg = m.context.turns[0].segments[m.context.turns[0].segments.length - 1];
    expect(lastSeg.tools.some((t) => t.id === "t1" && t.status === "done")).toBe(true);
    assertPlanHealthy(m);
  });

  it("late tool result does not attach to new openSegment after next stream chunk", () => {
    let m = scenario([
      beginTurn(userMsg("u1")),
      toolStart("t1", "Read"),
      streamText("a1", "assistant"),
      segmentComplete(0),
      streamText("a2", "next api"),
    ]);
    m = dispatchTranscriptEvent(m, toolResult("t1", "tool output"));
    expect(m.context.turns[0].segments[0].tools[0].result).toBe("tool output");
    expect(m.context.openSegment?.tools.length ?? 0).toBe(0);
    assertPlanHealthy(m);
  });

  it("two consecutive user turns each get own segments", () => {
    let m = scenario([
      beginTurn(userMsg("u1")),
      streamText("a1", "r1"),
      { type: "TURN_COMPLETE" },
    ]);
    m = dispatchTranscriptEvent(m, beginTurn(userMsg("u2")));
    m = dispatchTranscriptEvent(m, streamText("a2", "r2"));
    m = dispatchTranscriptEvent(m, { type: "TURN_COMPLETE" });
    expect(m.context.turns).toHaveLength(2);
    const plan = assertPlanHealthy(m);
    expect(plan.filter((n) => n.kind === "user")).toHaveLength(2);
  });

  it("streaming-args tool excluded from flat until input_complete", () => {
    const m = scenario([
      beginTurn(userMsg("u1")),
      toolStart("t1", "Read"),
      toolDelta("t1", '{"path":'),
    ]);
    expect(m.context.openSegment?.tools[0].status).toBe("streaming-args");
    const flat = transcriptToFlatMessages(m);
    expect(flat.some((x) => x.role === "tool")).toBe(false);
  });

  it("mapEvents full lifecycle via scenario helper", () => {
    const m = scenario([
      beginTurn(userMsg("u1")),
      streamText("mid", "hi"),
      toolStart("tc", "InvokeSkill"),
      toolDelta("tc", "{}"),
      toolComplete("tc", "InvokeSkill", {}),
      toolResult("tc", "ok"),
      segmentComplete(0),
      { type: "TURN_COMPLETE" },
    ]);
    expect(m.context.turns[0].segments[0].tools[0].status).toBe("done");
    assertPlanHealthy(m);
  });

  it("ANSWER_QUESTION clears pause and allows streaming again", () => {
    let m = scenario([
      beginTurn(userMsg("u1")),
      streamText("a1", "pick one"),
      { type: "ASK_USER_QUESTION" },
    ]);
    expect(m.phase).toBe("pausedForQuestion");
    m = dispatchTranscriptEvent(m, { type: "ANSWER_QUESTION" });
    m = dispatchTranscriptEvent(m, streamText("a1", "thanks"));
    expect(m.phase).toBe("segmentStreaming");
    assertPlanHealthy(m);
  });

  it("idle rejects late tool events after turn complete", () => {
    let m = scenario([
      beginTurn(userMsg("u1")),
      streamText("a1", "x"),
      { type: "TURN_COMPLETE" },
    ]);
    const before = JSON.stringify(m.context);
    m = dispatchTranscriptEvent(m, toolResult("late", "nope"));
    expect(JSON.stringify(m.context)).toBe(before);
  });
});
