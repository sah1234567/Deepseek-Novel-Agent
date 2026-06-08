import type { ContentBlock, ForkRunState, UIMessage } from "../types/messages";
import { emptyForkMachine, hydrateForkMachine } from "./transcript";

/** Ensure `forkRuns` contains a shell entry when the overlay opens. */
export function mergeForkRunOnOpen(
  forkRuns: Map<string, ForkRunState>,
  forkRunId: string,
): Map<string, ForkRunState> {
  const existing = forkRuns.get(forkRunId);
  const next = new Map(forkRuns);
  next.set(forkRunId, {
    forkRunId,
    agentType: existing?.agentType ?? "加载中…",
    taskPreview: existing?.taskPreview ?? "",
    source: existing?.source ?? "tool",
    parentToolCallId: existing?.parentToolCallId,
    machine: existing?.machine ?? emptyForkMachine(),
    status: existing?.status ?? "complete",
    reportOutput: existing?.reportOutput,
  });
  return next;
}

/** Apply persisted fork transcript (including while run is still `running`). */
export function applyForkDbSnapshot(
  run: ForkRunState,
  dbFlatMessages: UIMessage[],
): ForkRunState {
  if (dbFlatMessages.length === 0) {
    return run;
  }
  return { ...run, machine: hydrateForkMachine(dbFlatMessages) };
}

export function applyForkDbToMap(
  forkRuns: Map<string, ForkRunState>,
  forkRunId: string,
  dbFlatMessages: UIMessage[],
): Map<string, ForkRunState> {
  const run = forkRuns.get(forkRunId);
  if (!run) return forkRuns;
  const next = new Map(forkRuns);
  next.set(forkRunId, applyForkDbSnapshot(run, dbFlatMessages));
  return next;
}

export type { ContentBlock };
