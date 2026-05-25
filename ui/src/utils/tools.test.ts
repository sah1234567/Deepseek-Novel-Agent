import { describe, expect, it } from "vitest";
import { extractToolPath, formatToolSummary, nextTodoStatus } from "./tools";

describe("extractToolPath", () => {
  it("reads file_path from input", () => {
    expect(extractToolPath({ file_path: "chapters/ch01.md" })).toBe("chapters/ch01.md");
  });

  it("returns null for empty input", () => {
    expect(extractToolPath(null)).toBeNull();
  });
});

describe("formatToolSummary", () => {
  it("includes path for Read tool", () => {
    expect(formatToolSummary("Read", { path: "knowledge/INDEX.md" })).toBe(
      "Read: knowledge/INDEX.md",
    );
  });
});

describe("nextTodoStatus", () => {
  it("cycles pending -> in_progress -> completed -> pending", () => {
    expect(nextTodoStatus("pending")).toBe("in_progress");
    expect(nextTodoStatus("in_progress")).toBe("completed");
    expect(nextTodoStatus("completed")).toBe("pending");
  });
});
