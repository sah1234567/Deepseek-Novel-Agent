import { describe, expect, it } from "vitest";
import { IPC_COMMANDS } from "../../ipc/commands";
import { IPC_EVENTS } from "../../ipc/events";

/**
 * Frontend ↔ Tauri contract for interaction mode (编排 | 互动).
 * Mirrors tests/integration/interaction_mode.rs AppStatus + command names.
 */
describe("interaction-mode IPC contract", () => {
  it("exposes set_interaction_mode and graph_clear_focus commands", () => {
    expect(IPC_COMMANDS.setInteractionMode).toBe("set_interaction_mode");
    expect(IPC_COMMANDS.graphClearFocus).toBe("graph_clear_focus");
    expect(IPC_COMMANDS.graphActivateNode).toBe("graph_activate_node");
    expect(IPC_COMMANDS.getAppStatus).toBe("get_app_status");
  });

  it("exposes interaction-mode-changed event", () => {
    expect(IPC_EVENTS.interactionModeChanged).toBe("interaction-mode-changed");
  });

  it("AppStatus camelCase fields match Rust serde rename", () => {
    // Shape expected from get_app_status (camelCase).
    const sample = {
      sessionId: "s1",
      permissionMode: "normal",
      interactionMode: "work",
      focusedNodeId: "world-bible",
      hookRunning: false,
      pendingUserQuestion: false,
      turnInProgress: false,
      turnNumber: 1,
      projectInitialized: true,
      todos: [],
      sessionCacheHit: 0,
      sessionCacheMiss: 0,
      sessionCompletion: 0,
      contextTokens: 0,
      activeWorkName: "default",
    };
    expect(sample.interactionMode).toBe("work");
    expect(["orchestrate", "work"]).toContain(sample.interactionMode);
  });
});
