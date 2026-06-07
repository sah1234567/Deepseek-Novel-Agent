/** Coalesce high-frequency updates to one commit per animation frame. */
export function createRafBatcher<T>(flush: (items: T[]) => void) {
  const queue: T[] = [];
  let rafId: number | null = null;

  const runFlush = () => {
    rafId = null;
    if (queue.length === 0) return;
    const batch = queue.splice(0, queue.length);
    flush(batch);
  };

  return {
    push(item: T) {
      queue.push(item);
      if (rafId === null) {
        rafId = requestAnimationFrame(runFlush);
      }
    },
    flushNow() {
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
      runFlush();
    },
  };
}
