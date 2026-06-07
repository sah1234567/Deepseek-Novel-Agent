import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke,
}));

import {
  fetchActiveTurns,
  fetchArchiveTurns,
  fetchTranscriptLayout,
} from "./service";

describe("transcript service IPC", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue([]);
  });

  it("fetchTranscriptLayout passes sessionId camelCase", async () => {
    invoke.mockResolvedValue({
      hasContextRefresh: false,
      active: { minTurn: 1, maxTurn: 1 },
      archives: [],
    });
    await fetchTranscriptLayout("sid-1");
    expect(invoke).toHaveBeenCalledWith("get_session_transcript_layout", {
      sessionId: "sid-1",
    });
  });

  it("fetchActiveTurns passes fromTurn and toTurn", async () => {
    await fetchActiveTurns(null, 2, 5);
    expect(invoke).toHaveBeenCalledWith("get_session_message_turns", {
      sessionId: null,
      fromTurn: 2,
      toTurn: 5,
    });
  });

  it("fetchArchiveTurns passes epoch and turn range", async () => {
    await fetchArchiveTurns("sid", 3, 1, 4);
    expect(invoke).toHaveBeenCalledWith("get_session_archive_turns", {
      sessionId: "sid",
      epoch: 3,
      fromTurn: 1,
      toTurn: 4,
    });
  });
});
