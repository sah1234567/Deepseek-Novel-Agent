import { memo } from "react";
import ReactMarkdown from "react-markdown";
import type { ForkRunState, ToolCall, UIMessage } from "../../types/messages";
import type { LlmSegment, SegmentAssistant } from "../../transcript/types";
import { reportContentByForkRunId, resolveForkRunIdForToolCard } from "../../fork";
import { formatToolInput } from "../../utils/tools";
import { isContextRefreshUser } from "../../transcript/contextRefresh";
import { MessageBody } from "./MessageBody";
import { ContextRefreshBubble } from "./ContextRefreshBubble";
import { SubAgentForkCard, type SubAgentForkStatus } from "./SubAgentForkCard";
import { ToolUseCard } from "./ToolUseCard";
import "./ChatPanel.css";

function messageHeader(msg: UIMessage): string {
  if (msg.role === "user") return "作者";
  if (msg.role === "tool") return `工具 · ${msg.toolName}`;
  return "Agent";
}

function forkStatusFromRun(run: ForkRunState | undefined): SubAgentForkStatus {
  if (!run) return "complete";
  return run.status === "running" ? "running" : "complete";
}

function countChineseChars(text: string): number {
  let count = 0;
  for (const ch of text) {
    const c = ch.charCodeAt(0);
    if ((c >= 0x4e00 && c <= 0x9fff) || (c >= 0x3400 && c <= 0x4dbf)) count++;
  }
  return count;
}

function assistantText(blocks: SegmentAssistant["contentBlocks"]): string {
  return blocks.filter((b) => b.kind === "text").map((b) => b.text).join("");
}

function assistantThinking(blocks: SegmentAssistant["contentBlocks"]): string {
  return blocks.filter((b) => b.kind === "thinking").map((b) => b.text).join("");
}

export function AgentBubble({
  assistant,
  isStreaming = false,
  streamingCharCount,
}: {
  assistant: SegmentAssistant;
  isStreaming?: boolean;
  streamingCharCount?: number;
}) {
  const isPlaceholder = assistant.status === "placeholder";
  const thinking = assistantThinking(assistant.contentBlocks);
  const text = assistantText(assistant.contentBlocks);
  const charCount = streamingCharCount ?? (text ? countChineseChars(text) : 0);

  if (isPlaceholder && !thinking && !text) {
    return (
      <article className="message message-assistant">
        <header>Agent · 调用工具…</header>
      </article>
    );
  }

  return (
    <article
      className={`message message-assistant${isStreaming ? " message-streaming" : ""}`}
    >
      <header>
        Agent
        {isStreaming && <span className="streaming-dot" aria-hidden />}
        {isStreaming && charCount > 0 && (
          <span className="streaming-word-count">{charCount.toLocaleString()} 字</span>
        )}
      </header>
      <div className="message-body">
        {thinking && (
          <details className="thinking-stream" open={isStreaming}>
            <summary>{isStreaming ? "思考中…" : "思考过程"}</summary>
            {isStreaming ? (
              <div className="streaming-text streaming-text-plain">{thinking}</div>
            ) : (
              <ReactMarkdown>{thinking}</ReactMarkdown>
            )}
          </details>
        )}
        {text ? (
          isStreaming ? (
            <div className="streaming-text streaming-text-plain">{text}</div>
          ) : (
            <MessageBody blocks={assistant.contentBlocks} />
          )
        ) : (
          !thinking && <MessageBody blocks={assistant.contentBlocks} />
        )}
      </div>
    </article>
  );
}

export function ToolBubble({
  tool,
  forkBindingMessages,
  forkRuns,
  onApprove,
  onDeny,
  onOpenForkOverlay,
}: {
  tool: ToolCall;
  forkBindingMessages: UIMessage[];
  forkRuns?: Map<string, ForkRunState>;
  onApprove?: (id: string) => void;
  onDeny?: (id: string, reason?: string) => void;
  onOpenForkOverlay?: (forkRunId: string) => void;
}) {
  if (tool.name === "ForkSubAgent") {
    const toolMsgId = `tool-${tool.id}`;
    const runs = forkRuns ?? new Map();
    const reports = reportContentByForkRunId(forkBindingMessages);
    const forkRunId = resolveForkRunIdForToolCard(toolMsgId, runs, forkBindingMessages);
    const run = forkRunId ? runs.get(forkRunId) : undefined;
    const agentType =
      (typeof tool.input === "object" &&
        tool.input &&
        "agent_type" in tool.input &&
        typeof (tool.input as Record<string, unknown>).agent_type === "string" &&
        (tool.input as Record<string, unknown>).agent_type) ||
      run?.agentType ||
      "Subagent";
    const summary = formatToolInput("ForkSubAgent", tool.input ?? {});
    const reportContent =
      (forkRunId ? reports.get(forkRunId) : undefined) || run?.reportOutput;
    const canEnter = !!forkRunId;
    return (
      <SubAgentForkCard
        mode="tool"
        agentType={String(agentType)}
        summary={summary}
        status={forkStatusFromRun(run)}
        reportContent={reportContent}
        enterDisabled={!canEnter}
        enterHint={canEnter ? undefined : "子 Agent 尚未启动，请稍候"}
        onEnter={() => {
          if (forkRunId) onOpenForkOverlay?.(forkRunId);
        }}
      />
    );
  }

  const isStreamingInput = tool.status === "streaming-args";
  return (
    <article className="message message-tool">
      <header>工具 · {tool.name}</header>
      <ToolUseCard
        tool={tool}
        isStreamingInput={isStreamingInput}
        onApprove={
          tool.status === "pending" && onApprove ? (id) => onApprove(id) : undefined
        }
        onDeny={
          tool.status === "pending" && onDeny
            ? (id, reason) => onDeny(id, reason)
            : undefined
        }
      />
    </article>
  );
}

export const SegmentGroup = memo(function SegmentGroup({
  segment,
  variant,
  isStreaming = false,
  forkBindingMessages,
  forkRuns,
  onApproveTool,
  onDenyTool,
  onOpenForkOverlay,
}: {
  segment: LlmSegment;
  variant: "committed" | "live";
  isStreaming?: boolean;
  forkBindingMessages: UIMessage[];
  forkRuns?: Map<string, ForkRunState>;
  onApproveTool?: (id: string) => void;
  onDenyTool?: (id: string, reason?: string) => void;
  onOpenForkOverlay?: (forkRunId: string) => void;
}) {
  const live = variant === "live";
  const assistantStreaming = live && segment.assistant.status === "streaming";
  const text = assistantText(segment.assistant.contentBlocks);

  return (
    <div className="segment-group" data-segment-id={segment.segmentId}>
      <AgentBubble
        assistant={segment.assistant}
        isStreaming={assistantStreaming && isStreaming}
        streamingCharCount={assistantStreaming && text ? countChineseChars(text) : undefined}
      />
      {segment.tools.map((tool) => (
        <ToolBubble
          key={tool.id}
          tool={tool}
          forkBindingMessages={forkBindingMessages}
          forkRuns={forkRuns}
          onApprove={onApproveTool}
          onDeny={onDenyTool}
          onOpenForkOverlay={onOpenForkOverlay}
        />
      ))}
    </div>
  );
});

export function UserBubble({
  user,
  renderRef,
}: {
  user: UIMessage;
  renderRef?: (el: HTMLElement | null) => void;
}) {
  if (isContextRefreshUser(user)) {
    return <ContextRefreshBubble user={user} />;
  }
  return (
    <article ref={renderRef} className="message message-user">
      <header>{messageHeader(user)}</header>
      <MessageBody blocks={user.contentBlocks} />
    </article>
  );
}
