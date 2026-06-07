import type { UIMessage } from "../types/messages";
import { dispatchTranscriptEvent } from "./machine";
import type { TranscriptMachine } from "./types";
import type { UiTurnBundle } from "./service";
import type { ApiUiMessage } from "../utils/messages";

/** Test-only full hydrate via RESET + MERGE_TURNS (production uses turn-range loader). */
export function hydrateAllForTest(
  machine: TranscriptMachine,
  flatMessages: UIMessage[],
): TranscriptMachine {
  const bundles = flatMessagesToTestBundles(flatMessages);
  let m = dispatchTranscriptEvent(machine, { type: "RESET_TRANSCRIPT" });
  return dispatchTranscriptEvent(m, { type: "MERGE_TURNS", bundles });
}

function flatMessagesToTestBundles(flatMessages: UIMessage[]): UiTurnBundle[] {
  const bundles: UiTurnBundle[] = [];
  let chunk: ApiUiMessage[] = [];
  let turnIndex = 0;

  const flush = () => {
    if (chunk.length === 0) return;
    bundles.push({ turnNumber: turnIndex, messages: chunk });
    chunk = [];
    turnIndex += 1;
  };

  for (const msg of flatMessages) {
    if (msg.role === "user") {
      flush();
    }
    chunk.push({
      id: msg.id,
      role: msg.role,
      contentBlocks: msg.contentBlocks,
      toolName: msg.toolName,
      forkRunId: msg.forkRunId,
      messageKind: msg.messageKind,
    });
  }
  flush();
  return bundles;
}

export function hydrateBundlesForTest(
  machine: TranscriptMachine,
  bundles: UiTurnBundle[],
): TranscriptMachine {
  let m = dispatchTranscriptEvent(machine, { type: "RESET_TRANSCRIPT" });
  return dispatchTranscriptEvent(m, { type: "MERGE_TURNS", bundles });
}

export function bundlesFromUiMessages(flatMessages: UIMessage[]): UiTurnBundle[] {
  return flatMessagesToTestBundles(flatMessages);
}
