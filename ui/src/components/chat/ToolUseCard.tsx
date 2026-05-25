import { useState } from "react";
import type { ToolCall, StreamingToolUse } from "../../hooks/useAgent";
import { formatToolSummary, formatToolInput } from "../../utils/tools";
import "./ToolUseCard.css";

type ToolLike = ToolCall | (StreamingToolUse & { needsApproval?: boolean; status?: ToolCall["status"]; result?: string; progressDescription?: string });

export function ToolUseCard({
  tool,
  isStreamingInput,
  isStreaming: _isStreaming,
  onApprove,
  onDeny,
}: {
  tool: ToolLike;
  isStreamingInput?: boolean;
  isStreaming?: boolean;
  onApprove?: (id: string) => void;
  onDeny?: (id: string, reason?: string) => void;
}) {
  const [detailOpen, setDetailOpen] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const isStreamingTool = isStreamingInput && "unparsedInput" in tool;
  const input = isStreamingTool
    ? (tool.parsedInput !== undefined
        ? tool.parsedInput
        : (() => { try { return JSON.parse(tool.unparsedInput); } catch { return {}; } })())
    : "input" in tool ? tool.input : {};
  const status = "status" in tool && tool.status ? tool.status : "running";
  const needsApproval = "needsApproval" in tool && !!tool.needsApproval;
  const result = "result" in tool ? tool.result : undefined;
  const progress = "progressDescription" in tool ? tool.progressDescription : undefined;
  const toolName = tool.name;

  const statusIcon =
    status === "pending" ? "○" : status === "running" ? "◌" : status === "denied" ? "✕" : "✓";
  const statusClass =
    status === "pending" ? "pending" : status === "running" ? "running" : status === "denied" ? "denied" : "done";

  return (
    <article className={`tool-call tool-${statusClass}`}>
      <header>
        <span className={`tool-status-icon tool-${statusClass}`}>{statusIcon}</span>
        <strong className="tool-name">{toolName}</strong>
        <span className="tool-summary">{formatToolSummary(toolName, input)}</span>
        {isStreamingInput && <span className="tool-badge">...</span>}
        {progress && <span className="tool-badge tool-badge-info">{progress}</span>}
        {needsApproval && status === "pending" && (
          <span className="tool-badge tool-badge-warn">待批准</span>
        )}
      </header>

      {formatToolInput(toolName, input) && (
        <button
          type="button"
          className="tool-expand"
          onClick={() => setDetailOpen((v) => !v)}
        >
          {detailOpen ? "收起参数" : "查看参数"}
        </button>
      )}
      {detailOpen && (
        <div className="tool-detail">{formatToolInput(toolName, input)}</div>
      )}

      {result && (
        <>
          <button type="button" className="tool-expand" onClick={() => setExpanded((v) => !v)}>
            {expanded ? "收起结果" : "展开结果"}
          </button>
          {expanded && <pre className="tool-result">{result}</pre>}
        </>
      )}

      {needsApproval && status === "pending" && onApprove && onDeny && (
        <div className="tool-actions">
          <button type="button" onClick={() => onApprove(tool.id)}>
            批准
          </button>
          <button type="button" className="btn-deny" onClick={() => onDeny(tool.id, "用户拒绝")}>
            拒绝
          </button>
        </div>
      )}
    </article>
  );
}
