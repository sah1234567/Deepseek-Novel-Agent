import { FormEvent, useEffect, useMemo, useState } from "react";
import type { SessionSummary } from "../hooks/useAppStatus";
import "./SettingsPanel.css";

interface ApiConfig {
  api_key: string;
  api_base: string;
}

interface SettingsPanelProps {
  open: boolean;
  onClose: () => void;
  projectInitialized: boolean;
  sessionId: string;
  onInitProject: () => Promise<void>;
  onResumeSession: (sessionId: string) => Promise<void>;
  listSessions: () => Promise<SessionSummary[]>;
  onGetApiConfig: () => Promise<ApiConfig>;
  onSetApiConfig: (apiKey: string, apiBase: string) => Promise<void>;
}

export function SettingsPanel({
  open,
  onClose,
  projectInitialized,
  sessionId,
  onInitProject,
  onResumeSession,
  listSessions,
  onGetApiConfig,
  onSetApiConfig,
}: SettingsPanelProps) {
  const [resumeId, setResumeId] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [search, setSearch] = useState("");

  // API config state
  const [apiKey, setApiKey] = useState("");
  const [apiBase, setApiBase] = useState("https://api.deepseek.com/v1");
  const [showKey, setShowKey] = useState(false);
  const [apiDirty, setApiDirty] = useState(false);

  useEffect(() => {
    if (!open) return;
    void listSessions()
      .then(setSessions)
      .catch((e) => setMessage(String(e)));
    void onGetApiConfig()
      .then((c) => {
        setApiBase(c.api_base || "https://api.deepseek.com/v1");
        // Key is masked by backend, only set if user hasn't typed
        if (!apiDirty) setApiKey("");
      })
      .catch(() => {}); // No config yet, use defaults
  }, [open, listSessions, onGetApiConfig]);

  const filtered = useMemo(() => {
    if (!search.trim()) return sessions;
    const q = search.toLowerCase();
    return sessions.filter(
      (s) =>
        s.id.toLowerCase().includes(q) ||
        (s.title ?? "").toLowerCase().includes(q) ||
        s.status.toLowerCase().includes(q),
    );
  }, [sessions, search]);

  if (!open) return null;

  async function run(action: () => Promise<void>, okMsg: string) {
    setBusy(true);
    setMessage(null);
    try {
      await action();
      setMessage(okMsg);
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function onResume(e: FormEvent) {
    e.preventDefault();
    const id = resumeId.trim();
    if (!id) return;
    await run(() => onResumeSession(id), "会话已恢复");
  }

  async function handleSaveApiConfig() {
    if (!apiKey.trim()) {
      setMessage("API Key 不能为空");
      return;
    }
    await run(
      () => onSetApiConfig(apiKey.trim(), apiBase.trim() || "https://api.deepseek.com/v1"),
      "API 配置已保存。新会话生效。",
    );
    setApiDirty(false);
  }

  return (
    <div className="settings-overlay" onClick={onClose}>
      <div className="settings-modal" onClick={(e) => e.stopPropagation()}>
        <header className="settings-header">
          <h2>设置</h2>
          <button type="button" onClick={onClose} aria-label="关闭">
            ×
          </button>
        </header>

        <section>
          <h3>API 配置</h3>
          <p className="settings-hint">
            API Key 保存在 agent 全局配置（`.novel-agent/api_config.json`），不会上传。环境变量 `DEEPSEEK_API_KEY` 优先级更高。
          </p>
          <label className="settings-label">
            API Key
            <div className="api-key-row">
              <input
                type={showKey ? "text" : "password"}
                value={apiKey}
                onChange={(e) => { setApiKey(e.target.value); setApiDirty(true); }}
                placeholder="sk-…"
                disabled={busy}
              />
              <button
                type="button"
                className="toggle-vis"
                onClick={() => setShowKey((v) => !v)}
              >
                {showKey ? "隐藏" : "显示"}
              </button>
            </div>
          </label>
          <label className="settings-label">
            API URL
            <input
              type="text"
              value={apiBase}
              onChange={(e) => { setApiBase(e.target.value); setApiDirty(true); }}
              placeholder="https://api.deepseek.com/v1"
              disabled={busy}
            />
          </label>
          <button
            type="button"
            disabled={busy || (!apiKey.trim() && !apiDirty)}
            onClick={() => void handleSaveApiConfig()}
          >
            保存 API 配置
          </button>
        </section>

        <section>
          <h3>项目</h3>
          <p className="settings-hint">
            {projectInitialized
              ? "知识库目录与模板文件已就绪。"
              : "尚未初始化作品目录，请先创建脚手架。"}
          </p>
          <button
            type="button"
            disabled={busy}
            onClick={() => void run(onInitProject, "项目脚手架已创建")}
          >
            初始化小说项目
          </button>
        </section>

        <section>
          <h3>恢复会话</h3>
          <p className="settings-hint">当前会话 ID: {sessionId || "—"}</p>
          {sessions.length > 0 && (
            <>
              <input
                type="text"
                className="session-search"
                placeholder="搜索会话…"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                disabled={busy}
              />
              <ul className="session-list">
                {filtered.map((s) => (
                  <li key={s.id}>
                    <button
                      type="button"
                      disabled={busy}
                      className={s.id === sessionId ? "session-current" : undefined}
                      onClick={() =>
                        void run(() => onResumeSession(s.id), "会话已恢复")
                      }
                    >
                      <span className="session-title">
                        {s.title?.trim() || s.id.slice(0, 8) + "…"}
                      </span>
                      <span className="session-meta">
                        对话 {s.total_turns} 轮 · API {s.api_call_count} 次 · {s.status}
                      </span>
                    </button>
                  </li>
                ))}
                {filtered.length === 0 && (
                  <li className="session-empty">无匹配会话</li>
                )}
              </ul>
            </>
          )}
          <form onSubmit={(e) => void onResume(e)} className="resume-form">
            <input
              type="text"
              placeholder="粘贴 session UUID（手动恢复）"
              value={resumeId}
              onChange={(e) => setResumeId(e.target.value)}
              disabled={busy}
            />
            <button type="submit" disabled={busy || !resumeId.trim()}>
              恢复
            </button>
          </form>
        </section>

        {message && <p className="settings-message">{message}</p>}
      </div>
    </div>
  );
}
