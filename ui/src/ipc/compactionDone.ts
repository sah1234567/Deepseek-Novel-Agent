type CompactionDoneListener = () => void;

const listeners = new Set<CompactionDoneListener>();

/** Subscribe to compaction `action === "done"` (single listener hub for banner + transcript reload). */
export function onCompactionDone(listener: CompactionDoneListener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function emitCompactionDone(): void {
  for (const listener of listeners) {
    listener();
  }
}
