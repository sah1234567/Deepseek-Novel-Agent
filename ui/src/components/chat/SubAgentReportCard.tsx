import type { UIMessage } from "../../hooks/useAgent";
import { MessageBody } from "./MessageBody";
import "./SubAgentReportCard.css";

interface SubAgentReportCardProps {
  message: UIMessage;
  onViewDetails: () => void;
}

export function SubAgentReportCard({ message, onViewDetails }: SubAgentReportCardProps) {
  return (
    <div className="sub-agent-report-card">
      <MessageBody blocks={message.contentBlocks} />
      {message.forkRunId && (
        <button type="button" className="sub-agent-report-view-btn" onClick={onViewDetails}>
          查看 Subagent 详情
        </button>
      )}
    </div>
  );
}
