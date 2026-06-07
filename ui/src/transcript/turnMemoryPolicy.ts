import type { TurnSlot } from "./buildTurnSlots";
import {
  MAX_LOADED_TURNS,
  TAIL_LOADED_TURNS,
  TURN_LOAD_BATCH,
  VIEW_LOADED_TURNS,
} from "./loadPolicy";
import type { SessionTranscriptLayout } from "./service";
import { findTimelineAnchorIndex } from "./turnLoadPlan";
import { turnSlotKey } from "./turnSlotKey";

export function isInBottomAnchorZone(el: HTMLElement, threshold: number): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight <= threshold;
}

export type VisibleTimelineEnvelope = {
  minIndex: number | null;
  maxIndex: number | null;
};

export type MemoryWindowContext = {
  layout: SessionTranscriptLayout;
  turnSlots: TurnSlot[];
  visibleSlotKeys: ReadonlySet<string>;
  visibleEnvelope: VisibleTimelineEnvelope;
  isBottomAnchored: boolean;
  contentUnderflow: boolean;
  maxTurn: number;
  compactionPaused: boolean;
};

export type TailCompactionTarget = {
  turnNumber: number;
  archiveEpoch?: number;
};

export type MemoryReconcilePlan = {
  evict: TailCompactionTarget[];
  prefetchSlotKeys: string[];
};

function slotToTarget(slot: TurnSlot): TailCompactionTarget {
  return slot.kind === "archive"
    ? { turnNumber: slot.turnNumber, archiveEpoch: slot.epoch }
    : { turnNumber: slot.turnNumber };
}

function isProtectedSlot(
  slot: TurnSlot,
  layout: SessionTranscriptLayout,
  maxTurn: number,
): boolean {
  if (slot.kind === "active" && slot.turnNumber === maxTurn) return true;
  if (layout.hasContextRefresh && slot.kind === "active" && slot.turnNumber === 0) {
    return true;
  }
  return false;
}

function indexByKey(slots: TurnSlot[]): Map<string, number> {
  return new Map(slots.map((s, i) => [s.slotKey, i]));
}

function pickFarthestFromFocal(
  pool: string[],
  slots: TurnSlot[],
  focalIndex: number,
): string | undefined {
  if (pool.length === 0) return undefined;
  const idxMap = indexByKey(slots);
  pool.sort((a, b) => {
    const ia = idxMap.get(a) ?? 0;
    const ib = idxMap.get(b) ?? 0;
    return Math.abs(ib - focalIndex) - Math.abs(ia - focalIndex);
  });
  return pool[0];
}

/** Only used when loaded count exceeds MAX_LOADED_TURNS after window trim. */
function pickOverflowEvictionKey(
  candidateKeys: string[],
  slots: TurnSlot[],
  focalIndex: number,
  protectedKey: string,
  visibleKeys: ReadonlySet<string>,
): string | undefined {
  const candidates = candidateKeys.filter((k) => k !== protectedKey);
  if (candidates.length === 0) return undefined;

  if (visibleKeys.size > 0) {
    const hidden = candidates.filter((k) => !visibleKeys.has(k));
    if (hidden.length > 0) {
      return pickFarthestFromFocal(hidden, slots, focalIndex);
    }
  }

  return pickFarthestFromFocal(candidates, slots, focalIndex);
}

function hasOlderIdleToPrefetch(slots: TurnSlot[]): boolean {
  const minLoadedIdx = slots.findIndex((s) => s.status === "loaded");
  if (minLoadedIdx <= 0) return false;
  return slots[minLoadedIdx - 1]?.status === "idle";
}

/** Bottom-anchored underfill: prefetch idle turns above the leftmost loaded slot. */
export function planTailContentFill(ctx: MemoryWindowContext): string[] {
  if (!ctx.isBottomAnchored || ctx.compactionPaused || !ctx.contentUnderflow) {
    return [];
  }

  const { turnSlots } = ctx;
  const minLoadedIdx = turnSlots.findIndex((s) => s.status === "loaded");
  if (minLoadedIdx <= 0) return [];

  const prefetchKeys: string[] = [];
  for (let i = minLoadedIdx - 1; i >= 0 && prefetchKeys.length < TURN_LOAD_BATCH; i--) {
    if (turnSlots[i].status !== "idle") break;
    prefetchKeys.unshift(turnSlots[i].slotKey);
  }

  if (prefetchKeys.length === 0) return [];

  const loadedCount = turnSlots.filter((s) => s.status === "loaded").length;
  const maxPrefetch = Math.max(0, MAX_LOADED_TURNS - loadedCount);
  return prefetchKeys.slice(0, maxPrefetch);
}

/**
 * Eviction plan: bottom tail trim, browse VIEW window, then MAX overflow.
 * Skips tail evict when contentUnderflow and older idle slots remain (prefetch handles fill).
 */
export function planMemoryWindow(ctx: MemoryWindowContext): TailCompactionTarget[] {
  const { layout, turnSlots, visibleSlotKeys, visibleEnvelope, maxTurn } = ctx;
  if (maxTurn < 1) return [];

  const protectedKey = turnSlotKey("active", maxTurn);
  const idxMap = indexByKey(turnSlots);

  const loadedSlots = turnSlots.filter((s) => s.status === "loaded");
  if (loadedSlots.length === 0) return [];

  let evictKeys = new Set<string>();

  const addEvict = (slot: TurnSlot) => {
    if (isProtectedSlot(slot, layout, maxTurn)) return;
    evictKeys.add(slot.slotKey);
  };

  if (ctx.isBottomAnchored && !ctx.compactionPaused) {
    const skipTailEvict =
      ctx.contentUnderflow && hasOlderIdleToPrefetch(turnSlots);
    if (!skipTailEvict) {
      const keepFrom = Math.max(1, maxTurn - TAIL_LOADED_TURNS + 1);
      for (const slot of turnSlots) {
        if (slot.status !== "loaded") continue;
        if (slot.kind === "archive") {
          addEvict(slot);
          continue;
        }
        if (slot.turnNumber >= keepFrom && slot.turnNumber <= maxTurn) continue;
        addEvict(slot);
      }
    }
  } else {
    const visibleIndices: number[] = [];
    for (const key of visibleSlotKeys) {
      const idx = idxMap.get(key);
      if (idx !== undefined) visibleIndices.push(idx);
    }

    if (visibleIndices.length > 0) {
      const minIdx = Math.min(...visibleIndices);
      const maxIdx = Math.max(...visibleIndices);
      const visibleSpan = maxIdx - minIdx + 1;
      const focal = minIdx;
      let windowEnd =
        visibleSpan > VIEW_LOADED_TURNS ? maxIdx : focal + VIEW_LOADED_TURNS - 1;
      windowEnd = Math.min(windowEnd, turnSlots.length - 1);

      for (const slot of turnSlots) {
        if (slot.status !== "loaded") continue;
        const idx = idxMap.get(slot.slotKey);
        if (idx === undefined) continue;
        if (idx >= focal && idx <= windowEnd) continue;
        addEvict(slot);
      }
    }
  }

  let remainingLoaded = turnSlots.filter(
    (s) => s.status === "loaded" && !evictKeys.has(s.slotKey),
  );

  const focalIndex =
    visibleEnvelope.minIndex ??
    findTimelineAnchorIndex(turnSlots, maxTurn);

  while (remainingLoaded.length > MAX_LOADED_TURNS) {
    const candidateKeys = remainingLoaded
      .filter((s) => !isProtectedSlot(s, layout, maxTurn))
      .map((s) => s.slotKey);
    const evictKey = pickOverflowEvictionKey(
      candidateKeys,
      turnSlots,
      focalIndex,
      protectedKey,
      visibleSlotKeys,
    );
    if (!evictKey) break;
    evictKeys.add(evictKey);
    remainingLoaded = remainingLoaded.filter((s) => s.slotKey !== evictKey);
  }

  return turnSlots
    .filter((s) => evictKeys.has(s.slotKey))
    .map(slotToTarget);
}

export function planMemoryReconcile(ctx: MemoryWindowContext): MemoryReconcilePlan {
  const prefetchSlotKeys = planTailContentFill(ctx);
  const evict =
    prefetchSlotKeys.length > 0 ? [] : planMemoryWindow(ctx);
  return { evict, prefetchSlotKeys };
}

export function protectedMaxTurnKey(maxTurn: number): string {
  return turnSlotKey("active", maxTurn);
}
