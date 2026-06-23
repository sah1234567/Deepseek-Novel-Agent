import { describe, expect, it } from "vitest";
import { countIncompleteTodos, isIncompleteTodo } from "../../utils/todoDisplay";
import type { SessionTodo } from "../../hooks/useAppStatus";

const todos: SessionTodo[] = [
  { id: "1", content: "写细纲", status: "pending" },
  { id: "2", content: "写第一章", status: "in_progress" },
  { id: "3", content: "建人物卡", status: "completed" },
  { id: "4", content: "废弃项", status: "cancelled" },
];

describe("todoDisplay", () => {
  it("matches novel-state is_unfinished for all statuses", () => {
    expect(isIncompleteTodo({ id: "1", content: "x", status: "pending" })).toBe(true);
    expect(isIncompleteTodo({ id: "2", content: "x", status: "in_progress" })).toBe(true);
    expect(isIncompleteTodo({ id: "3", content: "x", status: "completed" })).toBe(false);
    expect(isIncompleteTodo({ id: "4", content: "x", status: "cancelled" })).toBe(false);
  });

  it("counts incomplete todos for StatusBar badge (cancelled is terminal)", () => {
    expect(countIncompleteTodos(todos)).toBe(2);
    expect(countIncompleteTodos([{ id: "x", content: "done", status: "completed" }])).toBe(0);
    expect(countIncompleteTodos([{ id: "x", content: "x", status: "cancelled" }])).toBe(0);
    expect(countIncompleteTodos([])).toBe(0);
  });
});
