import { useMemo } from "react";
import type { ForkRunState, PendingQuestion, UIMessage } from "../../hooks/useAgent";
import type { TranscriptMachine, Turn, LlmSegment } from "../../transcript/types";
import { flatMessagesToMachine } from "../../transcript/machine";
import { isSyntheticUser } from "../../transcript";
import { pauseSegmentId } from "../../transcript/selectors";
import { CompactionDivider } from "./CompactionDivider";
import { SegmentGroup, UserBubble } from "./segmentRender";
import type { ForkLink } from "../../utils/forkLinks";
import "./ChatPanel.css";

export interface ArchivedEpochView {
  epoch: number;
  flatMessages: UIMessage[];
}

function ArchivedEpochTranscript({
  epoch,
  flatMessages,
  forkRuns,
  forkLinks,
}: {
  epoch: number;
  flatMessages: UIMessage[];
  forkRuns?: Map<string, ForkRunState>;
  forkLinks?: Map<string, ForkLink>;
}) {
  const machine = useMemo(
    () => flatMessagesToMachine(flatMessages).machine,
    [flatMessages],
  );
  return (
    <div className="transcript-archived" data-compaction-epoch={epoch}>
      {machine.context.turns.map((turn: Turn) => (
        <div key={turn.turnId} className="transcript-turn">
          {!isSyntheticUser(turn.user) && <UserBubble user={turn.user} />}
          {turn.segments.map((seg: LlmSegment) => (
            <SegmentGroup
              key={seg.segmentId}
              segment={seg}
              variant="committed"
              flatMessages={flatMessages}
              forkRuns={forkRuns}
              forkLinks={forkLinks}
            />
          ))}
        </div>
      ))}
    </div>
  );
}

export function TranscriptView({
  machine,
  archivedEpochs = [],
  mode,
  pendingQuestion,
  questionSlot,
  forkRuns,
  forkLinks,
  flatMessages,
  onApproveTool,
  onDenyTool,
  onOpenForkOverlay,
  renderUserRef,
  isStreaming = false,
}: {
  machine: TranscriptMachine;
  archivedEpochs?: ArchivedEpochView[];
  mode: "main" | "fork";
  pendingQuestion?: PendingQuestion | null;
  questionSlot?: React.ReactNode;
  forkRuns?: Map<string, ForkRunState>;
  forkLinks?: Map<string, ForkLink>;
  flatMessages: UIMessage[];
  onApproveTool?: (id: string) => void;
  onDenyTool?: (id: string, reason?: string) => void;
  onOpenForkOverlay?: (forkRunId: string) => void;
  renderUserRef?: (user: UIMessage, el: HTMLElement | null) => void;
  isStreaming?: boolean;
}) {
  const pauseId = pendingQuestion ? pauseSegmentId(machine) : undefined;
  const { turns, openSegment } = machine.context;
  const hasContent =
    archivedEpochs.length > 0 ||
    turns.length > 0 ||
    openSegment !== null ||
    machine.phase === "segmentStreaming";

  if (!hasContent && mode === "fork") {
    return null;
  }

  return (
    <>
      {archivedEpochs.map((arch) => (
        <div key={`archive-${arch.epoch}`}>
          <ArchivedEpochTranscript
            epoch={arch.epoch}
            flatMessages={arch.flatMessages}
            forkRuns={forkRuns}
            forkLinks={forkLinks}
          />
          <CompactionDivider epoch={arch.epoch} />
        </div>
      ))}

      {turns.map((turn) => (
        <div key={turn.turnId} className="transcript-turn">
          {mode === "main" && !isSyntheticUser(turn.user) && (
            <UserBubble
              user={turn.user}
              renderRef={renderUserRef ? (el) => renderUserRef(turn.user, el) : undefined}
            />
          )}
          {turn.segments.map((seg) => (
            <div key={seg.segmentId}>
              <SegmentGroup
                segment={seg}
                variant="committed"
                flatMessages={flatMessages}
                forkRuns={forkRuns}
                forkLinks={forkLinks}
                onApproveTool={onApproveTool}
                onDenyTool={onDenyTool}
                onOpenForkOverlay={onOpenForkOverlay}
              />
              {pauseId === seg.segmentId && questionSlot}
            </div>
          ))}
        </div>
      ))}

      {machine.phase === "segmentStreaming" && openSegment && (
        <SegmentGroup
          segment={openSegment}
          variant="live"
          isStreaming={isStreaming}
          flatMessages={flatMessages}
          forkRuns={forkRuns}
          forkLinks={forkLinks}
          onApproveTool={onApproveTool}
          onDenyTool={onDenyTool}
          onOpenForkOverlay={onOpenForkOverlay}
        />
      )}

      {pauseId && !turns.some((t) => t.segments.some((s) => s.segmentId === pauseId)) && questionSlot}
    </>
  );
}
