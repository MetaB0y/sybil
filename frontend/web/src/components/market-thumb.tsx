"use client";

/**
 * Deterministic placeholder tile for a market — colored 40×40 square with
 * the first character of the market's name. Reserves the slot for when
 * backend exposes a real image URL: pass `imageUrl` and the placeholder is
 * skipped. Pure CSS (no next/image) to avoid having to register a remote
 * domain config until we know what host the real images live at.
 */

type Props = {
  marketId: number;
  name: string;
  imageUrl?: string | null;
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

export function MarketThumb({ marketId, name, imageUrl, size = 40 }: Props) {
  const tone = PALETTE[Math.abs(marketId) % PALETTE.length];
  const initial = firstGlyph(name);

  if (imageUrl) {
    return (
      <div
        style={thumbStyles(size)}
        aria-hidden
      >
        {/* eslint-disable-next-line @next/next/no-img-element -- placeholder slot; remote domains not yet decided */}
        <img
          src={imageUrl}
          alt=""
          style={{ width: "100%", height: "100%", objectFit: "cover", display: "block" }}
        />
      </div>
    );
  }

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
  // Skip leading "Will " for binary questions so the glyph is more meaningful.
  const stripped = trimmed.replace(/^(will|the)\s+/i, "");
  const ch = stripped[0] ?? trimmed[0] ?? "·";
  return ch.toUpperCase();
}
