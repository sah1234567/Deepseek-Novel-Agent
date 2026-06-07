import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type MutableRefObject,
} from "react";
import { onCompactionDone } from "../ipc/compactionDone";
import type { TranscriptEvent } from "../transcript/types";
import {
  fetchActiveTurns,
  fetchArchiveTurns,
  fetchTranscriptLayout,
  type SessionTranscriptLayout,
} from "../transcript/service";
import { appendMissingTurnSlots, buildTurnSlots, type TurnSlot } from "../transcript/buildTurnSlots";
import {
  TAIL_COMPACT_DEBOUNCE_MS,
  TAIL_LOADED_TURNS,
  TURN_LOAD_BATCH,
} from "../transcript/loadPolicy";
import { turnSlotKey, type TurnSlotKind } from "../transcript/turnSlotKey";
import { planTurnLoadSegments } from "../transcript/turnLoadPlan";
import {
  planMemoryReconcile,
  type VisibleTimelineEnvelope,
} from "../transcript/turnMemoryPolicy";

export type TranscriptLoaderView = {
  layout: SessionTranscriptLayout | null;
  turnSlots: TurnSlot[];
  /** True while layout + initial tail turns are loading after session switch. */
  isBootstrapping: boolean;
  bootstrapError: string | null;
  onLoadTurn: (slotKey: string) => void;
  reloadActiveTail: () => Promise<void>;
  onBottomAnchorChange: (anchored: boolean) => void;
  scheduleReconcile: () => void;
};

export type TranscriptLoaderOptions = {
  visibleSlotKeysRef: MutableRefObject<Set<string>>;
  envelopeRef: MutableRefObject<VisibleTimelineEnvelope>;
  isBottomAnchoredRef: MutableRefObject<boolean>;
  contentUnderflowRef: MutableRefObject<boolean>;
  compactionPausedRef: MutableRefObject<boolean>;
};

export function useTranscriptLoader(
  sessionId: string | undefined,
  dispatch: (event: TranscriptEvent) => void,
  options: TranscriptLoaderOptions,
): TranscriptLoaderView {
  const [layout, setLayout] = useState<SessionTranscriptLayout | null>(null);
  const [turnSlots, setTurnSlots] = useState<TurnSlot[]>([]);
  const [isBootstrapping, setIsBootstrapping] = useState(false);
  const [bootstrapError, setBootstrapError] = useState<string | null>(null);
  const layoutRef = useRef<SessionTranscriptLayout | null>(null);
  const turnSlotsRef = useRef<TurnSlot[]>([]);
  turnSlotsRef.current = turnSlots;
  const loadingKeysRef = useRef<Set<string>>(new Set());
  const bootstrapGenRef = useRef(0);
  const sessionIdRef = useRef(sessionId);
  sessionIdRef.current = sessionId;
  const dispatchRef = useRef(dispatch);
  dispatchRef.current = dispatch;
  const {
    visibleSlotKeysRef,
    envelopeRef,
    isBottomAnchoredRef,
    contentUnderflowRef,
    compactionPausedRef,
  } = options;
  const compactTimerRef = useRef<number | null>(null);
  const reconcileRafRef = useRef<number | null>(null);

  const markSlots = useCallback(
    (keys: string[], patch: Partial<TurnSlot>) => {
      const keySet = new Set(keys);
      setTurnSlots((prev) =>
        prev.map((s) => (keySet.has(s.slotKey) ? { ...s, ...patch } : s)),
      );
    },
    [],
  );

  const onLoadTurnRef = useRef<(slotKey: string) => void>(() => {});

  /** Unified memory reconcile: prefetch underfill first, then evict via planMemoryReconcile. */
  const reconcileMemoryWindow = useCallback(() => {
    const currentLayout = layoutRef.current;
    if (!currentLayout) return;

    const ctx = {
      layout: currentLayout,
      turnSlots: turnSlotsRef.current,
      visibleSlotKeys: visibleSlotKeysRef.current,
      visibleEnvelope: envelopeRef.current,
      isBottomAnchored: isBottomAnchoredRef.current,
      contentUnderflow: contentUnderflowRef.current,
      maxTurn: currentLayout.active.maxTurn,
      compactionPaused: compactionPausedRef.current,
    };

    const { evict, prefetchSlotKeys } = planMemoryReconcile(ctx);

    for (const key of prefetchSlotKeys) {
      onLoadTurnRef.current(key);
    }

    if (evict.length === 0) return;

    dispatchRef.current({ type: "EVICT_TURNS", turns: evict });
    const evictKeys = evict.map((t) =>
      t.archiveEpoch !== undefined
        ? turnSlotKey("archive", t.turnNumber, t.archiveEpoch)
        : turnSlotKey("active", t.turnNumber),
    );
    markSlots(evictKeys, { status: "idle", errorMessage: undefined });
  }, [
    compactionPausedRef,
    contentUnderflowRef,
    envelopeRef,
    isBottomAnchoredRef,
    markSlots,
    visibleSlotKeysRef,
  ]);

  const scheduleReconcile = useCallback(() => {
    if (reconcileRafRef.current !== null) {
      cancelAnimationFrame(reconcileRafRef.current);
    }
    reconcileRafRef.current = requestAnimationFrame(() => {
      reconcileRafRef.current = null;
      reconcileMemoryWindow();
    });
  }, [reconcileMemoryWindow]);

  const onBottomAnchorChange = useCallback(
    (anchored: boolean) => {
      isBottomAnchoredRef.current = anchored;

      if (compactTimerRef.current !== null) {
        window.clearTimeout(compactTimerRef.current);
        compactTimerRef.current = null;
      }

      if (!anchored) {
        scheduleReconcile();
        return;
      }

      compactTimerRef.current = window.setTimeout(() => {
        compactTimerRef.current = null;
        reconcileMemoryWindow();
      }, TAIL_COMPACT_DEBOUNCE_MS);
    },
    [isBottomAnchoredRef, reconcileMemoryWindow, scheduleReconcile],
  );

  useEffect(
    () => () => {
      if (compactTimerRef.current !== null) {
        window.clearTimeout(compactTimerRef.current);
      }
      if (reconcileRafRef.current !== null) {
        cancelAnimationFrame(reconcileRafRef.current);
      }
    },
    [],
  );

  const loadTurnRange = useCallback(
    async (
      kind: TurnSlotKind,
      fromTurn: number,
      toTurn: number,
      epoch?: number,
      slotKeys?: string[],
    ) => {
      const sid = sessionIdRef.current;
      if (!sid) return;
      const keys =
        slotKeys ??
        turnSlotsRef.current
          .filter(
            (s) =>
              s.kind === kind &&
              s.turnNumber >= fromTurn &&
              s.turnNumber <= toTurn &&
              (kind === "active" || s.epoch === epoch),
          )
          .map((s) => s.slotKey);

      if (keys.some((k) => loadingKeysRef.current.has(k))) return;
      keys.forEach((k) => loadingKeysRef.current.add(k));
      markSlots(keys, { status: "loading", errorMessage: undefined });

      try {
        const bundles =
          kind === "archive" && epoch !== undefined
            ? await fetchArchiveTurns(sid, epoch, fromTurn, toTurn)
            : await fetchActiveTurns(sid, fromTurn, toTurn);

        dispatchRef.current({
          type: "MERGE_TURNS",
          bundles,
          archiveEpoch: kind === "archive" ? epoch : undefined,
        });
        markSlots(keys, { status: "loaded" });
        scheduleReconcile();
      } catch (e) {
        markSlots(keys, { status: "error", errorMessage: String(e) });
      } finally {
        keys.forEach((k) => loadingKeysRef.current.delete(k));
      }
    },
    [markSlots, scheduleReconcile],
  );

  const onLoadTurn = useCallback(
    (slotKey: string) => {
      const segments = planTurnLoadSegments(
        turnSlotsRef.current,
        slotKey,
        TURN_LOAD_BATCH,
      );
      for (const seg of segments) {
        void loadTurnRange(seg.kind, seg.fromTurn, seg.toTurn, seg.epoch, seg.keys);
      }
    },
    [loadTurnRange],
  );

  onLoadTurnRef.current = onLoadTurn;

  const resetAndBootstrap = useCallback(async () => {
    const sid = sessionIdRef.current;
    if (!sid) {
      setLayout(null);
      setTurnSlots([]);
      layoutRef.current = null;
      visibleSlotKeysRef.current.clear();
      envelopeRef.current = { minIndex: null, maxIndex: null };
      setIsBootstrapping(false);
      return;
    }

    const gen = ++bootstrapGenRef.current;
    setIsBootstrapping(true);
    setBootstrapError(null);
    setLayout(null);
    setTurnSlots([]);
    layoutRef.current = null;
    visibleSlotKeysRef.current.clear();
    envelopeRef.current = { minIndex: null, maxIndex: null };
    loadingKeysRef.current.clear();

    try {
      const nextLayout = await fetchTranscriptLayout(sid);
      if (gen !== bootstrapGenRef.current) return;

      dispatchRef.current({ type: "RESET_TRANSCRIPT" });

      layoutRef.current = nextLayout;
      setLayout(nextLayout);
      const slots = buildTurnSlots(nextLayout);
      setTurnSlots(slots);

      if (nextLayout.hasContextRefresh) {
        await loadTurnRange("active", 0, 0, undefined, [turnSlotKey("active", 0)]);
        if (gen !== bootstrapGenRef.current) return;
      }

      const maxTurn = nextLayout.active.maxTurn;
      if (maxTurn >= 1) {
        const from = Math.max(1, maxTurn - TAIL_LOADED_TURNS + 1);
        const tailKeys = slots
          .filter(
            (s) =>
              s.kind === "active" && s.turnNumber >= from && s.turnNumber <= maxTurn,
          )
          .map((s) => s.slotKey);
        await loadTurnRange("active", from, maxTurn, undefined, tailKeys);
      }
    } catch (e) {
      if (gen !== bootstrapGenRef.current) return;
      setLayout(null);
      setTurnSlots([]);
      layoutRef.current = null;
      setBootstrapError(String(e));
    } finally {
      if (gen === bootstrapGenRef.current) {
        setIsBootstrapping(false);
      }
    }
  }, [envelopeRef, loadTurnRange, visibleSlotKeysRef]);

  const reloadActiveTail = useCallback(async () => {
    const sid = sessionIdRef.current;
    if (!sid) return;

    const nextLayout = await fetchTranscriptLayout(sid);
    layoutRef.current = nextLayout;
    setLayout(nextLayout);
    setTurnSlots((prev) => appendMissingTurnSlots(prev, nextLayout));

    const maxTurn = nextLayout.active.maxTurn;
    if (maxTurn < 1) return;

    await loadTurnRange("active", maxTurn, maxTurn, undefined, [
      turnSlotKey("active", maxTurn),
    ]);
  }, [loadTurnRange]);

  useEffect(() => {
    if (!sessionId) {
      setLayout(null);
      setTurnSlots([]);
      layoutRef.current = null;
      visibleSlotKeysRef.current.clear();
      envelopeRef.current = { minIndex: null, maxIndex: null };
      loadingKeysRef.current.clear();
      setIsBootstrapping(false);
      return;
    }
    void resetAndBootstrap();
  }, [envelopeRef, resetAndBootstrap, sessionId, visibleSlotKeysRef]);

  useEffect(() => {
    return onCompactionDone(() => {
      void resetAndBootstrap();
    });
  }, [resetAndBootstrap]);

  return {
    layout,
    turnSlots,
    isBootstrapping,
    bootstrapError,
    onLoadTurn,
    reloadActiveTail,
    onBottomAnchorChange,
    scheduleReconcile,
  };
}
