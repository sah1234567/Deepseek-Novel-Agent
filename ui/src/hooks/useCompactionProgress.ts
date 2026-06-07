import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { emitCompactionDone } from "../ipc/compactionDone";
import { IPC_EVENTS } from "../ipc/events";
import { mountTauriListeners } from "../utils/tauriEvents";
import {
  compactionProgressVariant,
  type CompactionAction,
  type CompactionProgressPayload,
} from "../utils/compactionMessages";

export interface CompactionBannerState {
  visible: boolean;
  action: CompactionAction;
  attempt?: number;
  tokensBefore?: number;
  tokensAfter?: number;
  reason?: string;
  variant: "info" | "success" | "warn";
}

const HIDDEN: CompactionBannerState = {
  visible: false,
  action: "started",
  variant: "info",
};

export function useCompactionProgress(): CompactionBannerState {
  const [state, setState] = useState<CompactionBannerState>(HIDDEN);

  useEffect(() => {
    return mountTauriListeners([
      () =>
        listen<CompactionProgressPayload>(IPC_EVENTS.compactionProgress, (event) => {
          const p = event.payload;
          const variant = compactionProgressVariant(p.action);
          setState({
            visible: true,
            action: p.action,
            attempt: p.attempt,
            tokensBefore: p.tokensBefore,
            tokensAfter: p.tokensAfter,
            reason: p.reason,
            variant,
          });
          if (p.action === "done") {
            emitCompactionDone();
            window.setTimeout(() => setState(HIDDEN), 3000);
          } else if (p.action === "failed") {
            window.setTimeout(() => setState(HIDDEN), 5000);
          }
        }),
    ]);
  }, []);

  return state;
}
