/**
 * Display category derivation.
 *
 * The backend (sybil-polymarket mirror) does the tag-to-bucket matching at
 * sync time and ships a list like `["Sports", "Politics"]` in
 * `market.categories`. This module owns the **display priority** — which one
 * to surface for the card chip when a market matches multiple buckets.
 *
 * Change this file to reorder priority. No backend rebuild required.
 *
 * Adding a brand-new bucket (or remapping tags) is still a backend change
 * (see `crates/sybil-polymarket/src/categorize.rs`) — but those tweaks are
 * far less frequent than priority shuffles.
 */

/**
 * Priority order: highest first.
 *
 * For a market with `categories = ["Sports", "Politics"]`, we walk this list
 * top-to-bottom and pick the first bucket that's in the market's list. The
 * other matched categories are still accessible (`pickDisplayCategory` also
 * returns them as `extras`) — useful if we ever want a tooltip or
 * multi-chip layout.
 */
export const CATEGORY_PRIORITY: readonly string[] = [
  "Politics",
  "Elections",
  "Geopolitics",
  "AI",
  "Tech",
  "Economy",
  "Culture",
  "Science",
  "World",
  "Finance",
  "Business",
  "Weather",
  "Mentions",
  "Sports",
  "Crypto",
  "Commodities",
] as const;

export type Category = (typeof CATEGORY_PRIORITY)[number];

const CATEGORY_COLORS: Readonly<Record<Category, string>> = {
  Politics: "#9F8FE8",
  Elections: "#B48CF0",
  Geopolitics: "#7FA7E8",
  AI: "#5BC4E0",
  Tech: "#3FB6D9",
  Economy: "#5BD99A",
  Culture: "#E58CC8",
  Science: "#7BCFE8",
  World: "#89A8C8",
  Finance: "#6FCC8A",
  Business: "#A4C86F",
  Weather: "#76B8E8",
  Mentions: "#A0A8B8",
  Sports: "#6FCC8A",
  Crypto: "#F2B244",
  Commodities: "#C9A87C",
};

const FALLBACK_CATEGORY_COLOR = "#7A8494";

export type CategoryPick = {
  /** The single category the card chip should show. `null` when the market has no matched categories. */
  primary: Category | null;
  /** All other matched categories, in priority order, excluding `primary`. */
  extras: Category[];
};

/**
 * Pick the highest-priority category to display for a card chip, plus any
 * additional matched categories (in priority order). Returns `{primary: null, extras: []}`
 * when the market has no `categories` array or none of its categories are in
 * the priority list.
 *
 * Falls back to the legacy singular `category` field when `categories` is
 * absent — that path covers sybil-native (non-mirrored) markets.
 */
export function pickDisplayCategory(
  categories: string[] | null | undefined,
  fallback?: string | null,
): CategoryPick {
  const set = new Set(categories ?? []);
  const matched: Category[] = CATEGORY_PRIORITY.filter((c) => set.has(c));
  if (matched.length > 0) {
    return { primary: matched[0]!, extras: matched.slice(1) };
  }
  // Fall back to the legacy singular `category` field (sybil-native markets).
  if (fallback && (CATEGORY_PRIORITY as readonly string[]).includes(fallback)) {
    return { primary: fallback as Category, extras: [] };
  }
  return { primary: null, extras: [] };
}

export function getCategoryColor(category: string | null | undefined): string {
  if (!category || !(CATEGORY_PRIORITY as readonly string[]).includes(category)) {
    return FALLBACK_CATEGORY_COLOR;
  }
  return CATEGORY_COLORS[category as Category] ?? FALLBACK_CATEGORY_COLOR;
}
