import { useEffect, useState } from "react";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
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
    const unlisteners: Promise<UnlistenFn>[] = [];
    unlisteners.push(
      listen<CompactionProgressPayload>("compaction-progress", (event) => {
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
          window.setTimeout(() => setState(HIDDEN), 3000);
        } else if (p.action === "failed") {
          window.setTimeout(() => setState(HIDDEN), 5000);
        }
      }),
    );
    return () => {
      void Promise.all(unlisteners).then((fns) => fns.forEach((fn) => fn()));
    };
  }, []);

  return state;
}
