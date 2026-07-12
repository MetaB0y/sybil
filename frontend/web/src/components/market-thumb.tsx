"use client";

/**
 * Market thumbnail with image → icon → deterministic-tile fallback.
 *
 * Index cards keep Next Image's lazy optimized loading. The larger detail-page
 * thumbnail additionally decode-gates sibling switches and cross-fades over the
 * previous artwork, avoiding both a blank flash and eager preloading of every
 * thumbnail in the market grid.
 */

import Image from "next/image";
import { useEffect, useMemo, useState } from "react";

type Props = {
  marketId: number;
  name: string;
  imageUrl?: string | null;
  fallbackIconUrl?: string | null;
  size?: number;
};

type Stage = "image" | "icon" | "tile";

const OPTIMIZED_IMAGE_HOST = "polymarket-upload.s3.us-east-2.amazonaws.com";
const OPTIMIZER_WIDTHS = [48, 64, 96, 128, 256, 384];

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

export function MarketThumb(props: Props) {
  const size = props.size ?? 40;
  return size >= 56 ? (
    <CrossfadeThumb {...props} size={size} />
  ) : (
    <LazyThumb {...props} size={size} />
  );
}

function LazyThumb({
  marketId,
  name,
  imageUrl,
  fallbackIconUrl,
  size,
}: Required<Pick<Props, "size">> & Omit<Props, "size">) {
  const initialStage: Stage = imageUrl
    ? "image"
    : fallbackIconUrl
      ? "icon"
      : "tile";
  const [stage, setStage] = useState<Stage>(initialStage);
  const [previous, setPrevious] = useState({ imageUrl, fallbackIconUrl });
  if (
    previous.imageUrl !== imageUrl ||
    previous.fallbackIconUrl !== fallbackIconUrl
  ) {
    setPrevious({ imageUrl, fallbackIconUrl });
    setStage(initialStage);
  }

  if (stage === "tile") {
    return <FallbackTile marketId={marketId} name={name} size={size} />;
  }

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
        // eslint-disable-next-line @next/next/no-img-element -- unknown hosts must not widen the image proxy allowlist
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

function CrossfadeThumb({
  marketId,
  name,
  imageUrl,
  fallbackIconUrl,
  size,
}: Required<Pick<Props, "size">> & Omit<Props, "size">) {
  const candidates = useMemo(
    () =>
      [imageUrl, fallbackIconUrl]
        .filter((url): url is string => !!url)
        .map((url) => toDisplayUrl(url, size)),
    [imageUrl, fallbackIconUrl, size],
  );
  const [painted, setPainted] = useState<string | null>(candidates[0] ?? null);
  const [previous, setPrevious] = useState<string | null>(null);
  const [seen, setSeen] = useState<string | null>(painted);
  if (seen !== painted) {
    setPrevious(seen);
    setSeen(painted);
  }

  useEffect(() => {
    let cancelled = false;
    void firstLoadable(candidates).then((url) => {
      if (!cancelled) setPainted(url);
    });
    return () => {
      cancelled = true;
    };
  }, [candidates]);

  if (!painted) {
    return <FallbackTile marketId={marketId} name={name} size={size} />;
  }

  return (
    <div
      aria-hidden
      style={{
        ...thumbStyles(size),
        position: "relative",
        background: "var(--surface-2)",
      }}
    >
      {previous && previous !== painted && (
        <span
          style={{
            position: "absolute",
            inset: 0,
            backgroundImage: `url(${JSON.stringify(previous)})`,
            backgroundPosition: "center",
            backgroundSize: "cover",
          }}
        />
      )}
      <span
        key={painted}
        style={{
          position: "absolute",
          inset: 0,
          backgroundImage: `url(${JSON.stringify(painted)})`,
          backgroundPosition: "center",
          backgroundSize: "cover",
          animation: "sybil-fade-swap var(--dur-swap) var(--ease-standard)",
        }}
      />
    </div>
  );
}

function FallbackTile({
  marketId,
  name,
  size,
}: {
  marketId: number;
  name: string;
  size: number;
}) {
  return (
    <div
      aria-hidden
      style={{
        ...thumbStyles(size),
        background: PALETTE[Math.abs(marketId) % PALETTE.length],
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
    >
      {firstGlyph(name)}
    </div>
  );
}

function firstLoadable(urls: string[]): Promise<string | null> {
  if (urls.length === 0) return Promise.resolve(null);
  const [head, ...rest] = urls;
  return new Promise<boolean>((resolve) => {
    const image = new window.Image();
    image.onload = () => resolve(true);
    image.onerror = () => resolve(false);
    image.src = head!;
  }).then((loaded) => (loaded ? head! : firstLoadable(rest)));
}

function toDisplayUrl(src: string, size: number): string {
  if (!isOptimizedImageUrl(src)) return src;
  const target = size * 2;
  const width =
    OPTIMIZER_WIDTHS.find((candidate) => candidate >= target) ??
    OPTIMIZER_WIDTHS[OPTIMIZER_WIDTHS.length - 1]!;
  return `/_next/image?url=${encodeURIComponent(src)}&w=${width}&q=60`;
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
  return (stripped[0] ?? trimmed[0] ?? "·").toUpperCase();
}
