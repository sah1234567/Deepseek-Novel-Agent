import type { GraphNodeView, GraphStateSnapshot, NodeStatus } from "./types";
import "./NodeSessionHeader.css";

type Props = {
  node: GraphNodeView;
  snapshot: GraphStateSnapshot;
  onBack: () => void;
  onStart: () => void;
  onApprove: () => void;
  onReject: () => void;
  onReopen: () => void;
};

function actionsForStatus(status: NodeStatus): Array<"start" | "approve" | "reject" | "reopen"> {
  switch (status) {
    case "ready":
      return ["start"];
    case "awaiting_approval":
      return ["approve", "reject"];
    case "achieved":
    case "failed":
      return ["reopen"];
    default:
      return [];
  }
}

const ACTION_LABEL: Record<"start" | "approve" | "reject" | "reopen", string> = {
  start: "Start",
  approve: "Approve",
  reject: "Reject",
  reopen: "Reopen",
};

export function NodeSessionHeader({
  node,
  snapshot,
  onBack,
  onStart,
  onApprove,
  onReject,
  onReopen,
}: Props) {
  const loop = node.loopId ? snapshot.loops.find((l) => l.loopId === node.loopId) : null;
  const hitl = snapshot.hitl.find((h) => h.nodeId === node.id);
  const actions = actionsForStatus(node.status);
  const runAction = {
    start: onStart,
    approve: onApprove,
    reject: onReject,
    reopen: onReopen,
  };

  const crumbs = [
    node.loopId ? `Loop · ${node.loopId}` : null,
    loop != null ? `Ch.${loop.cursor.chapter}` : null,
    node.title,
  ].filter(Boolean);

  return (
    <header className="node-session-header">
      <div className="node-session-header-main">
        <button type="button" className="node-session-back" onClick={onBack} title="返回 Graph">
          ← Graph
        </button>
        <div className="node-session-meta">
          <div className="node-session-title">{crumbs.join(" · ")}</div>
          <div className="node-session-sub">
            <span className={`node-session-status node-session-status--${node.status}`}>
              {node.status}
            </span>
            {node.cursorBadge ? <span className="node-session-badge">{node.cursorBadge}</span> : null}
            {hitl ? <span className="node-session-hitl">{hitl.label}</span> : null}
          </div>
        </div>
        <div className="node-session-actions">
          {actions.map((a) => (
            <button key={a} type="button" onClick={runAction[a]}>
              {ACTION_LABEL[a]}
            </button>
          ))}
        </div>
      </div>
      {node.effectiveSpec ? (
        <p className="node-session-spec">{node.effectiveSpec}</p>
      ) : null}
    </header>
  );
}
