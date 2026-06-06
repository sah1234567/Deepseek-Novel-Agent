import { MessageBody } from "./MessageBody";
import { agentLabelFromType } from "../../fork";
import "./SubAgentForkCard.css";

export type SubAgentForkStatus = "running" | "complete";

export interface SubAgentForkCardProps {
  agentType: string;
  summary: string;
  status: SubAgentForkStatus;
  reportContent?: string;
  onEnter: () => void;
  enterDisabled?: boolean;
  enterHint?: string;
  /** `tool` = ForkSubAgent tool row; `hook` = PostToolUse standalone card. */
  mode?: "tool" | "hook";
}

export function SubAgentForkCard({
  agentType,
  summary,
  status,
  reportContent,
  onEnter,
  enterDisabled = false,
  enterHint,
}: SubAgentForkCardProps) {
  const label = agentLabelFromType(agentType);
  const isRunning = status === "running";
  const hasReport = !!reportContent?.trim();

  return (
    <article
      className={`message message-assistant sub-agent-fork-card${isRunning ? " message-streaming" : ""}`}
    >
      <header className="sub-agent-fork-header">
        <span>Subagent · {label}</span>
        {isRunning && <span className="streaming-dot" aria-hidden />}
        {isRunning && <span className="sub-agent-fork-badge">运行中</span>}
        <button
          type="button"
          className="sub-agent-fork-enter"
          disabled={enterDisabled}
          title={enterHint}
          onClick={onEnter}
        >
          进入
        </button>
      </header>

      <div className="message-body">
        {summary.trim() && <p className="sub-agent-fork-summary">{summary}</p>}

        {hasReport && (
          <details className="thinking-stream">
            <summary>返回内容</summary>
            <MessageBody blocks={[{ blockIndex: 0, kind: "text", text: reportContent! }]} />
          </details>
        )}
      </div>
    </article>
  );
}
