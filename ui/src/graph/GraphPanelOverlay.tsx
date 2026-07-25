import type { ReactNode } from "react";

type Props = {
  onClose: () => void;
  children: ReactNode;
};

/** Full-stage overlay for the Graph canvas (opened from StatusBar). */
export function GraphPanelOverlay({ onClose, children }: Props) {
  return (
    <div className="graph-panel-overlay" role="dialog" aria-label="Plan Graph">
      <div className="graph-panel-toolbar">
        <strong>Plan Graph</strong>
        <button type="button" className="graph-toolbar-btn" onClick={onClose}>
          关闭
        </button>
      </div>
      <div className="graph-panel-body">{children}</div>
    </div>
  );
}
