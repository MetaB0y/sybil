"use client";

/**
 * Shared pagination for the market-detail "your holdings" lists (holdings,
 * open orders, closed orders). Each list caps at `PAGE_SIZE` rows per page and
 * renders a `<Pager>` footer when it overflows.
 *
 * `usePaged` is the page-state hook; it auto-clamps the active page when the
 * underlying list shrinks (e.g. an order fills or is cancelled), so callers
 * never have to reconcile a now-out-of-range page themselves.
 */

import type React from "react";
import { useState } from "react";

export const PAGE_SIZE = 10;

export interface Paged<T> {
  /** Active page index, clamped into range. */
  page: number;
  setPage: (p: number) => void;
  pageCount: number;
  total: number;
  /** Zero-based index of the first visible row. */
  start: number;
  /** One-past index of the last visible row. */
  end: number;
  /** The rows for the active page. */
  visible: T[];
}

export function usePaged<T>(items: T[], pageSize = PAGE_SIZE): Paged<T> {
  const [page, setPage] = useState(0);
  const total = items.length;
  const pageCount = Math.max(1, Math.ceil(total / pageSize));
  const current = Math.min(Math.max(0, page), pageCount - 1);
  const start = current * pageSize;
  const visible = items.slice(start, start + pageSize);
  return {
    page: current,
    setPage,
    pageCount,
    total,
    start,
    end: start + visible.length,
    visible,
  };
}

export function Pager<T>({
  paged,
}: {
  paged: Paged<T>;
}) {
  const { page, setPage, pageCount, total, start, end } = paged;
  if (pageCount <= 1) return null;
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: 10,
        paddingTop: 10,
        marginTop: 2,
        borderTop: "1px solid var(--border-1)",
        fontFamily: "var(--font-mono)",
        fontSize: 10,
        color: "var(--fg-4)",
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
      }}
    >
      <span>
        {start + 1}–{end} of {total}
      </span>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <PagerButton disabled={page <= 0} onClick={() => setPage(page - 1)}>
          ‹ prev
        </PagerButton>
        <span style={{ color: "var(--fg-3)" }}>
          {page + 1}/{pageCount}
        </span>
        <PagerButton
          disabled={page >= pageCount - 1}
          onClick={() => setPage(page + 1)}
        >
          next ›
        </PagerButton>
      </div>
    </div>
  );
}

function PagerButton({
  children,
  disabled,
  onClick,
}: {
  children: React.ReactNode;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      style={{
        padding: "3px 8px",
        borderRadius: 3,
        border: "1px solid var(--border-1)",
        background: "var(--bg-2)",
        color: disabled ? "var(--fg-4)" : "var(--fg-2)",
        fontFamily: "var(--font-mono)",
        fontSize: 10,
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
        cursor: disabled ? "not-allowed" : "pointer",
        opacity: disabled ? 0.5 : 1,
      }}
    >
      {children}
    </button>
  );
}
