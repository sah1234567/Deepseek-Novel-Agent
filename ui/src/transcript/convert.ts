import type { ToolCall, UIMessage } from "../types/messages";
import { flatMessagesToMachine } from "./flatParse";
import type { TranscriptMachine } from "./types";

export function flatMessagesToTranscript(flatMessages: UIMessage[]): TranscriptMachine {
  return flatMessagesToMachine(flatMessages).machine;
}

function toolToMessage(tool: ToolCall): UIMessage {
  return {
    id: `tool-${tool.id}`,
    role: "tool",
    toolName: tool.name,
    toolStatus: tool.status === "streaming-args" ? "running" : tool.status,
    needsApproval: tool.needsApproval,
    contentBlocks: [{ blockIndex: 0, kind: "text", text: tool.result ?? "" }],
    toolInput: tool.input,
  };
}

function assistantToMessage(assistant: {
  id: string;
  contentBlocks: UIMessage["contentBlocks"];
}): UIMessage {
  return {
    id: assistant.id,
    role: "assistant",
    contentBlocks: assistant.contentBlocks,
  };
}

export function transcriptToFlatMessages(machine: TranscriptMachine): UIMessage[] {
  const out: UIMessage[] = [];
  const ctx = machine.context;

  for (const turn of ctx.turns) {
    if (turn.user.contentBlocks.some((b) => b.text?.length) || turn.user.role === "user") {
      const isSynthetic =
        turn.user.id === "__synthetic_user__" &&
        !turn.user.contentBlocks.some((b) => b.text?.length);
      if (!isSynthetic) {
        out.push(turn.user);
      }
    }

    let reportIdx = 0;
    for (const seg of turn.segments) {
      out.push(assistantToMessage(seg.assistant));
      for (const tool of seg.tools) {
        if (tool.status !== "streaming-args") {
          out.push(toolToMessage(tool));
        }
      }
      while (reportIdx < turn.reports.length) {
        out.push(turn.reports[reportIdx]);
        reportIdx++;
        break;
      }
    }
    while (reportIdx < turn.reports.length) {
      out.push(turn.reports[reportIdx++]);
    }
  }

  if (machine.phase === "segmentStreaming" && ctx.openSegment) {
    const seg = ctx.openSegment;
    if (assistantHasContent(seg.assistant.contentBlocks) || seg.tools.length > 0) {
      out.push(assistantToMessage(seg.assistant));
      for (const tool of seg.tools) {
        if (tool.status !== "streaming-args" || tool.result) {
          out.push(toolToMessage(tool));
        }
      }
    }
  }

  return out;
}

function assistantHasContent(blocks: UIMessage["contentBlocks"]): boolean {
  return blocks.some((b) => (b.text ?? "").length > 0);
}
