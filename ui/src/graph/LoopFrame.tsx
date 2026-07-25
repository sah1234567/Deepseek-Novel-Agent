import type { NodeProps } from "@xyflow/react";
import type { LoopPhase } from "./types";

export type LoopFrameData = {
  loopId: string;
  chapter: number;
  targetChapters?: number | null;
  phase: LoopPhase;
  onPause: () => void;
  onResume: () => void;
};

const PHASE_COLOR: Record<LoopPhase, string> = {
  idle: "#94a3b8",
  running: "#22c55e",
  advancing: "#38bdf8",
  paused: "#f59e0b",
  completed: "#10b981",
};

export function LoopFrame({ data }: NodeProps & { data: LoopFrameData }) {
  const target = data.targetChapters ?? null;
  const progress =
    target && target > 0 ? Math.min(100, Math.round((data.chapter / target) * 100)) : null;

  return (
    <div
      style={{
        width: "100%",
        height: "100%",
        borderRadius: 10,
        border: "1px dashed #475569",
        background: "rgba(15, 23, 42, 0.55)",
        boxSizing: "border-box",
        pointerEvents: "none",
      }}
    >
      <div
        style={{
          pointerEvents: "auto",
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "6px 10px",
          borderBottom: "1px solid #334155",
          fontSize: 11,
          color: "#e2e8f0",
          background: "#0f172a",
          borderRadius: "9px 9px 0 0",
        }}
      >
        <strong style={{ fontSize: 12 }}>{data.loopId}</strong>
        <span style={{ color: "#94a3b8" }}>
          Ch.{data.chapter}
          {target != null ? ` / ${target}` : ""}
        </span>
        <span
          style={{
            color: PHASE_COLOR[data.phase],
            fontWeight: 600,
            textTransform: "capitalize",
          }}
        >
          {data.phase}
        </span>
        {progress != null ? (
          <span
            style={{
              flex: 1,
              height: 6,
              background: "#1e293b",
              borderRadius: 3,
              overflow: "hidden",
              minWidth: 48,
            }}
            title={`${progress}%`}
          >
            <span
              style={{
                display: "block",
                width: `${progress}%`,
                height: "100%",
                background: "#38bdf8",
              }}
            />
          </span>
        ) : (
          <span style={{ flex: 1 }} />
        )}
        {data.phase === "paused" ? (
          <button type="button" onClick={data.onResume}>
            Resume
          </button>
        ) : (
          <button type="button" onClick={data.onPause}>
            Pause
          </button>
        )}
      </div>
    </div>
  );
}
