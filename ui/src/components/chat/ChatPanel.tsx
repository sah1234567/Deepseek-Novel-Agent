import { FormEvent, useState } from "react";
import type { ForkRunState, PendingQuestion, UIMessage } from "../../hooks/useAgent";
import { useAgentContext } from "../../context/AgentContext";
import { APP_DISPLAY_NAME } from "../../constants/app";
import { ScrollViewport } from "../layout/ScrollViewport";
import { FilePreviewOverlay, type FilePreviewState } from "./FilePreviewOverlay";
import { MessageBody } from "./MessageBody";
import { SubAgentOverlay } from "./SubAgentOverlay";
import { SubAgentReportCard } from "./SubAgentReportCard";
import { ToolUseCard } from "./ToolUseCard";
import ReactMarkdown from "react-markdown";
import "./ChatPanel.css";
import "./DialogOverlays.css";
import "./SubAgentReportCard.css";

const MODE_TOOLTIPS: Record<string, string> = {
  normal: "常规模式：读取文件自动执行，写入/编辑文件需作者确认",
  plan: "策划模式：可只读全书；Write/Edit 仅允许 plan/ 目录（类似 Cursor Plan）。写 knowledge/、chapters/ 请切回其他模式",
  auto: "自动模式：所有操作自动批准，但关键决策问题仍会弹出询问作者",
  unattended: "无人值守模式：全自动执行。关键决策问题不再弹窗，Agent 自行分析选项并决策，决策过程在对话中可见",
};

function AskUserQuestionBlock({
  pendingQuestion,
  questionSelections,
  questionCustomText,
  questionError,
  isStreaming,
  toggleQuestionOption,
  setQuestionCustomText,
  answerQuestion,
}: {
  pendingQuestion: PendingQuestion;
  questionSelections: Record<string, string[]>;
  questionCustomText: Record<string, string>;
  questionError: string | null;
  isStreaming: boolean;
  toggleQuestionOption: (qid: string, oid: string, multi?: boolean) => void;
  setQuestionCustomText: (value: Record<string, string>) => void;
  answerQuestion: () => Promise<void>;
}) {
  const allAnswered = pendingQuestion.questions.every((q) => {
    if (questionSelections[q.id]?.length) return true;
    if (q.allowCustom && questionCustomText[q.id]?.trim()) return true;
    return false;
  });
  return (
    <article className="ask-user-question">
      <header>Agent 需要你的选择</header>
      {questionError && <p className="question-error">{questionError}</p>}
      {pendingQuestion.questions.map((q) => (
        <div key={q.id} className="question-block">
          <p className="question-prompt">{q.prompt}</p>
          <div className="question-options">
            {q.options.map((opt) => {
              const selected = questionSelections[q.id]?.includes(opt.id);
              return (
                <button
                  key={opt.id}
                  type="button"
                  className={selected ? "option selected" : "option"}
                  onClick={() => toggleQuestionOption(q.id, opt.id, q.allowMultiple)}
                >
                  {opt.label}
                </button>
              );
            })}
          </div>
          {q.allowCustom && (
            <input
              type="text"
              className="question-custom-input"
              placeholder="或输入自定义内容..."
              value={questionCustomText[q.id] ?? ""}
              onChange={(e) =>
                setQuestionCustomText({
                  ...questionCustomText,
                  [q.id]: e.target.value,
                })
              }
              disabled={isStreaming}
            />
          )}
        </div>
      ))}
      <button
        type="button"
        className="btn-confirm"
        disabled={!allAnswered || isStreaming}
        onClick={() => void answerQuestion()}
      >
        确认选择
      </button>
    </article>
  );
}

function messageHeader(msg: UIMessage): string {
  if (msg.role === "user") return "作者";
  if (msg.role === "tool") return `工具 · ${msg.toolName}`;
  if (msg.role === "subAgentReport") return "Subagent 报告";
  return "Agent";
}

function ChatMessageBody({
  msg,
  approveTool,
  denyTool,
  onOpenFork,
}: {
  msg: UIMessage;
  approveTool: (id: string) => Promise<void>;
  denyTool: (id: string, reason?: string) => Promise<void>;
  onOpenFork: (forkRunId: string) => void;
}) {
  if (msg.role === "tool") {
    return (
      <ToolUseCard
        tool={{
          id: msg.id.replace(/^tool-/, ""),
          name: msg.toolName ?? "Tool",
          input: msg.toolInput ?? {},
          status: msg.toolStatus ?? "done",
          needsApproval: msg.toolStatus === "pending" || !!msg.needsApproval,
          result: msg.contentBlocks[0]?.text,
        }}
        isStreaming={false}
        onApprove={
          msg.toolStatus === "pending" ? (id) => void approveTool(id) : undefined
        }
        onDeny={
          msg.toolStatus === "pending"
            ? (id, reason) => void denyTool(id, reason)
            : undefined
        }
      />
    );
  }
  if (msg.role === "subAgentReport") {
    return (
      <SubAgentReportCard
        message={msg}
        onViewDetails={() => {
          if (msg.forkRunId) onOpenFork(msg.forkRunId);
        }}
      />
    );
  }
  return <MessageBody blocks={msg.contentBlocks} />;
}

export function ChatPanel({
  permissionMode,
  onSetPermissionMode,
  overlayActive = false,
  filePreview = null,
  subAgentForkRun,
  onCloseSubAgent,
}: {
  permissionMode: string;
  onSetPermissionMode: (mode: string) => Promise<void>;
  /** True while an inner overlay covers the message list; preserves scroll underneath. */
  overlayActive?: boolean;
  filePreview?: FilePreviewState | null;
  subAgentForkRun?: ForkRunState;
  onCloseSubAgent?: () => void;
}) {
  const {
    messages,
    isStreaming,
    streamingText,
    streamingThinking,
    streamingToolUses,
    activeToolCalls,
    pendingQuestion,
    questionAnchorIndex,
    questionSelections,
    questionCustomText,
    questionError,
    hasInterruptibleToolInProgress,
    sendMessage,
    submitInterrupt,
    interrupt,
    approveTool,
    denyTool,
    toggleQuestionOption,
    setQuestionCustomText,
    answerQuestion,
    openForkOverlay,
    hookForkBanner,
    dismissHookForkBanner,
  } = useAgentContext();

  const [input, setInput] = useState("");

  const hasInput = input.trim().length > 0;
  const canSubmitInterrupt = isStreaming && hasInput && hasInterruptibleToolInProgress;
  const submitBlockedByTools = isStreaming && hasInput && !hasInterruptibleToolInProgress;

  const autoScrollDeps = [
    messages,
    streamingText,
    streamingThinking,
    activeToolCalls,
    streamingToolUses,
    pendingQuestion,
  ];

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    const text = input.trim();
    if (!text || pendingQuestion) return;
    setInput("");
    if (isStreaming) {
      if (canSubmitInterrupt) await submitInterrupt(text);
      return;
    }
    await sendMessage(text);
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void onSubmit(e as unknown as FormEvent);
    }
  }

  const anchor = questionAnchorIndex ?? messages.length;
  const messagesBeforeQuestion = pendingQuestion ? messages.slice(0, anchor) : messages;
  const messagesAfterQuestion = pendingQuestion ? messages.slice(anchor) : [];

  const showStreamingPreview =
    isStreaming && (streamingText || streamingThinking || streamingToolUses.length > 0);
  const activeIds = new Set(activeToolCalls.keys());
  const archivedToolIds = new Set(
    [...messagesBeforeQuestion, ...messagesAfterQuestion]
      .filter((m) => m.role === "tool")
      .map((m) => m.id.replace(/^tool-/, "")),
  );
  const streamingOnlyTools = streamingToolUses.filter(
    (t) => !activeIds.has(t.id) && !archivedToolIds.has(t.id),
  );
  const liveActiveTools = Array.from(activeToolCalls.values()).filter(
    (t) =>
      !archivedToolIds.has(t.id) &&
      (t.status === "pending" || t.status === "running"),
  );
  const pendingApprovalTools = liveActiveTools.filter((t) => t.status === "pending");
  const runningLiveTools = liveActiveTools.filter((t) => t.status === "running");

  return (
    <section className="chat-panel">
      <div className="dialog-viewport">
        <ScrollViewport
          className="message-list"
          autoScrollTo="bottom"
          autoScrollDeps={autoScrollDeps}
          overlayActive={overlayActive}
        >
          {messages.length === 0 && !showStreamingPreview && !pendingQuestion && (
            <p className="placeholder">发送消息开始与 {APP_DISPLAY_NAME} 对话…</p>
          )}
          {messagesBeforeQuestion.map((msg) => (
            <article
              key={msg.id}
              className={`message message-${msg.role}${isStreaming ? "" : ""}`}
            >
              <header>{messageHeader(msg)}</header>
              <ChatMessageBody
                msg={msg}
                approveTool={approveTool}
                denyTool={denyTool}
                onOpenFork={(forkRunId) => void openForkOverlay(forkRunId)}
              />
            </article>
          ))}

          {pendingQuestion && (
            <AskUserQuestionBlock
              pendingQuestion={pendingQuestion}
              questionSelections={questionSelections}
              questionCustomText={questionCustomText}
              questionError={questionError}
              isStreaming={isStreaming}
              toggleQuestionOption={toggleQuestionOption}
              setQuestionCustomText={setQuestionCustomText}
              answerQuestion={answerQuestion}
            />
          )}

          {messagesAfterQuestion.map((msg) => (
            <article
              key={msg.id}
              className={`message message-${msg.role}${isStreaming ? "" : ""}`}
            >
              <header>{messageHeader(msg)}</header>
              <ChatMessageBody
                msg={msg}
                approveTool={approveTool}
                denyTool={denyTool}
                onOpenFork={(forkRunId) => void openForkOverlay(forkRunId)}
              />
            </article>
          ))}

          {pendingApprovalTools.map((tool) => (
            <ToolUseCard
              key={tool.id}
              tool={tool}
              isStreaming={isStreaming}
              onApprove={(id) => void approveTool(id)}
              onDeny={(id, reason) => void denyTool(id, reason)}
            />
          ))}

          {showStreamingPreview && (
            <article className="message message-assistant message-streaming">
              <header>Agent {isStreaming && <span className="streaming-dot" aria-hidden />}</header>
              <div className="message-body">
                {streamingThinking && (
                  <details className="thinking-stream" open>
                    <summary>思考中…</summary>
                    <ReactMarkdown>{streamingThinking}</ReactMarkdown>
                  </details>
                )}
                {streamingText && (
                  <div className="streaming-text">
                    <ReactMarkdown>{streamingText}</ReactMarkdown>
                  </div>
                )}
              </div>
            </article>
          )}

          {streamingOnlyTools.map((tool) => (
            <ToolUseCard key={tool.id} tool={tool} isStreamingInput isStreaming={isStreaming} />
          ))}

          {runningLiveTools.map((tool) => (
            <ToolUseCard
              key={tool.id}
              tool={tool}
              isStreaming={isStreaming}
              onApprove={(id) => void approveTool(id)}
              onDeny={(id, reason) => void denyTool(id, reason)}
            />
          ))}
        </ScrollViewport>

        {filePreview && <FilePreviewOverlay preview={filePreview} />}
        {subAgentForkRun && onCloseSubAgent && (
          <SubAgentOverlay forkRun={subAgentForkRun} onClose={onCloseSubAgent} />
        )}
      </div>

      {hookForkBanner && (
        <div className="hook-fork-banner">
          <span>日志检查完成</span>
          <button
            type="button"
            onClick={() => {
              void openForkOverlay(hookForkBanner.forkRunId);
              dismissHookForkBanner();
            }}
          >
            查看详情
          </button>
          <button type="button" className="hook-fork-banner-dismiss" onClick={dismissHookForkBanner}>
            ✕
          </button>
        </div>
      )}

      <form className="input-box" onSubmit={onSubmit}>
        <textarea
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder={
            pendingQuestion
              ? "请先回答上方问题…"
              : isStreaming
                ? "可输入下一条指令（Ctrl+Enter 发送）…"
                : "输入创作指令… (Ctrl+Enter 发送)"
          }
          rows={3}
          disabled={!!pendingQuestion}
        />
        <div className="actions">
          <select
            className="mode-select"
            value={permissionMode}
            onChange={(e) => void onSetPermissionMode(e.target.value)}
            disabled={isStreaming || !!pendingQuestion}
            title={MODE_TOOLTIPS[permissionMode] ?? permissionMode}
          >
            <option value="normal">常规</option>
            <option value="plan">策划</option>
            <option value="auto">自动</option>
            <option value="unattended">无人值守</option>
          </select>
          {isStreaming && !hasInput && (
            <button type="button" className="interrupt-btn" onClick={() => void interrupt()}>
              中断
            </button>
          )}
          {isStreaming && hasInput && (
            <>
              <button type="button" className="interrupt-btn" onClick={() => void interrupt()}>
                中断
              </button>
              <button
                type="submit"
                disabled={!canSubmitInterrupt}
                title={
                  submitBlockedByTools
                    ? "当前工具执行中，请等待或使用「中断」"
                    : "发送并打断当前轮次"
                }
              >
                发送
              </button>
            </>
          )}
          {!isStreaming && (
            <button type="submit" disabled={!input.trim() || !!pendingQuestion}>
              发送
            </button>
          )}
        </div>
      </form>
    </section>
  );
}
