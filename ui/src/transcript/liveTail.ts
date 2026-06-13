import { findTurnInMachine } from "./merge";
import type { SessionTranscriptLayout } from "./service";
import type { TranscriptMachine, TranscriptPhase, Turn } from "./types";

export function isLiveOrphanTurn(turn: Turn): boolean {
  return turn.archiveEpoch === undefined && turn.turnNumber === undefined;
}

/** Remove optimistic live turns before a new BEGIN_TURN (after reload hydrated DB). */
export function dropLiveOrphanTurns(turns: Turn[]): Turn[] {
  return turns.filter((t) => !isLiveOrphanTurn(t));
}

/** Latest in-flight turn (optimistic `BEGIN_TURN`); never the first orphan in the list. */
export function findLiveTailTurn(machine: TranscriptMachine): Turn | undefined {
  const active = machine.context.turns.filter((t) => t.archiveEpoch === undefined);
  for (let i = active.length - 1; i >= 0; i--) {
    if (isLiveOrphanTurn(active[i])) {
      return active[i];
    }
  }
  if (machine.phase !== "idle" && active.length > 0) {
    return active[active.length - 1];
  }
  return undefined;
}

/**
 * Drop superseded optimistic turns after DB merge / turn end.
 * - Idle + persisted active turns: remove all live orphans (hydrated from `MERGE_TURNS`).
 * - Streaming: keep at most one live orphan (the tail).
 */
export function reconcileOrphanLiveTurns(
  turns: Turn[],
  phase: TranscriptPhase,
): Turn[] {
  const live = turns.filter(isLiveOrphanTurn);
  if (live.length === 0) return turns;

  const maxActiveTurn = Math.max(
    0,
    ...turns
      .filter((t) => t.archiveEpoch === undefined && t.turnNumber !== undefined)
      .map((t) => t.turnNumber!),
  );

  if (phase === "idle" && maxActiveTurn > 0) {
    return turns.filter((t) => !isLiveOrphanTurn(t));
  }

  if (live.length <= 1) return turns;

  const keepId = live[live.length - 1].turnId;
  return turns.filter((t) => !isLiveOrphanTurn(t) || t.turnId === keepId);
}

/**
 * Skip rendering the max-turn slot only when it would duplicate the same live tail turn.
 * Never hide a completed max-turn slot when a newer live turn is streaming.
 */
export function shouldSkipMaxTurnSlot(
  machine: TranscriptMachine,
  layout: SessionTranscriptLayout | null | undefined,
  liveTail: Turn | undefined,
  useLiveTail: boolean,
): boolean {
  if (!useLiveTail || !liveTail || !layout) return false;
  const maxTurn = layout.active.maxTurn;
  if (maxTurn < 1) return false;
  const loaded = findTurnInMachine(machine, maxTurn);
  if (!loaded) return false;
  return isLiveOrphanTurn(liveTail) && liveTail.turnId === loaded.turnId;
}
