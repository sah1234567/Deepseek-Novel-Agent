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

  it("collapsed preview prefers audit status when present", () => {
    const { container } = render(
      <ContextRefreshBubble
        user={refreshUser(
          "[上下文刷新]\n## 会话历史摘要\n## 创作进度\nx\n\n## 审计状态\nCh3 PA 已通过\n\n## 情节\ny",
        )}
      />,
    );
    const preview = within(container).getByText(/审计：/, { selector: ".context-refresh-preview" });
    expect(preview).toBeVisible();
  });

  it("expanded: shows audit block and summary, not skill body", () => {
    const { container } = render(
      <ContextRefreshBubble
        user={refreshUser(
          "[上下文刷新]\n## 激活 Skill\n### revision\nsecret skill\n\n## 会话历史摘要\n## 审计状态\nCh2 KA 待处理\n\n## 创作进度\nexpanded summary",
        )}
      />,
    );
    const root = within(container);
    fireEvent.click(root.getByText("上下文已刷新"));
    const audit = root.getByRole("heading", { name: "审计状态" }).closest(".context-refresh-audit")!;
    expect(within(audit as HTMLElement).getByText(/Ch2 KA/)).toBeVisible();
    const body = root.getByRole("heading", { name: "会话历史摘要" }).closest(".context-refresh-body")!;
    expect(within(body as HTMLElement).getByText(/expanded summary/)).toBeVisible();
    expect(screen.queryByText("secret skill")).not.toBeInTheDocument();
  });

  it("expanded: shows summary markdown only when no audit section", () => {
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
