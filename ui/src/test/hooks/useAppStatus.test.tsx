import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { IPC_EVENTS } from "../../ipc/events";

const invokeMock = vi.fn();
const listenMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}));

import { useAppStatus } from "../../hooks/useAppStatus";

const baseAppStatus = {
  sessionId: "s1",
  permissionMode: "normal",
  hookRunning: false,
  pendingUserQuestion: false,
  turnInProgress: false,
  turnNumber: 1,
  projectInitialized: true,
  todos: [],
  sessionCacheHit: 10,
  sessionCacheMiss: 20,
  sessionCompletion: 5,
  contextTokens: 35,
  activeWorkName: "default",
};

function installListenHandlers() {
  const handlers = new Map<string, (event: { payload: unknown }) => void>();
  listenMock.mockImplementation(
    async (eventName: string, handler: (event: { payload: unknown }) => void) => {
      handlers.set(eventName, handler);
      return () => {
        handlers.delete(eventName);
      };
    },
  );
  return handlers;
}

describe("useAppStatus setPermissionMode", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    listenMock.mockReset();
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "get_app_status") {
        return {
          projectInitialized: true,
          permissionMode: "normal",
          turnInProgress: false,
          pendingUserQuestion: false,
          turnNumber: 0,
        };
      }
      return undefined;
    });
    listenMock.mockResolvedValue(() => {});
  });

  it("surfaces invoke errors without refreshing on failure", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "set_permission_mode") {
        throw new Error("当前轮次进行中，无法切换权限模式");
      }
      if (cmd === "get_app_status") {
        return {
          projectInitialized: true,
          permissionMode: "normal",
          turnInProgress: true,
          pendingUserQuestion: false,
          turnNumber: 1,
        };
      }
      return undefined;
    });

    const { result } = renderHook(() => useAppStatus());
    await waitFor(() => expect(result.current.status).not.toBeNull());

    const statusCallsBefore = invokeMock.mock.calls.filter(
      ([cmd]) => cmd === "get_app_status",
    ).length;

    let thrown: unknown;
    await act(async () => {
      try {
        await result.current.setPermissionMode("auto");
      } catch (e) {
        thrown = e;
      }
    });
    expect(String(thrown)).toContain("当前轮次进行中");
    await waitFor(() =>
      expect(result.current.error).toContain("当前轮次进行中"),
    );
    const statusCallsAfter = invokeMock.mock.calls.filter(
      ([cmd]) => cmd === "get_app_status",
    ).length;
    expect(statusCallsAfter).toBe(statusCallsBefore);
  });
});

describe("useAppStatus session-tokens-updated", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    listenMock.mockReset();
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "get_app_status") {
        return { ...baseAppStatus };
      }
      return undefined;
    });
  });

  it("patches token fields from event without get_app_status", async () => {
    const handlers = installListenHandlers();
    const { result } = renderHook(() => useAppStatus());
    await waitFor(() => expect(result.current.status?.sessionCacheHit).toBe(10));

    const statusCallsBefore = invokeMock.mock.calls.filter(
      ([cmd]) => cmd === "get_app_status",
    ).length;

    const onTokens = handlers.get(IPC_EVENTS.sessionTokensUpdated);
    expect(onTokens).toBeDefined();

    await act(async () => {
      onTokens?.({
        payload: {
          cacheHitTokens: 100,
          cacheMissTokens: 200,
          completionTokens: 50,
          contextTokens: 350,
        },
      });
    });

    expect(result.current.status?.sessionCacheHit).toBe(100);
    expect(result.current.status?.sessionCacheMiss).toBe(200);
    expect(result.current.status?.sessionCompletion).toBe(50);
    expect(result.current.status?.contextTokens).toBe(350);

    const statusCallsAfter = invokeMock.mock.calls.filter(
      ([cmd]) => cmd === "get_app_status",
    ).length;
    expect(statusCallsAfter).toBe(statusCallsBefore);
  });

  it("subagent-style payload updates billing while context stays unchanged", async () => {
    const handlers = installListenHandlers();
    const { result } = renderHook(() => useAppStatus());
    await waitFor(() => expect(result.current.status?.contextTokens).toBe(35));

    const onTokens = handlers.get(IPC_EVENTS.sessionTokensUpdated);
    await act(async () => {
      onTokens?.({
        payload: {
          cacheHitTokens: 15,
          cacheMissTokens: 25,
          completionTokens: 8,
          contextTokens: 35,
        },
      });
    });

    expect(result.current.status?.sessionCacheHit).toBe(15);
    expect(result.current.status?.sessionCacheMiss).toBe(25);
    expect(result.current.status?.sessionCompletion).toBe(8);
    expect(result.current.status?.contextTokens).toBe(35);
  });

  it("patches todos from session-todos-updated without get_app_status", async () => {
    const handlers = installListenHandlers();
    const { result } = renderHook(() => useAppStatus());
    await waitFor(() => expect(result.current.status?.todos).toEqual([]));

    const statusCallsBefore = invokeMock.mock.calls.filter(
      ([cmd]) => cmd === "get_app_status",
    ).length;

    const onTodos = handlers.get(IPC_EVENTS.sessionTodosUpdated);
    expect(onTodos).toBeDefined();

    await act(async () => {
      onTodos?.({
        payload: {
          todos: [
            { id: "t1", content: "outline", status: "in_progress" },
          ],
        },
      });
    });

    expect(result.current.status?.todos).toEqual([
      { id: "t1", content: "outline", status: "in_progress" },
    ]);

    const statusCallsAfter = invokeMock.mock.calls.filter(
      ([cmd]) => cmd === "get_app_status",
    ).length;
    expect(statusCallsAfter).toBe(statusCallsBefore);
  });
});
