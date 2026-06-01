import { useState } from "react";
import ReactMarkdown from "react-markdown";
import type { UIMessage } from "../../hooks/useAgent";
import { parseContextRefreshSections } from "../../transcript/types";
import "./ContextRefreshBubble.css";

export function ContextRefreshBubble({ user }: { user: UIMessage }) {
  const text = user.contentBlocks.find((b) => b.kind === "text")?.text ?? "";
  const { skill, summary } = parseContextRefreshSections(text);
  const [skillOpen, setSkillOpen] = useState(true);
  const [summaryOpen, setSummaryOpen] = useState(true);

  return (
    <article className="message message-context-refresh">
      <header>上下文刷新</header>
      {skill && (
        <details open={skillOpen} onToggle={(e) => setSkillOpen(e.currentTarget.open)}>
          <summary>激活 Skill</summary>
          <ReactMarkdown>{skill}</ReactMarkdown>
        </details>
      )}
      {summary && (
        <details open={summaryOpen} onToggle={(e) => setSummaryOpen(e.currentTarget.open)}>
          <summary>会话历史摘要</summary>
          <ReactMarkdown>{summary}</ReactMarkdown>
        </details>
      )}
    </article>
  );
}
