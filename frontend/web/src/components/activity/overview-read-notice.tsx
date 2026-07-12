"use client";

import type { ActivityReadState } from "@/lib/activity/use-activity-overview";

export function ActivityOverviewReadNotice({
  state,
  retrying,
  onRetry,
}: {
  state: ActivityReadState;
  retrying: boolean;
  onRetry: () => void;
}) {
  if (state === "ready") return null;
  if (state === "loading") {
    return (
      <div role="status" aria-live="polite" style={noticeStyle}>
        loading activity totals…
      </div>
    );
  }

  const stale = state === "stale";
  return (
    <div
      role={stale ? "status" : "alert"}
      aria-live={stale ? "polite" : undefined}
      style={noticeStyle}
    >
      <span>
        {stale
          ? "activity totals refresh failed · showing saved data"
          : "some activity totals are unavailable · missing values are shown as —"}
      </span>
      <button
        type="button"
        disabled={retrying}
        onClick={onRetry}
        style={retryStyle(retrying)}
      >
        {retrying ? "retrying…" : "retry"}
      </button>
    </div>
  );
}

const noticeStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "var(--space-3)",
  margin: "0 24px",
  padding: "var(--space-3)",
  border: "1px solid color-mix(in srgb, var(--warn) 45%, var(--border-1))",
  borderRadius: "var(--radius-sm)",
  color: "var(--warn)",
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-12)",
};

function retryStyle(disabled: boolean): React.CSSProperties {
  return {
    minHeight: 32,
    padding: "0 var(--space-3)",
    border: "1px solid var(--border-2)",
    borderRadius: "var(--radius-sm)",
    background: "var(--surface-2)",
    color: "var(--fg-1)",
    font: "inherit",
    cursor: disabled ? "wait" : "pointer",
  };
}
