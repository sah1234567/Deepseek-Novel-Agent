import type { SessionTodo } from "../hooks/useAppStatus";

export type TodoDisplayStatus = "in_progress" | "pending" | "completed" | "cancelled";

/** Matches `SessionTodo::is_unfinished()` — canonical definition in `novel-state/src/todo.rs`. */
export function isIncompleteTodo(todo: SessionTodo): boolean {
  return todo.status === "pending" || todo.status === "in_progress";
}

export function countIncompleteTodos(todos: SessionTodo[]): number {
  return todos.filter(isIncompleteTodo).length;
}
