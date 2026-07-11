"use client";

import { CategoryTabs } from "./category-tabs";

import type { SortKey } from "@/lib/markets/sort";

export { SORT_KEYS, parseSortKey } from "@/lib/markets/sort";
export type { SortKey } from "@/lib/markets/sort";

type ChipDef = {
  key: SortKey;
  label: string;
  disabled?: boolean;
};

const SORTS: ChipDef[] = [
  { key: "volume", label: "Volume" },
  { key: "traders", label: "Traders" },
];

type Props = {
  sort: SortKey;
  onSortChange: (s: SortKey) => void;
  hideClosed: boolean;
  onHideClosedChange: (hide: boolean) => void;
};

export function MarketsFilterBar({
  sort,
  onSortChange,
  hideClosed,
  onHideClosedChange,
}: Props) {
  return (
    <div
      className="markets-filter-bar"
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
        className="markets-sort-controls"
        style={{
          display: "flex",
          alignItems: "center",
          gap: "var(--space-2)",
          flexShrink: 0,
        }}
      >
        {SORTS.map((s) => (
          <Chip
            key={s.key}
            active={sort === s.key}
            disabled={s.disabled ?? false}
            onClick={() => {
              if (!s.disabled) onSortChange(s.key);
            }}
          >
            {s.label}
          </Chip>
        ))}

        <Divider />

        <Chip
          active={hideClosed}
          aria-pressed={hideClosed}
          title={
            hideClosed
              ? "Closed markets hidden — click to show them greyed out"
              : "Closed markets shown — click to hide"
          }
          onClick={() => onHideClosedChange(!hideClosed)}
        >
          Hide closed
        </Chip>
      </div>
    </div>
  );
}

/** Shared pill styling for the sort chips and the on/off toggles. */
function Chip({
  active,
  disabled,
  title,
  onClick,
  children,
  ...rest
}: {
  active: boolean;
  disabled?: boolean | undefined;
  title?: string | undefined;
  onClick: () => void;
  children: React.ReactNode;
  "aria-pressed"?: boolean | undefined;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      title={title}
      onClick={onClick}
      style={{
        height: 26,
        padding: "0 var(--space-3)",
        background: active ? "var(--surface-2)" : "transparent",
        color: active ? "var(--fg-1)" : disabled ? "var(--fg-4)" : "var(--fg-3)",
        border: `1px solid ${active ? "var(--border-3)" : "var(--border-2)"}`,
        borderRadius: "var(--radius-sm)",
        fontFamily: "var(--font-mono)",
        fontSize: "11px",
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
        cursor: disabled ? "not-allowed" : "pointer",
        transition: "all var(--dur-fast) var(--ease-standard)",
      }}
      {...rest}
    >
      {children}
    </button>
  );
}

function Divider() {
  return (
    <span
      aria-hidden
      style={{
        width: 1,
        height: 16,
        background: "var(--border-2)",
        margin: "0 var(--space-1)",
      }}
    />
  );
}
