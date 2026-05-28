import { useEffect, useState } from "react";
import type { ForkRunState, UIMessage } from "../../hooks/useAgent";
import { MessageBody } from "./MessageBody";
import { ToolUseCard } from "./ToolUseCard";
import ReactMarkdown from "react-markdown";
import "./SubAgentReportCard.css";

interface SubAgentReportCardProps {
  message: UIMessage;
  forkRun?: ForkRunState;
  onLoadTranscript: (forkRunId: string) => void;
}

const AGENT_LABELS: Record<string, string> = {
  knowledgeauditor: "知识库核查",
  chaptercraftanalyzer: "章节品控",
  generalpurpose: "自定义 Subagent",
};

function parseReportMeta(content: string): {
  agentLabel: string;
  meta: string;
  summary: string;
} {
  const match = content.match(/\[子 Agent 完成:\s*(\w+)\]/);
  const agentType = match?.[1]?.toLowerCase() ?? "";
  const agentLabel = AGENT_LABELS[agentType] ?? agentType;

  if (agentType === "generalpurpose") {
    return { agentLabel, meta: "", summary: "" };
  }

  const summaryMatch = content.match(/\n1\.\s*\*?\*?摘要\*?\*?[：:]\s*(.+)/);
  const firstFinding = content.match(/^- \*\*(.+?)\*\*/m);

  let summary = summaryMatch?.[1]?.trim() ?? "";
  if (!summary && firstFinding) {
    summary = firstFinding[1].trim();
  }

  const findings = (content.match(/^- \*\*/gm) ?? []).length;
  const meta = findings > 0 ? `${findings} 项发现` : "";

  return { agentLabel, meta, summary };
}

export function SubAgentReportCard({
  message,
  forkRun,
  onLoadTranscript,
}: SubAgentReportCardProps) {
  const [expanded, setExpanded] = useState(false);
  const [showTask, setShowTask] = useState(false);
  const content = message.contentBlocks
    .map((b) => (typeof b.text === "string" ? b.text : ""))
    .join("\n");
  const { agentLabel, meta, summary } = parseReportMeta(content);

  useEffect(() => {
    if (expanded && message.forkRunId && (!forkRun || forkRun.messages.length === 0)) {
      onLoadTranscript(message.forkRunId);
    }
  }, [expanded, message.forkRunId, forkRun, onLoadTranscript]);

  const transcriptMsgs = forkRun?.messages ?? [];
  // Last message = report; everything before = transcript
  const reportMsg = transcriptMsgs.length > 0
    ? transcriptMsgs[transcriptMsgs.length - 1]
    : null;
  const toolMsgs = transcriptMsgs.filter((m) => m.role === "tool");
  const taskMsg = transcriptMsgs.find((m) => m.role === "user");

  return (
    <div className="sub-agent-report-card">
      <button
        type="button"
        className={`sub-agent-report-header${expanded ? " expanded" : ""}`}
        onClick={() => setExpanded((v) => !v)}
        title={expanded ? "收起" : "展开查看详情"}
      >
        <span className="sub-agent-report-badge">{agentLabel}</span>
        <span className="sub-agent-report-meta">{meta || summary}</span>
        <span className="sub-agent-report-chevron">{expanded ? "▾" : "▸"}</span>
      </button>

      {expanded && (
        <div className="sub-agent-report-body">
          {/* Task prompt — collapsible */}
          {taskMsg && (
            <details
              className="sub-agent-task-details"
              open={showTask}
              onToggle={(e) => setShowTask(e.currentTarget.open)}
            >
              <summary className="sub-agent-task-summary">
                任务提示词
              </summary>
              <div className="sub-agent-task-content">
                <MessageBody blocks={taskMsg.contentBlocks} />
              </div>
            </details>
          )}

          {/* Tool calls */}
          {toolMsgs.map((tm) => (
            <ToolUseCard
              key={tm.id}
              tool={{
                id: tm.id.replace(/^tool-/, ""),
                name: tm.toolName ?? "Tool",
                input: tm.toolInput ?? {},
                status: tm.toolStatus ?? "done",
                needsApproval: false,
                result: tm.contentBlocks[0]?.text,
              }}
              isStreaming={false}
            />
          ))}

          {/* Thinking blocks from assistant messages */}
          {transcriptMsgs
            .filter((m) => m.role === "assistant")
            .map((am) =>
              am.contentBlocks.map((block, i) => {
                if (block.kind === "thinking" && block.text) {
                  return (
                    <details key={`${am.id}-t${i}`} className="thinking-stream" open>
                      <summary>思考中…</summary>
                      <ReactMarkdown>{block.text}</ReactMarkdown>
                    </details>
                  );
                }
                return null;
              }),
            )}

          {/* Report — collapsible purple section at bottom */}
          {reportMsg && reportMsg.role === "assistant" && (
            <details className="sub-agent-report-details" open>
              <summary className="sub-agent-report-summary">审计报告</summary>
              <div className="sub-agent-report-content">
                <MessageBody blocks={reportMsg.contentBlocks} />
              </div>
            </details>
          )}

          {/* Loading state */}
          {transcriptMsgs.length === 0 && (
            <p className="sub-agent-report-loading">加载 transcript…</p>
          )}
        </div>
      )}
    </div>
  );
}
