"use client";

import { CategoryTabs } from "./category-tabs";

export type SortKey = "volume" | "new" | "traders";

export const SORT_KEYS: readonly SortKey[] = [
  "volume",
  "new",
  "traders",
] as const;

export function parseSortKey(raw: string | null | undefined): SortKey {
  if (raw && (SORT_KEYS as readonly string[]).includes(raw)) {
    return raw as SortKey;
  }
  return "volume";
}

type ChipDef = {
  key: SortKey;
  label: string;
  disabled?: boolean;
  title?: string;
};

const SORTS: ChipDef[] = [
  { key: "volume", label: "Volume" },
  { key: "new", label: "New" },
  { key: "traders", label: "Traders" },
];

type Props = {
  sort: SortKey;
  onSortChange: (s: SortKey) => void;
};

export function MarketsFilterBar({ sort, onSortChange }: Props) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: "var(--space-5)",
        padding: "var(--space-3) 0",
        borderTop: "1px solid var(--border-1)",
        borderBottom: "1px solid var(--border-1)",
      }}
    >
      <CategoryTabs />

      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: "var(--space-2)",
          flexShrink: 0,
        }}
      >
        {SORTS.map((s) => {
          const active = sort === s.key;
          return (
            <button
              key={s.key}
              type="button"
              disabled={s.disabled}
              title={s.title}
              onClick={() => !s.disabled && onSortChange(s.key)}
              style={{
                height: 26,
                padding: "0 var(--space-3)",
                background: active ? "var(--surface-2)" : "transparent",
                color: active
                  ? "var(--fg-1)"
                  : s.disabled
                    ? "var(--fg-4)"
                    : "var(--fg-3)",
                border: `1px solid ${
                  active ? "var(--border-3)" : "var(--border-2)"
                }`,
                borderRadius: "var(--radius-sm)",
                fontFamily: "var(--font-mono)",
                fontSize: "11px",
                letterSpacing: "var(--track-wide)",
                textTransform: "uppercase",
                cursor: s.disabled
                  ? "not-allowed"
                  : active
                    ? "default"
                    : "pointer",
                transition: "all var(--dur-fast) var(--ease-standard)",
              }}
            >
              {s.label}
            </button>
          );
        })}
      </div>
    </div>
  );
}
