import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

export interface SessionTodo {
  id: string;
  content: string;
  status: string;
}

export interface SessionSummary {
  id: string;
  title: string | null;
  status: string;
  last_active_at: string;
  total_turns: number;
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
  turnNumber: number;
  projectInitialized: boolean;
  hasInterruptibleToolInProgress?: boolean;
  todos: SessionTodo[];
  sessionCacheHit: number;
  sessionCacheMiss: number;
  sessionCompletion: number;
  sessionTotalTokens: number;
  projectRoot: string;
  activeWorkName: string;
}

export function useAppStatus() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const s = await invoke<AppStatus>("get_app_status");
      setStatus(s);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const interval = setInterval(() => void refresh(), 5000);
    return () => clearInterval(interval);
  }, [refresh]);

  useEffect(() => {
    const unlisteners: Promise<UnlistenFn>[] = [];
    unlisteners.push(
      listen("turn-complete", () => {
        void refresh();
      }),
    );
    unlisteners.push(
      listen("session-resumed", () => {
        void refresh();
      }),
    );
    unlisteners.push(
      listen("permission-mode-changed", () => {
        void refresh();
      }),
    );
    return () => {
      void Promise.all(unlisteners).then((fns) => fns.forEach((fn) => fn()));
    };
  }, [refresh]);

  const initProject = useCallback(async () => {
    await invoke("init_novel_project");
    await refresh();
  }, [refresh]);

  const setPermissionMode = useCallback(
    async (mode: string) => {
      await invoke("set_permission_mode", { mode });
      await refresh();
    },
    [refresh],
  );

  const resumeSession = useCallback(
    async (sessionId: string) => {
      await invoke("resume_session", { session_id: sessionId });
      await refresh();
    },
    [refresh],
  );

  const listSessions = useCallback(async () => {
    return invoke<SessionSummary[]>("list_sessions");
  }, []);

  const listWorks = useCallback(async () => {
    return invoke<WorkSummary[]>("list_works");
  }, []);

  const createSession = useCallback(async () => {
    await invoke("create_session");
    await refresh();
  }, [refresh]);

  const createWork = useCallback(
    async (name: string) => {
      await invoke("create_work", { name });
      await refresh();
    },
    [refresh],
  );

  const openWork = useCallback(
    async (name: string) => {
      await invoke("open_work", { name });
      await refresh();
    },
    [refresh],
  );

  const getApiConfig = useCallback(async () => {
    return invoke<{ api_key: string; api_base: string }>("get_api_config");
  }, []);

  const setApiConfig = useCallback(async (apiKey: string, apiBase: string) => {
    await invoke("set_api_config", { apiKey, apiBase });
    await refresh();
  }, [refresh]);

  const updateSessionTodo = useCallback(
    async (todoId: string, status: string) => {
      await invoke("update_session_todo", { todo_id: todoId, status });
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
