import type { GraphHitlHint } from "./types";

type Props = {
  items: GraphHitlHint[];
  onOpenGraph: () => void;
  onApprove: (nodeId: string) => void;
  onReject: (nodeId: string) => void;
  onDismiss: () => void;
};

/** Shown when Graph panel is closed and a node needs human approval. */
export function GraphHitlModal({
  items,
  onOpenGraph,
  onApprove,
  onReject,
  onDismiss,
}: Props) {
  if (items.length === 0) return null;
  return (
    <div className="graph-modal-backdrop" role="presentation" onClick={onDismiss}>
      <div
        className="graph-modal"
        role="dialog"
        aria-labelledby="graph-hitl-title"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="graph-modal-header">
          <h2 id="graph-hitl-title">需要人工确认</h2>
          <button type="button" className="graph-modal-close" onClick={onDismiss} aria-label="关闭">
            ×
          </button>
        </header>
        <ul className="graph-hitl-list">
          {items.map((h) => (
            <li key={`${h.nodeId}-${h.kind}`} className="graph-hitl-item">
              <div className="graph-hitl-item-main">
                <strong>{h.label || h.nodeId}</strong>
                <span className="graph-hitl-meta">
                  {h.nodeId} · {h.kind}
                </span>
              </div>
              {h.kind === "node_approval" ? (
                <div className="graph-hitl-actions">
                  <button type="button" onClick={() => onApprove(h.nodeId)}>
                    批准
                  </button>
                  <button type="button" onClick={() => onReject(h.nodeId)}>
                    驳回
                  </button>
                </div>
              ) : null}
            </li>
          ))}
        </ul>
        <footer className="graph-modal-footer">
          <button type="button" className="graph-modal-primary" onClick={onOpenGraph}>
            打开 Graph
          </button>
        </footer>
      </div>
    </div>
  );
}
