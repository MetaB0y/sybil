/**
 * Time-windowed rollups for the Activity page's pulse strip (24h vs prior 24h).
 *
 * "All-time" is mocked elsewhere (see `mocks.ts`) until the backend exposes a
 * /v1/activity/overview endpoint — scanning all blocks from genesis on the
 * frontend is unbounded. We do derive the two 24h windows from whatever
 * blocks the store has, knowing the result is partial (we may not have all
 * ~1440 blocks of the prior window). The hook layer attaches a `blockCount`
 * so the UI can be honest about "based on N blocks".
 */

import { parseNanos } from "../format/nanos";
import type { Block, WindowStats } from "./types";

const DAY_MS = 86_400_000;

const EMPTY_WINDOW: WindowStats = {
  matchedVolumeNanos: 0n,
  ordersPlaced: 0,
  ordersMatched: 0,
  ordersUnmatched: 0,
  traders: 0,
  blockCount: 0,
  firstTimestampMs: null,
  lastTimestampMs: null,
};

/**
 * Roll `blocks` into two adjacent 24h windows ending at `nowMs`.
 *
 * - `last24h`  : `nowMs - 24h ≤ ts < nowMs`
 * - `prior24h` : `nowMs - 48h ≤ ts < nowMs - 24h`
 *
 * Blocks outside both windows are ignored. Order of `blocks` doesn't matter.
 */
export function deriveWindowedStats(
  blocks: Block[],
  nowMs: number
): { last24h: WindowStats; prior24h: WindowStats } {
  const last24Start = nowMs - DAY_MS;
  const prior24Start = nowMs - 2 * DAY_MS;

  const last24Fills: { account_id?: number | null }[] = [];
  const prior24Fills: { account_id?: number | null }[] = [];

  let last: WindowStats = { ...EMPTY_WINDOW };
  let prior: WindowStats = { ...EMPTY_WINDOW };

  for (const b of blocks) {
    const ts = b.timestamp_ms;
    if (ts >= last24Start && ts < nowMs) {
      last = accumulate(last, b);
      if (b.fills) last24Fills.push(...b.fills);
    } else if (ts >= prior24Start && ts < last24Start) {
      prior = accumulate(prior, b);
      if (b.fills) prior24Fills.push(...b.fills);
    }
  }

  last.traders = countUnique(last24Fills);
  prior.traders = countUnique(prior24Fills);

  return { last24h: last, prior24h: prior };
}

/**
 * Convenience: signed percent delta (current vs prior) for one numeric field.
 * Returns `null` when `prior` is 0 (avoid divide-by-zero — the UI shows "—").
 */
export function pctDeltaNumber(current: number, prior: number): number | null {
  if (prior === 0) return null;
  return ((current - prior) / prior) * 100;
}

/** Same, but for bigint money fields. Rounded to one decimal place via Number(). */
export function pctDeltaBigint(current: bigint, prior: bigint): number | null {
  if (prior === 0n) return null;
  // Scale up by 1000 before converting to Number → safe precision for ±xxx.x%.
  const scaled = ((current - prior) * 1000n) / prior;
  return Number(scaled) / 10;
}

function accumulate(stats: WindowStats, b: Block): WindowStats {
  const placed = b.order_count;
  const matched = b.orders_filled;
  const rejections = b.rejections?.length ?? 0;
  const unmatched = Math.max(0, placed - matched - rejections);
  const ts = b.timestamp_ms;
  return {
    matchedVolumeNanos:
      stats.matchedVolumeNanos + parseNanos(b.total_volume_nanos),
    ordersPlaced: stats.ordersPlaced + placed,
    ordersMatched: stats.ordersMatched + matched,
    ordersUnmatched: stats.ordersUnmatched + unmatched,
    traders: 0, // filled at the end after deduping fills across the window
    blockCount: stats.blockCount + 1,
    firstTimestampMs:
      stats.firstTimestampMs == null || ts < stats.firstTimestampMs
        ? ts
        : stats.firstTimestampMs,
    lastTimestampMs:
      stats.lastTimestampMs == null || ts > stats.lastTimestampMs
        ? ts
        : stats.lastTimestampMs,
  };
}

function countUnique(fills: { account_id?: number | null }[]): number {
  const set = new Set<number>();
  for (const f of fills) {
    if (f.account_id != null) set.add(f.account_id);
  }
  return set.size;
}
