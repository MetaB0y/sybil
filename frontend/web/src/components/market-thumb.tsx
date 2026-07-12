"use client";

/**
 * Market thumbnail with two-step fallback chain:
 *
 *   imageUrl (404) → fallbackIconUrl (404) → deterministic colored tile
 *
 * Known Polymarket artwork is resized and re-encoded by `next/image`; unknown
 * hosts stay browser-loaded and lazy rather than widening the server-side
 * image-proxy allowlist. Fallback tile uses the first glyph of the market name
 * on a palette-keyed background.
 */

import Image from "next/image";
import { useState } from "react";

type Props = {
  marketId: number;
  name: string;
  imageUrl?: string | null;
  fallbackIconUrl?: string | null;
  size?: number;
};

type Stage = "image" | "icon" | "tile";

const OPTIMIZED_IMAGE_HOST = "polymarket-upload.s3.us-east-2.amazonaws.com";

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

export function MarketThumb({
  marketId,
  name,
  imageUrl,
  fallbackIconUrl,
  size = 40,
}: Props) {
  const initialStage: Stage = imageUrl
    ? "image"
    : fallbackIconUrl
      ? "icon"
      : "tile";
  const [stage, setStage] = useState<Stage>(initialStage);

  // Reset stage when input URLs change (parent reuses this component for a
  // different market). React's documented "reset state when prop changes"
  // pattern: track prev props in state and reset during render rather than
  // in an effect.
  const [prevImage, setPrevImage] = useState(imageUrl);
  const [prevIcon, setPrevIcon] = useState(fallbackIconUrl);
  if (prevImage !== imageUrl || prevIcon !== fallbackIconUrl) {
    setPrevImage(imageUrl);
    setPrevIcon(fallbackIconUrl);
    setStage(initialStage);
  }

  if (stage !== "tile") {
    const src = stage === "image" ? imageUrl! : fallbackIconUrl!;
    const onError = () => {
      if (stage === "image" && fallbackIconUrl) setStage("icon");
      else setStage("tile");
    };
    return (
      <div style={thumbStyles(size)} aria-hidden>
        {isOptimizedImageUrl(src) ? (
          <Image
            src={src}
            alt=""
            width={size}
            height={size}
            sizes={`${size}px`}
            quality={60}
            style={imageStyles}
            onError={onError}
          />
        ) : (
          /* eslint-disable-next-line @next/next/no-img-element -- unknown hosts must not widen the server image proxy */
          <img
            src={src}
            alt=""
            width={size}
            height={size}
            loading="lazy"
            decoding="async"
            style={imageStyles}
            onError={onError}
          />
        )}
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

const imageStyles: React.CSSProperties = {
  width: "100%",
  height: "100%",
  objectFit: "cover",
  display: "block",
};

export function isOptimizedImageUrl(src: string): boolean {
  try {
    const url = new URL(src);
    return (
      url.protocol === "https:" &&
      url.hostname === OPTIMIZED_IMAGE_HOST &&
      url.port === "" &&
      url.search === ""
    );
  } catch {
    return false;
  }
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
