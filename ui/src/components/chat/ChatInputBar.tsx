import { FormEvent } from "react";

const MODE_TOOLTIPS: Record<string, string> = {
  normal: "常规模式：读取文件自动执行，写入/编辑文件需作者确认",
  plan: "策划模式：可只读全书；Write/Edit 仅允许 plan/ 目录（类似 Cursor Plan）。写 knowledge/、chapters/ 请切回其他模式",
  auto: "自动模式：所有操作自动批准，但关键决策问题仍会弹出询问作者",
  unattended: "无人值守模式：全自动执行。关键决策问题不再弹窗，Agent 自行分析选项并决策，决策过程在对话中可见",
};

export function ChatInputBar({
  input,
  setInput,
  onSubmit,
  pendingQuestion,
  isStreaming,
  hasInput,
  canSubmitInterrupt,
  submitBlockedByTools,
  permissionMode,
  modeSwitchBlocked,
  onSetPermissionMode,
  model,
  setModel,
  turnInProgress,
  onInterrupt,
}: {
  input: string;
  setInput: (value: string) => void;
  onSubmit: (e: FormEvent) => void | Promise<void>;
  pendingQuestion: boolean;
  isStreaming: boolean;
  hasInput: boolean;
  canSubmitInterrupt: boolean;
  submitBlockedByTools: boolean;
  permissionMode: string;
  modeSwitchBlocked: boolean;
  onSetPermissionMode: (mode: string) => Promise<void>;
  model: string;
  setModel: (model: string) => void;
  turnInProgress: boolean;
  onInterrupt: () => void;
}) {
  function onKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void onSubmit(e as unknown as FormEvent);
    }
  }

  return (
    <form className="input-box" onSubmit={onSubmit}>
      <textarea
        value={input}
        onChange={(e) => setInput(e.target.value)}
        onKeyDown={onKeyDown}
        placeholder={
          pendingQuestion
            ? "请先回答上方问题…"
            : isStreaming
              ? "可输入下一条指令（Ctrl+Enter 发送）…"
              : "输入创作指令… (Ctrl+Enter 发送)"
        }
        rows={3}
        disabled={pendingQuestion}
      />
      <div className="actions">
        <select
          className="mode-select"
          value={permissionMode}
          onChange={(e) => void onSetPermissionMode(e.target.value)}
          disabled={modeSwitchBlocked}
          title={
            modeSwitchBlocked
              ? "当前轮次进行中，结束后或中断后才可切换权限模式"
              : MODE_TOOLTIPS[permissionMode] ?? permissionMode
          }
        >
          <option value="normal">常规</option>
          <option value="plan">策划</option>
          <option value="auto">自动</option>
          <option value="unattended">无人值守</option>
        </select>
        <select
          className="model-select"
          value={model}
          onChange={(e) => setModel(e.target.value)}
          disabled={turnInProgress}
          title={
            turnInProgress
              ? "当前轮次进行中，结束后才可切换模型"
              : "选择模型（切换模型将导致 KV Cache 失效）"
          }
        >
          <option value="deepseek-v4-pro">v4-pro</option>
          <option value="deepseek-v4-flash">v4-flash</option>
        </select>
        {isStreaming && !hasInput && (
          <button type="button" className="interrupt-btn" onClick={onInterrupt}>
            中断
          </button>
        )}
        {isStreaming && hasInput && (
          <>
            <button type="button" className="interrupt-btn" onClick={onInterrupt}>
              中断
            </button>
            <button
              type="submit"
              disabled={!canSubmitInterrupt}
              title={
                submitBlockedByTools
                  ? "当前工具执行中，请等待或使用「中断」"
                  : "发送并打断当前轮次"
              }
            >
              发送
            </button>
          </>
        )}
        {!isStreaming && (
          <button type="submit" disabled={!input.trim() || pendingQuestion}>
            发送
          </button>
        )}
      </div>
    </form>
  );
}
