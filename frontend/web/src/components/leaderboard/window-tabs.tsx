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
      /* Three 11px labels in one track — the coarse-pointer floor made it a
         44px slab under the page title. See `.hit-target-group`. */
      className="hit-target-group"
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
            onMouseEnter={(e) => {
              if (!active) e.currentTarget.style.color = "var(--fg-1)";
            }}
            onMouseLeave={(e) => {
              if (!active) e.currentTarget.style.color = "var(--fg-3)";
            }}
            style={{
              padding: "4px 12px",
              minHeight: 28,
              border: 0,
              borderRadius: 3,
              background: active ? "var(--surface-2)" : "transparent",
              color: active ? "var(--fg-1)" : "var(--fg-3)",
              fontFamily: "var(--font-mono)",
              fontSize: 11,
              letterSpacing: "var(--track-wide)",
              cursor: "pointer",
              transition:
                "background var(--dur-fast) var(--ease-standard), color var(--dur-fast) var(--ease-standard)",
            }}
          >
            {w}
          </button>
        );
      })}
    </div>
  );
}
