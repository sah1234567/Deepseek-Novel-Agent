import ReactMarkdown from "react-markdown";
import type { UIMessage } from "../../types/messages";
import {
  extractActivatedSkillLabels,
  parseContextRefreshSections,
  summaryPreview,
} from "../../transcript/contextRefresh";
import "./ContextRefreshBubble.css";

export function ContextRefreshBubble({ user }: { user: UIMessage }) {
  const text = user.contentBlocks.find((b) => b.kind === "text")?.text ?? "";
  const { skill, summary } = parseContextRefreshSections(text);
  const skillLabels = extractActivatedSkillLabels(skill);
  const preview = summaryPreview(summary);

  return (
    <article className="message message-context-refresh">
      <details>
        <summary className="context-refresh-toggle">
          <span className="context-refresh-title">上下文已刷新</span>
          {preview && <span className="context-refresh-preview">{preview}</span>}
          {skillLabels.length > 0 && (
            <span className="context-refresh-skills">
              已激活 Skill：{skillLabels.join("、")}
            </span>
          )}
        </summary>
        {summary && (
          <div className="context-refresh-body">
            <h3 className="context-refresh-body-title">会话历史摘要</h3>
            <ReactMarkdown>{summary}</ReactMarkdown>
          </div>
        )}
      </details>
    </article>
  );
}
