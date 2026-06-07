import type { ContentBlock, ToolCall, UIMessage } from "../types/messages";
import {
  SYNTHETIC_USER_ID,
  type LlmSegment,
  type TranscriptContext,
  type TranscriptMachine,
  type Turn,
} from "./types";

function assistantHasContent(blocks: ContentBlock[]): boolean {
  return blocks.some((b) => (b.text ?? "").length > 0);
}

function upsertToolInSegment(
  segment: LlmSegment,
  toolCallId: string,
  patch: Partial<ToolCall> & { name?: string },
): void {
  const idx = segment.tools.findIndex((t) => t.id === toolCallId);
  if (idx >= 0) {
    segment.tools[idx] = { ...segment.tools[idx], ...patch, id: toolCallId };
  } else if (patch.name) {
    segment.tools.push({
      id: toolCallId,
      name: patch.name,
      input: patch.input ?? {},
      status: patch.status ?? "streaming-args",
      needsApproval: patch.needsApproval ?? false,
      result: patch.result,
      progressDescription: patch.progressDescription,
      unparsedInput: patch.unparsedInput,
      parsedInput: patch.parsedInput,
    });
  }
}

export function flatMessagesToMachine(flatMessages: UIMessage[]): {
  machine: TranscriptMachine;
} {
  const ctx: TranscriptContext = {
    turns: [],
    openSegment: null,
    streamingMessageId: null,
  };

  let currentTurn: Turn | null = null;
  let currentSegment: LlmSegment | null = null;
  let segmentIndex = 0;

  for (const msg of flatMessages) {
    if (msg.role === "user") {
      currentTurn = {
        turnId: msg.id,
        user: msg,
        segments: [],
        reports: [],
      };
      ctx.turns.push(currentTurn);
      currentSegment = null;
      segmentIndex = 0;
      continue;
    }
    if (msg.role === "subAgentReport") {
      if (currentTurn) currentTurn.reports.push(msg);
      continue;
    }
    if (msg.role === "assistant") {
      if (!currentTurn) {
        currentTurn = {
          turnId: SYNTHETIC_USER_ID,
          user: { id: SYNTHETIC_USER_ID, role: "user", contentBlocks: [] },
          segments: [],
          reports: [],
        };
        ctx.turns.push(currentTurn);
      }
      const hasContent = assistantHasContent(msg.contentBlocks);
      currentSegment = {
        segmentId: `${currentTurn.turnId}:${segmentIndex}`,
        segmentIndex,
        assistant: {
          id: msg.id,
          status: hasContent ? "committed" : "placeholder",
          contentBlocks: msg.contentBlocks,
        },
        tools: [],
      };
      currentTurn.segments.push(currentSegment);
      segmentIndex++;
      continue;
    }
    if (msg.role === "tool") {
      if (!currentTurn) {
        currentTurn = {
          turnId: SYNTHETIC_USER_ID,
          user: { id: SYNTHETIC_USER_ID, role: "user", contentBlocks: [] },
          segments: [],
          reports: [],
        };
        ctx.turns.push(currentTurn);
      }
      if (!currentSegment) {
        currentSegment = {
          segmentId: `${currentTurn.turnId}:${segmentIndex}`,
          segmentIndex,
          assistant: {
            id: `placeholder-${segmentIndex}`,
            status: "placeholder",
            contentBlocks: [],
          },
          tools: [],
        };
        currentTurn.segments.push(currentSegment);
        segmentIndex++;
      }
      const toolId = msg.id.replace(/^tool-/, "");
      upsertToolInSegment(currentSegment, toolId, {
        name: msg.toolName ?? "Tool",
        input: msg.toolInput ?? {},
        status: (msg.toolStatus as ToolCall["status"]) ?? "done",
        needsApproval: !!msg.needsApproval,
        result: msg.contentBlocks[0]?.text,
      });
    }
  }

  return { machine: { phase: "idle", context: ctx } };
}
