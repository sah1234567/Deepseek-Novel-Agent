import { describe, expect, it } from "vitest";
import { createInitialMachine, dispatchTranscriptEvent } from "./machine";
import { findTurnInMachine, mergeTurnsIntoMachine } from "./merge";
import {
  findLiveTailTurn,
  reconcileOrphanLiveTurns,
  shouldSkipMaxTurnSlot,
} from "./liveTail";
import { userMsg } from "../test/fixtures/transcript";
import type { UiTurnBundle } from "./service";
import type { Turn } from "./types";

function bundle(turnNumber: number, userId: string, text: string): UiTurnBundle {
  return {
    turnNumber,
    messages: [
      {
        id: userId,
        role: "user",
        contentBlocks: [{ blockIndex: 0, kind: "text", text }],
      },
      {
        id: `a-${turnNumber}`,
        role: "assistant",
        contentBlocks: [{ blockIndex: 0, kind: "text", text: `reply-${turnNumber}` }],
      },
    ],
  };
}

describe("findLiveTailTurn", () => {
  it("returns the latest live orphan, not the first", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, {
      type: "BEGIN_TURN",
      user: userMsg("u1", "first"),
    });
    m = dispatchTranscriptEvent(m, { type: "TURN_COMPLETE" });
    m = dispatchTranscriptEvent(m, {
      type: "BEGIN_TURN",
      user: userMsg("u2", "second"),
    });
    const tail = findLiveTailTurn(m);
    expect(tail?.user.contentBlocks[0].text).toBe("second");
  });
});

describe("reconcileOrphanLiveTurns", () => {
  it("drops all live orphans when idle and numbered active turns exist", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, {
      type: "BEGIN_TURN",
      user: userMsg("live-1", "live"),
    });
    m = dispatchTranscriptEvent(m, { type: "TURN_COMPLETE" });
    m = dispatchTranscriptEvent(m, {
      type: "MERGE_TURNS",
      bundles: [bundle(1, "db-1", "persisted")],
    });
    expect(m.context.turns.every((t) => t.turnNumber !== undefined)).toBe(true);
    expect(findTurnInMachine(m, 1)?.user.contentBlocks[0].text).toBe("persisted");
  });

  it("keeps only the last live orphan while streaming", () => {
    let turns: Turn[] = [
      {
        turnId: "live-1",
        user: userMsg("live-1", "old"),
        segments: [],
        reports: [],
      },
      {
        turnId: "live-2",
        user: userMsg("live-2", "new"),
        segments: [],
        reports: [],
      },
    ];
    turns = reconcileOrphanLiveTurns(turns, "segmentStreaming");
    expect(turns).toHaveLength(1);
    expect(turns[0].user.contentBlocks[0].text).toBe("new");
  });
});

describe("shouldSkipMaxTurnSlot", () => {
  it("does not skip max turn slot when a newer live turn is streaming", () => {
    let m = createInitialMachine();
    m = mergeTurnsIntoMachine(m, [bundle(1, "u1", "done")]);
    m = dispatchTranscriptEvent(m, {
      type: "BEGIN_TURN",
      user: userMsg("u2", "next"),
    });
    const liveTail = findLiveTailTurn(m)!;
    const layout = {
      hasContextRefresh: false,
      active: { minTurn: 1, maxTurn: 1 },
      archives: [],
    };
    expect(shouldSkipMaxTurnSlot(m, layout, liveTail, true)).toBe(false);
  });
});
