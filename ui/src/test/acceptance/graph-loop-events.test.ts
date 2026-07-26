import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { IPC_EVENTS } from "../../ipc/events";

const fixtureDir = resolve(__dirname, "../../../../tests/fixtures/graph");

describe("graph loop / hitl events", () => {
  it("loop-changed fixture matches IPC event name contract", () => {
    expect(IPC_EVENTS.graphLoopChanged).toBe("graph-loop-changed");
    expect(IPC_EVENTS.nodeSessionReset).toBe("node-session-reset");
    const raw = readFileSync(resolve(fixtureDir, "event_graph_loop_changed.json"), "utf8");
    const ev = JSON.parse(raw) as {
      loopId: string;
      snapshotKey: string;
      counters: Record<string, number>;
      resetNodeIds: string[];
    };
    expect(ev.loopId).toBe("book-body");
    expect(ev.snapshotKey).toBe("chapter=38");
    expect(ev.counters.chapter).toBe(38);
    expect(ev.resetNodeIds).toContain("write-chapter");
  });

  it("hitl fixture drives badge payload shape", () => {
    expect(IPC_EVENTS.graphHitl).toBe("graph-hitl");
    const raw = readFileSync(resolve(fixtureDir, "event_graph_hitl.json"), "utf8");
    const hitl = JSON.parse(raw) as Array<{ nodeId: string; kind: string; label: string }>;
    expect(hitl[0]?.kind).toBe("node_approval");
    expect(hitl[0]?.label).toContain("放行");
  });
});
