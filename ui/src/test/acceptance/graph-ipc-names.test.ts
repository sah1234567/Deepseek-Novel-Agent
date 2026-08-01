import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { IPC_COMMANDS } from "../../ipc/commands";
import { IPC_EVENTS } from "../../ipc/events";

const fixtureDir = resolve(__dirname, "../../../../tests/fixtures/graph");

describe("graph IPC contract", () => {
  it("exposes graph_* commands", () => {
    expect(IPC_COMMANDS.graphGetState).toBe("graph_get_state");
    expect(IPC_COMMANDS.graphApprove).toBe("graph_approve");
    expect(IPC_COMMANDS.graphLoopPause).toBe("graph_loop_pause");
    expect(IPC_COMMANDS.graphLoopListHistory).toBe("graph_loop_list_history");
  });

  it("exposes graph events", () => {
    expect(IPC_EVENTS.graphStateChanged).toBe("graph-state-changed");
    expect(IPC_EVENTS.graphLoopChanged).toBe("graph-loop-changed");
    expect(IPC_EVENTS.graphHitl).toBe("graph-hitl");
    expect(IPC_EVENTS.graphPlanCommitted).toBe("graph-plan-committed");
  });

  it("plan fixture has book-body loop", () => {
    const raw = readFileSync(resolve(fixtureDir, "plan_book_loop.json"), "utf8");
    const plan = JSON.parse(raw) as {
      loops: Array<{ id: string; stations: string[] }>;
      nodes: Array<{ id: string }>;
    };
    expect(plan.loops[0]?.id).toBe("book-body");
    expect(plan.loops[0]?.stations).toContain("write-chapter");
    expect(plan.nodes.some((n) => n.id === "world-bible")).toBe(true);
  });
});
