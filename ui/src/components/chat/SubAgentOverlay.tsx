import ReactMarkdown from "react-markdown";
import type { ForkRunState } from "../../hooks/useAgent";
import { ScrollViewport } from "../layout/ScrollViewport";
import { MessageBody } from "./MessageBody";
import { ToolUseCard } from "./ToolUseCard";
import "./DialogOverlays.css";
import "./SubAgentOverlay.css";

function forkAgentLabel(agentType: string): string {
  const t = agentType.toLowerCase();
  if (t.includes("knowledgeauditor")) return "KnowledgeAuditor";
  if (t.includes("chaptercraft")) return "ChapterCraftAnalyzer";
  if (t.includes("general")) return "GeneralPurpose";
  return agentType || "Subagent";
}

interface SubAgentOverlayProps {
  forkRun: ForkRunState | undefined;
  onClose: () => void;
}

export function SubAgentOverlay({ forkRun, onClose }: SubAgentOverlayProps) {
  if (!forkRun) return null;

  const liveTools = Array.from(forkRun.activeTools.values()).filter(
    (t) => t.status === "pending" || t.status === "running",
  );
  const showStreaming =
    forkRun.status === "running" &&
    (forkRun.streamingText || forkRun.streamingThinking || liveTools.length > 0);

  return (
    <div className="dialog-inner-overlay sub-agent-overlay">
      <header className="dialog-inner-overlay-header sub-agent-overlay-header">
        <span className="dialog-inner-overlay-title sub-agent-overlay-title">
          🔀 {forkAgentLabel(forkRun.agentType)}
          {forkRun.status === "running" ? " · 运行中" : " · 已完成"}
        </span>
        <button
          type="button"
          className="dialog-inner-overlay-close sub-agent-overlay-close"
          onClick={onClose}
          title="关闭"
        >
          ✕
        </button>
      </header>
      <ScrollViewport
        className="dialog-inner-overlay-scroll sub-agent-overlay-scroll"
        autoScrollTo="bottom"
        autoScrollDeps={[
          forkRun.messages.length,
          forkRun.streamingText,
          forkRun.streamingThinking,
          liveTools.length,
        ]}
      >
        <div className="sub-agent-overlay-messages">
          {forkRun.messages.length === 0 && !showStreaming && (
            <p className="sub-agent-overlay-placeholder">暂无 transcript…</p>
          )}
          {forkRun.messages.map((msg) => (
            <article key={msg.id} className={`message message-${msg.role}`}>
              <header>
                {msg.role === "user"
                  ? "任务"
                  : msg.role === "tool"
                    ? `工具 · ${msg.toolName}`
                    : "Subagent"}
              </header>
              {msg.role === "tool" ? (
                <ToolUseCard
                  tool={{
                    id: msg.id.replace(/^tool-/, ""),
                    name: msg.toolName ?? "Tool",
                    input: msg.toolInput ?? {},
                    status: msg.toolStatus ?? "done",
                    needsApproval: false,
                    result: msg.contentBlocks[0]?.text,
                  }}
                  isStreaming={false}
                />
              ) : (
                <MessageBody blocks={msg.contentBlocks} />
              )}
            </article>
          ))}
          {showStreaming && (forkRun.streamingText || forkRun.streamingThinking) && (
            <article className="message message-assistant message-streaming">
              <header>
                Subagent
                {forkRun.status === "running" && <span className="streaming-dot" aria-hidden />}
              </header>
              <div className="message-body">
                {forkRun.streamingThinking && (
                  <details className="thinking-stream" open>
                    <summary>思考中…</summary>
                    <ReactMarkdown>{forkRun.streamingThinking}</ReactMarkdown>
                  </details>
                )}
                {forkRun.streamingText && (
                  <div className="streaming-text">
                    <ReactMarkdown>{forkRun.streamingText}</ReactMarkdown>
                  </div>
                )}
              </div>
            </article>
          )}
          {liveTools.map((tool) => (
            <ToolUseCard key={tool.id} tool={tool} isStreaming={forkRun.status === "running"} />
          ))}
        </div>
      </ScrollViewport>
    </div>
  );
}
