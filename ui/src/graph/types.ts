/** Mirrors novel_graph::GraphStateSnapshot (camelCase). */

export type NodeStatus =
  | "waiting"
  | "ready"
  | "running"
  | "verifying"
  | "awaiting_approval"
  | "achieved"
  | "failed";

export type LoopPhase = "idle" | "running" | "advancing" | "paused" | "completed";

export type GraphNodeView = {
  id: string;
  title: string;
  kind: string;
  status: NodeStatus;
  loopId?: string | null;
  cursorBadge?: string | null;
  effectiveSpec?: string | null;
  humanIntervened?: boolean;
};

export type GraphEdgeView = {
  source: string;
  target: string;
  kind: string;
};

export type GraphLoopView = {
  loopId: string;
  stationIds: string[];
  cursor: { chapter: number; volume: number; fineOutlineThrough: number; round: number };
  targetChapters?: number | null;
  phase: LoopPhase;
  activeStationId?: string | null;
  lastAdvanceAt?: string | null;
  pausedReason?: string | null;
  worldStateBoard: string[];
  entry: string;
  advanceAfter: string;
};

export type GraphHitlHint = {
  nodeId: string;
  kind: string;
  label: string;
};

export type LoopSummary = {
  loopId: string;
  cursorLabel: string;
  phase: LoopPhase;
};

export type GraphStateSnapshot = {
  planVersion: string;
  nodes: GraphNodeView[];
  edges: GraphEdgeView[];
  loops: GraphLoopView[];
  focusedNodeId?: string | null;
  runningNodeIds: string[];
  hitl: GraphHitlHint[];
  loopSummaries?: LoopSummary[];
  enforceGates: boolean;
  /** False when work has no formal plan-graph yet. */
  hasPlan?: boolean;
};

export type LoopHistoryRow = {
  chapter: number;
  artifactPath: string;
  handoffSummaryPreview: string;
  achievedAt?: string | null;
};
