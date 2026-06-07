export type TurnSlotKind = "active" | "archive";

export function turnSlotKey(
  kind: TurnSlotKind,
  turnNumber: number,
  epoch?: number,
): string {
  if (kind === "archive") {
    return `r:${epoch}:${turnNumber}`;
  }
  return `a:${turnNumber}`;
}

export function parseTurnSlotKey(key: string): {
  kind: TurnSlotKind;
  turnNumber: number;
  epoch?: number;
} | null {
  if (key.startsWith("a:")) {
    const turnNumber = Number(key.slice(2));
    if (Number.isNaN(turnNumber)) return null;
    return { kind: "active", turnNumber };
  }
  if (key.startsWith("r:")) {
    const parts = key.slice(2).split(":");
    if (parts.length !== 2) return null;
    const epoch = Number(parts[0]);
    const turnNumber = Number(parts[1]);
    if (Number.isNaN(epoch) || Number.isNaN(turnNumber)) return null;
    return { kind: "archive", epoch, turnNumber };
  }
  return null;
}
