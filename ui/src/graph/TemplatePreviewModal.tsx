type Props = {
  json: string;
  busy: boolean;
  hasPlan: boolean;
  onClose: () => void;
  onApply: (force: boolean) => void;
};

/** Preview bundled plan-graph template; apply writes formal plan. */
export function TemplatePreviewModal({ json, busy, hasPlan, onClose, onApply }: Props) {
  return (
    <div className="graph-modal-backdrop" role="presentation" onClick={onClose}>
      <div
        className="graph-modal graph-modal--wide"
        role="dialog"
        aria-labelledby="template-preview-title"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="graph-modal-header">
          <h2 id="template-preview-title">默认 Plan Graph 模板</h2>
          <button type="button" className="graph-modal-close" onClick={onClose} aria-label="关闭">
            ×
          </button>
        </header>
        <p className="graph-modal-hint">
          仅预览，不会静默写入作品。确认后可用「应用模板」落盘为正式{" "}
          <code>plan-graph.json</code>。
        </p>
        <pre className="graph-template-pre">{json}</pre>
        <footer className="graph-modal-footer">
          <button type="button" onClick={onClose} disabled={busy}>
            取消
          </button>
          {hasPlan ? (
            <button
              type="button"
              className="graph-modal-danger"
              disabled={busy}
              onClick={() => onApply(true)}
              title="替换现有正式图"
            >
              {busy ? "应用中…" : "强制替换"}
            </button>
          ) : (
            <button
              type="button"
              className="graph-modal-primary"
              disabled={busy}
              onClick={() => onApply(false)}
            >
              {busy ? "应用中…" : "应用模板"}
            </button>
          )}
        </footer>
      </div>
    </div>
  );
}
