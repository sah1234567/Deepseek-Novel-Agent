/** @vitest-environment jsdom */
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useCompactionProgress } from "./useCompactionProgress";

type CompactionHandler = (event: { payload: unknown }) => void;

const listeners: Record<string, CompactionHandler> = {};

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((eventName: string, handler: CompactionHandler) => {
    listeners[eventName] = handler;
    return Promise.resolve(() => {
      delete listeners[eventName];
    });
  }),
}));

function emit(payload: unknown) {
  listeners["compaction-progress"]?.({ payload });
}

async function mountHook() {
  const hook = renderHook(() => useCompactionProgress());
  await act(async () => {
    await Promise.resolve();
  });
  expect(listeners["compaction-progress"]).toBeDefined();
  return hook;
}

describe("useCompactionProgress", () => {
  beforeEach(() => {
    delete listeners["compaction-progress"];
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("transitions started → generating-summary → rebuilding-session → done", async () => {
    vi.useFakeTimers();
    const { result } = await mountHook();

    act(() => emit({ action: "started", attempt: 1 }));
    expect(result.current.visible).toBe(true);
    expect(result.current.action).toBe("started");
    expect(result.current.variant).toBe("info");

    act(() => emit({ action: "generating-summary", attempt: 1 }));
    expect(result.current.action).toBe("generating-summary");

    act(() => emit({ action: "rebuilding-session", attempt: 1 }));
    expect(result.current.action).toBe("rebuilding-session");

    act(() =>
      emit({
        action: "done",
        attempt: 1,
        tokensBefore: 12000,
        tokensAfter: 4000,
      }),
    );
    expect(result.current.action).toBe("done");
    expect(result.current.tokensBefore).toBe(12000);
    expect(result.current.tokensAfter).toBe(4000);
    expect(result.current.variant).toBe("success");

    await act(async () => {
      vi.advanceTimersByTime(3000);
    });
    expect(result.current.visible).toBe(false);
  });

  it("shows failed reason then auto-clears after 5s", async () => {
    vi.useFakeTimers();
    const { result } = await mountHook();

    act(() =>
      emit({ action: "failed", reason: "timeout", attempt: 3 }),
    );
    expect(result.current.visible).toBe(true);
    expect(result.current.action).toBe("failed");
    expect(result.current.reason).toBe("timeout");
    expect(result.current.variant).toBe("warn");

    await act(async () => {
      vi.advanceTimersByTime(5000);
    });
    expect(result.current.visible).toBe(false);
  });
});
