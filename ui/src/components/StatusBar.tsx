import { useCallback, useEffect, useMemo, useState } from "react";
import type { AppStatus, SessionSummary, SessionTodo, WorkSummary } from "../hooks/useAppStatus";
import type { TurnStats } from "../hooks/useAgent";
import { useAgentContext } from "../context/AgentContext";
import "./StatusBar.css";

interface StatusBarProps {
  status: AppStatus | null;
  hookRunning: boolean;
  activeSubAgent: string | null;
  activeForkCount: number;
  runningForkRunId: string | null;
  onOpenForkOverlay: (forkRunId: string) => void;
  lastTurnStats: TurnStats | null;
  listWorks: () => Promise<WorkSummary[]>;
  listSessions: () => Promise<SessionSummary[]>;
  onOpenWork: (name: string) => Promise<void>;
  onCreateWork: (name: string) => Promise<void>;
  onResumeSession: (sessionId: string) => Promise<void>;
  onOpenSettings: () => void;
  onNewSession: () => void;
  onCycleTodo: (todoId: string, nextStatus: string) => void;
}

function fmt(n: number): string {
  return n.toLocaleString();
}

const STATUS_LABEL: Record<string, string> = {
  pending: "待办",
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
  const activeTodos = todos.filter(
    (t) => t.status !== "completed" && t.status !== "cancelled"
  );
  const activeCount = activeTodos.length;
  return (
    <div className="todo-dropdown-wrapper">
      <button
        type="button"
        className={`todo-dropdown-btn${activeCount > 0 ? " has-active" : ""}${open ? " is-open" : ""}`}
        onClick={onToggle}
        title="当前进度"
      >
        进度{activeCount > 0 ? ` ${activeCount}` : ""}
      </button>
      {open && (
        <div className="todo-dropdown">
          {activeCount === 0 ? (
            <p className="todo-dropdown-empty">暂无待办</p>
          ) : (
            <ul className="todo-dropdown-list">
              {activeTodos.map((todo) => (
                <li key={todo.id} className={`todo-dropdown-item todo-dropdown-${todo.status}`}>
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
          )}
        </div>
      )}
    </div>
  );
}

function subAgentStatusLabel(agentType: string): string {
  const t = agentType.toLowerCase();
  if (t.includes("knowledgeauditor")) return "知识库审计中…";
  if (t.includes("chaptercraft")) return "章节工艺分析中…";
  if (t.includes("general")) return "自定义 Subagent 运行中…";
  return `${agentType} 运行中…`;
}

function sessionOptionLabel(
  s: SessionSummary,
  currentId?: string,
  currentTurn?: number,
): string {
  const shortId = `${s.id.slice(0, 8)}…`;
  const name = s.title?.trim() || shortId;
  const turns =
    s.id === currentId && currentTurn !== undefined ? currentTurn : s.total_turns;
  return `${name} · Turn ${turns}`;
}

function TokenStat({
  label,
  value,
  pending,
  variant,
}: {
  label: string;
  value: number;
  pending?: boolean;
  variant: "turn" | "session";
}) {
  return (
    <span className={`token-stat token-stat-${variant}`}>
      <span className="token-stat-label">{label}</span>
      <span className="token-stat-value">{pending ? "…" : fmt(value)}</span>
    </span>
  );
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
  hookRunning,
  activeSubAgent,
  activeForkCount,
  runningForkRunId,
  onOpenForkOverlay,
  lastTurnStats,
  listWorks,
  listSessions,
  onOpenWork,
  onCreateWork,
  onResumeSession,
  onOpenSettings,
  onNewSession,
  onCycleTodo,
}: StatusBarProps) {
  const { resolveTurnTokens } = useAgentContext();
  const [todoOpen, setTodoOpen] = useState(false);
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

  useEffect(() => {
    if (!status || !lastTurnStats?.wasInterrupted) return;
    resolveTurnTokens(
      status.sessionCacheHit,
      status.sessionCacheMiss,
      status.sessionCompletion,
    );
  }, [
    status?.sessionCacheHit,
    status?.sessionCacheMiss,
    status?.sessionCompletion,
    lastTurnStats?.wasInterrupted,
    resolveTurnTokens,
    status,
  ]);

  const sessionOptions = useMemo(() => {
    if (!status?.sessionId) return sessions;
    if (sessions.some((s) => s.id === status.sessionId)) return sessions;
    return [
      {
        id: status.sessionId,
        title: null,
        status: "active",
        last_active_at: "",
        total_turns: status.turnNumber,
      },
      ...sessions,
    ];
  }, [sessions, status?.sessionId, status?.turnNumber]);

  const turnNumber = status?.turnNumber ?? 0;

  const turnHit = lastTurnStats?.turnHit ?? 0;
  const turnMiss = lastTurnStats?.turnMiss ?? 0;
  const turnComp = lastTurnStats?.turnComp ?? 0;
  const wasInterrupted = lastTurnStats?.wasInterrupted ?? false;
  const turnPending = wasInterrupted && turnHit === 0 && turnMiss === 0 && turnComp === 0;

  const sessionHit = status?.sessionCacheHit ?? 0;
  const sessionMiss = status?.sessionCacheMiss ?? 0;
  const sessionComp = status?.sessionCompletion ?? 0;
  const sessionTotal = status?.sessionTotalTokens ?? 0;

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
    } finally {
      setSessionBusy(false);
    }
  }

  return (
    <div className="status-bar">
      <div className="status-items">
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
            className="session-select"
            value={status?.sessionId ?? ""}
            disabled={!status?.sessionId || sessionBusy}
            onChange={(e) => void handleSessionChange(e.target.value)}
            aria-label="选择会话"
          >
            {sessionOptions.length === 0 && status?.sessionId && (
              <option value={status.sessionId}>
                {sessionOptionLabel(
                  {
                    id: status.sessionId,
                    title: null,
                    status: "active",
                    last_active_at: "",
                    total_turns: turnNumber,
                  },
                  status.sessionId,
                )}
              </option>
            )}
            {sessionOptions.map((s) => (
              <option key={s.id} value={s.id}>
                {sessionOptionLabel(s, status?.sessionId, turnNumber)}
              </option>
            ))}
          </select>
          <button
            type="button"
            className="new-session-btn"
            onClick={onNewSession}
            disabled={sessionBusy}
            title="新建会话"
            aria-label="新建会话"
          >
            +
          </button>
        </label>

        <TokenGroup title={`本轮 Turn ${turnNumber}`} variant="turn">
          <TokenStat label="缓存复用" value={turnHit} pending={turnPending} variant="turn" />
          <TokenStat label="新输入" value={turnMiss} pending={turnPending} variant="turn" />
          <TokenStat label="生成" value={turnComp} pending={turnPending} variant="turn" />
        </TokenGroup>

        <TokenGroup title="会话累计" variant="session">
          <span className="token-total">{fmt(sessionTotal)}</span>
          <TokenStat label="缓存复用" value={sessionHit} variant="session" />
          <TokenStat label="新输入" value={sessionMiss} variant="session" />
          <TokenStat label="生成" value={sessionComp} variant="session" />
        </TokenGroup>

        {(activeSubAgent || hookRunning) && (
          <button
            type="button"
            className="status-chip status-hook status-chip-clickable"
            disabled={!runningForkRunId}
            onClick={() => {
              if (runningForkRunId) onOpenForkOverlay(runningForkRunId);
            }}
            title={runningForkRunId ? "查看 Subagent 详情" : "Subagent 运行中"}
          >
            {subAgentStatusLabel(activeSubAgent ?? "KnowledgeAuditor")}
            {activeForkCount > 1 ? ` (${activeForkCount})` : ""}
          </button>
        )}
        {status && !status.projectInitialized && (
          <span className="status-chip status-warn">项目未初始化</span>
        )}

        <TodoDropdown
          todos={status?.todos ?? []}
          open={todoOpen}
          onToggle={() => setTodoOpen((v) => !v)}
          onCycle={onCycleTodo}
        />
      </div>

      <button type="button" className="settings-btn" onClick={onOpenSettings}>
        设置
      </button>
    </div>
  );
}
