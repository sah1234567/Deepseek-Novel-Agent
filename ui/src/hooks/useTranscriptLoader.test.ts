/** @vitest-environment jsdom */
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TranscriptEvent } from "../transcript/types";
import { TAIL_COMPACT_DEBOUNCE_MS, TAIL_LOADED_TURNS } from "../transcript/loadPolicy";
import type { TranscriptLoaderOptions } from "./useTranscriptLoader";
import { useTranscriptLoader } from "./useTranscriptLoader";

const fetchTranscriptLayout = vi.fn();
const fetchActiveTurns = vi.fn();

vi.mock("../transcript/service", () => ({
  fetchTranscriptLayout: (...args: unknown[]) => fetchTranscriptLayout(...args),
  fetchActiveTurns: (...args: unknown[]) => fetchActiveTurns(...args),
  fetchArchiveTurns: vi.fn().mockResolvedValue([]),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

function mockLoaderOptions(compactionPaused = false): TranscriptLoaderOptions {
  return {
    visibleSlotKeysRef: { current: new Set<string>() },
    envelopeRef: { current: { minIndex: null, maxIndex: null } },
    isBottomAnchoredRef: { current: false },
    contentUnderflowRef: { current: false },
    compactionPausedRef: { current: compactionPaused },
    isStreamingRef: { current: false },
    turnInProgressRef: { current: false },
    appTurnInProgressRef: { current: false },
  };
}

describe("useTranscriptLoader bootstrap stability", () => {
  beforeEach(() => {
    fetchTranscriptLayout.mockReset();
    fetchActiveTurns.mockReset();
    fetchTranscriptLayout.mockResolvedValue({
      hasContextRefresh: false,
      active: { minTurn: 1, maxTurn: 2 },
      archives: [],
    });
    fetchActiveTurns.mockResolvedValue([
      { turnNumber: 1, messages: [] },
      { turnNumber: 2, messages: [] },
    ]);
  });

  it("bootstraps once per sessionId even when turn slot status updates", async () => {
    const dispatch = vi.fn<(event: TranscriptEvent) => void>();
    const loaderOptions = mockLoaderOptions();
    const { rerender } = renderHook(
      ({ sessionId }: { sessionId: string }) =>
        useTranscriptLoader(sessionId, dispatch, loaderOptions),
      { initialProps: { sessionId: "session-a" } },
    );

    await waitFor(() => {
      expect(fetchTranscriptLayout).toHaveBeenCalledTimes(1);
    });
    await waitFor(() => {
      expect(fetchActiveTurns).toHaveBeenCalledTimes(1);
    });

    const resetCalls = dispatch.mock.calls.filter(
      ([event]) => event.type === "RESET_TRANSCRIPT",
    ).length;
    expect(resetCalls).toBe(1);

    dispatch.mockClear();
    fetchTranscriptLayout.mockClear();
    fetchActiveTurns.mockClear();

    rerender({ sessionId: "session-a" });

    await act(async () => {
      await new Promise((r) => setTimeout(r, 20));
    });

    expect(fetchTranscriptLayout).not.toHaveBeenCalled();
    expect(dispatch.mock.calls.filter(([e]) => e.type === "RESET_TRANSCRIPT")).toHaveLength(0);
  });
});

describe("useTranscriptLoader tail compaction", () => {
  beforeEach(() => {
    fetchTranscriptLayout.mockReset();
    fetchActiveTurns.mockReset();
    const maxTurn = 10;
    fetchTranscriptLayout.mockResolvedValue({
      hasContextRefresh: false,
      active: { minTurn: 1, maxTurn },
      archives: [],
    });
    fetchActiveTurns.mockImplementation(async (_sid: string, from: number, to: number) =>
      Array.from({ length: to - from + 1 }, (_, i) => ({
        turnNumber: from + i,
        messages: [],
      })),
    );
  });

  it("evicts turns outside tail window after bottom anchor debounce", async () => {
    const dispatch = vi.fn<(event: TranscriptEvent) => void>();
    const loaderOptions = mockLoaderOptions(false);

    const { result } = renderHook(() =>
      useTranscriptLoader("session-compact", dispatch, loaderOptions),
    );

    await waitFor(() => {
      expect(result.current.isBootstrapping).toBe(false);
    });

    const loadedCount = result.current.turnSlots.filter((s) => s.status === "loaded").length;
    expect(loadedCount).toBe(TAIL_LOADED_TURNS);

    await act(async () => {
      result.current.onLoadTurn("a:1");
    });

    await waitFor(() => {
      expect(result.current.turnSlots.filter((s) => s.status === "loaded").length).toBeGreaterThan(
        TAIL_LOADED_TURNS,
      );
    });

    dispatch.mockClear();

    vi.useFakeTimers();
    try {
      act(() => {
        result.current.onBottomAnchorChange(true);
      });
      await act(async () => {
        vi.advanceTimersByTime(TAIL_COMPACT_DEBOUNCE_MS + 10);
      });
    } finally {
      vi.useRealTimers();
    }

    await waitFor(() => {
      const evictCalls = dispatch.mock.calls.filter(([e]) => e.type === "EVICT_TURNS");
      expect(evictCalls.length).toBeGreaterThan(0);
    });

    const keepFrom = 10 - TAIL_LOADED_TURNS + 1;
    const loadedTurns = result.current.turnSlots
      .filter((s) => s.status === "loaded" && s.kind === "active")
      .map((s) => s.turnNumber)
      .sort((a, b) => a - b);
    expect(loadedTurns).toEqual(
      Array.from({ length: TAIL_LOADED_TURNS }, (_, i) => keepFrom + i),
    );
  });

  it("skips compaction while streaming pause ref is set", async () => {
    const dispatch = vi.fn<(event: TranscriptEvent) => void>();
    const loaderOptions = mockLoaderOptions(true);

    const { result } = renderHook(() =>
      useTranscriptLoader("session-paused", dispatch, loaderOptions),
    );

    await waitFor(() => {
      expect(result.current.isBootstrapping).toBe(false);
    });

    dispatch.mockClear();

    vi.useFakeTimers();
    try {
      act(() => {
        result.current.onBottomAnchorChange(true);
      });
      await act(async () => {
        vi.advanceTimersByTime(TAIL_COMPACT_DEBOUNCE_MS + 50);
      });
    } finally {
      vi.useRealTimers();
    }

    expect(dispatch.mock.calls.filter(([e]) => e.type === "EVICT_TURNS")).toHaveLength(0);
  });

  it("prefetches idle turns when content underflow", async () => {
    const dispatch = vi.fn<(event: TranscriptEvent) => void>();
    const loaderOptions = mockLoaderOptions(false);
    loaderOptions.isBottomAnchoredRef.current = true;
    loaderOptions.contentUnderflowRef.current = true;

    const maxTurn = 8;
    fetchTranscriptLayout.mockResolvedValue({
      hasContextRefresh: false,
      active: { minTurn: 1, maxTurn },
      archives: [],
    });

    const { result } = renderHook(() =>
      useTranscriptLoader("session-underflow", dispatch, loaderOptions),
    );

    await waitFor(() => {
      expect(result.current.isBootstrapping).toBe(false);
    });

    const fetchCallsAfterBootstrap = fetchActiveTurns.mock.calls.length;
    dispatch.mockClear();

    act(() => {
      result.current.scheduleReconcile();
    });

    await waitFor(() => {
      expect(fetchActiveTurns.mock.calls.length).toBeGreaterThan(fetchCallsAfterBootstrap);
    });
  });
});
