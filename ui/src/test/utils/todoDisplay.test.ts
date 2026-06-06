import { describe, expect, it } from "vitest";
import {
  countIncompleteTodos,
  groupTodosForDisplay,
  hasVisibleTodos,
} from "../../utils/todoDisplay";
import type { SessionTodo } from "../../hooks/useAppStatus";

const todos: SessionTodo[] = [
  { id: "1", content: "写细纲", status: "pending" },
  { id: "2", content: "写第一章", status: "in_progress" },
  { id: "3", content: "建人物卡", status: "completed" },
  { id: "4", content: "废弃项", status: "cancelled" },
];

describe("todoDisplay", () => {
  it("groups todos into three sections in order", () => {
    const groups = groupTodosForDisplay(todos);
    expect(groups.map((g) => g.status)).toEqual(["in_progress", "pending", "completed"]);
    expect(groups[0].items).toHaveLength(1);
    expect(groups[1].items[0].content).toBe("写细纲");
  });

  it("counts incomplete todos", () => {
    expect(countIncompleteTodos(todos)).toBe(2);
    expect(countIncompleteTodos([{ id: "x", content: "done", status: "completed" }])).toBe(0);
  });

  it("hasVisibleTodos ignores cancelled-only lists", () => {
    expect(hasVisibleTodos(todos)).toBe(true);
    expect(hasVisibleTodos([{ id: "x", content: "x", status: "cancelled" }])).toBe(false);
    expect(hasVisibleTodos([])).toBe(false);
  });
});
