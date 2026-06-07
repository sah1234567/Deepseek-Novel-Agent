import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

import { useAppStatus } from "../../hooks/useAppStatus";

describe("useAppStatus setPermissionMode", () => {
  beforeEach(() => {
    invokeMock.mockReset();
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
