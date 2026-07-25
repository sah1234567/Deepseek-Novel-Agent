import { useEffect, useState } from "react";
import type { LoopHistoryRow } from "./types";

type LoopHistoryDrawerProps = {
  loopId: string;
  open: boolean;
  onClose: () => void;
  listHistory: (loopId: string, limit?: number) => Promise<LoopHistoryRow[]>;
};

export function LoopHistoryDrawer({
  loopId,
  open,
  onClose,
  listHistory,
}: LoopHistoryDrawerProps) {
  const [rows, setRows] = useState<LoopHistoryRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!open || !loopId) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    void listHistory(loopId, 50)
      .then((data) => {
        if (!cancelled) setRows(data);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, loopId, listHistory]);

  if (!open) return null;

  return (
    <div
      role="dialog"
      aria-label="Loop history"
      style={{
        position: "absolute",
        top: 0,
        right: 0,
        bottom: 0,
        width: 260,
        zIndex: 10,
        background: "#0f172a",
        borderLeft: "1px solid #334155",
        display: "flex",
        flexDirection: "column",
        fontSize: 11,
        color: "#e2e8f0",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          padding: "8px 10px",
          borderBottom: "1px solid #334155",
        }}
      >
        <strong>History · {loopId}</strong>
        <button type="button" onClick={onClose} aria-label="Close history">
          ×
        </button>
      </div>
      <div style={{ flex: 1, overflow: "auto", padding: 8 }}>
        {loading ? <p style={{ margin: 0, opacity: 0.7 }}>Loading…</p> : null}
        {error ? <p style={{ margin: 0, color: "#f87171" }}>{error}</p> : null}
        {!loading && !error && rows.length === 0 ? (
          <p style={{ margin: 0, opacity: 0.7 }}>No chapter history yet.</p>
        ) : null}
        <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
          {rows.map((row) => (
            <li
              key={`${row.chapter}-${row.artifactPath}`}
              style={{
                padding: "6px 0",
                borderBottom: "1px solid #1e293b",
              }}
            >
              <div style={{ fontWeight: 600 }}>Ch.{row.chapter}</div>
              <div style={{ color: "#94a3b8", wordBreak: "break-all" }}>{row.artifactPath}</div>
              {row.handoffSummaryPreview ? (
                <div style={{ marginTop: 4, opacity: 0.85 }}>{row.handoffSummaryPreview}</div>
              ) : null}
              {row.achievedAt ? (
                <div style={{ marginTop: 2, fontSize: 10, color: "#64748b" }}>{row.achievedAt}</div>
              ) : null}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
