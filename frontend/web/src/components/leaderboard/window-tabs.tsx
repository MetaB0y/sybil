"use client";

import type { LeaderboardWindow } from "@/lib/leaderboard/use-leaderboard";

const WINDOWS: LeaderboardWindow[] = ["7D", "30D", "ALL"];

export function WindowTabs({
  value,
  onChange,
}: {
  value: LeaderboardWindow;
  onChange: (w: LeaderboardWindow) => void;
}) {
  return (
    <div
      style={{
        display: "inline-flex",
        background: "var(--bg-2)",
        border: "1px solid var(--border-1)",
        borderRadius: 4,
        padding: 2,
        gap: 2,
      }}
    >
      {WINDOWS.map((w) => {
        const active = value === w;
        return (
          <button
            key={w}
            type="button"
            onClick={() => onChange(w)}
            style={{
              padding: "4px 12px",
              border: 0,
              borderRadius: 3,
              background: active ? "var(--surface-2)" : "transparent",
              color: active ? "var(--fg-1)" : "var(--fg-3)",
              fontFamily: "var(--font-mono)",
              fontSize: 11,
              letterSpacing: "var(--track-wide)",
              cursor: "pointer",
            }}
          >
            {w}
          </button>
        );
      })}
    </div>
  );
}
