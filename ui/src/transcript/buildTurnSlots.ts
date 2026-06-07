import type { SessionTranscriptLayout } from "./service";
import { turnSlotKey } from "./turnSlotKey";

export type TurnSlotStatus = "idle" | "loading" | "loaded" | "error";

export type TurnSlot = {
  slotKey: string;
  kind: "archive" | "active";
  turnNumber: number;
  epoch?: number;
  status: TurnSlotStatus;
  errorMessage?: string;
};

/** Build lazy-load slot list from server layout (all slots start as `idle`). */
export function buildTurnSlots(layout: SessionTranscriptLayout): TurnSlot[] {
  const slots: TurnSlot[] = [];
  for (const arch of layout.archives) {
    for (let t = arch.bounds.minTurn; t <= arch.bounds.maxTurn; t++) {
      slots.push({
        slotKey: turnSlotKey("archive", t, arch.epoch),
        kind: "archive",
        turnNumber: t,
        epoch: arch.epoch,
        status: "idle",
      });
    }
  }
  if (layout.hasContextRefresh) {
    slots.push({
      slotKey: turnSlotKey("active", 0),
      kind: "active",
      turnNumber: 0,
      status: "idle",
    });
  }
  const start = layout.hasContextRefresh
    ? Math.max(1, layout.active.minTurn)
    : layout.active.minTurn;
  for (let t = start; t <= layout.active.maxTurn; t++) {
    if (t === 0) continue;
    slots.push({
      slotKey: turnSlotKey("active", t),
      kind: "active",
      turnNumber: t,
      status: "idle",
    });
  }
  return slots;
}

/** Preserve loaded/error state when layout grows (e.g. after turn-complete tail reload). */
export function appendMissingTurnSlots(
  existing: TurnSlot[],
  layout: SessionTranscriptLayout,
): TurnSlot[] {
  const byKey = new Map(existing.map((s) => [s.slotKey, s]));
  return buildTurnSlots(layout).map((slot) => byKey.get(slot.slotKey) ?? slot);
}
