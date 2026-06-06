import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { AppStatus, SessionSummary, SessionTodo, WorkSummary } from "../hooks/useAppStatus";
import { useAgentContext } from "../context/AgentContext";
import {
  countIncompleteTodos,
  groupTodosForDisplay,
  hasVisibleTodos,
} from "../utils/todoDisplay";
import "./StatusBar.css";

interface StatusBarProps {
  status: AppStatus | null;
  listWorks: () => Promise<WorkSummary[]>;
  listSessions: () => Promise<SessionSummary[]>;
  onOpenWork: (name: string) => Promise<void>;
  onCreateWork: (name: string) => Promise<void>;
  onResumeSession: (sessionId: string) => Promise<void>;
  onOpenSettings: () => void;
  onNewSession: () => Promise<void>;
  onCycleTodo: (todoId: string, nextStatus: string) => void;
  onSessionError?: (message: string) => void;
}

function fmt(n: number): string {
  return n.toLocaleString();
}

const STATUS_LABEL: Record<string, string> = {
  pending: "未进行",
  in_progress: "进行中",
  completed: "已完成",
  cancelled: "已取消",
};

const STATUS_ICON: Record<string, string> = {
  pending: "○",
  in_progress: "◉",
  completed: "✓",
  cancelled: "✗",
};

const STATUS_CYCLE: Record<string, string> = {
  pending: "in_progress",
  in_progress: "completed",
  completed: "pending",
  cancelled: "pending",
};

function TodoDropdown({
  todos,
  open,
  onToggle,
  onCycle,
}: {
  todos: SessionTodo[];
  open: boolean;
  onToggle: () => void;
  onCycle: (id: string, next: string) => void;
}) {
  const incompleteCount = countIncompleteTodos(todos);
  const sections = groupTodosForDisplay(todos);
  const showEmpty = !hasVisibleTodos(todos);

  return (
    <div className="todo-dropdown-wrapper">
      <button
        type="button"
        className={`todo-dropdown-btn${incompleteCount > 0 ? " has-active" : ""}${open ? " is-open" : ""}`}
        onClick={onToggle}
        title="当前待办事项"
        aria-expanded={open}
      >
        待办事项{incompleteCount > 0 ? ` ${incompleteCount}` : ""}
      </button>
      {open && (
        <div className="todo-dropdown">
          {showEmpty ? (
            <p className="todo-dropdown-empty">暂无待办事项</p>
          ) : (
            sections.map((section) => (
              <section key={section.status} className="todo-dropdown-section">
                <h3 className="todo-dropdown-section-title">{section.title}</h3>
                <ul className="todo-dropdown-list">
                  {section.items.map((todo) => (
                    <li
                      key={todo.id}
                      className={`todo-dropdown-item todo-dropdown-${todo.status}`}
                    >
                      <button
                        type="button"
                        className="todo-dropdown-status"
                        title="点击切换状态"
                        onClick={() =>
                          onCycle(todo.id, STATUS_CYCLE[todo.status] ?? "pending")
                        }
                      >
                        <span className="todo-dropdown-icon">
                          {STATUS_ICON[todo.status] ?? "○"}
                        </span>
                        {STATUS_LABEL[todo.status] ?? todo.status}
                      </button>
                      <span className="todo-dropdown-content">{todo.content}</span>
                    </li>
                  ))}
                </ul>
              </section>
            ))
          )}
        </div>
      )}
    </div>
  );
}

function sessionOptionLabel(s: SessionSummary): string {
  const name = s.title?.trim() || "新会话";
  const last = new Date(s.last_active_at);
  const now = new Date();
  const diffMin = Math.floor((now.getTime() - last.getTime()) / 60000);
  const timeStr =
    diffMin < 1 ? "刚刚" :
    diffMin < 60 ? `${diffMin}分钟前` :
    diffMin < 1440 ? `${Math.floor(diffMin / 60)}小时前` :
    `${Math.floor(diffMin / 1440)}天前`;
  const modelLabel = s.model?.includes("flash") ? "flash" : s.model?.includes("pro") ? "pro" : s.model;
  return `${name} · 对话 ${s.total_turns} 轮 · ${timeStr}${modelLabel ? ` · ${modelLabel}` : ""}`;
}

function TokenGroup({
  title,
  variant,
  children,
}: {
  title: string;
  variant: "turn" | "session";
  children: React.ReactNode;
}) {
  return (
    <div className={`token-group token-group-${variant}`} aria-label={title}>
      <span className="token-group-title">{title}</span>
      <div className="token-group-stats">{children}</div>
    </div>
  );
}

export function StatusBar({
  status,
  listWorks,
  listSessions,
  onOpenWork,
  onCreateWork,
  onResumeSession,
  onOpenSettings,
  onNewSession,
  onCycleTodo,
  onSessionError,
}: StatusBarProps) {
  const { isStreaming } = useAgentContext();
  const [todoOpen, setTodoOpen] = useState(false);
  const prevIncompleteRef = useRef(0);
  useEffect(() => {
    prevIncompleteRef.current = 0;
    setTodoOpen(false);
  }, [status?.sessionId]);

  useEffect(() => {
    const incomplete = countIncompleteTodos(status?.todos ?? []);
    if (incomplete > 0 && prevIncompleteRef.current === 0) {
      setTodoOpen(true);
    }
    prevIncompleteRef.current = incomplete;
  }, [status?.todos]);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [works, setWorks] = useState<WorkSummary[]>([]);
  const [sessionBusy, setSessionBusy] = useState(false);
  const [workBusy, setWorkBusy] = useState(false);

  const loadWorks = useCallback(async () => {
    try {
      const list = await listWorks();
      setWorks(list);
    } catch {
      /* surfaced via ErrorBanner */
    }
  }, [listWorks]);

  const loadSessions = useCallback(async () => {
    try {
      const list = await listSessions();
      setSessions(list);
    } catch {
      /* list failure surfaced elsewhere */
    }
  }, [listSessions]);

  useEffect(() => {
    void loadWorks();
  }, [loadWorks, status?.activeWorkName]);

  useEffect(() => {
    void loadSessions();
  }, [loadSessions, status?.sessionId]);

  const sessionOptions = useMemo(() => {
    if (!status?.sessionId) return sessions;
    if (sessions.some((s) => s.id === status.sessionId)) return sessions;
    return [
      {
        id: status.sessionId,
        title: null,
        status: "active",
        model: "",
        last_active_at: "",
        created_at: "",
        total_turns: status.turnNumber,
        api_call_count: 0,
      },
      ...sessions,
    ];
  }, [sessions, status?.sessionId, status?.turnNumber]);

  const currentSessionLabel = useMemo(() => {
    const current = sessionOptions.find((s) => s.id === status?.sessionId);
    if (current) return sessionOptionLabel(current);
    if (!status?.sessionId) return "";
    return sessionOptionLabel({
      id: status.sessionId,
      title: null,
      status: "active",
      model: "",
      last_active_at: "",
      created_at: "",
      total_turns: status.turnNumber ?? 0,
      api_call_count: 0,
    });
  }, [sessionOptions, status?.sessionId, status?.turnNumber]);

  const sessionHit = status?.sessionCacheHit ?? 0;
  const sessionMiss = status?.sessionCacheMiss ?? 0;
  const sessionComp = status?.sessionCompletion ?? 0;

  async function handleWorkChange(nextName: string) {
    if (!nextName || nextName === status?.activeWorkName || workBusy) return;
    setWorkBusy(true);
    try {
      await onOpenWork(nextName);
      await loadWorks();
      await loadSessions();
    } finally {
      setWorkBusy(false);
    }
  }

  async function handleCreateWork() {
    const name = window.prompt("新建作品 — 输入作品名（不含路径）");
    if (!name?.trim() || workBusy) return;
    setWorkBusy(true);
    try {
      await onCreateWork(name.trim());
      await loadWorks();
      await loadSessions();
    } finally {
      setWorkBusy(false);
    }
  }

  async function handleSessionChange(nextId: string) {
    if (!nextId || nextId === status?.sessionId || sessionBusy) return;
    setSessionBusy(true);
    try {
      await onResumeSession(nextId);
      await loadSessions();
    } catch (e) {
      onSessionError?.(String(e));
    } finally {
      setSessionBusy(false);
    }
  }

  return (
    <div className="status-bar">
      <div className="status-items">
        <TodoDropdown
          todos={status?.todos ?? []}
          open={todoOpen}
          onToggle={() => setTodoOpen((v) => !v)}
          onCycle={onCycleTodo}
        />

        <label className="session-control work-control">
          <span className="session-control-label">作品</span>
          <select
            className="session-select"
            value={status?.activeWorkName ?? ""}
            disabled={workBusy}
            onChange={(e) => void handleWorkChange(e.target.value)}
            aria-label="选择作品"
          >
            {works.length === 0 && status?.activeWorkName && (
              <option value={status.activeWorkName}>{status.activeWorkName}</option>
            )}
            {works.map((w) => (
              <option key={w.name} value={w.name}>
                {w.name}
                {!w.initialized ? "（未初始化）" : ""}
              </option>
            ))}
          </select>
          <button
            type="button"
            className="new-session-btn"
            onClick={() => void handleCreateWork()}
            disabled={workBusy}
            title="新建作品"
            aria-label="新建作品"
          >
            +
          </button>
        </label>

        <label className="session-control">
          <span className="session-control-label">会话</span>
          <select
            className="session-select session-select-session"
            value={status?.sessionId ?? ""}
            disabled={!status?.sessionId || sessionBusy || isStreaming}
            title={currentSessionLabel || undefined}
            onChange={(e) => {
              if (sessionBusy || isStreaming) return;
              void handleSessionChange(e.target.value);
            }}
            aria-label="选择会话"
          >
            {sessionOptions.length === 0 && status?.sessionId && (
              <option value={status.sessionId}>
                {sessionOptionLabel({
                  id: status.sessionId,
                  title: null,
                  status: "active",
                  model: "",
                  last_active_at: "",
                  created_at: "",
                  total_turns: status?.turnNumber ?? 0,
                  api_call_count: 0,
                })}
              </option>
            )}
            {sessionOptions.map((s) => (
              <option key={s.id} value={s.id}>
                {sessionOptionLabel(s)}
              </option>
            ))}
          </select>
          <button
            type="button"
            className="new-session-btn"
            onClick={() => {
              if (sessionBusy || isStreaming) return;
              setSessionBusy(true);
              onNewSession().finally(() => setSessionBusy(false));
            }}
            disabled={sessionBusy || isStreaming}
            title={isStreaming ? "请等待当前任务完成" : sessionBusy ? "切换中…" : "新建会话"}
            aria-label="新建会话"
          >
            +
          </button>
        </label>

        <TokenGroup title="当前上下文" variant="session">
          <span className="token-total">{fmt(status?.contextTokens ?? 0)}</span>
        </TokenGroup>

        <TokenGroup title="会话用量" variant="session">
          <div className="token-group-stats">
            <span className="token-stat token-stat-session">
              <span className="token-stat-label">缓存复用</span>
              <span className="token-stat-value">{fmt(sessionHit)}</span>
            </span>
            <span className="token-stat token-stat-session">
              <span className="token-stat-label">新输入</span>
              <span className="token-stat-value">{fmt(sessionMiss)}</span>
            </span>
            <span className="token-stat token-stat-session">
              <span className="token-stat-label">生成</span>
              <span className="token-stat-value">{fmt(sessionComp)}</span>
            </span>
          </div>
        </TokenGroup>

        {status && !status.projectInitialized && (
          <span className="status-chip status-warn">项目未初始化</span>
        )}

      </div>

      <button type="button" className="settings-btn" onClick={onOpenSettings}>
        设置
      </button>
    </div>
  );
}
