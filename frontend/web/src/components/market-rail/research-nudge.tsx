"use client";

/**
 * Research nudge — a subtle footer under the Degen CTA that nudges unsure
 * bettors toward AI research tools. The platform name cycles through the list
 * with a fade, and the visible name is a live external link. Cycling pauses on
 * hover/focus so the moving target is actually clickable. The "soon" pill hints
 * that native in-app research is a coming-soon feature preview.
 */

import { useEffect, useRef, useState } from "react";

const PLATFORMS = [
  { name: "Perplexity", url: "https://www.perplexity.ai/" },
  { name: "Futuresearch", url: "https://futuresearch.ai/" },
  { name: "Mantic", url: "https://www.mantic.com/" },
  { name: "Lightningrod", url: "http://lightningrod.ai/" },
  { name: "Elicit", url: "https://elicit.com/" },
  { name: "Consensus", url: "https://consensus.app/" },
] as const;

const CYCLE_MS = 2500;

export function ResearchNudge() {
  const [index, setIndex] = useState(0);
  const [paused, setPaused] = useState(false);
  const linkRef = useRef<HTMLAnchorElement>(null);

  useEffect(() => {
    if (paused) return;
    const id = window.setInterval(() => {
      setIndex((i) => (i + 1) % PLATFORMS.length);
    }, CYCLE_MS);
    return () => window.clearInterval(id);
  }, [paused]);

  // Replay a short fade each time the platform changes, without remounting the
  // link — so hover/focus state (and therefore the pause) survives the swap.
  useEffect(() => {
    const el = linkRef.current;
    if (!el) return;
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    el.animate(
      [
        { opacity: 0, transform: "translateY(2px)" },
        { opacity: 1, transform: "translateY(0)" },
      ],
      { duration: 240, easing: "ease-out" },
    );
  }, [index]);

  const platform = PLATFORMS[index] ?? PLATFORMS[0];

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        gap: 6,
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        color: "var(--fg-3)",
        letterSpacing: "0.01em",
      }}
    >
      <span>not sure? ask</span>

      {/* Reserve width to the longest label so the dot + pill don't jump as
          names cycle. */}
      <span
        style={{
          display: "inline-flex",
          justifyContent: "center",
          minWidth: 104,
        }}
      >
        <a
          ref={linkRef}
          href={platform.url}
          target="_blank"
          rel="noreferrer noopener"
          onMouseEnter={() => setPaused(true)}
          onMouseLeave={() => setPaused(false)}
          onFocus={() => setPaused(true)}
          onBlur={() => setPaused(false)}
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 2,
            color: "var(--accent)",
            textDecoration: "none",
            fontWeight: 600,
          }}
        >
          {platform.name}
          <span aria-hidden="true">↗</span>
        </a>
      </span>

      <span aria-hidden="true" style={{ color: "var(--fg-4)" }}>
        ·
      </span>

      <span
        title="Native AI research is coming to Sybil — this is an early preview."
        style={{
          fontSize: 9,
          textTransform: "uppercase",
          letterSpacing: "0.08em",
          color: "var(--accent)",
          background: "color-mix(in srgb, var(--accent) 14%, transparent)",
          border: "1px solid color-mix(in srgb, var(--accent) 30%, transparent)",
          borderRadius: "var(--radius-pill, 999px)",
          padding: "1px 6px",
          cursor: "help",
        }}
      >
        soon
      </span>
    </div>
  );
}
