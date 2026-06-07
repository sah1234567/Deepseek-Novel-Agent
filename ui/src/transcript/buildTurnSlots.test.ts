import { describe, expect, it } from "vitest";
import { appendMissingTurnSlots, buildTurnSlots } from "./buildTurnSlots";
import { turnSlotKey } from "./turnSlotKey";

describe("buildTurnSlots", () => {
  it("includes archive slots per epoch and active tail range", () => {
    const slots = buildTurnSlots({
      hasContextRefresh: false,
      active: { minTurn: 2, maxTurn: 3 },
      archives: [{ epoch: 1, bounds: { minTurn: 1, maxTurn: 1 } }],
    });

    expect(slots.map((s) => s.slotKey)).toEqual([
      turnSlotKey("archive", 1, 1),
      turnSlotKey("active", 2),
      turnSlotKey("active", 3),
    ]);
    expect(slots.every((s) => s.status === "idle")).toBe(true);
  });

  it("inserts context-refresh slot at turn 0 when flagged", () => {
    const slots = buildTurnSlots({
      hasContextRefresh: true,
      active: { minTurn: 0, maxTurn: 2 },
      archives: [],
    });

    expect(slots.map((s) => s.slotKey)).toEqual([
      turnSlotKey("active", 0),
      turnSlotKey("active", 1),
      turnSlotKey("active", 2),
    ]);
  });

  it("appendMissingTurnSlots preserves status and adds new tail slots", () => {
    const initial = buildTurnSlots({
      hasContextRefresh: false,
      active: { minTurn: 1, maxTurn: 2 },
      archives: [],
    }).map((s) =>
      s.turnNumber === 2 ? { ...s, status: "loaded" as const } : s,
    );
    const grown = appendMissingTurnSlots(initial, {
      hasContextRefresh: false,
      active: { minTurn: 1, maxTurn: 3 },
      archives: [],
    });
    expect(grown).toHaveLength(3);
    expect(grown.find((s) => s.turnNumber === 2)?.status).toBe("loaded");
    expect(grown.find((s) => s.turnNumber === 3)?.status).toBe("idle");
  });
});
