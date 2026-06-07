/** @vitest-environment jsdom */
import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useViewportContentFill } from "./useViewportContentFill";
import { TAIL_CONTENT_UNDERFLOW_PX } from "../transcript/loadPolicy";

describe("useViewportContentFill", () => {
  let resizeCallback: (() => void) | null = null;

  beforeEach(() => {
    resizeCallback = null;
    globalThis.ResizeObserver = class {
      observe = vi.fn();
      unobserve = vi.fn();
      disconnect = vi.fn();
      constructor(cb: () => void) {
        resizeCallback = cb;
      }
    } as unknown as typeof ResizeObserver;
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("sets underflow true when bottom-anchored and content shorter than viewport", () => {
    const scrollRootRef = {
      current: {
        scrollHeight: 100,
        clientHeight: 200,
        firstElementChild: null,
      } as unknown as HTMLDivElement,
    };
    const isBottomAnchoredRef = { current: true };
    const onUnderflowChange = vi.fn();

    const { result } = renderHook(() =>
      useViewportContentFill(scrollRootRef, isBottomAnchoredRef, onUnderflowChange),
    );

    resizeCallback?.();
    expect(result.current.current).toBe(true);
    expect(onUnderflowChange).toHaveBeenCalled();
    expect(100).toBeLessThan(200 + TAIL_CONTENT_UNDERFLOW_PX);
  });

  it("sets underflow false when not bottom-anchored", () => {
    const scrollRootRef = {
      current: {
        scrollHeight: 100,
        clientHeight: 200,
        firstElementChild: null,
      } as unknown as HTMLDivElement,
    };
    const isBottomAnchoredRef = { current: false };

    const { result } = renderHook(() =>
      useViewportContentFill(scrollRootRef, isBottomAnchoredRef),
    );

    resizeCallback?.();
    expect(result.current.current).toBe(false);
  });
});
