import { useCallback, useRef, type RefObject } from "react";
import type { TurnSlot } from "../transcript/buildTurnSlots";
import type { VisibleTimelineEnvelope } from "../transcript/turnMemoryPolicy";

export type { VisibleTimelineEnvelope };

/** Tracks visible turn slot keys and timeline envelope for planMemoryWindow focal. */
export function useSlotVisibility(
  turnSlotsRef: RefObject<TurnSlot[]>,
  onVisibilityChange?: () => void,
) {
  const visibleSlotKeysRef = useRef<Set<string>>(new Set());
  const envelopeRef = useRef<VisibleTimelineEnvelope>({
    minIndex: null,
    maxIndex: null,
  });

  const recomputeEnvelope = useCallback((turnSlots: TurnSlot[]) => {
    const indices: number[] = [];
    for (const key of visibleSlotKeysRef.current) {
      const idx = turnSlots.findIndex((s) => s.slotKey === key);
      if (idx >= 0) indices.push(idx);
    }
    if (indices.length === 0) {
      envelopeRef.current = { minIndex: null, maxIndex: null };
    } else {
      envelopeRef.current = {
        minIndex: Math.min(...indices),
        maxIndex: Math.max(...indices),
      };
    }
  }, []);

  const setSlotVisibility = useCallback(
    (slotKey: string, visible: boolean) => {
      if (visible) {
        visibleSlotKeysRef.current.add(slotKey);
      } else {
        visibleSlotKeysRef.current.delete(slotKey);
      }
      recomputeEnvelope(turnSlotsRef.current ?? []);
      onVisibilityChange?.();
    },
    [onVisibilityChange, recomputeEnvelope, turnSlotsRef],
  );

  return { visibleSlotKeysRef, envelopeRef, setSlotVisibility, recomputeEnvelope };
}
