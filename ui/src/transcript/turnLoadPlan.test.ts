import { describe, expect, it } from "vitest";
import type { TurnSlot } from "./buildTurnSlots";
import {
  collectAdjacentIdleWindow,
  planTurnLoadSegments,
  shouldRenderSlotInTimeline,
  splitWindowIntoLoadSegments,
} from "./turnLoadPlan";
import { turnSlotKey } from "./turnSlotKey";

function slot(
  kind: TurnSlot["kind"],
  turnNumber: number,
  epoch?: number,
  status: TurnSlot["status"] = "idle",
): TurnSlot {
  return {
    slotKey:
      kind === "archive" ? turnSlotKey("archive", turnNumber, epoch!) : turnSlotKey("active", turnNumber),
    kind,
    turnNumber,
    epoch,
    status,
  };
}

describe("collectAdjacentIdleWindow", () => {
  it("spans archive and active idle slots without stopping at compact boundary", () => {
    const slots: TurnSlot[] = [
      slot("archive", 19, 1),
      slot("archive", 20, 1),
      slot("active", 0),
      slot("active", 1),
      slot("active", 2, undefined, "loaded"),
    ];
    const window = collectAdjacentIdleWindow(slots, turnSlotKey("archive", 19, 1), 4);
    expect(window?.map((s) => s.slotKey)).toEqual([
      turnSlotKey("archive", 19, 1),
      turnSlotKey("archive", 20, 1),
      turnSlotKey("active", 0),
      turnSlotKey("active", 1),
    ]);
  });

  it("stops at non-idle slot", () => {
    const slots: TurnSlot[] = [
      slot("active", 1),
      slot("active", 2, undefined, "loading"),
      slot("active", 3),
    ];
    const window = collectAdjacentIdleWindow(slots, turnSlotKey("active", 1), 3);
    expect(window).toHaveLength(1);
  });
});

describe("splitWindowIntoLoadSegments", () => {
  it("splits archive and active into separate IPC segments", () => {
    const window: TurnSlot[] = [
      slot("archive", 19, 1),
      slot("archive", 20, 1),
      slot("active", 0),
      slot("active", 1),
    ];
    const segments = splitWindowIntoLoadSegments(window);
    expect(segments).toEqual([
      {
        kind: "archive",
        epoch: 1,
        fromTurn: 19,
        toTurn: 20,
        keys: [turnSlotKey("archive", 19, 1), turnSlotKey("archive", 20, 1)],
      },
      {
        kind: "active",
        epoch: undefined,
        fromTurn: 0,
        toTurn: 1,
        keys: [turnSlotKey("active", 0), turnSlotKey("active", 1)],
      },
    ]);
  });

  it("splits archive epochs separately", () => {
    const window: TurnSlot[] = [slot("archive", 2, 1), slot("archive", 1, 2)];
    const segments = splitWindowIntoLoadSegments(window);
    expect(segments).toHaveLength(2);
    expect(segments[0].epoch).toBe(1);
    expect(segments[1].epoch).toBe(2);
  });
});

describe("planTurnLoadSegments", () => {
  it("plans parallel segments across compact boundary", () => {
    const slots: TurnSlot[] = [
      slot("archive", 20, 1),
      slot("active", 0),
      slot("active", 1),
    ];
    const segments = planTurnLoadSegments(slots, turnSlotKey("archive", 20, 1), 3);
    expect(segments).toHaveLength(2);
    expect(segments[0].kind).toBe("archive");
    expect(segments[1].kind).toBe("active");
  });
});

describe("shouldRenderSlotInTimeline", () => {
  it("omits idle slots outside loaded window", () => {
    const slots: TurnSlot[] = Array.from({ length: 10 }, (_, i) =>
      slot("active", i + 1, undefined, i >= 7 ? "loaded" : "idle"),
    );
    expect(shouldRenderSlotInTimeline(slots[0], slots)).toBe(false);
    expect(shouldRenderSlotInTimeline(slots[5], slots)).toBe(false);
    expect(shouldRenderSlotInTimeline(slots[6], slots)).toBe(true);
    expect(shouldRenderSlotInTimeline(slots[7], slots)).toBe(true);
  });

  it("always renders loaded and loading slots", () => {
    const slots = [slot("active", 1, undefined, "loading"), slot("active", 2, undefined, "idle")];
    expect(shouldRenderSlotInTimeline(slots[0], slots)).toBe(true);
    expect(shouldRenderSlotInTimeline(slots[1], slots)).toBe(true);
  });
});
