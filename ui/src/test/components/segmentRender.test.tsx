import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AgentBubble, ToolBubble } from "../../components/chat/segmentRender";
import type { ToolCall } from "../../hooks/useAgent";
import { createInitialMachine } from "../../transcript";

describe("segmentRender", () => {
  it("AgentBubble shows thinking once when committed with text and CoT", () => {
    render(
      <AgentBubble
        assistant={{
          id: "a1",
          status: "committed",
          contentBlocks: [
            { blockIndex: 0, kind: "thinking", text: "chain of thought" },
            { blockIndex: 1, kind: "text", text: "answer body" },
          ],
        }}
      />,
    );
    expect(screen.getAllByText("思考过程")).toHaveLength(1);
    expect(screen.queryByText("推理过程")).not.toBeInTheDocument();
    expect(screen.getByText("answer body")).toBeInTheDocument();
  });

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
        forkRuns={
          new Map([
            [
              "run-1",
              {
                forkRunId: "run-1",
                agentType: "GeneralPurpose",
                taskPreview: "audit",
                source: "tool",
                parentToolCallId: "fork-1",
                machine: createInitialMachine(),
                status: "running",
              },
            ],
          ])
        }
        onOpenForkOverlay={onOpen}
      />,
    );
    expect(screen.getByText(/Subagent · GeneralPurpose/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "进入" })).toBeInTheDocument();
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
