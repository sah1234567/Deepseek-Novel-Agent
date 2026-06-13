import { describe, expect, it } from "vitest";
import { countIncompleteTodos, visibleTodosForDisplay } from "../../utils/todoDisplay";
import type { SessionTodo } from "../../hooks/useAppStatus";

const todos: SessionTodo[] = [
  { id: "1", content: "写细纲", status: "pending" },
  { id: "2", content: "写第一章", status: "in_progress" },
  { id: "3", content: "建人物卡", status: "completed" },
  { id: "4", content: "废弃项", status: "cancelled" },
];

describe("todoDisplay", () => {
  it("keeps backend order and hides cancelled todos", () => {
    const visible = visibleTodosForDisplay(todos);
    expect(visible.map((t) => t.id)).toEqual(["1", "2", "3"]);
    expect(visible.map((t) => t.content)).toEqual(["写细纲", "写第一章", "建人物卡"]);
  });

  it("still shows completed todos in place", () => {
    const visible = visibleTodosForDisplay([{ id: "x", content: "done", status: "completed" }]);
    expect(visible).toHaveLength(1);
    expect(visible[0].status).toBe("completed");
  });

  it("counts incomplete todos for StatusBar badge and empty state", () => {
    expect(countIncompleteTodos(todos)).toBe(2);
    expect(countIncompleteTodos([{ id: "x", content: "done", status: "completed" }])).toBe(0);
    expect(countIncompleteTodos([{ id: "x", content: "x", status: "cancelled" }])).toBe(0);
    expect(countIncompleteTodos([])).toBe(0);
  });
});
