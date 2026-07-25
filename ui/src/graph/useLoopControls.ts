import { useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { IPC_COMMANDS } from "../ipc/commands";

/** Pause / resume Book Loop. Target/cursor/getLoop IPC are backend-ready via `IPC_COMMANDS` but not yet exposed in this hook. */
export function useLoopControls(onMutate?: () => Promise<unknown>) {
  const after = useCallback(async () => {
    if (onMutate) await onMutate();
  }, [onMutate]);

  const pause = useCallback(
    async (loopId: string, reason = "user") => {
      await invoke(IPC_COMMANDS.graphLoopPause, { loopId, reason });
      await after();
    },
    [after],
  );

  const resume = useCallback(
    async (loopId: string) => {
      await invoke(IPC_COMMANDS.graphLoopResume, { loopId });
      await after();
    },
    [after],
  );

  return { pause, resume };
}
