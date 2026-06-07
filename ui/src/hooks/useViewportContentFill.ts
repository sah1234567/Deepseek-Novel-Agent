import { useEffect, useRef, type MutableRefObject, type RefObject } from "react";
import { TAIL_CONTENT_UNDERFLOW_PX } from "../transcript/loadPolicy";

/**
 * Measures whether scroll content underfills the viewport while bottom-anchored.
 * Loader reads contentUnderflowRef only — no DOM access in reconcile logic.
 */
export function useViewportContentFill(
  scrollRootRef: RefObject<HTMLElement | null>,
  isBottomAnchoredRef: RefObject<boolean>,
  onUnderflowChange?: () => void,
): MutableRefObject<boolean> {
  const contentUnderflowRef = useRef(false);

  useEffect(() => {
    const el = scrollRootRef.current;
    if (!el) return;

    const measure = () => {
      const underflow =
        !!isBottomAnchoredRef.current &&
        el.scrollHeight < el.clientHeight + TAIL_CONTENT_UNDERFLOW_PX;
      if (underflow !== contentUnderflowRef.current) {
        contentUnderflowRef.current = underflow;
        onUnderflowChange?.();
      }
    };

    const observer = new ResizeObserver(measure);
    observer.observe(el);
    const contentRoot = el.firstElementChild;
    if (contentRoot) {
      observer.observe(contentRoot);
    }
    measure();
    return () => observer.disconnect();
  }, [scrollRootRef, isBottomAnchoredRef, onUnderflowChange]);

  return contentUnderflowRef;
}
