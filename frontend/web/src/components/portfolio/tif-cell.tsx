"use client";

import { BLOCK_INTERVAL_MS } from "@/lib/constants";
import { formatAge } from "@/lib/format/nanos";
import { selectLatestHeight, useStore } from "@/lib/store";

const GTC_THRESHOLD = 1_000_000;

/**
 * Time-in-force for a resting order, on a single line. Rather than a raw batch
 * count ("3 batches"), show the human time the batches translate to — one batch
 * is BLOCK_INTERVAL_MS, so "~30s" reads far more intuitively than "3 batches".
 * The imminent and open-ended edges get words instead of a number:
 *   - next batch (1 batch left)  · GTC (good till cancel)  · expired
 */
export function TifCell({ expiresAtBlock }: { expiresAtBlock: number }) {
  const latestHeight = useStore(selectLatestHeight);
  const remaining =
    typeof latestHeight === "number" ? expiresAtBlock - latestHeight : null;

  let label: string;
  let title: string;
  let accent = false;

  if (remaining == null) {
    label = `@${expiresAtBlock}`;
    title = `Expires at block ${expiresAtBlock}`;
  } else if (remaining > GTC_THRESHOLD) {
    label = "GTC";
    title = "Good till cancelled — rests until you cancel it";
    accent = true;
  } else if (remaining <= 0) {
    label = "expired";
    title = `Expired at block ${expiresAtBlock}`;
  } else if (remaining === 1) {
    label = "next batch";
    title = "Expires at the next batch";
  } else {
    const eta = formatAge(remaining * BLOCK_INTERVAL_MS);
    label = `~${eta}`;
    title = `~${eta} left (${remaining} batches)`;
  }

  return (
    <span
      className="tabular"
      title={title}
      style={{
        fontFamily: "var(--font-mono)",
        fontSize: 11.5,
        color: accent ? "var(--accent)" : "var(--fg-1)",
        fontWeight: accent ? 600 : 500,
        whiteSpace: "nowrap",
      }}
    >
      {label}
    </span>
  );
}
