import { FormEvent, useCallback, useRef, useState } from "react";
import type { ForkRunState, PendingQuestion } from "../../hooks/useAgent";
import { useAgentContext } from "../../context/AgentContext";
import { APP_DISPLAY_NAME } from "../../constants/app";
import { isSyntheticUser } from "../../transcript";
import { ScrollViewport } from "../layout/ScrollViewport";
import { FilePreviewOverlay, type FilePreviewState } from "./FilePreviewOverlay";
import { TranscriptView } from "./TranscriptView";
import { CompactionBanner } from "./CompactionBanner";
import { useCompactionProgress } from "../../hooks/useCompactionProgress";
import { SubAgentOverlay } from "./SubAgentOverlay";
import { SubAgentForkCard } from "./SubAgentForkCard";
import { agentLabelFromType, listHookForkRuns } from "../../fork";
import "./ChatPanel.css";
import "./DialogOverlays.css";
import "./SubAgentForkCard.css";

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

function HookForkCards({
  forkRuns,
  openForkOverlay,
}: {
  forkRuns: Map<string, ForkRunState>;
  openForkOverlay: (forkRunId: string) => void;
}) {
  const hookRuns = listHookForkRuns(forkRuns);
  if (hookRuns.length === 0) return null;
  return (
    <>
      {hookRuns.map((run) => (
        <article key={run.forkRunId} className="message message-tool">
          <header>Subagent · {agentLabelFromType(run.agentType)}</header>
          <SubAgentForkCard
            mode="hook"
            agentType={run.agentType}
            summary={run.taskPreview}
            status={run.status === "running" ? "running" : "complete"}
            reportContent={run.reportOutput}
            onEnter={() => void openForkOverlay(run.forkRunId)}
          />
        </article>
      ))}
    </>
  );
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
  overlayActive?: boolean;
  filePreview?: FilePreviewState | null;
  subAgentForkRun?: ForkRunState;
  onCloseSubAgent?: () => void;
}) {
  const {
    transcriptMachine,
    archivedEpochs,
    flatMessages,
    isStreaming,
    pendingQuestion,
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
    model,
    setModel,
    forkRuns,
    turnInProgress,
  } = useAgentContext();

  const compaction = useCompactionProgress();
  const [input, setInput] = useState("");
  const [stickyPrompt, setStickyPrompt] = useState<string | null>(null);
  const userMsgRef = useRef<HTMLElement | null>(null);


  const lastUserMsg = [...transcriptMachine.context.turns]
    .reverse()
    .find((t) => !isSyntheticUser(t.user))?.user;
  const lastUserMsgText =
    lastUserMsg?.contentBlocks?.[0]?.text?.slice(0, 200) ?? "";

  const onScrollPosition = useCallback(
    (scrollTop: number) => {
      const el = userMsgRef.current;
      if (!el) return;
      if (scrollTop > el.offsetTop + el.offsetHeight + 8) {
        if (!stickyPrompt) setStickyPrompt(lastUserMsgText);
      } else {
        if (stickyPrompt) setStickyPrompt(null);
      }
    },
    [lastUserMsgText, stickyPrompt],
  );

  const scrollToUserMessage = useCallback(() => {
    userMsgRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
    setStickyPrompt(null);
  }, []);

  const hasInput = input.trim().length > 0;
  const canSubmitInterrupt = isStreaming && hasInput && hasInterruptibleToolInProgress;
  const submitBlockedByTools = isStreaming && hasInput && !hasInterruptibleToolInProgress;

  const autoScrollDeps = [transcriptMachine, pendingQuestion, forkRuns];

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

  const renderUserRef = useCallback(
    (user: (typeof flatMessages)[0], el: HTMLElement | null) => {
      if (user === lastUserMsg) userMsgRef.current = el;
    },
    [lastUserMsg],
  );

  const questionSlot = pendingQuestion ? (
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
  ) : null;

  const hasTranscript =
    archivedEpochs.length > 0 ||
    transcriptMachine.context.turns.length > 0 ||
    transcriptMachine.context.openSegment !== null;

  return (
    <section className="chat-panel">
      <div className="dialog-viewport">
        <CompactionBanner state={compaction} />
        {stickyPrompt && (
          <button
            type="button"
            className="sticky-prompt-header"
            onClick={scrollToUserMessage}
            title="点击回到本轮对话起点"
          >
            {stickyPrompt}
          </button>
        )}
        <ScrollViewport
          className="message-list"
          autoScrollTo="bottom"
          autoScrollDeps={autoScrollDeps}
          overlayActive={overlayActive}
          onScrollPositionChange={onScrollPosition}
        >
          {!hasTranscript && !pendingQuestion && (
            <p className="placeholder">发送消息开始与 {APP_DISPLAY_NAME} 对话…</p>
          )}
          <TranscriptView
            machine={transcriptMachine}
            archivedEpochs={archivedEpochs}
            mode="main"
            pendingQuestion={pendingQuestion}
            questionSlot={questionSlot}
            forkRuns={forkRuns}
            flatMessages={flatMessages}
            onApproveTool={(id) => void approveTool(id)}
            onDenyTool={(id, reason) => void denyTool(id, reason)}
            onOpenForkOverlay={(id) => void openForkOverlay(id)}
            renderUserRef={renderUserRef}
            isStreaming={isStreaming}
          />

          <HookForkCards forkRuns={forkRuns} openForkOverlay={(id) => void openForkOverlay(id)} />
        </ScrollViewport>

        {filePreview && <FilePreviewOverlay preview={filePreview} />}
        {subAgentForkRun && onCloseSubAgent && (
          <SubAgentOverlay forkRun={subAgentForkRun} onClose={onCloseSubAgent} />
        )}
      </div>

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
            disabled={turnInProgress}
            title={MODE_TOOLTIPS[permissionMode] ?? permissionMode}
          >
            <option value="normal">常规</option>
            <option value="plan">策划</option>
            <option value="auto">自动</option>
            <option value="unattended">无人值守</option>
          </select>
          <select
            className="model-select"
            value={model}
            onChange={(e) => setModel(e.target.value)}
            disabled={turnInProgress}
            title={
              turnInProgress
                ? "当前轮次进行中，结束后才可切换模型"
                : "选择模型（切换模型将导致 KV Cache 失效）"
            }
          >
            <option value="deepseek-v4-pro">v4-pro</option>
            <option value="deepseek-v4-flash">v4-flash</option>
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
