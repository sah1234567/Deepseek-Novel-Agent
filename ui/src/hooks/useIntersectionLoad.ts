import { useEffect, type RefObject } from "react";
import { INTERSECTION_ROOT_MARGIN } from "../transcript/loadPolicy";

export function useIntersectionLoad(opts: {
  enabled: boolean;
  targetRef: RefObject<Element | null>;
  scrollRootRef?: RefObject<Element | null>;
  onIntersect: () => void;
}): void {
  const { enabled, targetRef, scrollRootRef, onIntersect } = opts;

  useEffect(() => {
    if (!enabled) return;
    const target = targetRef.current;
    if (!target) return;
    if (typeof IntersectionObserver === "undefined") return;

    const root = scrollRootRef?.current ?? null;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          onIntersect();
        }
      },
      { root, rootMargin: INTERSECTION_ROOT_MARGIN },
    );
    observer.observe(target);
    return () => observer.disconnect();
  }, [enabled, targetRef, scrollRootRef, onIntersect]);
}
