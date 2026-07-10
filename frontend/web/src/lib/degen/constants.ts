/**
 * Degen-buy tunables. Re-tuning the degen tax or order lifetime happens here
 * and nowhere else.
 *
 * See docs/superpowers/specs/2026-05-22-degen-buy-core-logic-design.md
 */

/** 1 USD in nanos — the unit used across the order path. */
export const ONE_DOLLAR_NANOS = 1_000_000_000n;

/** Deviation at 50¢: the peak of the degen-tax hump. $0.04 = 4¢. */
export const DEGEN_PEAK_NANOS = 40_000_000n;

/** Curve steepness: higher = the tax collapses faster toward the 0/$1 edges. */
export const DEGEN_EXPONENT = 1.3;

/** Order stays eligible for the next N batches (1 block = 1 batch). */
export const DEGEN_BATCHES = 12n;
