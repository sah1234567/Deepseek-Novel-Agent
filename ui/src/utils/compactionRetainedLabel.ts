/** Human-readable retained turn range for compaction divider / refresh hints. */
export function formatRetainedTurnLabel(
  retainedMinTurn?: number,
  retainedMaxTurn?: number,
): string | null {
  if (retainedMinTurn === undefined || retainedMaxTurn === undefined) {
    return null;
  }
  if (retainedMinTurn === retainedMaxTurn) {
    return `保留 turn ${retainedMinTurn}`;
  }
  return `保留 turn ${retainedMinTurn}–${retainedMaxTurn}`;
}
