import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { IPC_COMMANDS } from "../ipc/commands";
import { IPC_EVENTS } from "../ipc/events";
import type { GraphNodeView, GraphStateSnapshot, LoopHistoryRow } from "./types";

/** Graph snapshot, panel, and node-session header (Chat stays primary). */
export function useGraphState() {
  const [snapshot, setSnapshot] = useState<GraphStateSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** When set, node session header is shown above Chat. */
  const [sessionNodeId, setSessionNodeId] = useState<string | null>(null);
  const [graphPanelOpen, setGraphPanelOpen] = useState(false);
  const [hitlModalOpen, setHitlModalOpen] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const snap = await invoke<GraphStateSnapshot>(IPC_COMMANDS.graphGetState);
      setSnapshot(snap);
      setError(null);
      return snap;
    } catch (e) {
      setError(String(e));
      return null;
    }
  }, []);

  const openHitlIfNeeded = useCallback((snap: GraphStateSnapshot | null) => {
    if ((snap?.hitl?.length ?? 0) > 0) {
      setHitlModalOpen(true);
    }
  }, []);

  useEffect(() => {
    void refresh().then(openHitlIfNeeded);
    const unsubs: Array<() => void> = [];
    const on = (event: string, handler: () => void) => {
      void listen(event, handler).then((u) => unsubs.push(u));
    };
    on(IPC_EVENTS.graphStateChanged, () => {
      void refresh().then(openHitlIfNeeded);
    });
    on(IPC_EVENTS.graphLoopChanged, () => {
      void refresh();
    });
    on(IPC_EVENTS.graphHitl, () => {
      void refresh();
      setHitlModalOpen(true);
    });
    on(IPC_EVENTS.graphApprovalRequired, () => {
      void refresh();
      setHitlModalOpen(true);
    });
    on(IPC_EVENTS.graphPlanCommitted, () => {
      void refresh();
      setGraphPanelOpen(true);
      setHitlModalOpen(false);
    });
    on(IPC_EVENTS.nodeSessionReset, () => {
      setSessionNodeId(null);
      void refresh();
    });
    return () => {
      for (const u of unsubs) u();
    };
  }, [refresh, openHitlIfNeeded]);

  useEffect(() => {
    if (graphPanelOpen) {
      setHitlModalOpen(false);
    }
  }, [graphPanelOpen]);

  const openGraph = useCallback(() => {
    setGraphPanelOpen(true);
    setHitlModalOpen(false);
    void refresh();
  }, [refresh]);

  const closeGraph = useCallback(() => {
    setGraphPanelOpen(false);
  }, []);

  const enterNodeSession = useCallback(
    async (nodeId: string) => {
      setSessionNodeId(nodeId);
      setGraphPanelOpen(false);
      await invoke(IPC_COMMANDS.graphActivateNode, { nodeId });
      await refresh();
    },
    [refresh],
  );

  const backToChat = useCallback(async () => {
    setSessionNodeId(null);
    try {
      await invoke(IPC_COMMANDS.graphClearFocus);
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }, [refresh]);

  const start = useCallback(
    async (nodeId: string) => {
      setSessionNodeId(nodeId);
      setGraphPanelOpen(false);
      await invoke(IPC_COMMANDS.graphStartNode, { nodeId });
      await refresh();
    },
    [refresh],
  );

  const approve = useCallback(
    async (nodeId: string) => {
      await invoke(IPC_COMMANDS.graphApprove, { nodeId });
      await refresh();
    },
    [refresh],
  );

  const reject = useCallback(
    async (nodeId: string, note: string) => {
      await invoke(IPC_COMMANDS.graphReject, { nodeId, note });
      await refresh();
    },
    [refresh],
  );

  const reopen = useCallback(
    async (nodeId: string) => {
      await invoke(IPC_COMMANDS.graphReopen, { nodeId, cascadeDownstream: true });
      await refresh();
    },
    [refresh],
  );

  const listLoopHistory = useCallback(async (loopId: string, limit = 50) => {
    return invoke<LoopHistoryRow[]>(IPC_COMMANDS.graphLoopListHistory, { loopId, limit });
  }, []);

  const dismissHitlModal = useCallback(() => {
    setHitlModalOpen(false);
  }, []);

  const sessionNode: GraphNodeView | null =
    snapshot && sessionNodeId
      ? (snapshot.nodes.find((n) => n.id === sessionNodeId) ?? null)
      : null;

  const showHitlModal =
    hitlModalOpen && (snapshot?.hitl?.length ?? 0) > 0 && !graphPanelOpen;

  return {
    snapshot,
    error,
    refresh,
    sessionNode,
    enterNodeSession,
    backToChat,
    start,
    approve,
    reject,
    reopen,
    listLoopHistory,
    graphPanelOpen,
    openGraph,
    closeGraph,
    showHitlModal,
    dismissHitlModal,
  };
}

export type GraphStateApi = ReturnType<typeof useGraphState>;
