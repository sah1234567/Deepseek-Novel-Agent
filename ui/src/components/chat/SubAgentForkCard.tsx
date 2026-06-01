import { useState } from "react";
import { MessageBody } from "./MessageBody";
import { agentLabelFromType } from "../../utils/forkLinks";
import "./SubAgentForkCard.css";

export type SubAgentForkStatus = "running" | "complete";

export interface SubAgentForkCardProps {
  agentType: string;
  summary: string;
  status: SubAgentForkStatus;
  reportContent?: string;
  onEnter: () => void;
  /** `tool` = ForkSubAgent tool row; `hook` = PostToolUse standalone card. */
  mode?: "tool" | "hook";
}

export function SubAgentForkCard({
  agentType,
  summary,
  status,
  reportContent,
  onEnter,
  mode = "tool",
}: SubAgentForkCardProps) {
  const [reportOpen, setReportOpen] = useState(false);
  const label = agentLabelFromType(agentType);
  const displayName = mode === "tool" ? "ForkSubAgent" : label;
  const statusIcon = status === "running" ? "◌" : "✓";
  const statusClass = status === "running" ? "running" : "done";
  const hasReport = !!reportContent?.trim();

  return (
    <article className={`sub-agent-fork-card sub-agent-fork-${statusClass}`}>
      <header className="sub-agent-fork-header">
        <span className={`sub-agent-fork-status-icon sub-agent-fork-${statusClass}`}>
          {statusIcon}
        </span>
        <strong className="sub-agent-fork-name">{displayName}</strong>
        <span className="sub-agent-fork-summary">
          {mode === "tool" ? label : ""}
          {summary ? `${mode === "tool" ? " · " : ""}${summary}` : ""}
        </span>
        {status === "running" && <span className="sub-agent-fork-badge">运行中</span>}
        <button type="button" className="sub-agent-fork-enter" onClick={onEnter}>
          进入
        </button>
      </header>

      {hasReport && (
        <>
          <button
            type="button"
            className="sub-agent-fork-expand"
            onClick={() => setReportOpen((v) => !v)}
          >
            {reportOpen ? "收起返回内容" : "查看返回内容"}
          </button>
          {reportOpen && (
            <div className="sub-agent-fork-report">
              <MessageBody blocks={[{ blockIndex: 0, kind: "text", text: reportContent! }]} />
            </div>
          )}
        </>
      )}
    </article>
  );
}
