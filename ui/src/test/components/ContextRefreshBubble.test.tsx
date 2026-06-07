import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { ContextRefreshBubble } from "../../components/chat/ContextRefreshBubble";
import { userMsg } from "../fixtures/transcript";

function refreshUser(text: string) {
  const user = userMsg("ctx");
  user.contentBlocks = [{ blockIndex: 0, kind: "text", text }];
  return user;
}

describe("ContextRefreshBubble", () => {
  afterEach(() => cleanup());

  it("collapsed: title, skill labels, summary preview; hides skill body and full summary", () => {
    const { container } = render(
      <ContextRefreshBubble
        user={refreshUser(
          "[上下文刷新]\n## 激活 Skill\n### write-chapter\nfull skill body hidden\n\n## 会话历史摘要\nsummary text for preview",
        )}
      />,
    );
    const root = within(container);
    expect(root.getByText("上下文已刷新")).toBeInTheDocument();
    expect(root.getByText(/已激活 Skill：write-chapter/)).toBeInTheDocument();
    expect(root.getByText(/summary text for preview/, { selector: ".context-refresh-preview" })).toBeVisible();
    expect(screen.queryByText("full skill body hidden")).not.toBeInTheDocument();
    expect(root.getByRole("heading", { name: "会话历史摘要", hidden: true })).not.toBeVisible();
  });

  it("expanded: shows summary markdown only, not skill body", () => {
    const { container } = render(
      <ContextRefreshBubble
        user={refreshUser(
          "[上下文刷新]\n## 激活 Skill\n### revision\nsecret skill\n\n## 会话历史摘要\nexpanded summary",
        )}
      />,
    );
    const root = within(container);
    fireEvent.click(root.getByText("上下文已刷新"));
    const body = root.getByRole("heading", { name: "会话历史摘要" }).closest(".context-refresh-body")!;
    expect(body).toBeTruthy();
    expect(within(body as HTMLElement).getByText("expanded summary")).toBeVisible();
    expect(screen.queryByText("secret skill")).not.toBeInTheDocument();
  });
});
