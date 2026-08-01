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

/** Generic cursor — counters and tags are defined by the plan JSON. */
export type Cursor = {
  counters: Record<string, number>;
  tags: Record<string, string>;
};

export type GraphNodeView = {
  id: string;
  title: string;
  /** Domain tags (replaces old `kind: string`). */
  tags: string[];
  status: NodeStatus;
  loopId?: string | null;
  cursorBadge?: string | null;
  effectiveSpec?: string | null;
  humanIntervened?: boolean;
};

type GraphEdgeView = {
  source: string;
  target: string;
  kind: string;
};

type GraphLoopView = {
  loopId: string;
  stationIds: string[];
  cursor: Cursor;
  /** Generic settings (replaces old `targetChapters?: number`). */
  settings: Record<string, unknown>;
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

type LoopSummary = {
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
  /** Generic snapshot key (replaces old `chapter: number`). */
  snapshotKey: string;
  artifactPath: string;
  handoffSummaryPreview: string;
  achievedAt?: string | null;
};
