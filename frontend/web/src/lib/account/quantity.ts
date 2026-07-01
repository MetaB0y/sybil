export const SHARE_SCALE = 1_000n;

export function sharesToUnits(shares: number): bigint {
  if (!Number.isFinite(shares) || shares <= 0) return 0n;
  return BigInt(Math.max(0, Math.floor(shares * Number(SHARE_SCALE) + 1e-9)));
}

export function unitsToShares(units: number | bigint): number {
  return Number(units) / Number(SHARE_SCALE);
}

export function formatShareUnits(units: number | bigint): string {
  const shares = unitsToShares(units);
  return new Intl.NumberFormat("en-US", {
    maximumFractionDigits: 3,
  }).format(shares);
}

export function notionalNanos(priceNanos: bigint, quantityUnits: number | bigint): bigint {
  return (priceNanos * BigInt(quantityUnits)) / SHARE_SCALE;
}

export function notionalNanosCeil(priceNanos: bigint, quantityUnits: number | bigint): bigint {
  const numerator = priceNanos * BigInt(quantityUnits);
  return (numerator + SHARE_SCALE - 1n) / SHARE_SCALE;
}

export function priceNanosFromNotional(
  notionalNanosValue: bigint,
  quantityUnits: number | bigint,
): bigint | null {
  const units = BigInt(quantityUnits);
  if (units === 0n) return null;
  return (notionalNanosValue * SHARE_SCALE) / units;
}
