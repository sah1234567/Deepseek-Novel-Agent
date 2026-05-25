import type { SessionTodo } from "../hooks/useAppStatus";
import "./TodoPanel.css";

interface TodoPanelProps {
  todos: SessionTodo[];
  onCycleStatus: (todoId: string, nextStatus: string) => void;
}

const STATUS_LABEL: Record<string, string> = {
  pending: "待办",
  in_progress: "进行中",
  completed: "已完成",
  cancelled: "已取消",
};

const STATUS_CYCLE: Record<string, string> = {
  pending: "in_progress",
  in_progress: "completed",
  completed: "pending",
  cancelled: "pending",
};

export function TodoPanel({ todos, onCycleStatus }: TodoPanelProps) {
  return (
    <aside className="todo-panel">
      <h2>会话待办</h2>
      {todos.length === 0 ? (
        <p className="todo-empty">Agent 调用 TodoWrite 后会显示在这里</p>
      ) : (
        <ul className="todo-list">
          {todos.map((todo) => (
            <li key={todo.id} className={`todo-item todo-${todo.status}`}>
              <button
                type="button"
                className="todo-status"
                title="点击切换状态"
                onClick={() =>
                  onCycleStatus(todo.id, STATUS_CYCLE[todo.status] ?? "pending")
                }
              >
                {STATUS_LABEL[todo.status] ?? todo.status}
              </button>
              <span className="todo-content">{todo.content}</span>
            </li>
          ))}
        </ul>
      )}
    </aside>
  );
}
