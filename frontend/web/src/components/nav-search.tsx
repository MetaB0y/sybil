"use client";

import { useRouter, useSearchParams } from "next/navigation";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
  type KeyboardEvent,
} from "react";
import { useMarketsIndex, type IndexMarket } from "@/lib/markets/use-markets";
import { buildIndexCards } from "@/lib/markets/build-index-cards";
import {
  selectIndexCards,
  type CardItem,
} from "@/lib/markets/select-index-cards";
import { selectPricesByMarketId, useStore } from "@/lib/store";
import { formatCompactDollars, formatPercent } from "@/lib/format/nanos";
import { getCategoryColor } from "@/lib/categorize";
import { MarketThumb } from "./market-thumb";

/** Rows rendered in the dropdown before the "see all" footer. */
const MAX_RESULTS = 8;
/** Trader counts don't matter for the volume-sorted preview. */
const EMPTY_TRADERS: Map<string, number> = new Map();

/**
 * Global search. Typing opens a dropdown preview of matching events/markets —
 * it does NOT navigate, so the page the user is exploring stays put. Enter
 * commits the query to the markets grid (`/?q=`); arrow-keys + Enter (or a
 * click) jump straight to a specific market. `/` focuses the box from anywhere.
 *
 * The preview filters with the same defaults a fresh `/?q=` landing uses
 * (volume sort, no category, open-only) so what you see is what the grid shows.
 */
export function NavSearch() {
  const searchParams = useSearchParams();
  const router = useRouter();
  const urlQ = searchParams.get("q") ?? "";

  const [q, setQ] = useState(urlQ);
  const [open, setOpen] = useState(false);
  // -1 = nothing highlighted → Enter goes to the grid (the default action).
  const [highlight, setHighlight] = useState(-1);

  const inputRef = useRef<HTMLInputElement>(null);
  const shellRef = useRef<HTMLDivElement>(null);

  // Reflect the grid's `?q=` into the box when the URL changes underneath us
  // (shared link, back/forward) — but never while the user is mid-type with
  // the dropdown open, or we'd clobber what they're typing. Render-phase sync
  // (not an effect) so the box is correct on first paint, no extra render.
  const [prevUrlQ, setPrevUrlQ] = useState(urlQ);
  if (urlQ !== prevUrlQ && !open) {
    setPrevUrlQ(urlQ);
    setQ(urlQ);
  }

  const { bundle } = useMarketsIndex();
  const prices = useStore(selectPricesByMarketId);
  const items = useMemo(
    () => (bundle ? buildIndexCards(bundle) : []),
    [bundle],
  );

  const results = useMemo(() => {
    if (!q.trim()) return [] as CardItem[];
    return selectIndexCards(items, {
      query: q,
      sort: "volume",
      category: null,
      showClosed: false,
      eventTraders: EMPTY_TRADERS,
    });
  }, [items, q]);
  const top = results.slice(0, MAX_RESULTS);

  const close = useCallback(() => {
    setOpen(false);
    setHighlight(-1);
  }, []);

  const goToGrid = useCallback(
    (query: string) => {
      const v = query.trim();
      // Start clean so the landing set matches the preview (the dropdown
      // ignores category/closed); preserve only the active sort.
      const params = new URLSearchParams();
      const sort = searchParams.get("sort");
      if (sort) params.set("sort", sort);
      if (v) params.set("q", v);
      const qs = params.toString();
      router.push(qs ? `/?${qs}` : "/", { scroll: false });
      close();
      inputRef.current?.blur();
    },
    [router, searchParams, close],
  );

  const goToItem = useCallback(
    (item: CardItem) => {
      const id =
        item.kind === "binary"
          ? item.market.market_id
          : pickLeaderId(item.markets);
      router.push(`/m/${id}`, { scroll: false });
      close();
      inputRef.current?.blur();
    },
    [router, close],
  );

  const onChange = useCallback((e: ChangeEvent<HTMLInputElement>) => {
    setQ(e.target.value);
    setOpen(true);
    setHighlight(-1);
  }, []);

  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLInputElement>) => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setOpen(true);
        setHighlight((h) => Math.min(h + 1, top.length - 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setHighlight((h) => Math.max(h - 1, -1));
      } else if (e.key === "Enter") {
        const item = highlight >= 0 ? top[highlight] : undefined;
        if (item) goToItem(item);
        else goToGrid(q);
      } else if (e.key === "Escape") {
        if (open) close();
        else {
          setQ("");
          inputRef.current?.blur();
        }
      }
    },
    [top, highlight, q, open, goToItem, goToGrid, close],
  );

  // `/` focuses search from anywhere — the leading glyph promises it. Ignore
  // when the user is already typing somewhere (input/textarea/contentEditable).
  useEffect(() => {
    const onSlash = (e: globalThis.KeyboardEvent) => {
      if (e.key !== "/" || e.metaKey || e.ctrlKey || e.altKey) return;
      const t = e.target as HTMLElement | null;
      const tag = t?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || t?.isContentEditable) return;
      e.preventDefault();
      inputRef.current?.focus();
    };
    document.addEventListener("keydown", onSlash);
    return () => document.removeEventListener("keydown", onSlash);
  }, []);

  // Click outside closes the dropdown.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!shellRef.current?.contains(e.target as Node)) close();
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open, close]);

  const showDropdown = open && q.trim().length > 0;

  return (
    <div ref={shellRef} style={{ position: "relative" }}>
      <div className="nav-search-shell" style={searchShellStyle}>
        <span aria-hidden className="text-mono" style={searchSlashStyle}>
          /
        </span>
        <input
          ref={inputRef}
          value={q}
          onChange={onChange}
          onKeyDown={onKeyDown}
          onFocus={() => {
            if (q.trim()) setOpen(true);
          }}
          placeholder="search events, markets"
          aria-label="search markets"
          role="combobox"
          aria-expanded={showDropdown}
          aria-controls="nav-search-results"
          aria-autocomplete="list"
          style={searchInputStyle}
        />
      </div>

      {showDropdown && (
        <div
          id="nav-search-results"
          role="listbox"
          className="nav-search-dropdown"
          style={dropdownStyle}
        >
          {top.length === 0 ? (
            <div style={emptyStyle}>
              no events or markets match “{q.trim()}”
            </div>
          ) : (
            <>
              {top.map((item, i) => (
                <ResultRow
                  key={resultKey(item)}
                  item={item}
                  prices={prices}
                  active={i === highlight}
                  onPick={() => goToItem(item)}
                  onHover={() => setHighlight(i)}
                />
              ))}
              <button
                type="button"
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => goToGrid(q)}
                style={footerStyle}
              >
                <span>
                  see all {results.length} result
                  {results.length === 1 ? "" : "s"}
                </span>
                <span aria-hidden>↵</span>
              </button>
            </>
          )}
        </div>
      )}
    </div>
  );
}

export function NavSearchSkeleton() {
  return (
    <div className="nav-search-shell" style={searchShellStyle} aria-hidden />
  );
}

function ResultRow({
  item,
  prices,
  active,
  onPick,
  onHover,
}: {
  item: CardItem;
  prices: Record<number, { yes: bigint; no: bigint }>;
  active: boolean;
  onPick: () => void;
  onHover: () => void;
}) {
  const name = item.kind === "binary" ? item.market.name : item.name;
  const thumb = thumbProps(item);
  const vol =
    item.volumeNanos > 0n ? formatCompactDollars(item.volumeNanos) : "—";

  // Binary → YES odds; multi → outcome count (the per-outcome prices live on
  // the detail/card, not in a single nav row).
  let detail: string;
  if (item.kind === "binary") {
    const p = prices[item.market.market_id];
    detail = p ? formatPercent(p.yes) : "—";
  } else {
    detail = `${item.markets.length} outcomes`;
  }

  return (
    <button
      type="button"
      role="option"
      aria-selected={active}
      // mousedown would blur the input (closing the dropdown) before click
      // fires — prevent it so the navigation in onClick runs.
      onMouseDown={(e) => e.preventDefault()}
      onMouseEnter={onHover}
      onClick={onPick}
      style={{
        ...rowStyle,
        background: active ? "var(--surface-2)" : "transparent",
      }}
    >
      <MarketThumb
        marketId={thumb.id}
        name={name}
        imageUrl={thumb.imageUrl}
        fallbackIconUrl={thumb.fallbackIconUrl}
        size={28}
      />
      <span style={rowNameStyle}>
        {item.primaryCategory && (
          <span
            aria-hidden
            style={{
              width: 6,
              height: 6,
              borderRadius: "50%",
              background: getCategoryColor(item.primaryCategory),
              flexShrink: 0,
            }}
          />
        )}
        <span style={rowTitleStyle}>{name}</span>
      </span>
      <span className="text-mono tabular" style={rowMetaStyle}>
        <span style={{ color: "var(--fg-2)" }}>{detail}</span>
        <span style={{ color: "var(--fg-4)" }}>{vol}</span>
      </span>
    </button>
  );
}

/** Leader = open outcome with the most volume (matches MultiCard's ranking). */
function pickLeaderId(markets: IndexMarket[]): number {
  let best = markets[0]!;
  for (const m of markets) {
    const bClosed = best.closed === true ? 1 : 0;
    const mClosed = m.closed === true ? 1 : 0;
    if (mClosed !== bClosed) {
      if (mClosed < bClosed) best = m;
      continue;
    }
    const bv = best.volume_nanos ? BigInt(best.volume_nanos) : 0n;
    const mv = m.volume_nanos ? BigInt(m.volume_nanos) : 0n;
    if (mv > bv) best = m;
  }
  return best.market_id;
}

function thumbProps(item: CardItem): {
  id: number;
  imageUrl: string | null;
  fallbackIconUrl: string | null;
} {
  if (item.kind === "binary") {
    return {
      id: item.market.market_id,
      imageUrl: item.market.market_image_url ?? null,
      fallbackIconUrl: item.market.market_icon_url ?? null,
    };
  }
  const first = item.markets[0]!;
  return {
    id: first.market_id,
    imageUrl: first.event_image_url ?? null,
    fallbackIconUrl: first.event_icon_url ?? null,
  };
}

function resultKey(item: CardItem): string {
  return item.kind === "binary"
    ? `m${item.market.market_id}`
    : `e${item.eventId}`;
}

const searchShellStyle: React.CSSProperties = {};

const searchSlashStyle: React.CSSProperties = {
  color: "var(--fg-4)",
  fontSize: "var(--fs-12)",
  letterSpacing: "var(--track-wide)",
  marginRight: "var(--space-2)",
};

const searchInputStyle: React.CSSProperties = {
  flex: 1,
  background: "transparent",
  border: 0,
  outline: "none",
  color: "var(--fg-1)",
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-12)",
  padding: 0,
};

const dropdownStyle: React.CSSProperties = {};

const rowStyle: React.CSSProperties = {
  display: "grid",
  gridTemplateColumns: "28px minmax(0, 1fr) auto",
  alignItems: "center",
  gap: "var(--space-3)",
  width: "100%",
  minHeight: 40,
  padding: "var(--space-2) var(--space-2)",
  background: "transparent",
  border: 0,
  borderRadius: "var(--radius-sm)",
  cursor: "pointer",
  textAlign: "left",
};

const rowNameStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "var(--space-2)",
  minWidth: 0,
};

const rowTitleStyle: React.CSSProperties = {
  fontFamily: "var(--font-sans)",
  fontSize: "var(--fs-13)",
  color: "var(--fg-1)",
  whiteSpace: "nowrap",
  overflow: "hidden",
  textOverflow: "ellipsis",
};

const rowMetaStyle: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  alignItems: "flex-end",
  gap: 2,
  fontSize: "11px",
  whiteSpace: "nowrap",
};

const emptyStyle: React.CSSProperties = {
  padding: "var(--space-3)",
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-12)",
  color: "var(--fg-3)",
};

const footerStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  width: "100%",
  marginTop: 2,
  minHeight: 40,
  padding: "var(--space-2) var(--space-2)",
  background: "transparent",
  border: 0,
  borderTop: "1px solid var(--border-1)",
  borderRadius: 0,
  cursor: "pointer",
  fontFamily: "var(--font-mono)",
  fontSize: "11px",
  letterSpacing: "var(--track-wide)",
  textTransform: "uppercase",
  color: "var(--fg-3)",
};
