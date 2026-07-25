import { MarkerType, type Edge } from "@xyflow/react";

/** Solid gray dependency edges from backend `edges[]`. */
const DEP_EDGE_STYLE = { stroke: "#64748b" };

/** Decorative dashed pink cycle hint inside loops (not real deps). */
const CYCLE_EDGE_STYLE = { stroke: "#f472b6", strokeDasharray: "4 4" };

export function depEdge(id: string, source: string, target: string): Edge {
  return {
    id,
    source,
    target,
    markerEnd: { type: MarkerType.ArrowClosed },
    style: DEP_EDGE_STYLE,
  };
}

export function cycleEdge(id: string, source: string, target: string, label = "↻ 轮转"): Edge {
  return {
    id,
    source,
    target,
    animated: true,
    style: CYCLE_EDGE_STYLE,
    label,
  };
}
