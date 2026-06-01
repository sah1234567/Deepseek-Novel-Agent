import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AgentBubble, ToolBubble } from "../../components/chat/segmentRender";
import type { ToolCall } from "../../hooks/useAgent";

describe("segmentRender", () => {
  it("AgentBubble shows placeholder header for tool-only segment", () => {
    render(
      <AgentBubble
        assistant={{
          id: "ph-1",
          status: "placeholder",
          contentBlocks: [],
        }}
      />,
    );
    expect(screen.getByText(/调用工具/)).toBeInTheDocument();
  });

  it("ToolBubble renders ForkSubAgent card and enter handler", () => {
    const onOpen = vi.fn();
    const tool: ToolCall = {
      id: "fork-1",
      name: "ForkSubAgent",
      input: { agent_type: "GeneralPurpose", task: "audit" },
      status: "done",
      needsApproval: false,
      result: "",
    };
    render(
      <ToolBubble
        tool={tool}
        flatMessages={[
          {
            id: "tool-fork-1",
            role: "tool",
            toolName: "ForkSubAgent",
            contentBlocks: [],
            toolInput: tool.input,
          },
        ]}
        forkRuns={new Map()}
        forkLinks={new Map()}
        onOpenForkOverlay={onOpen}
      />,
    );
    expect(screen.getByText("ForkSubAgent")).toBeInTheDocument();
  });

  it("ToolBubble shows approve buttons for pending Write", () => {
    const onApprove = vi.fn();
    const onDeny = vi.fn();
    const tool: ToolCall = {
      id: "w1",
      name: "Write",
      input: { path: "ch.md" },
      status: "pending",
      needsApproval: true,
    };
    render(
      <ToolBubble
        tool={tool}
        flatMessages={[]}
        onApprove={onApprove}
        onDeny={onDeny}
      />,
    );
    screen.getByText("批准").click();
    expect(onApprove).toHaveBeenCalledWith("w1");
  });
});
