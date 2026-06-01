import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { ContextRefreshBubble } from "../../components/chat/ContextRefreshBubble";
import { userMsg } from "../fixtures/transcript";

describe("ContextRefreshBubble", () => {
  it("renders skill and summary sections", () => {
    const user = userMsg("ctx");
    user.contentBlocks = [
      {
        blockIndex: 0,
        kind: "text",
        text: "[上下文刷新]\n## 激活 Skill\nskill body\n\n## 会话历史摘要\nsummary text",
      },
    ];
    render(<ContextRefreshBubble user={user} />);
    expect(screen.getByText("上下文刷新")).toBeInTheDocument();
    expect(screen.getByText("skill body")).toBeInTheDocument();
    expect(screen.getByText("summary text")).toBeInTheDocument();
  });
});
