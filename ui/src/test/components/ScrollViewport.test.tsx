/** @vitest-environment jsdom */
import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ScrollViewport } from "../../components/layout/ScrollViewport";
import { BOTTOM_ANCHOR_THRESHOLD_PX } from "../../transcript/loadPolicy";

describe("ScrollViewport bottom follow", () => {
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

    Object.defineProperty(HTMLElement.prototype, "scrollHeight", {
      configurable: true,
      get() {
        return (this as HTMLElement & { _scrollHeight?: number })._scrollHeight ?? 500;
      },
    });
    Object.defineProperty(HTMLElement.prototype, "clientHeight", {
      configurable: true,
      get() {
        return 200;
      },
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("re-pins and scrolls to bottom when content grows while near bottom", () => {
    const onBottomAnchorChange = vi.fn();
    const { rerender } = render(
      <ScrollViewport
        autoScrollTo="bottom"
        autoScrollDeps={["v1"]}
        onBottomAnchorChange={onBottomAnchorChange}
      >
        <div>content</div>
      </ScrollViewport>,
    );

    const viewport = document.querySelector(".scroll-viewport-body") as HTMLElement & {
      _scrollHeight?: number;
    };
    expect(viewport).toBeTruthy();
    viewport._scrollHeight = 500;
    viewport.scrollTop = 500 - 200 - 50;
    Object.defineProperty(viewport, "scrollTop", {
      configurable: true,
      writable: true,
      value: 500 - 200 - 50,
    });

    act(() => {
      viewport.scrollTop = 500 - 200 - 50;
      viewport.dispatchEvent(new Event("scroll"));
    });

    viewport._scrollHeight = 900;
    act(() => {
      resizeCallback?.();
    });

    expect(viewport.scrollTop).toBe(900);

    rerender(
      <ScrollViewport
        autoScrollTo="bottom"
        autoScrollDeps={["v2"]}
        onBottomAnchorChange={onBottomAnchorChange}
      >
        <div>more content</div>
      </ScrollViewport>,
    );

    act(() => {
      resizeCallback?.();
    });

    expect(viewport.scrollTop).toBe(900);
    expect(onBottomAnchorChange).toHaveBeenCalled();
  });

  it("uses BOTTOM_ANCHOR_THRESHOLD_PX for anchor detection", () => {
    expect(BOTTOM_ANCHOR_THRESHOLD_PX).toBeGreaterThan(64);
  });

  it("follows bottom when near-bottom but not pinned", async () => {
    const autoScrollControlRef = {
      current: null as import("../../components/layout/ScrollViewport").ScrollViewportAutoScrollControl | null,
    };
    render(
      <ScrollViewport
        autoScrollTo="bottom"
        autoScrollDeps={["v1"]}
        autoScrollControlRef={autoScrollControlRef}
      >
        <div>content</div>
      </ScrollViewport>,
    );

    const viewport = document.querySelector(".scroll-viewport-body") as HTMLElement & {
      _scrollHeight?: number;
      _scrollTop?: number;
    };
    expect(viewport).toBeTruthy();

    await act(async () => {
      await Promise.resolve();
    });
    autoScrollControlRef.current?.unpin();

    viewport._scrollHeight = 500;
    const nearBottomTop = 500 - 200 - 100;
    Object.defineProperty(viewport, "scrollTop", {
      configurable: true,
      get() {
        return (this as HTMLElement & { _scrollTop?: number })._scrollTop ?? 0;
      },
      set(v: number) {
        (this as HTMLElement & { _scrollTop?: number })._scrollTop = v;
      },
    });

    act(() => {
      viewport.scrollTop = nearBottomTop;
      viewport.dispatchEvent(new Event("scroll"));
    });

    viewport._scrollHeight = 800;
    act(() => {
      resizeCallback?.();
    });

    await act(async () => {
      await new Promise((r) => requestAnimationFrame(r));
    });

    expect(viewport.scrollTop).toBe(800);
    expect(nearBottomTop).toBeLessThanOrEqual(500 - 200 - 64);
  });

  it("does not follow when far from bottom", () => {
    render(
      <ScrollViewport autoScrollTo="bottom" autoScrollDeps={["v1"]}>
        <div>content</div>
      </ScrollViewport>,
    );

    const viewport = document.querySelector(".scroll-viewport-body") as HTMLElement & {
      _scrollHeight?: number;
    };
    viewport._scrollHeight = 1000;
    Object.defineProperty(viewport, "scrollTop", {
      configurable: true,
      writable: true,
      value: 50,
    });

    act(() => {
      viewport.dispatchEvent(new Event("scroll"));
    });

    viewport._scrollHeight = 1500;
    const scrollTopBefore = viewport.scrollTop;
    act(() => {
      resizeCallback?.();
    });

    expect(viewport.scrollTop).toBe(scrollTopBefore);
  });
});
