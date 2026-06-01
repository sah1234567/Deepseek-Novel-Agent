import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TranscriptView } from "../../components/chat/TranscriptView";
import { createInitialMachine, dispatchTranscriptEvent } from "../../transcript";
import { transcriptToFlatMessages } from "../../transcript/convert";
import { SYNTHETIC_USER_ID } from "../../transcript/types";
import { userMsg } from "../fixtures/transcript";

function machineWithLiveToolUnderAgent() {
  let m = createInitialMachine();
  m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1", "hi") });
  m = dispatchTranscriptEvent(m, {
    type: "STREAM_CHUNK",
    messageId: "a1",
    delta: "Agent 正在回复",
    kind: "text",
  });
  m = dispatchTranscriptEvent(m, {
    type: "TOOL",
    phase: "start",
    toolCallId: "t1",
    toolName: "Read",
  });
  m = dispatchTranscriptEvent(m, {
    type: "TOOL",
    phase: "result",
    toolCallId: "t1",
    content: "file body",
    toolName: "Read",
  });
  return m;
}

describe("TranscriptView DOM order", () => {
  it("renders Agent bubble before Tool bubble in live SegmentGroup", () => {
    const machine = machineWithLiveToolUnderAgent();
    const flatMessages = transcriptToFlatMessages(machine);
    const { container } = render(
      <TranscriptView machine={machine} mode="main" flatMessages={flatMessages} isStreaming />,
    );
    const assistant = container.querySelector(".message-assistant");
    const tool = container.querySelector(".message-tool");
    expect(assistant).toBeTruthy();
    expect(tool).toBeTruthy();
    const position = assistant!.compareDocumentPosition(tool!);
    expect(position & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("inserts question slot after pause segment tools", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1", "choose") });
    m = dispatchTranscriptEvent(m, {
      type: "STREAM_CHUNK",
      messageId: "a1",
      delta: "pick",
      kind: "text",
    });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "input_complete",
      toolCallId: "ask1",
      toolName: "AskUserQuestion",
      input: {},
    });
    m = dispatchTranscriptEvent(m, { type: "ASK_USER_QUESTION" });
    const flatMessages = transcriptToFlatMessages(m);

    render(
      <TranscriptView
        machine={m}
        mode="main"
        flatMessages={flatMessages}
        pendingQuestion={{
          toolCallId: "ask1",
          questions: [{ id: "q1", prompt: "Pick?", options: [{ id: "o1", label: "A" }] }],
        }}
        questionSlot={<div data-testid="question-panel">Question</div>}
      />,
    );

    expect(screen.getByTestId("question-panel")).toBeInTheDocument();
    const tool = document.querySelector(".message-tool");
    const panel = screen.getByTestId("question-panel");
    expect(tool!.compareDocumentPosition(panel) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("fork mode hides synthetic user bubble", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, {
      type: "BEGIN_TURN",
      user: { id: SYNTHETIC_USER_ID, role: "user", contentBlocks: [] },
    });
    m = dispatchTranscriptEvent(m, {
      type: "STREAM_CHUNK",
      messageId: "a1",
      delta: "sub agent work",
      kind: "text",
    });
    const flatMessages = transcriptToFlatMessages(m);
    const { container } = render(
      <TranscriptView machine={m} mode="fork" flatMessages={flatMessages} isStreaming />,
    );
    expect(container.querySelector(".message-user")).toBeNull();
    expect(container.querySelector(".message-assistant")).toBeTruthy();
  });

  it("committed segment renders assistant before each tool", () => {
    let m = createInitialMachine();
    m = dispatchTranscriptEvent(m, { type: "BEGIN_TURN", user: userMsg("u1", "go") });
    m = dispatchTranscriptEvent(m, {
      type: "STREAM_CHUNK",
      messageId: "a1",
      delta: "done",
      kind: "text",
    });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "input_complete",
      toolCallId: "t1",
      toolName: "Read",
      input: {},
    });
    m = dispatchTranscriptEvent(m, {
      type: "TOOL",
      phase: "result",
      toolCallId: "t1",
      content: "out",
    });
    m = dispatchTranscriptEvent(m, { type: "SEGMENT_COMPLETE", segmentIndex: 0 });
    m = dispatchTranscriptEvent(m, { type: "TURN_COMPLETE" });
    const flatMessages = transcriptToFlatMessages(m);

    const { container } = render(
      <TranscriptView machine={m} mode="main" flatMessages={flatMessages} />,
    );
    const group = container.querySelector(".segment-group");
    expect(group).toBeTruthy();
    const children = group!.querySelectorAll(".message-assistant, .message-tool");
    expect(children.length).toBeGreaterThanOrEqual(2);
    expect(children[0].classList.contains("message-assistant")).toBe(true);
    expect(children[1].classList.contains("message-tool")).toBe(true);
  });
});
