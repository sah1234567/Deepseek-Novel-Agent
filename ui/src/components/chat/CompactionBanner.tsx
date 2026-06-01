import { compactionProgressLabel } from "../../utils/compactionMessages";
import type { CompactionBannerState } from "../../hooks/useCompactionProgress";
import "./CompactionBanner.css";

export function CompactionBanner({ state }: { state: CompactionBannerState }) {
  if (!state.visible) return null;
  const label = compactionProgressLabel(state);
  const showSpinner =
    state.action === "started" ||
    state.action === "generating-summary" ||
    state.action === "rebuilding-session";
  return (
    <div
      className={`compaction-banner compaction-banner-${state.variant}`}
      role="status"
      aria-live="polite"
    >
      {showSpinner && <span className="compaction-banner-spinner" aria-hidden />}
      <span>{label}</span>
    </div>
  );
}
