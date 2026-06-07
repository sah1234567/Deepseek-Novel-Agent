import type { ForkRunState } from "../../types/messages";
import { flatMessagesFromMachine } from "../../transcript";
import { agentLabelFromType } from "../../fork";
import { ScrollViewport } from "../layout/ScrollViewport";
import { TranscriptView } from "./TranscriptView";
import "./DialogOverlays.css";
import "./SubAgentOverlay.css";
import "./ChatPanel.css";

interface SubAgentOverlayProps {
  forkRun: ForkRunState | undefined;
  onClose: () => void;
  forkRuns: Map<string, ForkRunState>;
  onApproveTool?: (id: string) => void;
  onDenyTool?: (id: string, reason?: string) => void;
  onOpenForkOverlay?: (forkRunId: string) => void;
}

export function SubAgentOverlay({
  forkRun,
  onClose,
  forkRuns,
  onApproveTool,
  onDenyTool,
  onOpenForkOverlay,
}: SubAgentOverlayProps) {
  if (!forkRun) return null;

  const forkBindingMessages = flatMessagesFromMachine(forkRun.machine);
  const hasTranscript =
    forkRun.machine.context.turns.length > 0 ||
    forkRun.machine.context.openSegment !== null;

  return (
    <div className="dialog-inner-overlay sub-agent-overlay">
      <header className="dialog-inner-overlay-header sub-agent-overlay-header">
        <span className="dialog-inner-overlay-title sub-agent-overlay-title">
          {agentLabelFromType(forkRun.agentType)}
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
        className="dialog-inner-overlay-scroll sub-agent-overlay-scroll message-list"
        autoScrollTo="bottom"
        autoScrollDeps={[forkRun.machine]}
      >
        {!hasTranscript && (
          <p className="sub-agent-overlay-placeholder">暂无子 Agent 对话记录…</p>
        )}
        <TranscriptView
          machine={forkRun.machine}
          mode="fork"
          forkBindingMessages={forkBindingMessages}
          forkRuns={forkRuns}
          onApproveTool={onApproveTool}
          onDenyTool={onDenyTool}
          onOpenForkOverlay={onOpenForkOverlay}
          isStreaming={forkRun.status === "running"}
        />
      </ScrollViewport>
    </div>
  );
}
