import { useEffect, useRef } from "react";
import type { ForkRunState, PendingQuestion, UIMessage } from "../../types/messages";
import type { TurnSlot } from "../../transcript/buildTurnSlots";
import type { SessionTranscriptLayout } from "../../transcript/service";
import { findLiveTailTurn, shouldSkipMaxTurnSlot } from "../../transcript/liveTail";
import { findTurnInMachine } from "../../transcript/merge";
import type { TranscriptMachine, Turn } from "../../transcript/types";
import { isSyntheticUser } from "../../transcript";
import { pauseSegmentId } from "../../transcript/selectors";
import { INTERSECTION_ROOT_MARGIN, TURN_PLACEHOLDER_MIN_HEIGHT } from "../../transcript/loadPolicy";
import { shouldRenderSlotInTimeline } from "../../transcript/turnLoadPlan";
import { useIntersectionLoad } from "../../hooks/useIntersectionLoad";
import { CompactionDivider } from "./CompactionDivider";
import { SegmentGroup, UserBubble } from "./segmentRender";
import "./ChatPanel.css";

function LazyTurnSlot({
  slot,
  scrollRootRef,
  onLoadTurn,
}: {
  slot: TurnSlot;
  scrollRootRef?: React.RefObject<HTMLElement | null>;
  onLoadTurn: (slotKey: string) => void;
}) {
  const placeholderRef = useRef<HTMLDivElement>(null);
  useIntersectionLoad({
    enabled: slot.status === "idle",
    targetRef: placeholderRef,
    scrollRootRef,
    onIntersect: () => onLoadTurn(slot.slotKey),
  });

  if (slot.status === "loaded") {
    return null;
  }

  return (
    <div
      ref={placeholderRef}
      className="transcript-turn-placeholder"
      data-turn-slot={slot.slotKey}
      style={{ minHeight: TURN_PLACEHOLDER_MIN_HEIGHT }}
      aria-busy={slot.status === "loading"}
    >
      {slot.status === "loading" && <span className="archive-loading">加载历史…</span>}
      {slot.status === "error" && (
        <span className="archive-error">{slot.errorMessage ?? "加载失败"}</span>
      )}
    </div>
  );
}

function TurnSegments({
  turn,
  mode,
  forkBindingMessages,
  forkRuns,
  pauseId,
  questionSlot,
  onApproveTool,
  onDenyTool,
  onOpenForkOverlay,
  anchorUserId,
  anchorUserRef,
}: {
  turn: Turn;
  mode: "main" | "fork";
  forkBindingMessages: UIMessage[];
  forkRuns?: Map<string, ForkRunState>;
  pauseId?: string;
  questionSlot?: React.ReactNode;
  onApproveTool?: (id: string) => void;
  onDenyTool?: (id: string, reason?: string) => void;
  onOpenForkOverlay?: (forkRunId: string) => void;
  anchorUserId?: string;
  anchorUserRef?: (el: HTMLElement | null) => void;
}) {
  const userRef =
    mode === "main" && anchorUserId === turn.user.id ? anchorUserRef : undefined;
  return (
    <>
      {mode === "main" && !isSyntheticUser(turn.user) && (
        <UserBubble user={turn.user} renderRef={userRef} />
      )}
      {turn.segments.map((seg) => (
        <div key={seg.segmentId}>
          <SegmentGroup
            segment={seg}
            variant="committed"
            forkBindingMessages={forkBindingMessages}
            forkRuns={forkRuns}
            onApproveTool={onApproveTool}
            onDenyTool={onDenyTool}
            onOpenForkOverlay={onOpenForkOverlay}
          />
          {pauseId === seg.segmentId && questionSlot}
        </div>
      ))}
    </>
  );
}

function useLoadedSlotVisibility(
  blockRef: React.MutableRefObject<HTMLDivElement | null>,
  slotKey: string,
  scrollRootRef: React.RefObject<HTMLElement | null> | undefined,
  setSlotVisibility: ((slotKey: string, visible: boolean) => void) | undefined,
) {
  useEffect(() => {
    if (!setSlotVisibility) return;
    const target = blockRef.current;
    if (!target) return;
    if (typeof IntersectionObserver === "undefined") return;

    const root = scrollRootRef?.current ?? null;
    const observer = new IntersectionObserver(
      (entries) => {
        setSlotVisibility(slotKey, entries.some((e) => e.isIntersecting));
      },
      { root, rootMargin: INTERSECTION_ROOT_MARGIN },
    );
    observer.observe(target);
    return () => {
      setSlotVisibility(slotKey, false);
      observer.disconnect();
    };
  }, [blockRef, slotKey, scrollRootRef, setSlotVisibility]);
}

function LoadedTurnBlock({
  slotKey,
  turn,
  mode,
  forkBindingMessages,
  forkRuns,
  pauseId,
  questionSlot,
  onApproveTool,
  onDenyTool,
  onOpenForkOverlay,
  anchorUserId,
  anchorUserRef,
  scrollRootRef,
  setSlotVisibility,
}: {
  slotKey: string;
  turn: Turn;
  mode: "main" | "fork";
  forkBindingMessages: UIMessage[];
  forkRuns?: Map<string, ForkRunState>;
  pauseId?: string;
  questionSlot?: React.ReactNode;
  onApproveTool?: (id: string) => void;
  onDenyTool?: (id: string, reason?: string) => void;
  onOpenForkOverlay?: (forkRunId: string) => void;
  anchorUserId?: string;
  anchorUserRef?: (el: HTMLElement | null) => void;
  scrollRootRef?: React.RefObject<HTMLElement | null>;
  setSlotVisibility?: (slotKey: string, visible: boolean) => void;
}) {
  const blockRef = useRef<HTMLDivElement | null>(null);
  useLoadedSlotVisibility(blockRef, slotKey, scrollRootRef, setSlotVisibility);

  return (
    <div
      key={turn.turnId}
      ref={blockRef}
      className="transcript-turn"
      data-turn-slot={slotKey}
    >
      <TurnSegments
        turn={turn}
        mode={mode}
        forkBindingMessages={forkBindingMessages}
        forkRuns={forkRuns}
        pauseId={pauseId}
        questionSlot={questionSlot}
        onApproveTool={onApproveTool}
        onDenyTool={onDenyTool}
        onOpenForkOverlay={onOpenForkOverlay}
        anchorUserId={anchorUserId}
        anchorUserRef={anchorUserRef}
      />
    </div>
  );
}

export function TranscriptView({
  machine,
  layout = null,
  turnSlots = [],
  mode,
  pendingQuestion,
  questionSlot,
  forkRuns,
  forkBindingMessages,
  onApproveTool,
  onDenyTool,
  onOpenForkOverlay,
  anchorUserId,
  anchorUserRef,
  turnAnchorRef,
  scrollRootRef,
  onLoadTurn,
  setSlotVisibility,
  isStreaming = false,
}: {
  machine: TranscriptMachine;
  layout?: SessionTranscriptLayout | null;
  turnSlots?: TurnSlot[];
  mode: "main" | "fork";
  pendingQuestion?: PendingQuestion | null;
  questionSlot?: React.ReactNode;
  forkRuns?: Map<string, ForkRunState>;
  forkBindingMessages: UIMessage[];
  onApproveTool?: (id: string) => void;
  onDenyTool?: (id: string, reason?: string) => void;
  onOpenForkOverlay?: (forkRunId: string) => void;
  anchorUserId?: string;
  anchorUserRef?: (el: HTMLElement | null) => void;
  turnAnchorRef?: React.Ref<HTMLDivElement>;
  scrollRootRef?: React.RefObject<HTMLElement | null>;
  onLoadTurn?: (slotKey: string) => void;
  setSlotVisibility?: (slotKey: string, visible: boolean) => void;
  isStreaming?: boolean;
}) {
  const pauseId = pendingQuestion ? pauseSegmentId(machine) : undefined;
  const { turns, openSegment } = machine.context;
  const maxTurn = layout?.active.maxTurn ?? 0;
  const liveTail = findLiveTailTurn(machine);
  const useLiveTail = mode === "main" && (!!liveTail || machine.phase === "segmentStreaming");
  const skipMaxTurnSlot = shouldSkipMaxTurnSlot(machine, layout, liveTail, useLiveTail);

  const hasContent =
    turnSlots.length > 0 ||
    turns.length > 0 ||
    openSegment !== null ||
    machine.phase === "segmentStreaming";

  if (!hasContent && mode === "fork") {
    return null;
  }

  const pauseSegmentMounted =
    !!pauseId && turns.some((t) => t.segments.some((s) => s.segmentId === pauseId));
  const endQuestionSlot =
    pendingQuestion && questionSlot && !pauseSegmentMounted ? questionSlot : null;

  const renderTimeline = () => {
    if (mode === "fork") {
      const lastTurnIndex = turns.length - 1;
      const committedTurns = turns.slice(0, lastTurnIndex);
      const lastTurn = lastTurnIndex >= 0 ? turns[lastTurnIndex] : null;
      return (
        <>
          {committedTurns.map((turn) => (
            <div key={turn.turnId} className="transcript-turn">
              <TurnSegments
                turn={turn}
                mode={mode}
                forkBindingMessages={forkBindingMessages}
                forkRuns={forkRuns}
                pauseId={pauseId}
                questionSlot={questionSlot}
                onApproveTool={onApproveTool}
                onDenyTool={onDenyTool}
                onOpenForkOverlay={onOpenForkOverlay}
              />
            </div>
          ))}
          {lastTurn && (
            <div key={lastTurn.turnId} className="transcript-turn">
              <TurnSegments
                turn={lastTurn}
                mode={mode}
                forkBindingMessages={forkBindingMessages}
                forkRuns={forkRuns}
                pauseId={pauseId}
                questionSlot={questionSlot}
                onApproveTool={onApproveTool}
                onDenyTool={onDenyTool}
                onOpenForkOverlay={onOpenForkOverlay}
              />
              {machine.phase === "segmentStreaming" && openSegment && (
                <SegmentGroup
                  segment={openSegment}
                  variant="live"
                  isStreaming={isStreaming}
                  forkBindingMessages={forkBindingMessages}
                  forkRuns={forkRuns}
                  onApproveTool={onApproveTool}
                  onDenyTool={onDenyTool}
                  onOpenForkOverlay={onOpenForkOverlay}
                />
              )}
              {endQuestionSlot}
            </div>
          )}
        </>
      );
    }

    const nodes: React.ReactNode[] = [];
    let lastArchiveEpoch: number | undefined;

    const closeArchiveEpoch = () => {
      if (lastArchiveEpoch === undefined) return;
      nodes.push(<CompactionDivider key={`divider-${lastArchiveEpoch}`} epoch={lastArchiveEpoch} />);
      lastArchiveEpoch = undefined;
    };

    for (const slot of turnSlots) {
      if (!shouldRenderSlotInTimeline(slot, turnSlots)) {
        continue;
      }

      if (
        skipMaxTurnSlot &&
        slot.kind === "active" &&
        slot.turnNumber === maxTurn
      ) {
        continue;
      }

      if (slot.kind === "archive") {
        if (slot.epoch !== lastArchiveEpoch) {
          closeArchiveEpoch();
          lastArchiveEpoch = slot.epoch;
        }
      } else {
        closeArchiveEpoch();
      }

      const loadedTurn = findTurnInMachine(
        machine,
        slot.turnNumber,
        slot.kind === "archive" ? slot.epoch : undefined,
      );

      if (loadedTurn) {
        nodes.push(
          <LoadedTurnBlock
            key={slot.slotKey}
            slotKey={slot.slotKey}
            turn={loadedTurn}
            mode={mode}
            forkBindingMessages={forkBindingMessages}
            forkRuns={forkRuns}
            pauseId={pauseId}
            questionSlot={questionSlot}
            onApproveTool={onApproveTool}
            onDenyTool={onDenyTool}
            onOpenForkOverlay={onOpenForkOverlay}
            scrollRootRef={scrollRootRef}
            setSlotVisibility={setSlotVisibility}
          />,
        );
      } else {
        nodes.push(
          <LazyTurnSlot
            key={slot.slotKey}
            slot={slot}
            scrollRootRef={scrollRootRef}
            onLoadTurn={onLoadTurn ?? (() => {})}
          />,
        );
      }
    }

    closeArchiveEpoch();

    return <>{nodes}</>;
  };

  return (
    <>
      {renderTimeline()}

      {useLiveTail && (
        <div
          ref={mode === "main" ? turnAnchorRef : undefined}
          className={mode === "main" ? "transcript-turn transcript-turn-anchor" : "transcript-turn"}
        >
          {liveTail && (
            <TurnSegments
              turn={liveTail}
              mode={mode}
              forkBindingMessages={forkBindingMessages}
              forkRuns={forkRuns}
              pauseId={pauseId}
              questionSlot={questionSlot}
              onApproveTool={onApproveTool}
              onDenyTool={onDenyTool}
              onOpenForkOverlay={onOpenForkOverlay}
              anchorUserId={anchorUserId}
              anchorUserRef={anchorUserRef}
            />
          )}

          {machine.phase === "segmentStreaming" && openSegment && (
            <SegmentGroup
              segment={openSegment}
              variant="live"
              isStreaming={isStreaming}
              forkBindingMessages={forkBindingMessages}
              forkRuns={forkRuns}
              onApproveTool={onApproveTool}
              onDenyTool={onDenyTool}
              onOpenForkOverlay={onOpenForkOverlay}
            />
          )}

          {endQuestionSlot}
        </div>
      )}

      {!useLiveTail && endQuestionSlot}
    </>
  );
}
