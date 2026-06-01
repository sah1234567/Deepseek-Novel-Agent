import "./CompactionDivider.css";

export function CompactionDivider({ epoch }: { epoch: number }) {
  return (
    <div className="compaction-divider" role="separator" aria-label={`第 ${epoch} 次上下文压缩`}>
      <span>上下文已压缩 · 第 {epoch} 次</span>
    </div>
  );
}
