import type { SessionTodo } from "../hooks/useAppStatus";

export type TodoDisplayStatus = "in_progress" | "pending" | "completed" | "cancelled";

export function isIncompleteTodo(todo: SessionTodo): boolean {
  return todo.status === "pending" || todo.status === "in_progress";
}

export function countIncompleteTodos(todos: SessionTodo[]): number {
  return todos.filter(isIncompleteTodo).length;
}

/** Preserve backend order; hide cancelled items only. */
export function visibleTodosForDisplay(todos: SessionTodo[]): SessionTodo[] {
  return todos.filter((t) => t.status !== "cancelled");
}
