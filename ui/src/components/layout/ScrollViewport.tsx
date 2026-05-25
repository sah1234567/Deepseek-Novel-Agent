import {
  ReactNode,
  useCallback,
  useEffect,
  useRef,
  useState,
  type UIEvent,
} from "react";
import "./ScrollViewport.css";

const NEAR_EDGE_PX = 64;
const SCROLL_STEP_PX = 120;
const HOLD_INTERVAL_MS = 50;

function isNearBottom(el: HTMLElement, threshold = NEAR_EDGE_PX) {
  return el.scrollHeight - el.scrollTop - el.clientHeight <= threshold;
}

function isNearTop(el: HTMLElement, threshold = NEAR_EDGE_PX) {
  return el.scrollTop <= threshold;
}

export function ScrollViewport({
  children,
  className,
  autoScrollDeps = [],
  autoScrollTo = "none",
  resetScrollKey,
  initialScrollTop = 0,
  overlayActive = false,
  onScrollPositionChange,
}: {
  children: ReactNode;
  className?: string;
  /** When these values change, auto-scroll if the user was already near the edge. */
  autoScrollDeps?: unknown[];
  autoScrollTo?: "bottom" | "none";
  /** When this value changes, restore scroll to `initialScrollTop` (e.g. opened another file). */
  resetScrollKey?: unknown;
  /** Scroll position to restore when `resetScrollKey` changes. */
  initialScrollTop?: number;
  /** While true, freeze auto-scroll and restore prior position when it becomes false. */
  overlayActive?: boolean;
  onScrollPositionChange?: (scrollTop: number) => void;
}) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const holdTimerRef = useRef<number | null>(null);
  const pinnedToBottomRef = useRef(autoScrollTo === "bottom");
  const savedScrollRef = useRef<{ scrollTop: number; pinnedToBottom: boolean } | null>(null);
  const prevOverlayActiveRef = useRef(overlayActive);
  const [canScrollUp, setCanScrollUp] = useState(false);
  const [canScrollDown, setCanScrollDown] = useState(false);

  const updateScrollState = useCallback(() => {
    const el = viewportRef.current;
    if (!el) return;
    const overflowing = el.scrollHeight > el.clientHeight + 1;
    setCanScrollUp(overflowing && !isNearTop(el));
    setCanScrollDown(overflowing && !isNearBottom(el));
  }, []);

  const scrollBy = useCallback((delta: number) => {
    viewportRef.current?.scrollBy({ top: delta, behavior: "auto" });
  }, []);

  const scrollToBottom = useCallback((behavior: ScrollBehavior = "smooth") => {
    const el = viewportRef.current;
    if (!el) return;
    el.scrollTo({ top: el.scrollHeight, behavior });
  }, []);

  const stopHoldScroll = useCallback(() => {
    if (holdTimerRef.current !== null) {
      window.clearInterval(holdTimerRef.current);
      holdTimerRef.current = null;
    }
  }, []);

  const startHoldScroll = useCallback(
    (delta: number) => {
      stopHoldScroll();
      scrollBy(delta);
      holdTimerRef.current = window.setInterval(() => scrollBy(delta), HOLD_INTERVAL_MS);
    },
    [scrollBy, stopHoldScroll],
  );

  useEffect(() => {
    if (resetScrollKey === undefined) return;
    const el = viewportRef.current;
    if (!el) return;
    el.scrollTo({ top: initialScrollTop, behavior: "auto" });
    pinnedToBottomRef.current = autoScrollTo === "bottom" && isNearBottom(el);
    updateScrollState();
  }, [resetScrollKey, initialScrollTop, autoScrollTo, updateScrollState]);

  useEffect(() => {
    const el = viewportRef.current;
    if (!el) return;

    const wasActive = prevOverlayActiveRef.current;
    const isActive = overlayActive;
    prevOverlayActiveRef.current = isActive;

    if (!wasActive && isActive) {
      savedScrollRef.current = {
        scrollTop: el.scrollTop,
        pinnedToBottom: pinnedToBottomRef.current,
      };
      return;
    }

    if (wasActive && !isActive && savedScrollRef.current) {
      if (savedScrollRef.current.pinnedToBottom) {
        scrollToBottom("auto");
        pinnedToBottomRef.current = true;
      } else {
        el.scrollTo({ top: savedScrollRef.current.scrollTop, behavior: "auto" });
        pinnedToBottomRef.current = savedScrollRef.current.pinnedToBottom;
      }
      savedScrollRef.current = null;
      updateScrollState();
    }
  }, [overlayActive, updateScrollState, scrollToBottom]);

  useEffect(() => {
    const el = viewportRef.current;
    if (!el || overlayActive) return;
    updateScrollState();
    if (autoScrollTo === "bottom" && pinnedToBottomRef.current) {
      scrollToBottom("auto");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...autoScrollDeps, overlayActive]);

  useEffect(() => {
    const el = viewportRef.current;
    if (!el) return;
    const observer = new ResizeObserver(() => {
      updateScrollState();
      if (!overlayActive && autoScrollTo === "bottom" && pinnedToBottomRef.current) {
        scrollToBottom("auto");
      }
    });
    observer.observe(el);
    for (const child of el.children) {
      observer.observe(child);
    }
    return () => observer.disconnect();
  }, [updateScrollState, children, overlayActive, autoScrollTo, scrollToBottom]);

  useEffect(() => () => stopHoldScroll(), [stopHoldScroll]);

  function onScroll(e: UIEvent<HTMLDivElement>) {
    const el = e.currentTarget;
    if (autoScrollTo === "bottom") {
      pinnedToBottomRef.current = isNearBottom(el);
    }
    onScrollPositionChange?.(el.scrollTop);
    const overflowing = el.scrollHeight > el.clientHeight + 1;
    setCanScrollUp(overflowing && !isNearTop(el));
    setCanScrollDown(overflowing && !isNearBottom(el));
  }

  const showControls = canScrollUp || canScrollDown;

  return (
    <div className="scroll-viewport">
      <div
        ref={viewportRef}
        className={className ? `scroll-viewport-body ${className}` : "scroll-viewport-body"}
        onScroll={onScroll}
      >
        {children}
      </div>
      {showControls && (
        <div className="scroll-viewport-controls" aria-hidden={!showControls}>
          {canScrollUp && (
            <button
              type="button"
              className="scroll-viewport-btn scroll-viewport-btn-up"
              title="向上查看更早内容（按住连续滚动）"
              aria-label="向上滚动"
              onMouseDown={(e) => {
                e.preventDefault();
                startHoldScroll(-SCROLL_STEP_PX);
              }}
              onMouseUp={stopHoldScroll}
              onMouseLeave={stopHoldScroll}
            >
              ↑
            </button>
          )}
          {canScrollDown && (
            <button
              type="button"
              className="scroll-viewport-btn scroll-viewport-btn-down"
              title="向下查看更新内容（按住连续滚动）"
              aria-label="向下滚动"
              onMouseDown={(e) => {
                e.preventDefault();
                startHoldScroll(SCROLL_STEP_PX);
              }}
              onMouseUp={stopHoldScroll}
              onMouseLeave={stopHoldScroll}
            >
              ↓
            </button>
          )}
        </div>
      )}
    </div>
  );
}
