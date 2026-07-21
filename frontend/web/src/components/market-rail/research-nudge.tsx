"use client";

/** A quiet research prompt with a human-vetted vertical service-name rotation. */

import { useEffect, useState } from "react";

const PLATFORMS = [
  { name: "Perplexity", url: "https://www.perplexity.ai/" },
  { name: "Futuresearch", url: "https://futuresearch.ai/" },
  { name: "Mantic", url: "https://www.mantic.com/" },
  { name: "Lightningrod", url: "http://lightningrod.ai/" },
  { name: "Elicit", url: "https://elicit.com/" },
  { name: "Consensus", url: "https://consensus.app/" },
] as const;

const ROTATE_MS = 2_600;
const SWAP_MS = 440;

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
  const [outgoing, setOutgoing] = useState<number | null>(null);
  const [paused, setPaused] = useState(false);
  const [reducedMotion, setReducedMotion] = useState(false);
  // The rotation is only tappable because a mouse can hover it still. A phone
  // has no hover, so every 2.6s the link you were reaching for slid away and
  // another took its place — visible for a moment, impossible to hit.
  const [noHover, setNoHover] = useState(false);

  useEffect(() => {
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    const sync = () => setReducedMotion(media.matches);
    sync();
    media.addEventListener("change", sync);
    return () => media.removeEventListener("change", sync);
  }, []);

  useEffect(() => {
    const media = window.matchMedia("(hover: none)");
    const sync = () => setNoHover(media.matches);
    sync();
    media.addEventListener("change", sync);
    return () => media.removeEventListener("change", sync);
  }, []);

  // Frozen, not fixed: the rotation exists so no one service owns the slot, so
  // a touch device draws its one service at random instead of always the first.
  // Only read on the no-hover path, which never runs during hydration (the
  // media query resolves in an effect), so the server's markup still matches.
  const [frozenIndex] = useState(() =>
    Math.floor(Math.random() * PLATFORMS.length),
  );

  useEffect(() => {
    // Reduced-motion means no autonomous content replacement, not merely a
    // hard cut without the slide animation.
    if (paused || reducedMotion || noHover) return;
    const interval = window.setInterval(() => {
      setIndex((current) => {
        if (!reducedMotion) setOutgoing(current);
        return (current + 1) % PLATFORMS.length;
      });
    }, ROTATE_MS);
    return () => window.clearInterval(interval);
  }, [paused, reducedMotion, noHover]);

  useEffect(() => {
    if (outgoing == null) return;
    const timeout = window.setTimeout(() => setOutgoing(null), SWAP_MS);
    return () => window.clearTimeout(timeout);
  }, [outgoing]);

  const shownIndex = noHover ? frozenIndex : index;
  const platform = PLATFORMS[shownIndex]!;
  const outgoingPlatform =
    outgoing == null || noHover ? null : PLATFORMS[outgoing];

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
      <span
        onMouseEnter={() => setPaused(true)}
        onMouseLeave={() => setPaused(false)}
        onFocusCapture={() => setPaused(true)}
        onBlurCapture={(event) => {
          if (!event.currentTarget.contains(event.relatedTarget))
            setPaused(false);
        }}
        style={{
          position: "relative",
          display: "inline-block",
          overflow: "hidden",
          minWidth: 108,
          height: "1.4em",
          lineHeight: "1.4em",
        }}
      >
        {outgoingPlatform && (
          <span
            aria-hidden
            style={{
              ...nameStyle,
              position: "absolute",
              inset: "0 auto auto 0",
              display: "inline-flex",
              alignItems: "center",
              gap: 2,
              animation: `sybil-rot-out ${SWAP_MS}ms var(--ease-standard) forwards`,
            }}
          >
            {outgoingPlatform.name}
            <span>↗</span>
          </span>
        )}
        <a
          key={shownIndex}
          className="mobile-action-link"
          href={platform.url}
          target="_blank"
          rel="noreferrer noopener"
          style={{
            ...nameStyle,
            position: "absolute",
            inset: "0 auto auto 0",
            display: "inline-flex",
            alignItems: "center",
            gap: 2,
            animation: reducedMotion
              ? undefined
              : `sybil-rot-in ${SWAP_MS}ms var(--ease-standard)`,
          }}
        >
          {platform.name}
          <span aria-hidden>↗</span>
        </a>
      </span>
    </div>
  );
}
