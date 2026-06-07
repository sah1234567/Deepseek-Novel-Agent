import { formatRetainedTurnLabel } from "../../utils/compactionRetainedLabel";
import "./CompactionDivider.css";

export function CompactionDivider({
  epoch,
  retainedMinTurn,
  retainedMaxTurn,
}: {
  epoch: number;
  retainedMinTurn?: number;
  retainedMaxTurn?: number;
}) {
  const retained = formatRetainedTurnLabel(retainedMinTurn, retainedMaxTurn);
  const label = retained
    ? `上下文已压缩 · 第 ${epoch} 次 · ${retained}`
    : `上下文已压缩 · 第 ${epoch} 次`;

  return (
    <div className="compaction-divider" role="separator" aria-label={label}>
      <span>{label}</span>
    </div>
  );
}
