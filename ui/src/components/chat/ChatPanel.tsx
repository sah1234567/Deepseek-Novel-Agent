import { FormEvent, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ForkRunState, PendingQuestion } from "../../hooks/useAgent";
import { useAgentContext } from "../../context/AgentContext";
import { APP_DISPLAY_NAME } from "../../constants/app";
import { isSyntheticUser } from "../../transcript";
import { isContextRefreshUser } from "../../transcript/types";
import { ScrollViewport } from "../layout/ScrollViewport";
import { FilePreviewOverlay, type FilePreviewState } from "./FilePreviewOverlay";
import { TranscriptView } from "./TranscriptView";
import { CompactionBanner } from "./CompactionBanner";
import { useCompactionProgress } from "../../hooks/useCompactionProgress";
import { SubAgentOverlay } from "./SubAgentOverlay";
import { SubAgentForkCard } from "./SubAgentForkCard";
import { listHookForkRuns } from "../../fork";
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
        <SubAgentForkCard
          key={run.forkRunId}
          mode="hook"
          agentType={run.agentType}
          summary={run.taskPreview}
          status={run.status === "running" ? "running" : "complete"}
          reportContent={run.reportOutput}
          onEnter={() => void openForkOverlay(run.forkRunId)}
        />
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
  const [stickyDismissed, setStickyDismissed] = useState(false);
  const [userAnchorVersion, setUserAnchorVersion] = useState(0);
  const scrollViewportRef = useRef<HTMLDivElement>(null);
  const turnAnchorRef = useRef<HTMLDivElement>(null);
  const userMsgRef = useRef<HTMLElement | null>(null);

  const lastUserMsg = [...transcriptMachine.context.turns]
    .reverse()
    .find((t) => !isSyntheticUser(t.user) && !isContextRefreshUser(t.user))?.user;
  const lastUserMsgText = (() => {
    const raw = lastUserMsg?.contentBlocks?.[0]?.text ?? "";
    const trimmed = raw.trimStart();
    const paraEnd = trimmed.search(/\n\s*\n/);
    const collapsed =
      paraEnd >= 0 ? trimmed.slice(0, paraEnd) : trimmed;
    return collapsed.slice(0, 200).replace(/\s+/g, " ").trim();
  })();

  useLayoutEffect(() => {
    const viewport = scrollViewportRef.current;
    const anchor = turnAnchorRef.current;
    if (!viewport || !anchor) {
      anchor?.style.removeProperty("min-height");
      return;
    }
    const applyMinHeight = () => {
      anchor.style.minHeight = `${viewport.clientHeight}px`;
    };
    applyMinHeight();
    const observer = new ResizeObserver(applyMinHeight);
    observer.observe(viewport);
    return () => {
      observer.disconnect();
      anchor?.style.removeProperty("min-height");
    };
  }, [transcriptMachine, archivedEpochs, pendingQuestion, forkRuns]);

  useLayoutEffect(() => {
    setStickyDismissed(false);
  }, [lastUserMsg]);

  useEffect(() => {
    const root = scrollViewportRef.current;
    const target = userMsgRef.current;
    if (!root || !target || !lastUserMsgText) {
      setStickyPrompt(null);
      return;
    }

    const updateSticky = () => {
      const rootRect = root.getBoundingClientRect();
      const targetRect = target.getBoundingClientRect();
      // User bubble fully scrolled above the viewport — show sticky even when pinned to bottom.
      const scrolledPast = targetRect.bottom < rootRect.top + 4;
      const userVisible =
        targetRect.bottom > rootRect.top && targetRect.top < rootRect.bottom;
      if (scrolledPast && !stickyDismissed) {
        setStickyPrompt(lastUserMsgText);
      } else if (userVisible) {
        setStickyPrompt(null);
      }
    };

    const observer = new IntersectionObserver(() => updateSticky(), {
      root,
      threshold: 0,
    });

    observer.observe(target);
    root.addEventListener("scroll", updateSticky, { passive: true });
    const resizeObserver = new ResizeObserver(() => updateSticky());
    resizeObserver.observe(root);
    resizeObserver.observe(target);
    updateSticky();
    return () => {
      observer.disconnect();
      resizeObserver.disconnect();
      root.removeEventListener("scroll", updateSticky);
    };
  }, [lastUserMsg, lastUserMsgText, stickyDismissed, userAnchorVersion]);

  const scrollToUserMessage = useCallback(() => {
    const root = scrollViewportRef.current;
    const el = userMsgRef.current;
    if (!root || !el) return;
    setStickyDismissed(true);
    setStickyPrompt(null);
    const top =
      el.getBoundingClientRect().top -
      root.getBoundingClientRect().top +
      root.scrollTop;
    root.scrollTo({ top: Math.max(0, top), behavior: "smooth" });
  }, []);

  const hasInput = input.trim().length > 0;
  const canSubmitInterrupt = isStreaming && hasInput && hasInterruptibleToolInProgress;
  const submitBlockedByTools = isStreaming && hasInput && !hasInterruptibleToolInProgress;

  const autoScrollDeps = [transcriptMachine, archivedEpochs, pendingQuestion, forkRuns, isStreaming];

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
      if (user !== lastUserMsg) return;
      userMsgRef.current = el;
      if (el) setUserAnchorVersion((v) => v + 1);
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
          ref={scrollViewportRef}
          className={
            stickyPrompt || stickyDismissed
              ? "message-list message-list--sticky-active"
              : "message-list"
          }
          autoScrollTo="bottom"
          autoScrollDeps={autoScrollDeps}
          overlayActive={overlayActive}
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
            turnAnchorRef={turnAnchorRef}
            isStreaming={isStreaming}
          />

          <HookForkCards forkRuns={forkRuns} openForkOverlay={(id) => void openForkOverlay(id)} />
        </ScrollViewport>

        {filePreview && <FilePreviewOverlay preview={filePreview} />}
        {subAgentForkRun && onCloseSubAgent && (
          <SubAgentOverlay
            forkRun={subAgentForkRun}
            onClose={onCloseSubAgent}
            forkRuns={forkRuns}
            onApproveTool={(id) => void approveTool(id)}
            onDenyTool={(id, reason) => void denyTool(id, reason)}
            onOpenForkOverlay={(id) => void openForkOverlay(id)}
          />
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
