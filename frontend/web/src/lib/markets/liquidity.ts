/**
 * Liquidity display helpers.
 *
 * The backend field `liquidity_avg10_nanos` is misnamed: it's the *sum* of the
 * per-block near-the-money band depth over the last `LIQUIDITY_RING_BLOCKS`
 * blocks (`sum_last_n` in the sequencer), NOT an average. What we want to show
 * the user is the average dollars resting within ±band of the indicative price
 * per batch, so we divide by the ring length here on the client.
 *
 * The ring gets one push per block and caps at `LIQUIDITY_RING_BLOCKS`, so for
 * any market older than ~100s (10 × 10s blocks) the divisor is exact. Younger
 * markets — the first 10 blocks after genesis or a state wipe — under-report
 * slightly, which is a fine "warming up" behavior: the missing blocks are
 * simply treated as zero liquidity.
 *
 * When the backend eventually serves a true average (`avg_last_n`) this helper
 * collapses to identity and the call sites can drop it.
 */

/** Ring length in blocks — must match `LIQUIDITY_RING_CAP` in the sequencer. */
export const LIQUIDITY_RING_BLOCKS = 10n;

/** Sum-over-last-10-blocks band depth → average band depth per batch. */
export function avgLiquidityNanos(sumNanos: bigint): bigint {
  return sumNanos / LIQUIDITY_RING_BLOCKS;
}
