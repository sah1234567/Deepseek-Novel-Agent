import type { ReactNode } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ChatPanel } from "../../components/chat/ChatPanel";
import { createInitialMachine } from "../../transcript";
import type { UseAgentReturn } from "../../hooks/useAgent";

const mockUseAgentContext = vi.fn<() => UseAgentReturn>();

vi.mock("../../context/AgentContext", () => ({
  useAgentContext: () => mockUseAgentContext(),
}));

vi.mock("../../hooks/useCompactionProgress", () => ({
  useCompactionProgress: () => ({
    visible: false,
    action: "started",
    variant: "info" as const,
  }),
}));

vi.mock("../../hooks/useTranscriptLoader", () => ({
  useTranscriptLoader: () => ({
    layout: null,
    turnSlots: [],
    isBootstrapping: false,
    bootstrapError: null,
    onLoadTurn: vi.fn(),
    reloadActiveTail: vi.fn(),
    onBottomAnchorChange: vi.fn(),
    scheduleReconcile: vi.fn(),
  }),
}));

vi.mock("../../components/layout/ScrollViewport", () => ({
  ScrollViewport: ({ children }: { children: ReactNode }) => (
    <div data-testid="scroll-viewport">{children}</div>
  ),
}));

function stubAgent(overrides: Partial<UseAgentReturn> = {}): UseAgentReturn {
  const noop = vi.fn();
  return {
    transcriptMachine: createInitialMachine(),
    forkBindingMessages: [],
    isStreaming: false,
    forkRuns: new Map(),
    openForkRunId: null,
    pendingQuestion: null,
    questionSelections: {},
    questionCustomText: {},
    questionError: null,
    hasInterruptibleToolInProgress: false,
    sendMessage: noop,
    submitInterrupt: noop,
    interrupt: noop,
    approveTool: noop,
    denyTool: noop,
    toggleQuestionOption: noop,
    setQuestionCustomText: noop,
    answerQuestion: noop,
    dispatchTranscript: noop,
    registerReloadActiveTail: () => noop,
    clearQuestionError: noop,
    openForkOverlay: noop,
    closeForkOverlay: noop,
    model: "deepseek-v4-pro",
    setModel: noop,
    turnInProgress: false,
    ...overrides,
  };
}

describe("ChatPanel permission mode select", () => {
  beforeEach(() => {
    cleanup();
    Element.prototype.scrollTo = vi.fn();
    globalThis.ResizeObserver = class {
      observe = vi.fn();
      unobserve = vi.fn();
      disconnect = vi.fn();
      constructor(_cb: ResizeObserverCallback) {}
    };
    mockUseAgentContext.mockReturnValue(stubAgent());
  });

  it("disables mode select when app turn is in progress", () => {
    const { container } = render(
      <ChatPanel
        permissionMode="normal"
        appTurnInProgress
        onSetPermissionMode={vi.fn()}
      />,
    );
    const select = container.querySelector(".mode-select") as HTMLSelectElement;
    expect(select).toBeDisabled();
    expect(select).toHaveAttribute(
      "title",
      "当前轮次进行中，结束后或中断后才可切换权限模式",
    );
  });

  it("disables mode select when agent turn is in progress", () => {
    mockUseAgentContext.mockReturnValue(stubAgent({ turnInProgress: true }));
    render(
      <ChatPanel permissionMode="normal" onSetPermissionMode={vi.fn()} />,
    );
    expect(screen.getAllByRole("combobox")[0]).toBeDisabled();
  });

  it("calls onSetPermissionMode when idle and user changes mode", () => {
    const onSetPermissionMode = vi.fn().mockResolvedValue(undefined);
    const { container } = render(
      <ChatPanel
        permissionMode="normal"
        onSetPermissionMode={onSetPermissionMode}
      />,
    );
    const select = container.querySelector(".mode-select") as HTMLSelectElement;
    expect(select.disabled).toBe(false);
    fireEvent.change(select, { target: { value: "auto" } });
    expect(onSetPermissionMode).toHaveBeenCalledWith("auto");
  });
});
