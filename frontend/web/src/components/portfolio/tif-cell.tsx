"use client";

import { selectLatestHeight, useStore } from "@/lib/store";

const GTC_THRESHOLD = 1_000_000;

export function TifCell({ expiresAtBlock }: { expiresAtBlock: number }) {
  const latestHeight = useStore(selectLatestHeight);
  const remaining =
    typeof latestHeight === "number" ? expiresAtBlock - latestHeight : null;

  let label: string;
  let sub: string;
  let accent = false;

  if (remaining == null) {
    label = `@${expiresAtBlock}`;
    sub = "expiry block";
  } else if (remaining > GTC_THRESHOLD) {
    label = "GTC";
    sub = "till cancel";
    accent = true;
  } else if (remaining <= 0) {
    label = "expired";
    sub = `@${expiresAtBlock}`;
  } else if (remaining === 1) {
    label = "1 batch";
    sub = "next batch";
  } else {
    label = `${remaining} batches`;
    sub = `${remaining} left`;
  }

  return (
    <span
      style={{
        display: "inline-flex",
        flexDirection: "column",
        alignItems: "flex-end",
        gap: 1,
      }}
    >
      <span
        className="tabular"
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 12,
          color: accent ? "var(--accent)" : "var(--fg-1)",
          fontWeight: accent ? 600 : 500,
        }}
      >
        {label}
      </span>
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 9.5,
          color: "var(--fg-4)",
          letterSpacing: "var(--track-wide)",
        }}
      >
        {sub}
      </span>
    </span>
  );
}
