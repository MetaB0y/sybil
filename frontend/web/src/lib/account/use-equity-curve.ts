"use client";

/**
 * Deterministic mock equity curve over time.
 *
 * Backend has no per-account portfolio-value history (OPEN_QUESTIONS #12).
 * For now we synthesise a curve from `(accountId, range)` as the seed,
 * anchored to the real endpoints (start = total_deposited, end = current
 * portfolio_value). The shape is consistent per account so the page
 * doesn't flicker on every render.
 *
 * Always render with a `<MockValue>` marker on the chart frame.
 */

import { useMemo } from "react";

export type EquityRange = "24H" | "7D" | "30D" | "ALL";

const POINTS_PER_RANGE: Record<EquityRange, number> = {
  "24H": 24,
  "7D": 30,
  "30D": 60,
  ALL: 142,
};

export interface EquityCurve {
  range: EquityRange;
  points: number[]; // dollars (Number, not bigint — chart precision OK at $)
  baseline: number; // dashed-line floor: net deposits
  startEquity: number;
  endEquity: number;
  deltaAbs: number; // endEquity − startForRange (NOT baseline)
  deltaPct: number; // delta / startForRange
}

export function useEquityCurve(args: {
  accountId: number;
  range: EquityRange;
  currentValueDollars: number;
  baselineDepositsDollars: number;
}): EquityCurve {
  const { accountId, range, currentValueDollars, baselineDepositsDollars } = args;

  return useMemo(() => {
    const n = POINTS_PER_RANGE[range];
    const seed = hashSeed(accountId, range);
    const rand = seededRand(seed);

    const baseline = baselineDepositsDollars;
    const end = currentValueDollars;

    // Start-of-range value: scale the baseline-to-end trajectory back so
    // that the shorter ranges show a shallower swing than ALL.
    const rangeStart =
      range === "ALL"
        ? baseline
        : baseline + (end - baseline) * (1 - rangeFraction(range));

    const points: number[] = new Array(n);
    // Pre-roll the noise sequence so we taper at endpoints.
    const noise: number[] = [];
    let acc = 0;
    for (let i = 0; i < n; i++) {
      acc += (rand() - 0.5) * 2; // random walk -1..+1 step
      noise.push(acc);
    }
    const noiseMax = Math.max(1, ...noise.map((v) => Math.abs(v)));
    const swing = Math.max(Math.abs(end - rangeStart), 1) * 0.18;

    for (let i = 0; i < n; i++) {
      const t = n === 1 ? 1 : i / (n - 1);
      const trend = rangeStart + (end - rangeStart) * t;
      const taper = Math.sin(t * Math.PI); // 0 at endpoints, 1 at middle
      const noiseAmt = (noise[i]! / noiseMax) * swing * taper;
      points[i] = trend + noiseAmt;
    }
    // Anchor the endpoints exactly (taper already minimises drift).
    points[0] = rangeStart;
    points[n - 1] = end;

    const deltaAbs = end - rangeStart;
    const deltaPct = rangeStart === 0 ? 0 : (deltaAbs / rangeStart) * 100;

    return {
      range,
      points,
      baseline,
      startEquity: rangeStart,
      endEquity: end,
      deltaAbs,
      deltaPct,
    };
  }, [accountId, range, currentValueDollars, baselineDepositsDollars]);
}

function rangeFraction(range: EquityRange): number {
  // What portion of the full baseline→current span this range covers.
  switch (range) {
    case "24H":
      return 0.08;
    case "7D":
      return 0.28;
    case "30D":
      return 0.55;
    case "ALL":
      return 1;
  }
}

function hashSeed(accountId: number, range: EquityRange): number {
  let h = (accountId | 0) ^ 0x9e3779b1;
  for (const c of range) {
    h = Math.imul(h ^ c.charCodeAt(0), 0x85ebca6b);
    h = (h ^ (h >>> 13)) >>> 0;
  }
  return h >>> 0;
}

function seededRand(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s ^= s << 13;
    s ^= s >>> 17;
    s ^= s << 5;
    s >>>= 0;
    return s / 0x100000000;
  };
}
