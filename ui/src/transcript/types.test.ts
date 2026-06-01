import { describe, expect, it } from "vitest";
import {
  isContextRefreshUser,
  isSyntheticUser,
  parseContextRefreshSections,
  segmentMessageId,
  SYNTHETIC_USER_ID,
} from "./types";
import { userMsg } from "../test/fixtures/transcript";

describe("transcript types helpers", () => {
  it("segmentMessageId uses base id for segment 0", () => {
    expect(segmentMessageId("msg-1", 0)).toBe("msg-1");
  });

  it("segmentMessageId suffixes non-zero segments", () => {
    expect(segmentMessageId("msg-1", 2)).toBe("msg-1-seg-2");
  });

  it("isSyntheticUser identifies synthetic fork user", () => {
    expect(isSyntheticUser({ id: SYNTHETIC_USER_ID, role: "user", contentBlocks: [] })).toBe(true);
    expect(isSyntheticUser(userMsg("real-user"))).toBe(false);
  });

  it("isContextRefreshUser detects merged refresh bubble", () => {
    const user = userMsg("ctx");
    user.messageKind = "contextRefresh";
    user.contentBlocks = [{ blockIndex: 0, kind: "text", text: "[上下文刷新]\n## 会话历史摘要\nx" }];
    expect(isContextRefreshUser(user)).toBe(true);
  });

  it("parseContextRefreshSections splits skill and summary", () => {
    const text =
      "[上下文刷新]\n## 激活 Skill\nskill body\n\n## 会话历史摘要\nsummary body";
    expect(parseContextRefreshSections(text)).toEqual({
      skill: "skill body",
      summary: "summary body",
    });
  });
});
