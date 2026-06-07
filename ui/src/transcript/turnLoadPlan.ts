import type { TurnSlot } from "./buildTurnSlots";
import type { TurnSlotKind } from "./turnSlotKey";

const LOADED_WINDOW_STATUSES: TurnSlot["status"][] = ["loaded", "loading"];

/**
 * Idle slots far from the loaded window are omitted from the DOM (no 4rem × N scroll inflation).
 * Keeps at most one idle sentinel on each side of the loaded range for IntersectionObserver load.
 */
export function shouldRenderSlotInTimeline(slot: TurnSlot, turnSlots: TurnSlot[]): boolean {
  if (slot.status !== "idle") return true;

  const loadedIndices: number[] = [];
  for (let i = 0; i < turnSlots.length; i++) {
    if (LOADED_WINDOW_STATUSES.includes(turnSlots[i].status)) {
      loadedIndices.push(i);
    }
  }
  if (loadedIndices.length === 0) return false;

  const min = Math.min(...loadedIndices);
  const max = Math.max(...loadedIndices);
  const idx = turnSlots.findIndex((s) => s.slotKey === slot.slotKey);
  if (idx < 0) return false;
  return idx >= min - 1 && idx <= max + 1;
}

export type TurnLoadSegment = {
  kind: TurnSlotKind;
  epoch?: number;
  fromTurn: number;
  toTurn: number;
  keys: string[];
};

function sameStorage(a: TurnSlot, b: TurnSlot): boolean {
  return a.kind === b.kind && a.epoch === b.epoch;
}

/** Index of the streaming tail anchor (`active` + `maxTurn`) on the timeline. */
export function findTimelineAnchorIndex(slots: TurnSlot[], anchorTurn: number): number {
  const idx = slots.findIndex(
    (s) => s.kind === "active" && s.turnNumber === anchorTurn,
  );
  return idx >= 0 ? idx : Math.max(0, slots.length - 1);
}

/** Collect up to `maxBatch` contiguous idle slots forward on the timeline (ignores compact boundaries). */
export function collectAdjacentIdleWindow(
  slots: TurnSlot[],
  startKey: string,
  maxBatch: number,
): TurnSlot[] | null {
  const idx = slots.findIndex((s) => s.slotKey === startKey);
  if (idx < 0) return null;
  const slot = slots[idx];
  if (slot.status === "loading" || slot.status === "loaded") return null;

  const window: TurnSlot[] = [slot];
  for (let i = idx + 1; i < slots.length && window.length < maxBatch; i++) {
    if (slots[i].status !== "idle") break;
    window.push(slots[i]);
  }
  return window;
}

/** Split a timeline window into homogeneous IPC segments (`kind` + `epoch` + consecutive turn numbers). */
export function splitWindowIntoLoadSegments(window: TurnSlot[]): TurnLoadSegment[] {
  if (window.length === 0) return [];

  const segments: TurnLoadSegment[] = [];
  let chunk: TurnSlot[] = [window[0]];

  for (let i = 1; i < window.length; i++) {
    const prev = window[i - 1];
    const next = window[i];
    if (!sameStorage(prev, next) || next.turnNumber !== prev.turnNumber + 1) {
      segments.push(segmentFromSlots(chunk));
      chunk = [next];
    } else {
      chunk.push(next);
    }
  }
  segments.push(segmentFromSlots(chunk));
  return segments;
}

function segmentFromSlots(slots: TurnSlot[]): TurnLoadSegment {
  return {
    kind: slots[0].kind,
    epoch: slots[0].epoch,
    fromTurn: slots[0].turnNumber,
    toTurn: slots[slots.length - 1].turnNumber,
    keys: slots.map((s) => s.slotKey),
  };
}

export function planTurnLoadSegments(
  slots: TurnSlot[],
  startKey: string,
  maxBatch: number,
): TurnLoadSegment[] {
  const window = collectAdjacentIdleWindow(slots, startKey, maxBatch);
  if (!window) return [];
  return splitWindowIntoLoadSegments(window);
}
