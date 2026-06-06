import type { SessionTodo } from "../hooks/useAppStatus";

export type TodoDisplayStatus = "in_progress" | "pending" | "completed" | "cancelled";

export const TODO_SECTION_ORDER: Array<{
  status: TodoDisplayStatus;
  title: string;
}> = [
  { status: "in_progress", title: "进行中" },
  { status: "pending", title: "未进行" },
  { status: "completed", title: "已完成" },
];

export function isIncompleteTodo(todo: SessionTodo): boolean {
  return todo.status === "pending" || todo.status === "in_progress";
}

export function countIncompleteTodos(todos: SessionTodo[]): number {
  return todos.filter(isIncompleteTodo).length;
}

export function groupTodosForDisplay(todos: SessionTodo[]) {
  const visible = todos.filter((t) => t.status !== "cancelled");
  return TODO_SECTION_ORDER.map((section) => ({
    ...section,
    items: visible.filter((t) => t.status === section.status),
  })).filter((section) => section.items.length > 0);
}

export function hasVisibleTodos(todos: SessionTodo[]): boolean {
  return todos.some((t) => t.status !== "cancelled");
}
