/**
 * Click-to-sort helpers shared by every account table — the four portfolio tabs
 * and the three market-detail lists (holdings / open orders / closed orders).
 *
 * Seven components had grown their own `nextSort` and `cmpBig`, plus a
 * hand-rolled four-guard null comparison per nullable column. They agreed, but
 * only by coincidence.
 */

export type SortDir = "asc" | "desc";

export interface Sort<K extends string> {
  key: K;
  dir: SortDir;
}

export interface Column<K extends string> {
  key: K;
  label: string;
  align: "left" | "right";
  /** Glossary term to hang a `?` badge off, if the column needs explaining. */
  info?: string;
}

/**
 * Next sort state for a header click: re-clicking a column flips direction,
 * otherwise text columns open A→Z and numeric columns open high→low.
 */
export function nextSort<K extends string>(
  prev: Sort<K> | null,
  key: K,
  numeric: boolean,
): Sort<K> {
  if (prev && prev.key === key) {
    return { key, dir: prev.dir === "asc" ? "desc" : "asc" };
  }
  return { key, dir: numeric ? "desc" : "asc" };
}

/** Ascending bigint comparison, for the `compareBy` in each table. */
export function cmpBig(a: bigint, b: bigint): number {
  return a > b ? 1 : a < b ? -1 : 0;
}

/**
 * Ascending comparison of two nullable bigints, nulls lowest. Every table has a
 * handful of columns that are unknown for some rows (no fill price yet, no
 * realized PnL on a buy) and each had hand-rolled the same four guards.
 */
export function cmpNullableBig(
  a: bigint | null | undefined,
  b: bigint | null | undefined,
): number {
  if (a == null && b == null) return 0;
  if (a == null) return -1;
  if (b == null) return 1;
  return cmpBig(a, b);
}
