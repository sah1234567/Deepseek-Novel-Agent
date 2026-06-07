import { describe, expect, it } from "vitest";
import type { TurnSlot } from "./buildTurnSlots";
import {
  MAX_LOADED_TURNS,
  TAIL_LOADED_TURNS,
  VIEW_LOADED_TURNS,
} from "./loadPolicy";
import type { MemoryWindowContext } from "./turnMemoryPolicy";
import {
  isInBottomAnchorZone,
  planMemoryReconcile,
  planMemoryWindow,
  planTailContentFill,
  protectedMaxTurnKey,
} from "./turnMemoryPolicy";
import { turnSlotKey } from "./turnSlotKey";

function slot(
  kind: TurnSlot["kind"],
  turnNumber: number,
  epoch?: number,
  status: TurnSlot["status"] = "idle",
): TurnSlot {
  return {
    slotKey:
      kind === "archive"
        ? turnSlotKey("archive", turnNumber, epoch!)
        : turnSlotKey("active", turnNumber),
    kind,
    turnNumber,
    epoch,
    status,
  };
}

function baseLayout(maxTurn: number, hasContextRefresh = false) {
  return {
    hasContextRefresh,
    active: { minTurn: hasContextRefresh ? 0 : 1, maxTurn },
    archives: [] as { epoch: number; bounds: { minTurn: number; maxTurn: number } }[],
  };
}

function ctx(
  partial: Partial<MemoryWindowContext> & Pick<MemoryWindowContext, "turnSlots">,
): MemoryWindowContext {
  const maxTurn = partial.maxTurn ?? partial.layout?.active.maxTurn ?? 10;
  return {
    layout: partial.layout ?? baseLayout(maxTurn),
    turnSlots: partial.turnSlots,
    visibleSlotKeys: partial.visibleSlotKeys ?? new Set(),
    visibleEnvelope: partial.visibleEnvelope ?? { minIndex: null, maxIndex: null },
    isBottomAnchored: partial.isBottomAnchored ?? false,
    contentUnderflow: partial.contentUnderflow ?? false,
    maxTurn: partial.maxTurn ?? maxTurn,
    compactionPaused: partial.compactionPaused ?? false,
  };
}

describe("isInBottomAnchorZone", () => {
  it("returns true when within threshold of bottom", () => {
    const el = {
      scrollHeight: 1000,
      scrollTop: 900,
      clientHeight: 50,
    } as HTMLElement;
    expect(isInBottomAnchorZone(el, 128)).toBe(true);
  });

  it("returns false when far from bottom", () => {
    const el = {
      scrollHeight: 1000,
      scrollTop: 100,
      clientHeight: 50,
    } as HTMLElement;
    expect(isInBottomAnchorZone(el, 128)).toBe(false);
  });
});

describe("planMemoryWindow", () => {
  it("keeps TAIL_LOADED_TURNS window at bottom anchor", () => {
    const layout = baseLayout(10);
    const slots: TurnSlot[] = Array.from({ length: 10 }, (_, i) =>
      slot("active", i + 1, undefined, "loaded"),
    );
    const keepFrom = 10 - TAIL_LOADED_TURNS + 1;
    const targets = planMemoryWindow(
      ctx({ layout, turnSlots: slots, isBottomAnchored: true, maxTurn: 10 }),
    );
    expect(targets.map((t) => t.turnNumber).sort((a, b) => a - b)).toEqual(
      Array.from({ length: keepFrom - 1 }, (_, i) => i + 1),
    );
  });

  it("includes loaded archive slots for eviction at bottom", () => {
    const layout = {
      ...baseLayout(3),
      archives: [{ epoch: 1, bounds: { minTurn: 1, maxTurn: 2 } }],
    };
    const slots: TurnSlot[] = [
      slot("archive", 1, 1, "loaded"),
      slot("active", 1, undefined, "loaded"),
      slot("active", 2, undefined, "loaded"),
      slot("active", 3, undefined, "loaded"),
    ];
    const targets = planMemoryWindow(
      ctx({ layout, turnSlots: slots, isBottomAnchored: true, maxTurn: 3 }),
    );
    expect(targets).toContainEqual({ turnNumber: 1, archiveEpoch: 1 });
  });

  it("preserves context refresh turn 0 at bottom", () => {
    const layout = baseLayout(3, true);
    const slots: TurnSlot[] = [
      slot("active", 0, undefined, "loaded"),
      slot("active", 1, undefined, "loaded"),
      slot("active", 2, undefined, "loaded"),
      slot("active", 3, undefined, "loaded"),
    ];
    const targets = planMemoryWindow(
      ctx({ layout, turnSlots: slots, isBottomAnchored: true, maxTurn: 3 }),
    );
    expect(targets.some((t) => t.turnNumber === 0)).toBe(false);
  });

  it("evicts loaded slots outside visible browse window", () => {
    const layout = baseLayout(20);
    const slots: TurnSlot[] = Array.from({ length: 20 }, (_, i) =>
      slot("active", i + 1, undefined, "loaded"),
    );
    const visibleKeys = new Set([
      turnSlotKey("active", 11),
      turnSlotKey("active", 12),
      turnSlotKey("active", 13),
    ]);
    const targets = planMemoryWindow(
      ctx({
        layout,
        turnSlots: slots,
        visibleSlotKeys: visibleKeys,
        visibleEnvelope: { minIndex: 10, maxIndex: 12 },
        isBottomAnchored: false,
        maxTurn: 20,
      }),
    );
    const evicted = targets.map((t) => t.turnNumber);
    expect(evicted).toContain(8);
    expect(evicted).not.toContain(20);
    expect(evicted).not.toContain(11);
    expect(evicted).not.toContain(12);
    expect(evicted).not.toContain(13);
  });

  it("does not evict visible slots when span exceeds VIEW_LOADED_TURNS", () => {
    const layout = baseLayout(15);
    const slots: TurnSlot[] = Array.from({ length: 15 }, (_, i) =>
      slot("active", i + 1, undefined, "loaded"),
    );
    const visibleKeys = new Set(
      Array.from({ length: 8 }, (_, i) => turnSlotKey("active", 5 + i)),
    );
    const targets = planMemoryWindow(
      ctx({
        layout,
        turnSlots: slots,
        visibleSlotKeys: visibleKeys,
        visibleEnvelope: { minIndex: 4, maxIndex: 11 },
        isBottomAnchored: false,
        maxTurn: 15,
      }),
    );
    for (let t = 5; t <= 12; t++) {
      expect(targets.some((x) => x.turnNumber === t)).toBe(false);
    }
    expect(VIEW_LOADED_TURNS).toBe(6);
  });

  it("overflow evicts to MAX_LOADED_TURNS preferring non-visible", () => {
    const layout = baseLayout(20);
    const slots: TurnSlot[] = Array.from({ length: 20 }, (_, i) =>
      slot("active", i + 1, undefined, "loaded"),
    );
    const visibleKeys = new Set([turnSlotKey("active", 10)]);
    const targets = planMemoryWindow(
      ctx({
        layout,
        turnSlots: slots,
        visibleSlotKeys: visibleKeys,
        visibleEnvelope: { minIndex: 9, maxIndex: 9 },
        isBottomAnchored: false,
        maxTurn: 20,
      }),
    );
    const remaining = 20 - targets.length;
    expect(remaining).toBeLessThanOrEqual(MAX_LOADED_TURNS);
    expect(targets.some((t) => t.turnNumber === 10)).toBe(false);
    expect(targets.some((t) => t.turnNumber === 20)).toBe(false);
  });

  it("skips tail compaction when compactionPaused", () => {
    const layout = baseLayout(10);
    const slots: TurnSlot[] = Array.from({ length: 10 }, (_, i) =>
      slot("active", i + 1, undefined, "loaded"),
    );
    const targets = planMemoryWindow(
      ctx({
        layout,
        turnSlots: slots,
        isBottomAnchored: true,
        compactionPaused: true,
        maxTurn: 10,
      }),
    );
    expect(targets).toHaveLength(0);
  });

  it("skips tail evict when underflow and older idle exists", () => {
    const layout = baseLayout(6);
    const slots: TurnSlot[] = [
      slot("active", 1, undefined, "idle"),
      slot("active", 2, undefined, "idle"),
      ...Array.from({ length: 4 }, (_, i) =>
        slot("active", i + 3, undefined, "loaded"),
      ),
    ];
    const targets = planMemoryWindow(
      ctx({
        layout,
        turnSlots: slots,
        isBottomAnchored: true,
        contentUnderflow: true,
        maxTurn: 6,
      }),
    );
    expect(targets).toHaveLength(0);
  });
});

describe("planTailContentFill", () => {
  it("returns prefetch keys when underflow and idle above loaded", () => {
    const layout = baseLayout(6);
    const slots: TurnSlot[] = [
      slot("active", 1, undefined, "idle"),
      slot("active", 2, undefined, "idle"),
      ...Array.from({ length: 4 }, (_, i) =>
        slot("active", i + 3, undefined, "loaded"),
      ),
    ];
    const keys = planTailContentFill(
      ctx({
        layout,
        turnSlots: slots,
        isBottomAnchored: true,
        contentUnderflow: true,
        maxTurn: 6,
      }),
    );
    expect(keys.length).toBeGreaterThan(0);
    expect(keys).toContain(turnSlotKey("active", 2));
  });

  it("returns empty when not bottom anchored", () => {
    const layout = baseLayout(6);
    const slots: TurnSlot[] = [
      slot("active", 1, undefined, "idle"),
      slot("active", 2, undefined, "loaded"),
    ];
    expect(
      planTailContentFill(
        ctx({
          layout,
          turnSlots: slots,
          isBottomAnchored: false,
          contentUnderflow: true,
          maxTurn: 6,
        }),
      ),
    ).toEqual([]);
  });

  it("returns empty when no idle above loaded", () => {
    const layout = baseLayout(3);
    const slots: TurnSlot[] = Array.from({ length: 3 }, (_, i) =>
      slot("active", i + 1, undefined, "loaded"),
    );
    expect(
      planTailContentFill(
        ctx({
          layout,
          turnSlots: slots,
          isBottomAnchored: true,
          contentUnderflow: true,
          maxTurn: 3,
        }),
      ),
    ).toEqual([]);
  });
});

describe("planMemoryReconcile", () => {
  it("returns empty evict when prefetching for underflow", () => {
    const layout = baseLayout(6);
    const slots: TurnSlot[] = [
      slot("active", 1, undefined, "idle"),
      slot("active", 2, undefined, "idle"),
      ...Array.from({ length: 4 }, (_, i) =>
        slot("active", i + 3, undefined, "loaded"),
      ),
    ];
    const plan = planMemoryReconcile(
      ctx({
        layout,
        turnSlots: slots,
        isBottomAnchored: true,
        contentUnderflow: true,
        maxTurn: 6,
      }),
    );
    expect(plan.prefetchSlotKeys.length).toBeGreaterThan(0);
    expect(plan.evict).toEqual([]);
  });

  it("evicts when no prefetch needed", () => {
    const layout = baseLayout(10);
    const slots: TurnSlot[] = Array.from({ length: 10 }, (_, i) =>
      slot("active", i + 1, undefined, "loaded"),
    );
    const plan = planMemoryReconcile(
      ctx({
        layout,
        turnSlots: slots,
        isBottomAnchored: true,
        maxTurn: 10,
      }),
    );
    expect(plan.prefetchSlotKeys).toEqual([]);
    expect(plan.evict.length).toBeGreaterThan(0);
  });
});

describe("protectedMaxTurnKey", () => {
  it("returns active key for max turn", () => {
    expect(protectedMaxTurnKey(5)).toBe(turnSlotKey("active", 5));
  });
});
