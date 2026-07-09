"use client";

/**
 * Market thumbnail with a two-step fallback chain:
 *
 *   imageUrl (404) → fallbackIconUrl (404) → deterministic colored tile
 *
 * The resolved image is painted as a CSS background and only advanced to a new
 * URL once that URL has decoded. Switching markets (e.g. picking a sibling
 * outcome) therefore holds the previous thumbnail until the next one is ready
 * instead of flashing blank — a plain `<img>` blanks the moment its `src`
 * changes. Pure CSS (no next/image) — avoids registering remote domain config
 * for the Polymarket S3 bucket. The tile fallback uses the first glyph of the
 * market name on a palette-keyed background.
 */

import { useEffect, useMemo, useState } from "react";

type Props = {
  marketId: number;
  name: string;
  imageUrl?: string | null;
  fallbackIconUrl?: string | null;
  size?: number;
};

const PALETTE = [
  "var(--accent-soft)",
  "var(--yes-faint)",
  "var(--no-faint)",
  "var(--info-soft)",
  "var(--warn-soft)",
  "var(--surface-2)",
  "var(--surface-3)",
  "var(--accent-faint)",
];

/**
 * Resolve to the first URL in `urls` that decodes, or `null` if none do.
 * Sequential (recursive, no await-in-loop) so the fallback order is honoured:
 * try the market image, then the icon, then give up to the tile.
 */
function firstLoadable(urls: string[]): Promise<string | null> {
  if (urls.length === 0) return Promise.resolve(null);
  const [head, ...rest] = urls;
  return new Promise<boolean>((resolve) => {
    const img = new Image();
    img.onload = () => resolve(true);
    img.onerror = () => resolve(false);
    img.src = head!;
  }).then((ok) => (ok ? head! : firstLoadable(rest)));
}

export function MarketThumb({
  marketId,
  name,
  imageUrl,
  fallbackIconUrl,
  size = 40,
}: Props) {
  const candidates = useMemo(
    () => [imageUrl, fallbackIconUrl].filter((u): u is string => !!u),
    [imageUrl, fallbackIconUrl],
  );

  // The URL currently painted. Seeded with the best candidate for first paint,
  // then only advanced once a candidate has decoded — so it holds across a
  // market switch instead of flashing blank. `null` → deterministic tile.
  const [painted, setPainted] = useState<string | null>(candidates[0] ?? null);

  // The previously painted URL, kept one render behind so the new image can
  // cross-fade in over the old one instead of hard-cutting. Render-safe
  // "previous value" pattern (adjust state during render), not a ref.
  const [prev, setPrev] = useState<string | null>(null);
  const [seen, setSeen] = useState<string | null>(painted);
  if (seen !== painted) {
    setPrev(seen);
    setSeen(painted);
  }

  useEffect(() => {
    let cancelled = false;
    firstLoadable(candidates).then((url) => {
      if (!cancelled) setPainted(url);
    });
    return () => {
      cancelled = true;
    };
  }, [candidates]);

  if (painted) {
    return (
      <div
        aria-hidden
        style={{
          ...thumbStyles(size),
          position: "relative",
          background: "var(--surface-2)",
        }}
      >
        {prev && prev !== painted && (
          <span
            style={{
              position: "absolute",
              inset: 0,
              background: `url("${prev}") center / cover no-repeat`,
            }}
          />
        )}
        <span
          key={painted}
          style={{
            position: "absolute",
            inset: 0,
            background: `url("${painted}") center / cover no-repeat`,
            animation: "sybil-fade-swap var(--dur-swap) var(--ease-standard)",
          }}
        />
      </div>
    );
  }

  const tone = PALETTE[Math.abs(marketId) % PALETTE.length];
  const initial = firstGlyph(name);
  return (
    <div
      style={{
        ...thumbStyles(size),
        background: tone,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        fontFamily: "var(--font-display)",
        fontWeight: 600,
        fontSize: Math.round(size * 0.45),
        lineHeight: 1,
        color: "var(--fg-1)",
        userSelect: "none",
      }}
      aria-hidden
    >
      {initial}
    </div>
  );
}

function thumbStyles(size: number): React.CSSProperties {
  return {
    flexShrink: 0,
    width: size,
    height: size,
    borderRadius: "var(--radius-md)",
    border: "1px solid var(--border-1)",
    overflow: "hidden",
  };
}

function firstGlyph(name: string): string {
  const trimmed = name.trim();
  if (!trimmed) return "·";
  const stripped = trimmed.replace(/^(will|the)\s+/i, "");
  const ch = stripped[0] ?? trimmed[0] ?? "·";
  return ch.toUpperCase();
}
