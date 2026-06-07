import type { UIMessage } from "../types/messages";
import { apiMessagesToUi } from "../utils/messages";
import type { UiTurnBundle } from "./service";
import { flatMessagesToMachine } from "./flatParse";
import type { TranscriptMachine, Turn } from "./types";

function isLiveOrphanTurnLocal(turn: Turn): boolean {
  return turn.archiveEpoch === undefined && turn.turnNumber === undefined;
}

/** Timeline order: archive epoch, then turn number; live orphans always last. */
export function compareTurnTimelineOrder(a: Turn, b: Turn): number {
  const ae = a.archiveEpoch ?? -1;
  const be = b.archiveEpoch ?? -1;
  if (ae !== be) return ae - be;
  const aOrphan = isLiveOrphanTurnLocal(a);
  const bOrphan = isLiveOrphanTurnLocal(b);
  if (aOrphan !== bOrphan) return aOrphan ? 1 : -1;
  return (a.turnNumber ?? 0) - (b.turnNumber ?? 0);
}

/** One `turn_number` row group must load atomically (user + ReAct tool chain). */
/** Re-merge with the same `turnNumber` overwrites the prior turn (tail reload / idempotent fetch). */
function turnIdentity(turn: Turn): string {
  if (turn.archiveEpoch !== undefined && turn.turnNumber !== undefined) {
    return `r:${turn.archiveEpoch}:${turn.turnNumber}`;
  }
  if (turn.turnNumber !== undefined) {
    return `a:${turn.turnNumber}`;
  }
  return `live:${turn.turnId}`;
}

export function flatMessagesToTurn(
  messages: UIMessage[],
  turnNumber: number,
  archiveEpoch?: number,
): Turn | null {
  if (messages.length === 0) return null;
  const { machine } = flatMessagesToMachine(messages);
  const turn = machine.context.turns[0];
  if (!turn) return null;
  return { ...turn, turnNumber, archiveEpoch };
}

export function mergeTurnsIntoMachine(
  machine: TranscriptMachine,
  bundles: UiTurnBundle[],
  archiveEpoch?: number,
): TranscriptMachine {
  if (bundles.length === 0) return machine;

  let turns = [...machine.context.turns];
  for (const bundle of bundles) {
    const uiMessages = apiMessagesToUi(bundle.messages);
    const newTurn = flatMessagesToTurn(uiMessages, bundle.turnNumber, archiveEpoch);
    if (!newTurn) continue;

    const id = turnIdentity(newTurn);
    const idx = turns.findIndex((t) => turnIdentity(t) === id);
    if (idx >= 0) {
      turns[idx] = newTurn;
    } else {
      turns.push(newTurn);
    }
  }

  turns.sort(compareTurnTimelineOrder);
  return {
    ...machine,
    context: {
      ...machine.context,
      turns,
    },
  };
}

export function evictTurnsFromMachine(
  machine: TranscriptMachine,
  targets: { turnNumber: number; archiveEpoch?: number }[],
): TranscriptMachine {
  if (targets.length === 0) return machine;
  const evict = new Set(
    targets.map((t) =>
      t.archiveEpoch !== undefined
        ? `r:${t.archiveEpoch}:${t.turnNumber}`
        : `a:${t.turnNumber}`,
    ),
  );
  return {
    ...machine,
    context: {
      ...machine.context,
      turns: machine.context.turns.filter((t) => !evict.has(turnIdentity(t))),
    },
  };
}

export function findTurnInMachine(
  machine: TranscriptMachine,
  turnNumber: number,
  archiveEpoch?: number,
): Turn | undefined {
  const id =
    archiveEpoch !== undefined
      ? `r:${archiveEpoch}:${turnNumber}`
      : `a:${turnNumber}`;
  return machine.context.turns.find((t) => turnIdentity(t) === id);
}
