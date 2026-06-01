import { expect } from "vitest";
import {
  assistantBeforeToolsInPlan,
  buildTranscriptRenderPlan,
  validateMachineStructure,
  validateRenderPlan,
} from "../../transcript/renderPlan";
import type { TranscriptMachine } from "../../transcript/types";

export function assertPlanHealthy(
  machine: TranscriptMachine,
  opts: { mode?: "main" | "fork"; includeQuestion?: boolean } = {},
) {
  const plan = buildTranscriptRenderPlan(machine, {
    mode: opts.mode ?? "main",
    includeQuestion: opts.includeQuestion ?? machine.phase === "pausedForQuestion",
  });
  expect(validateRenderPlan(plan)).toEqual([]);
  expect(validateMachineStructure(machine)).toEqual([]);
  expect(assistantBeforeToolsInPlan(plan)).toBe(true);
  return plan;
}
