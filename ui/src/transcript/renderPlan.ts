import { isSyntheticUser } from "./types";
import type { TranscriptMachine } from "./types";
import { pauseSegmentId } from "./selectors";

export type RenderNode =
  | { kind: "user"; turnId: string }
  | {
      kind: "segment";
      segmentId: string;
      segmentIndex: number;
      variant: "committed" | "live";
      assistantId: string;
      toolIds: string[];
    }
  | { kind: "question"; afterSegmentId: string };

export interface RenderPlanOptions {
  mode: "main" | "fork";
  /** When set (e.g. pending AskUserQuestion), question node is inserted after this segment. */
  includeQuestion?: boolean;
}

/** Structural acceptance subset of TranscriptView order (no archive epochs / intersection lazy load). */
export function buildTranscriptRenderPlan(
  machine: TranscriptMachine,
  opts: RenderPlanOptions,
): RenderNode[] {
  const plan: RenderNode[] = [];
  const pauseId = opts.includeQuestion ? pauseSegmentId(machine) : undefined;
  const { turns, openSegment } = machine.context;

  for (const turn of turns) {
    if (opts.mode === "main" && !isSyntheticUser(turn.user)) {
      plan.push({ kind: "user", turnId: turn.turnId });
    }
    for (const seg of turn.segments) {
      plan.push({
        kind: "segment",
        segmentId: seg.segmentId,
        segmentIndex: seg.segmentIndex,
        variant: "committed",
        assistantId: seg.assistant.id,
        toolIds: seg.tools.map((t) => t.id),
      });
      if (pauseId === seg.segmentId) {
        plan.push({ kind: "question", afterSegmentId: seg.segmentId });
      }
    }
  }

  if (machine.phase === "segmentStreaming" && openSegment) {
    plan.push({
      kind: "segment",
      segmentId: openSegment.segmentId,
      segmentIndex: openSegment.segmentIndex,
      variant: "live",
      assistantId: openSegment.assistant.id,
      toolIds: openSegment.tools.map((t) => t.id),
    });
  }

  if (
    pauseId &&
    opts.includeQuestion &&
    !plan.some((n) => n.kind === "question" && n.afterSegmentId === pauseId)
  ) {
    plan.push({ kind: "question", afterSegmentId: pauseId });
  }

  return plan;
}

export function validateRenderPlan(plan: RenderNode[]): string[] {
  const errors: string[] = [];
  const seenSegments = new Set<string>();

  for (let i = 0; i < plan.length; i++) {
    const node = plan[i];
    if (node.kind === "segment") {
      if (seenSegments.has(node.segmentId) && node.variant === "committed") {
        errors.push(`duplicate committed segment ${node.segmentId}`);
      }
      if (node.variant === "committed") {
        seenSegments.add(node.segmentId);
      }
      const dupTools = node.toolIds.filter((id, idx) => node.toolIds.indexOf(id) !== idx);
      if (dupTools.length > 0) {
        errors.push(`segment ${node.segmentId} has duplicate tool ids: ${dupTools.join(",")}`);
      }
    }
    if (node.kind === "question") {
      const segIdx = plan.findIndex(
        (n) => n.kind === "segment" && n.segmentId === node.afterSegmentId,
      );
      if (segIdx < 0) {
        errors.push(`question references missing segment ${node.afterSegmentId}`);
        continue;
      }
      for (let j = segIdx + 1; j < i; j++) {
        const between = plan[j];
        if (between.kind === "segment" && between.segmentId !== node.afterSegmentId) {
          errors.push(
            `question for ${node.afterSegmentId} appears before segment ${between.segmentId} ended`,
          );
          break;
        }
      }
      const next = plan[i + 1];
      if (next?.kind === "segment") {
        const seg = plan[segIdx];
        if (seg.kind === "segment" && next.segmentId === seg.segmentId) {
          errors.push(`question must not be inside segment group ${seg.segmentId}`);
        }
      }
    }
  }

  const allToolIds: string[] = [];
  for (const node of plan) {
    if (node.kind !== "segment") continue;
    for (const tid of node.toolIds) {
      if (allToolIds.includes(tid)) {
        errors.push(`tool ${tid} appears in multiple segment groups`);
      }
      allToolIds.push(tid);
    }
  }

  return errors;
}

/** Every segment group lists assistant before tools (implicit in plan shape). */
export function assistantBeforeToolsInPlan(plan: RenderNode[]): boolean {
  for (const node of plan) {
    if (node.kind !== "segment") continue;
    // Plan encodes assistant first structurally; toolIds are children of segment node.
    if (node.toolIds.length === 0) continue;
    if (!node.assistantId) return false;
  }
  return true;
}

export function questionFollowsSegmentTools(
  plan: RenderNode[],
  segmentId: string,
): boolean {
  const seg = plan.find((n) => n.kind === "segment" && n.segmentId === segmentId);
  const q = plan.find((n) => n.kind === "question" && n.afterSegmentId === segmentId);
  if (!seg || !q) return false;
  const segIdx = plan.indexOf(seg);
  const qIdx = plan.indexOf(q);
  if (qIdx <= segIdx) return false;
  for (let i = segIdx + 1; i < qIdx; i++) {
    if (plan[i].kind === "segment") return false;
  }
  return true;
}

export function validateMachineStructure(machine: TranscriptMachine): string[] {
  const errors: string[] = [];
  const { turns, openSegment } = machine.context;

  for (const turn of turns) {
    for (const seg of turn.segments) {
      if (!seg.segmentId.startsWith(`${turn.turnId}:`)) {
        errors.push(`segment ${seg.segmentId} turnId mismatch`);
      }
    }
  }

  if (openSegment) {
    const turn = turns.length > 0 ? turns[turns.length - 1] : null;
    if (turn) {
      const committedIndexes = turn.segments.map((s) => s.segmentIndex);
      if (committedIndexes.includes(openSegment.segmentIndex)) {
        errors.push(`openSegment index ${openSegment.segmentIndex} collides with committed segment`);
      }
    }
  }

  if (machine.phase !== "segmentStreaming" && openSegment) {
    errors.push("openSegment set while phase is not segmentStreaming");
  }

  return errors;
}

export function segmentGroupsInOrder(plan: RenderNode[]): string[] {
  return plan.filter((n): n is Extract<RenderNode, { kind: "segment" }> => n.kind === "segment").map(
    (n) => `${n.segmentId}:${n.variant}`,
  );
}
