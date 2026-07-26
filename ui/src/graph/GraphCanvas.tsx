import { useMemo, useState } from "react";
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  type Node,
  type Edge,
  Handle,
  Position,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import type { GraphStateSnapshot, NodeStatus } from "./types";
import type { GraphStateApi } from "./useGraphState";
import { useLoopControls } from "./useLoopControls";
import { LoopFrame, type LoopFrameData } from "./LoopFrame";
import { HitlBadge } from "./HitlBadge";
import { cycleEdge, depEdge } from "./LoopCycleHint";
import { LoopHistoryDrawer } from "./LoopHistoryDrawer";

const STATUS_COLOR: Record<NodeStatus, string> = {
  waiting: "#94a3b8",
  ready: "#38bdf8",
  running: "#22c55e",
  verifying: "#a78bfa",
  awaiting_approval: "#f59e0b",
  achieved: "#10b981",
  failed: "#ef4444",
};

const NODE_W = 160;
const NODE_H = 72;
const LOOP_PAD = 16;
const LOOP_HEADER = 56;

function GraphNodeCard({
  data,
}: {
  data: {
    title: string;
    status: NodeStatus;
    cursorBadge?: string | null;
    hitl?: string | null;
  };
}) {
  return (
    <div
      style={{
        padding: "8px 10px",
        borderRadius: 8,
        border: `2px solid ${STATUS_COLOR[data.status]}`,
        background: "#0f172a",
        color: "#e2e8f0",
        minWidth: 140,
        fontSize: 12,
      }}
    >
      <Handle type="target" position={Position.Left} />
      <div style={{ fontWeight: 600 }}>{data.title}</div>
      <div style={{ opacity: 0.8 }}>{data.status}</div>
      {data.cursorBadge ? (
        <div style={{ marginTop: 4, color: "#38bdf8" }}>{data.cursorBadge}</div>
      ) : null}
      {data.hitl ? <HitlBadge label={data.hitl} /> : null}
      <Handle type="source" position={Position.Right} />
    </div>
  );
}

const nodeTypes = { graphNode: GraphNodeCard, loopFrame: LoopFrame };

type GraphCanvasProps = {
  graph: GraphStateApi;
};

export function GraphCanvas({ graph: g }: GraphCanvasProps) {
  const loopControls = useLoopControls(g.refresh);
  const snap = g.snapshot;
  const [historyOpen, setHistoryOpen] = useState(false);

  const activeLoopId = snap?.loopSummaries?.[0]?.loopId ?? snap?.loops[0]?.loopId ?? null;

  const { nodes, edges } = useMemo(() => {
    if (!snap) return { nodes: [] as Node[], edges: [] as Edge[] };
    return buildFlowGraph(snap, loopControls.pause, loopControls.resume);
  }, [snap, loopControls.pause, loopControls.resume]);

  if (g.error) {
    return (
      <div className="graph-canvas graph-canvas--error">
        <p>Graph: {g.error}</p>
        <button type="button" onClick={() => void g.refresh()}>
          Retry
        </button>
      </div>
    );
  }
  if (!snap) {
    return <div className="graph-canvas">Loading graph…</div>;
  }
  if (snap.hasPlan === false || snap.nodes.length === 0) {
    return (
      <div className="graph-canvas graph-canvas--empty">
        <p>尚无工作流计划。</p>
        <p style={{ opacity: 0.8, fontSize: 13 }}>
          在聊天中告诉 Agent 你的需求，Agent 会使用{" "}
          <code>PlanBuilder</code> 增量构建工作流图（add_node → add_loop → set_dep → commit）。
        </p>
      </div>
    );
  }

  return (
    <div className="graph-canvas">
      <div className="graph-canvas-toolbar">
        <strong>Graph</strong> · {snap.planVersion}
        {activeLoopId ? (
          <button type="button" className="graph-toolbar-btn" onClick={() => setHistoryOpen(true)}>
            History
          </button>
        ) : null}
        <button
          type="button"
          className="graph-toolbar-btn graph-toolbar-btn--right"
          onClick={() => void g.refresh()}
        >
          Refresh
        </button>
      </div>
      <div className="graph-canvas-flow">
        <ReactFlow
          nodes={nodes}
          edges={edges}
          nodeTypes={nodeTypes}
          fitView
          colorMode="dark"
          proOptions={{ hideAttribution: true }}
          onNodeClick={(_, n) => {
            if (n.type === "loopFrame") return;
            void g.enterNodeSession(n.id);
          }}
        >
          <Background />
          <Controls />
          <MiniMap />
        </ReactFlow>
      </div>
      {activeLoopId ? (
        <LoopHistoryDrawer
          loopId={activeLoopId}
          open={historyOpen}
          onClose={() => setHistoryOpen(false)}
          listHistory={g.listLoopHistory}
        />
      ) : null}
    </div>
  );
}

function buildFlowGraph(
  snap: GraphStateSnapshot,
  onPause: (loopId: string) => void,
  onResume: (loopId: string) => void,
): { nodes: Node[]; edges: Edge[] } {
  const hitlMap = new Map(snap.hitl.map((h) => [h.nodeId, h.label]));
  const absPos = layoutNodes(snap);
  const loopByStation = new Map<string, string>();
  for (const lp of snap.loops) {
    for (const sid of lp.stationIds) {
      loopByStation.set(sid, lp.loopId);
    }
  }

  const nodes: Node[] = [];
  const edges: Edge[] = snap.edges.map((e, i) => depEdge(`deps-${i}`, e.source, e.target));

  for (const lp of snap.loops) {
    if (lp.stationIds.length >= 2) {
      edges.push(cycleEdge(`cycle-${lp.loopId}`, lp.advanceAfter, lp.entry));
    }
  }

  for (const n of snap.nodes) {
    if (loopByStation.has(n.id)) continue;
    nodes.push(stationNode(n, absPos.get(n.id) ?? { x: 0, y: 0 }, hitlMap, snap.focusedNodeId));
  }

  for (const lp of snap.loops) {
    const stations = lp.stationIds.filter((id) => snap.nodes.some((n) => n.id === id));
    if (stations.length === 0) continue;

    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const id of stations) {
      const p = absPos.get(id) ?? { x: 0, y: 0 };
      minX = Math.min(minX, p.x);
      minY = Math.min(minY, p.y);
      maxX = Math.max(maxX, p.x + NODE_W);
      maxY = Math.max(maxY, p.y + NODE_H);
    }

    const parentX = minX - LOOP_PAD;
    const parentY = minY - LOOP_PAD - LOOP_HEADER;
    const parentW = maxX - minX + LOOP_PAD * 2;
    const parentH = maxY - minY + LOOP_PAD * 2 + LOOP_HEADER;

    const frameData: LoopFrameData = {
      loopId: lp.loopId,
      cursor: lp.cursor,
      settings: lp.settings,
      phase: lp.phase,
      onPause: () => void onPause(lp.loopId),
      onResume: () => void onResume(lp.loopId),
    };

    nodes.push({
      id: lp.loopId,
      type: "loopFrame",
      position: { x: parentX, y: parentY },
      // Transparent shell — LoopFrame paints the dashed frame (avoids RF default white parent).
      style: {
        width: parentW,
        height: parentH,
        background: "transparent",
        border: "none",
        padding: 0,
      },
      data: frameData,
      draggable: false,
      selectable: false,
      zIndex: -1,
    });

    for (const id of stations) {
      const n = snap.nodes.find((x) => x.id === id);
      if (!n) continue;
      const p = absPos.get(id) ?? { x: 0, y: 0 };
      nodes.push({
        ...stationNode(
          n,
          { x: p.x - parentX, y: p.y - parentY + LOOP_HEADER },
          hitlMap,
          snap.focusedNodeId,
        ),
        parentId: lp.loopId,
        extent: "parent",
      });
    }
  }

  return { nodes, edges };
}

function stationNode(
  n: GraphStateSnapshot["nodes"][number],
  position: { x: number; y: number },
  hitlMap: Map<string, string>,
  focusedNodeId?: string | null,
): Node {
  return {
    id: n.id,
    type: "graphNode",
    position,
    data: {
      title: n.title,
      status: n.status,
      cursorBadge: n.cursorBadge,
      hitl: hitlMap.get(n.id) ?? null,
    },
    style: n.id === focusedNodeId ? { boxShadow: "0 0 0 2px #fff" } : undefined,
  };
}

function layoutNodes(snap: GraphStateSnapshot): Map<string, { x: number; y: number }> {
  const indeg = new Map<string, number>();
  const children = new Map<string, string[]>();
  for (const n of snap.nodes) {
    indeg.set(n.id, 0);
    children.set(n.id, []);
  }
  for (const e of snap.edges) {
    indeg.set(e.target, (indeg.get(e.target) ?? 0) + 1);
    children.get(e.source)?.push(e.target);
  }
  const layers = new Map<string, number>();
  const q = [...snap.nodes.filter((n) => (indeg.get(n.id) ?? 0) === 0).map((n) => n.id)];
  for (const id of q) layers.set(id, 0);
  while (q.length) {
    const u = q.shift()!;
    const lu = layers.get(u) ?? 0;
    for (const v of children.get(u) ?? []) {
      layers.set(v, Math.max(layers.get(v) ?? 0, lu + 1));
      const d = (indeg.get(v) ?? 1) - 1;
      indeg.set(v, d);
      if (d === 0) q.push(v);
    }
  }
  const byLayer = new Map<number, string[]>();
  for (const n of snap.nodes) {
    const L = layers.get(n.id) ?? 0;
    if (!byLayer.has(L)) byLayer.set(L, []);
    byLayer.get(L)!.push(n.id);
  }
  const pos = new Map<string, { x: number; y: number }>();
  for (const [L, ids] of byLayer) {
    ids.forEach((id, i) => {
      pos.set(id, { x: L * 220, y: i * 100 });
    });
  }
  return pos;
}
