import type { UIMessage } from "../types/messages";
import { createInitialMachine, dispatchTranscriptEvent } from "../transcript";
import { flatMessagesToMachine } from "../transcript/flatParse";
import type { TranscriptEvent, TranscriptMachine } from "../transcript/types";
import { SYNTHETIC_USER_ID } from "../transcript/types";

function syntheticUser(): UIMessage {
  return { id: SYNTHETIC_USER_ID, role: "user", contentBlocks: [] };
}

/** Empty fork overlay transcript (streaming shell). */
export function emptyForkMachine(): TranscriptMachine {
  return dispatchTranscriptEvent(createInitialMachine(), {
    type: "BEGIN_TURN",
    user: syntheticUser(),
  });
}

function forkEventNeedsTurn(event: TranscriptEvent): boolean {
  return (
    event.type === "STREAM_CHUNK" ||
    event.type === "TOOL" ||
    event.type === "SEGMENT_COMPLETE"
  );
}

/** Sub-agent overlay FSM: resume turn shell after hydrate-idle so live events apply. */
export function dispatchForkEvent(
  machine: TranscriptMachine,
  event: TranscriptEvent,
): TranscriptMachine {
  let next = machine;
  if (machine.phase === "idle" && forkEventNeedsTurn(event)) {
    next = dispatchTranscriptEvent(machine, {
      type: "BEGIN_TURN",
      user: syntheticUser(),
    });
  }
  return dispatchTranscriptEvent(next, event);
}

function resumeForkAfterHydrate(machine: TranscriptMachine): TranscriptMachine {
  const turn = machine.context.turns[machine.context.turns.length - 1];
  if (!turn) {
    return dispatchTranscriptEvent(machine, {
      type: "BEGIN_TURN",
      user: syntheticUser(),
    });
  }
  if (turn.segments.length > 0 || machine.context.openSegment) {
    return { phase: "segmentCommitted", context: machine.context };
  }
  return dispatchTranscriptEvent(machine, {
    type: "BEGIN_TURN",
    user: syntheticUser(),
  });
}

/** Load fork messages from DB for a completed or initial open. */
export function hydrateForkMachine(flatMessages: UIMessage[]): TranscriptMachine {
  const { machine } = flatMessagesToMachine(flatMessages);
  if (machine.phase === "idle" && flatMessages.length > 0) {
    return resumeForkAfterHydrate(machine);
  }
  return machine;
}

