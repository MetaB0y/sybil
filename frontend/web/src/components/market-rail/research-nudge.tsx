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

  useEffect(() => {
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    const sync = () => setReducedMotion(media.matches);
    sync();
    media.addEventListener("change", sync);
    return () => media.removeEventListener("change", sync);
  }, []);

  // The same query the 44px control floor uses in globals.css. The name sits in
  // a fixed-height clipping slot, and a 44px link inside a 15px slot rendered
  // its text below the clip — the row read as "not sure? ask" and nothing else.
  // The slot grows to hold it rather than the link shrinking, so the target
  // stays real and nothing invisible overlaps the bet button above.
  const [coarse, setCoarse] = useState(false);
  useEffect(() => {
    const media = window.matchMedia(
      "(max-width: 1280px) and (pointer: coarse)",
    );
    const sync = () => setCoarse(media.matches);
    sync();
    media.addEventListener("change", sync);
    return () => media.removeEventListener("change", sync);
  }, []);

  useEffect(() => {
    // Reduced-motion means no autonomous content replacement, not merely a
    // hard cut without the slide animation.
    if (paused || reducedMotion) return;
    const interval = window.setInterval(() => {
      setIndex((current) => {
        if (!reducedMotion) setOutgoing(current);
        return (current + 1) % PLATFORMS.length;
      });
    }, ROTATE_MS);
    return () => window.clearInterval(interval);
  }, [paused, reducedMotion]);

  useEffect(() => {
    if (outgoing == null) return;
    const timeout = window.setTimeout(() => setOutgoing(null), SWAP_MS);
    return () => window.clearTimeout(timeout);
  }, [outgoing]);

  const platform = PLATFORMS[index]!;
  const outgoingPlatform = outgoing == null ? null : PLATFORMS[outgoing];

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
        // A finger cannot hover, so without this the name could swap between
        // the press and the release — and a click that starts on one element
        // and ends on another never fires at all.
        onPointerDown={() => setPaused(true)}
        onPointerUp={() => setPaused(false)}
        onPointerCancel={() => setPaused(false)}
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
          height: coarse ? 44 : "1.4em",
          lineHeight: coarse ? "44px" : "1.4em",
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
          key={index}
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
