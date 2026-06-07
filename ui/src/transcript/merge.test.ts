import { describe, expect, it } from "vitest";
import { createInitialMachine, dispatchTranscriptEvent } from "./machine";
import {
  compareTurnTimelineOrder,
  evictTurnsFromMachine,
  findTurnInMachine,
  mergeTurnsIntoMachine,
} from "./merge";
import { isLiveOrphanTurn } from "./liveTail";
import { forkBindingSnapshotKey } from "./selectors";
import type { UiTurnBundle } from "./service";
import { userMsg } from "../test/fixtures/transcript";
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

describe("compareTurnTimelineOrder", () => {
  it("places live orphans after numbered active turns", () => {
    const numbered: Turn = {
      turnId: "t20",
      turnNumber: 20,
      user: userMsg("u20", "old"),
      segments: [],
      reports: [],
    };
    const orphan: Turn = {
      turnId: "live",
      user: userMsg("live", "继续"),
      segments: [],
      reports: [],
    };
    expect(compareTurnTimelineOrder(numbered, orphan)).toBeLessThan(0);
    expect(compareTurnTimelineOrder(orphan, numbered)).toBeGreaterThan(0);
  });
});

describe("mergeTurnsIntoMachine", () => {
  it("merges bundles in turn order", () => {
    const m = mergeTurnsIntoMachine(createInitialMachine(), [
      bundle(2, "u2", "second"),
      bundle(1, "u1", "first"),
    ]);
    expect(m.context.turns.map((t) => t.turnNumber)).toEqual([1, 2]);
    expect(m.context.turns[0].user.id).toBe("u1");
  });

  it("overwrites same turnNumber on reload", () => {
    let m = mergeTurnsIntoMachine(createInitialMachine(), [bundle(1, "u1", "old")]);
    m = mergeTurnsIntoMachine(m, [bundle(1, "u1-new", "new")]);
    expect(m.context.turns).toHaveLength(1);
    expect(m.context.turns[0].user.contentBlocks[0].text).toBe("new");
  });

  it("keeps archive and active turns distinct by epoch", () => {
    let m = mergeTurnsIntoMachine(createInitialMachine(), [bundle(1, "u-arch", "arch")], 1);
    m = mergeTurnsIntoMachine(m, [bundle(1, "u-act", "act")]);
    expect(m.context.turns).toHaveLength(2);
    expect(findTurnInMachine(m, 1, 1)?.user.id).toBe("u-arch");
    expect(findTurnInMachine(m, 1)?.user.id).toBe("u-act");
  });

  it("keeps live orphan last after merging numbered turns", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, {
      type: "BEGIN_TURN",
      user: userMsg("live", "继续"),
    });
    m = mergeTurnsIntoMachine(m, [bundle(20, "u20", "chapter 20")]);
    const active = m.context.turns.filter((t) => t.archiveEpoch === undefined);
    expect(active[active.length - 1].user.contentBlocks[0].text).toBe("继续");
    expect(isLiveOrphanTurn(active[active.length - 1])).toBe(true);
  });

  it("commits streaming segments to orphan after mid-stream merge", () => {
    let m = mergeTurnsIntoMachine(createInitialMachine(), [bundle(20, "u20", "old")]);
    m = dispatchTranscriptEvent(m, {
      type: "BEGIN_TURN",
      user: userMsg("live", "继续"),
    });
    m = mergeTurnsIntoMachine(m, [bundle(19, "u19", "nineteen")]);
    m = dispatchTranscriptEvent(m, {
      type: "STREAM_CHUNK",
      messageId: "a-live",
      delta: "thinking",
      kind: "text",
    });
    m = dispatchTranscriptEvent(m, { type: "SEGMENT_COMPLETE", segmentIndex: 0 });
    const orphan = m.context.turns.find((t) => t.turnId === "live");
    expect(orphan?.segments).toHaveLength(1);
    expect(findTurnInMachine(m, 20)?.segments).toHaveLength(1);
    expect(findTurnInMachine(m, 20)?.segments[0].assistant.contentBlocks[0].text).toBe(
      "reply-20",
    );
  });
});

describe("evictTurnsFromMachine", () => {
  it("removes targeted turns without throwing selectors", () => {
    let m = mergeTurnsIntoMachine(createInitialMachine(), [
      bundle(1, "u1", "one"),
      bundle(2, "u2", "two"),
    ]);
    m = evictTurnsFromMachine(m, [{ turnNumber: 1 }]);
    expect(m.context.turns).toHaveLength(1);
    expect(m.context.turns[0].turnNumber).toBe(2);
    expect(() => forkBindingSnapshotKey(m)).not.toThrow();
  });
});
