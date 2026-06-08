import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { IPC_COMMANDS } from "../ipc/commands";
import { IPC_EVENTS } from "../ipc/events";
import { mountTauriListeners } from "../utils/tauriEvents";

export interface SessionTodo {
  id: string;
  content: string;
  status: string;
}

export interface SessionSummary {
  id: string;
  title: string | null;
  status: string;
  model: string;
  last_active_at: string;
  created_at: string;
  /** User dialogue rounds (one per user message). */
  total_turns: number;
  /** LLM API call count. */
  api_call_count: number;
}

export interface WorkSummary {
  name: string;
  path: string;
  initialized: boolean;
}

export interface AppStatus {
  sessionId: string;
  permissionMode: string;
  hookRunning: boolean;
  pendingUserQuestion: boolean;
  turnInProgress: boolean;
  turnNumber: number;
  projectInitialized: boolean;
  hasInterruptibleToolInProgress?: boolean;
  todos: SessionTodo[];
  sessionCacheHit: number;
  sessionCacheMiss: number;
  sessionCompletion: number;
  contextTokens: number;
  activeWorkName: string;
}

export function useAppStatus() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async (prefetched?: AppStatus) => {
    try {
      const s = prefetched ?? (await invoke<AppStatus>(IPC_COMMANDS.getAppStatus));
      setStatus(s);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  // Initial load + 30s fallback for non-token fields (todos, turn flags). Runtime token
  // counters are driven by `session-tokens-updated` (main and subagent LLM calls).
  useEffect(() => {
    void refresh();
    const interval = setInterval(() => void refresh(), 30000);
    return () => clearInterval(interval);
  }, [refresh]);

  useEffect(() => {
    return mountTauriListeners([
      () =>
        listen<{
          cacheHitTokens: number;
          cacheMissTokens: number;
          completionTokens: number;
          contextTokens: number;
        }>(IPC_EVENTS.sessionTokensUpdated, (event) => {
          const p = event.payload;
          setStatus((prev) =>
            prev
              ? {
                  ...prev,
                  sessionCacheHit: p.cacheHitTokens,
                  sessionCacheMiss: p.cacheMissTokens,
                  sessionCompletion: p.completionTokens,
                  contextTokens: p.contextTokens,
                }
              : prev,
          );
        }),
      // Tool result refresh: todos and turn flags — not the primary token update path.
      () =>
        listen<{ phase?: string }>(IPC_EVENTS.toolCallRequest, (event) => {
          if (event.payload.phase === "result") {
            void refresh();
          }
        }),
      // Turn-end refresh (`AgentProvider.onTurnComplete`): turnNumber, todos, etc.
      // Token fields stay event-driven via `session-tokens-updated`.
      // Session switches refresh via invoke callers (resumeSession / createSession / openWork).
      () => listen(IPC_EVENTS.permissionModeChanged, () => void refresh()),
    ]);
  }, [refresh]);

  const initProject = useCallback(async () => {
    await invoke(IPC_COMMANDS.initNovelProject);
    await refresh();
  }, [refresh]);

  const setPermissionMode = useCallback(
    async (mode: string) => {
      try {
        await invoke(IPC_COMMANDS.setPermissionMode, { mode });
        await refresh();
        setError(null);
      } catch (e) {
        setError(String(e));
        throw e;
      }
    },
    [refresh],
  );

  const resumeSession = useCallback(
    async (sessionId: string) => {
      try {
        await invoke(IPC_COMMANDS.resumeSession, { sessionId });
        await refresh();
        setError(null);
      } catch (e) {
        setError(String(e));
        throw e;
      }
    },
    [refresh],
  );

  const listSessions = useCallback(async () => {
    return invoke<SessionSummary[]>(IPC_COMMANDS.listSessions);
  }, []);

  const listWorks = useCallback(async () => {
    return invoke<WorkSummary[]>(IPC_COMMANDS.listWorks);
  }, []);

  const createSession = useCallback(async () => {
    await invoke(IPC_COMMANDS.createSession);
    await refresh();
  }, [refresh]);

  const createWork = useCallback(
    async (name: string) => {
      await invoke(IPC_COMMANDS.createWork, { name });
      await refresh();
    },
    [refresh],
  );

  const openWork = useCallback(
    async (name: string) => {
      await invoke(IPC_COMMANDS.openWork, { name });
      await refresh();
    },
    [refresh],
  );

  const getApiConfig = useCallback(async () => {
    return invoke<{ api_key: string; api_base: string }>(IPC_COMMANDS.getApiConfig);
  }, []);

  const setApiConfig = useCallback(async (apiKey: string, apiBase: string) => {
    await invoke(IPC_COMMANDS.setApiConfig, { apiKey, apiBase });
    await refresh();
  }, [refresh]);

  const updateSessionTodo = useCallback(
    async (todoId: string, status: string) => {
      await invoke(IPC_COMMANDS.updateSessionTodo, { todoId, status });
      await refresh();
    },
    [refresh],
  );

  return {
    status,
    error,
    refresh,
    initProject,
    setPermissionMode,
    resumeSession,
    createSession,
    createWork,
    openWork,
    listWorks,
    listSessions,
    getApiConfig,
    setApiConfig,
    updateSessionTodo,
  };
}
