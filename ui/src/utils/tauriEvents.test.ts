import { beforeEach, describe, expect, it, vi } from "vitest";
import { mountTauriListeners } from "./tauriEvents";

const listen = vi.fn();

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listen(...args),
}));

describe("mountTauriListeners", () => {
  beforeEach(() => {
    listen.mockReset();
    listen.mockResolvedValue(() => {});
  });

  it("unsubscribes every mounted listener on cleanup", async () => {
    const unlistenA = vi.fn();
    const unlistenB = vi.fn();
    listen.mockResolvedValueOnce(unlistenA).mockResolvedValueOnce(unlistenB);

    const cleanup = mountTauriListeners([
      () => listen("event-a", vi.fn()),
      () => listen("event-b", vi.fn()),
    ]);

    await vi.waitFor(() => expect(listen).toHaveBeenCalledTimes(2));
    cleanup();
    await vi.waitFor(() => {
      expect(unlistenA).toHaveBeenCalledTimes(1);
      expect(unlistenB).toHaveBeenCalledTimes(1);
    });
  });
});
