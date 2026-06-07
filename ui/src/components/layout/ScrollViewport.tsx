import {
  forwardRef,
  ReactNode,
  useCallback,
  useEffect,
  useRef,
  useState,
  type UIEvent,
} from "react";
import { BOTTOM_ANCHOR_THRESHOLD_PX } from "../../transcript/loadPolicy";
import { isInBottomAnchorZone } from "../../transcript/turnMemoryPolicy";
import "./ScrollViewport.css";

const NEAR_TOP_PX = 64;
const SCROLL_STEP_PX = 120;
const HOLD_INTERVAL_MS = 50;

function isNearTop(el: HTMLElement, threshold = NEAR_TOP_PX) {
  return el.scrollTop <= threshold;
}

export type ScrollViewportAutoScrollControl = {
  /** Stop following the bottom while the user reads earlier content. */
  unpin: () => void;
  /** Pin to bottom and scroll instantly (e.g. after send). */
  pinAndScrollToBottom: () => void;
};

export const ScrollViewport = forwardRef(function ScrollViewport(
  {
    children,
    className,
    autoScrollDeps = [],
    autoScrollTo = "none",
    resetScrollKey,
    initialScrollTop = 0,
    overlayActive = false,
    onScrollPositionChange,
    onBottomAnchorChange,
    autoScrollControlRef,
    suspendAutoScrollRef,
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
    /** Fires when bottom-anchor zone membership changes (for tail compaction). */
    onBottomAnchorChange?: (anchored: boolean) => void;
    /** Imperative hook to unpin bottom-following before programmatic scroll. */
    autoScrollControlRef?: React.MutableRefObject<ScrollViewportAutoScrollControl | null>;
    /** While true, skip ResizeObserver / deps auto-scroll (e.g. sticky-prompt jump). */
    suspendAutoScrollRef?: React.MutableRefObject<boolean>;
  },
  ref: React.ForwardedRef<HTMLDivElement>,
) {
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const setViewportRef = useCallback(
    (node: HTMLDivElement | null) => {
      viewportRef.current = node;
      if (typeof ref === "function") {
        ref(node);
      } else if (ref) {
        ref.current = node;
      }
    },
    [ref],
  );
  const holdTimerRef = useRef<number | null>(null);
  const pinnedToBottomRef = useRef(autoScrollTo === "bottom");
  const savedScrollRef = useRef<{ scrollTop: number; pinnedToBottom: boolean } | null>(null);
  const prevOverlayActiveRef = useRef(overlayActive);
  const prevAnchoredRef = useRef<boolean | null>(null);
  const [canScrollUp, setCanScrollUp] = useState(false);
  const [canScrollDown, setCanScrollDown] = useState(false);

  const notifyBottomAnchor = useCallback(
    (el: HTMLElement) => {
      if (!onBottomAnchorChange || autoScrollTo !== "bottom") return;
      const anchored = isInBottomAnchorZone(el, BOTTOM_ANCHOR_THRESHOLD_PX);
      if (prevAnchoredRef.current === anchored) return;
      prevAnchoredRef.current = anchored;
      onBottomAnchorChange(anchored);
    },
    [autoScrollTo, onBottomAnchorChange],
  );

  const scrollToBottomInstant = useCallback((el: HTMLElement) => {
    el.scrollTop = el.scrollHeight;
  }, []);

  const followBottomIfAnchored = useCallback(() => {
    if (overlayActive || suspendAutoScrollRef?.current || autoScrollTo !== "bottom") {
      return;
    }
    const el = viewportRef.current;
    if (!el) return;

    const wasNearBottom =
      pinnedToBottomRef.current || isInBottomAnchorZone(el, BOTTOM_ANCHOR_THRESHOLD_PX);
    if (!wasNearBottom) return;

    scrollToBottomInstant(el);
    pinnedToBottomRef.current = true;
    requestAnimationFrame(() => {
      const node = viewportRef.current;
      if (!node) return;
      scrollToBottomInstant(node);
      notifyBottomAnchor(node);
    });
  }, [
    autoScrollTo,
    overlayActive,
    notifyBottomAnchor,
    scrollToBottomInstant,
    suspendAutoScrollRef,
  ]);

  const updateScrollState = useCallback(() => {
    const el = viewportRef.current;
    if (!el) return;
    const overflowing = el.scrollHeight > el.clientHeight + 1;
    setCanScrollUp(overflowing && !isNearTop(el));
    setCanScrollDown(
      overflowing && !isInBottomAnchorZone(el, BOTTOM_ANCHOR_THRESHOLD_PX),
    );
    notifyBottomAnchor(el);
  }, [notifyBottomAnchor]);

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
    pinnedToBottomRef.current =
      autoScrollTo === "bottom" && isInBottomAnchorZone(el, BOTTOM_ANCHOR_THRESHOLD_PX);
    prevAnchoredRef.current = null;
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

  const pinAndScrollToBottom = useCallback(() => {
    const el = viewportRef.current;
    if (!el) return;
    pinnedToBottomRef.current = true;
    scrollToBottomInstant(el);
    requestAnimationFrame(() => {
      const node = viewportRef.current;
      if (!node) return;
      scrollToBottomInstant(node);
      notifyBottomAnchor(node);
    });
  }, [notifyBottomAnchor, scrollToBottomInstant]);

  useEffect(() => {
    if (!autoScrollControlRef) return;
    autoScrollControlRef.current = {
      unpin: () => {
        pinnedToBottomRef.current = false;
      },
      pinAndScrollToBottom,
    };
    return () => {
      autoScrollControlRef.current = null;
    };
  }, [autoScrollControlRef, pinAndScrollToBottom]);

  useEffect(() => {
    const el = viewportRef.current;
    if (!el || overlayActive) return;
    updateScrollState();
    followBottomIfAnchored();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...autoScrollDeps, overlayActive, followBottomIfAnchored]);

  useEffect(() => {
    const el = viewportRef.current;
    if (!el) return;

    const onResize = () => {
      updateScrollState();
      followBottomIfAnchored();
    };

    const observer = new ResizeObserver(onResize);
    const observed = new Set<Element>();

    const observeEl = (node: Element) => {
      if (observed.has(node)) return;
      observer.observe(node);
      observed.add(node);
    };

    const observeScrollContent = () => {
      observeEl(el);
      for (const child of el.children) {
        observeEl(child);
      }
    };

    observeScrollContent();

    // Live tail / tool cards mount as later siblings — not firstElementChild.
    const mo = new MutationObserver((records) => {
      for (const record of records) {
        for (const node of record.addedNodes) {
          if (node instanceof Element) observeEl(node);
        }
      }
    });
    mo.observe(el, { childList: true });

    return () => {
      observer.disconnect();
      mo.disconnect();
    };
  }, [updateScrollState, followBottomIfAnchored]);

  useEffect(() => () => stopHoldScroll(), [stopHoldScroll]);

  function onScroll(e: UIEvent<HTMLDivElement>) {
    const el = e.currentTarget;
    if (autoScrollTo === "bottom") {
      pinnedToBottomRef.current = isInBottomAnchorZone(el, BOTTOM_ANCHOR_THRESHOLD_PX);
      notifyBottomAnchor(el);
    }
    onScrollPositionChange?.(el.scrollTop);
    const overflowing = el.scrollHeight > el.clientHeight + 1;
    setCanScrollUp(overflowing && !isNearTop(el));
    setCanScrollDown(
      overflowing && !isInBottomAnchorZone(el, BOTTOM_ANCHOR_THRESHOLD_PX),
    );
  }

  const showControls = canScrollUp || canScrollDown;

  return (
    <div className="scroll-viewport">
      <div
        ref={setViewportRef}
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
});

ScrollViewport.displayName = "ScrollViewport";
