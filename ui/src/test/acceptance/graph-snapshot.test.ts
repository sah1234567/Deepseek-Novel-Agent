import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import type { GraphStateSnapshot } from "../../graph/types";

const fixtureDir = resolve(__dirname, "../../../../tests/fixtures/graph");

describe("graph snapshot golden", () => {
  it("parses snapshot_ch37 with loop Ch.37 badge fields", () => {
    const raw = readFileSync(resolve(fixtureDir, "snapshot_ch37.json"), "utf8");
    const snap = JSON.parse(raw) as GraphStateSnapshot;
    expect(snap.loops[0]?.loopId).toBe("book-body");
    expect(snap.loops[0]?.cursor.counters.chapter).toBe(37);
    expect(snap.loops[0]?.phase).toBe("running");
    expect(snap.loops[0]?.stationIds).toContain("write-chapter");
    expect(snap.loopSummaries?.[0]?.cursorLabel).toContain("chapter=37");
    const write = snap.nodes.find((n) => n.id === "write-chapter");
    expect(write?.cursorBadge).toBe("chapter=37");
    expect(write?.status).toBe("running");
    expect(snap.edges.every((e) => e.kind === "deps")).toBe(true);
  });

  it("parses handoff_outline golden", () => {
    const raw = readFileSync(resolve(fixtureDir, "handoff_outline.json"), "utf8");
    const h = JSON.parse(raw) as {
      summary: string;
      files_touched: Array<{ path: string }>;
      artifacts: string[];
    };
    expect(h.summary.length).toBeGreaterThan(0);
    expect(h.files_touched.length).toBeGreaterThan(0);
    expect(h.artifacts.length).toBeGreaterThan(0);
  });

  it("parses snapshot_generic non-novel counters", () => {
    const raw = readFileSync(resolve(fixtureDir, "snapshot_generic.json"), "utf8");
    const snap = JSON.parse(raw) as GraphStateSnapshot;
    expect(snap.hasPlan).toBe(true);
    expect(snap.loops[0]?.loopId).toBe("paper-body");
    expect(snap.loops[0]?.cursor.counters.section).toBe(3);
    expect(snap.loops[0]?.settings.targetSections).toBe(10);
    expect(snap.loopSummaries?.[0]?.cursorLabel).toContain("section=3");
    const write = snap.nodes.find((n) => n.id === "write-section");
    expect(write?.tags).toContain("writing");
    expect(write?.cursorBadge).toBe("section=3");
    expect(write?.status).toBe("running");
  });
});
