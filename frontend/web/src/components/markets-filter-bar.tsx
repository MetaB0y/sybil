"use client";

import type { ChangeEvent } from "react";

export type SortKey =
  | "volume"
  | "name"
  | "count"
  | "closing"
  | "new";

export const SORT_KEYS: readonly SortKey[] = [
  "volume",
  "name",
  "count",
  "closing",
  "new",
] as const;

export function parseSortKey(raw: string | null | undefined): SortKey {
  if (raw && (SORT_KEYS as readonly string[]).includes(raw)) {
    return raw as SortKey;
  }
  return "volume";
}

const SORTS: { key: SortKey; label: string }[] = [
  { key: "volume", label: "Volume" },
  { key: "name", label: "Name" },
  { key: "count", label: "Outcomes" },
  { key: "closing", label: "Closing soon" },
  { key: "new", label: "New" },
];

type Props = {
  query: string;
  onQueryChange: (q: string) => void;
  sort: SortKey;
  onSortChange: (s: SortKey) => void;
  resultsCount: number;
};

export function MarketsFilterBar({
  query,
  onQueryChange,
  sort,
  onSortChange,
  resultsCount,
}: Props) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: "var(--space-4)",
        flexWrap: "wrap",
        padding: "var(--space-3) 0",
        borderTop: "1px solid var(--border-1)",
        borderBottom: "1px solid var(--border-1)",
      }}
    >
      {/* Search */}
      <div
        style={{
          position: "relative",
          flex: "1 1 280px",
          display: "flex",
          alignItems: "center",
          height: 36,
          background: "var(--surface-1)",
          border: "1px solid var(--border-2)",
          borderRadius: "var(--radius-md)",
          padding: "0 var(--space-3)",
        }}
      >
        <span
          aria-hidden
          className="text-mono"
          style={{
            color: "var(--fg-4)",
            fontSize: "var(--fs-12)",
            letterSpacing: "var(--track-wide)",
            textTransform: "uppercase",
            marginRight: "var(--space-2)",
          }}
        >
          /
        </span>
        <input
          value={query}
          onChange={(e: ChangeEvent<HTMLInputElement>) => onQueryChange(e.target.value)}
          placeholder="search events…"
          aria-label="search markets"
          style={{
            flex: 1,
            background: "transparent",
            border: 0,
            outline: "none",
            color: "var(--fg-1)",
            fontFamily: "var(--font-sans)",
            fontSize: "var(--fs-14)",
            padding: 0,
          }}
        />
        {query.length > 0 && (
          <button
            type="button"
            onClick={() => onQueryChange("")}
            aria-label="clear search"
            style={{
              background: "transparent",
              border: 0,
              color: "var(--fg-3)",
              cursor: "pointer",
              fontFamily: "var(--font-mono)",
              fontSize: "var(--fs-12)",
              padding: "0 var(--space-2)",
            }}
          >
            ×
          </button>
        )}
      </div>

      {/* Sort chips */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: "var(--space-2)",
        }}
      >
        <span className="text-meta" style={{ marginRight: "var(--space-1)" }}>
          Sort
        </span>
        {SORTS.map((s) => {
          const active = sort === s.key;
          return (
            <button
              key={s.key}
              type="button"
              onClick={() => onSortChange(s.key)}
              style={{
                height: 28,
                padding: "0 var(--space-3)",
                background: active ? "var(--accent-soft)" : "var(--surface-1)",
                color: active ? "var(--accent)" : "var(--fg-2)",
                border: `1px solid ${active ? "color-mix(in srgb, var(--accent) 32%, transparent)" : "var(--border-2)"}`,
                borderRadius: "var(--radius-pill)",
                fontFamily: "var(--font-sans)",
                fontSize: "var(--fs-12)",
                cursor: "pointer",
                transition: "all var(--dur-fast) var(--ease-standard)",
              }}
            >
              {s.label}
            </button>
          );
        })}
      </div>

      {/* Result count */}
      <span
        className="text-mono tabular"
        style={{
          marginLeft: "auto",
          color: "var(--fg-3)",
          fontSize: "var(--fs-12)",
        }}
      >
        {resultsCount} {resultsCount === 1 ? "event" : "events"}
      </span>
    </div>
  );
}
