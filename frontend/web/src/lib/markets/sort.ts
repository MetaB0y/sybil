/**
 * Sort keys for the markets index. Lives in the lib layer so domain helpers
 * (e.g. select-index-cards) can depend on it without importing from the UI.
 */

export type SortKey = "volume" | "traders";

export const SORT_KEYS: readonly SortKey[] = ["volume", "traders"] as const;

export function parseSortKey(raw: string | null | undefined): SortKey {
  if (raw && (SORT_KEYS as readonly string[]).includes(raw)) {
    return raw as SortKey;
  }
  return "volume";
}
