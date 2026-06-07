import { describe, expect, it } from "vitest";
import { isSyntheticUser, segmentMessageId, SYNTHETIC_USER_ID } from "./types";
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

});
