import { FormEvent, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ForkRunState } from "../../types/messages";
import { AskUserQuestionBlock } from "./AskUserQuestionBlock";
import { ChatInputBar } from "./ChatInputBar";
import { useAgentContext } from "../../context/AgentContext";
import { APP_DISPLAY_NAME } from "../../constants/app";
import { isSyntheticUser } from "../../transcript";
import { isContextRefreshUser } from "../../transcript/contextRefresh";
import {
  ScrollViewport,
  type ScrollViewportAutoScrollControl,
} from "../layout/ScrollViewport";
import { FilePreviewOverlay, type FilePreviewState } from "./FilePreviewOverlay";
import { TranscriptView } from "./TranscriptView";
import { CompactionBanner } from "./CompactionBanner";
import { useSlotVisibility } from "../../hooks/useSlotVisibility";
import { useTranscriptLoader } from "../../hooks/useTranscriptLoader";
import { useViewportContentFill } from "../../hooks/useViewportContentFill";
import type { TurnSlot } from "../../transcript/buildTurnSlots";
import { useCompactionProgress } from "../../hooks/useCompactionProgress";
import { SubAgentOverlay } from "./SubAgentOverlay";
import { SubAgentForkCard } from "./SubAgentForkCard";
import { listHookForkRuns } from "../../fork";
import "./ChatPanel.css";
import "./DialogOverlays.css";
import "./SubAgentForkCard.css";

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
  appTurnInProgress = false,
  sessionId,
  onSetPermissionMode,
  overlayActive = false,
  filePreview = null,
  subAgentForkRun,
  onCloseSubAgent,
  onTranscriptBootstrapError,
}: {
  permissionMode: string;
  appTurnInProgress?: boolean;
  sessionId?: string;
  onSetPermissionMode: (mode: string) => Promise<void>;
  overlayActive?: boolean;
  filePreview?: FilePreviewState | null;
  subAgentForkRun?: ForkRunState;
  onCloseSubAgent?: () => void;
  onTranscriptBootstrapError?: (message: string | null) => void;
}) {
  const {
    transcriptMachine,
    forkBindingMessages,
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
    dispatchTranscript,
    registerReloadActiveTail,
  } = useAgentContext();

  const turnSlotsRef = useRef<TurnSlot[]>([]);
  const isBottomAnchoredRef = useRef(false);
  const scheduleReconcileRef = useRef<(() => void) | null>(null);
  const compactionPausedRef = useRef(false);
  compactionPausedRef.current = isStreaming || turnInProgress || appTurnInProgress;

  const slotVisibility = useSlotVisibility(turnSlotsRef, () => {
    scheduleReconcileRef.current?.();
  });

  const scrollViewportRef = useRef<HTMLDivElement>(null);
  const contentUnderflowRef = useViewportContentFill(
    scrollViewportRef,
    isBottomAnchoredRef,
    () => scheduleReconcileRef.current?.(),
  );

  const transcriptLoader = useTranscriptLoader(sessionId, dispatchTranscript, {
    visibleSlotKeysRef: slotVisibility.visibleSlotKeysRef,
    envelopeRef: slotVisibility.envelopeRef,
    isBottomAnchoredRef,
    contentUnderflowRef,
    compactionPausedRef,
  });
  turnSlotsRef.current = transcriptLoader.turnSlots;
  scheduleReconcileRef.current = transcriptLoader.scheduleReconcile;

  useEffect(() => {
    onTranscriptBootstrapError?.(transcriptLoader.bootstrapError);
  }, [onTranscriptBootstrapError, transcriptLoader.bootstrapError]);

  useEffect(
    () => registerReloadActiveTail(transcriptLoader.reloadActiveTail),
    [registerReloadActiveTail, transcriptLoader.reloadActiveTail],
  );
  const compaction = useCompactionProgress();
  const [input, setInput] = useState("");
  const [stickyPrompt, setStickyPrompt] = useState<string | null>(null);
  const [stickyDismissed, setStickyDismissed] = useState(false);
  const [userAnchorVersion, setUserAnchorVersion] = useState(0);
  const turnAnchorRef = useRef<HTMLDivElement>(null);
  const userMsgRef = useRef<HTMLElement | null>(null);
  const autoScrollControlRef = useRef<ScrollViewportAutoScrollControl | null>(null);
  const suspendAutoScrollRef = useRef(false);

  const lastUserMsg = [...transcriptMachine.context.turns]
    .reverse()
    .find((t) => !isSyntheticUser(t.user) && !isContextRefreshUser(t.user))?.user;
  const modeSwitchBlocked = turnInProgress || appTurnInProgress;
  const lastUserMsgText = (() => {
    const raw = lastUserMsg?.contentBlocks?.[0]?.text ?? "";
    return raw.trim().slice(0, 200).replace(/\s+/g, " ").trim();
  })();

  const applyTurnFoldMinHeight =
    transcriptMachine.phase === "idle" &&
    !isStreaming &&
    !turnInProgress &&
    !appTurnInProgress;

  useLayoutEffect(() => {
    const viewport = scrollViewportRef.current;
    const anchor = turnAnchorRef.current;
    if (!viewport || !anchor) {
      anchor?.style.removeProperty("min-height");
      return;
    }
    const applyMinHeight = () => {
      if (!applyTurnFoldMinHeight) {
        anchor.style.removeProperty("min-height");
        return;
      }
      anchor.style.minHeight = `${viewport.clientHeight}px`;
    };
    applyMinHeight();
    const observer = new ResizeObserver(applyMinHeight);
    observer.observe(viewport);
    return () => {
      observer.disconnect();
      anchor?.style.removeProperty("min-height");
    };
  }, [
    applyTurnFoldMinHeight,
    transcriptMachine,
    transcriptLoader.turnSlots,
    pendingQuestion,
    forkRuns,
    isStreaming,
    turnInProgress,
    appTurnInProgress,
  ]);

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
        setStickyDismissed(true);
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

    autoScrollControlRef.current?.unpin();
    suspendAutoScrollRef.current = true;

    const releaseAutoScroll = () => {
      suspendAutoScrollRef.current = false;
    };

    const top =
      el.getBoundingClientRect().top -
      root.getBoundingClientRect().top +
      root.scrollTop;
    root.scrollTo({ top: Math.max(0, top), behavior: "smooth" });

    if ("onscrollend" in root) {
      const onScrollEnd = () => {
        root.removeEventListener("scrollend", onScrollEnd);
        releaseAutoScroll();
      };
      root.addEventListener("scrollend", onScrollEnd, { once: true });
    } else {
      window.setTimeout(releaseAutoScroll, 400);
    }
  }, []);

  const hasInput = input.trim().length > 0;
  const canSubmitInterrupt = isStreaming && hasInput && hasInterruptibleToolInProgress;
  const submitBlockedByTools = isStreaming && hasInput && !hasInterruptibleToolInProgress;

  const autoScrollDeps = [
    transcriptMachine,
    transcriptLoader.isBootstrapping ? null : transcriptLoader.turnSlots,
    transcriptLoader.isBootstrapping ? null : transcriptLoader.layout,
    pendingQuestion,
    forkRuns,
    isStreaming,
    turnInProgress,
    lastUserMsg?.id,
    transcriptLoader.isBootstrapping,
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
    autoScrollControlRef.current?.pinAndScrollToBottom();
  }

  const anchorUserId = lastUserMsg?.id;
  const anchorUserRef = useCallback((el: HTMLElement | null) => {
    userMsgRef.current = el;
  }, []);

  useLayoutEffect(() => {
    setUserAnchorVersion((v) => v + 1);
  }, [anchorUserId]);

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
    transcriptLoader.isBootstrapping ||
    transcriptLoader.turnSlots.length > 0 ||
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
          resetScrollKey={sessionId}
          overlayActive={overlayActive}
          autoScrollControlRef={autoScrollControlRef}
          suspendAutoScrollRef={suspendAutoScrollRef}
          onBottomAnchorChange={transcriptLoader.onBottomAnchorChange}
        >
          {!hasTranscript && !pendingQuestion && (
            <p className="placeholder">发送消息开始与 {APP_DISPLAY_NAME} 对话…</p>
          )}
          <TranscriptView
            machine={transcriptMachine}
            layout={transcriptLoader.layout}
            turnSlots={transcriptLoader.turnSlots}
            mode="main"
            pendingQuestion={pendingQuestion}
            questionSlot={questionSlot}
            forkRuns={forkRuns}
            forkBindingMessages={forkBindingMessages}
            onApproveTool={(id) => void approveTool(id)}
            onDenyTool={(id, reason) => void denyTool(id, reason)}
            onOpenForkOverlay={(id) => void openForkOverlay(id)}
            anchorUserId={anchorUserId}
            anchorUserRef={anchorUserRef}
            turnAnchorRef={turnAnchorRef}
            scrollRootRef={scrollViewportRef}
            onLoadTurn={transcriptLoader.onLoadTurn}
            setSlotVisibility={slotVisibility.setSlotVisibility}
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

      <ChatInputBar
        input={input}
        setInput={setInput}
        onSubmit={onSubmit}
        pendingQuestion={!!pendingQuestion}
        isStreaming={isStreaming}
        hasInput={hasInput}
        canSubmitInterrupt={canSubmitInterrupt}
        submitBlockedByTools={submitBlockedByTools}
        permissionMode={permissionMode}
        modeSwitchBlocked={modeSwitchBlocked}
        onSetPermissionMode={onSetPermissionMode}
        model={model}
        setModel={setModel}
        turnInProgress={turnInProgress}
        onInterrupt={() => void interrupt()}
      />
    </section>
  );
}
