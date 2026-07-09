"use client";

/**
 * Research nudge — a subtle footer under the Degen CTA nudging unsure bettors
 * toward AI research tools. The service name rotates through the list with a
 * vertical "rotating text" swap (the outgoing word slides up out of a clip box
 * while the next rises into place), and the visible name is a live external
 * link. Rotation pauses on hover/focus so the moving target stays clickable.
 */

import { useEffect, useState } from "react";

const PLATFORMS = [
  { name: "Perplexity", url: "https://www.perplexity.ai/" },
  { name: "Futuresearch", url: "https://futuresearch.ai/" },
  { name: "Mantic", url: "https://www.mantic.com/" },
  { name: "Lightningrod", url: "http://lightningrod.ai/" },
  { name: "Elicit", url: "https://elicit.com/" },
  { name: "Consensus", url: "https://consensus.app/" },
] as const;

// Matches the reactbits rotating-text preview: 2.6s between rotations.
const ROTATE_MS = 2600;
const SWAP_MS = 440;

// Shared look for the service name — accent + a soft underline so it reads as a
// highlighted, tappable target without shouting.
const nameStyle: React.CSSProperties = {
  color: "var(--accent)",
  fontWeight: 600,
  textDecoration: "underline",
  textDecorationColor: "color-mix(in srgb, var(--accent) 45%, transparent)",
  textUnderlineOffset: 3,
  textDecorationThickness: "1px",
  whiteSpace: "nowrap",
};

export function ResearchNudge() {
  const [index, setIndex] = useState(0);
  const [paused, setPaused] = useState(false);
  const [exiting, setExiting] = useState<number | null>(null);
  const [seen, setSeen] = useState(0);
  const [reduce, setReduce] = useState(false);

  // Render-safe previous-value pattern: when the index advances, the word we
  // were just showing becomes the one that slides out.
  if (seen !== index) {
    setSeen(index);
    if (!reduce) setExiting(seen);
  }

  useEffect(() => {
    const mq = window.matchMedia("(prefers-reduced-motion: reduce)");
    const sync = () => setReduce(mq.matches);
    sync();
    mq.addEventListener("change", sync);
    return () => mq.removeEventListener("change", sync);
  }, []);

  useEffect(() => {
    if (paused) return;
    const id = window.setInterval(
      () => setIndex((i) => (i + 1) % PLATFORMS.length),
      ROTATE_MS,
    );
    return () => window.clearInterval(id);
  }, [paused]);

  const platform = PLATFORMS[index] ?? PLATFORMS[0];
  const exitingPlatform = exiting != null ? PLATFORMS[exiting] : null;

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "flex-start",
        gap: 6,
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        color: "var(--fg-3)",
        letterSpacing: "0.01em",
      }}
    >
      <span>not sure? ask</span>

      {/* Clip box: absolute-positioned words slide through it, so it needs a set
          height and enough width to hold the longest name. */}
      <span
        onMouseEnter={() => setPaused(true)}
        onMouseLeave={() => setPaused(false)}
        style={{
          position: "relative",
          display: "inline-block",
          overflow: "hidden",
          minWidth: 108,
          height: "1.4em",
          lineHeight: "1.4em",
        }}
      >
        {exitingPlatform && (
          <span
            key={`out-${exiting}`}
            aria-hidden
            onAnimationEnd={() => setExiting(null)}
            style={{
              position: "absolute",
              left: 0,
              top: 0,
              display: "inline-flex",
              alignItems: "center",
              gap: 2,
              animation: `sybil-rot-out ${SWAP_MS}ms var(--ease-standard) forwards`,
              ...nameStyle,
            }}
          >
            {exitingPlatform.name}
            <span aria-hidden="true">↗</span>
          </span>
        )}
        <a
          key={`in-${index}`}
          href={platform.url}
          target="_blank"
          rel="noreferrer noopener"
          onFocus={() => setPaused(true)}
          onBlur={() => setPaused(false)}
          style={{
            position: "absolute",
            left: 0,
            top: 0,
            display: "inline-flex",
            alignItems: "center",
            gap: 2,
            animation: `sybil-rot-in ${SWAP_MS}ms var(--ease-standard)`,
            ...nameStyle,
          }}
        >
          {platform.name}
          <span aria-hidden="true">↗</span>
        </a>
      </span>
    </div>
  );
}
