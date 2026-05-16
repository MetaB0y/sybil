/**
 * Dev Zone formatters — ported verbatim from the Sybil console
 * (crates/sybil-api/static/index.html). Kept separate from
 * lib/format/nanos.ts per the Dev Zone isolation rule. Inputs accept
 * string | number because the wire sends JSON numbers but the generated
 * schema types *_nanos as string.
 */

type Num = number | string | null | undefined;

function n(v: Num): number {
  if (v == null) return NaN;
  return Number(v);
}

export function fmtPrice(nanos: Num): string {
  const v = n(nanos);
  if (Number.isNaN(v)) return "-";
  return "$" + (v / 1e9).toFixed(3);
}

export function fmtProb(v: Num): string {
  const x = n(v);
  if (Number.isNaN(x)) return "-";
  return (x * 100).toFixed(1) + "%";
}

/**
 * Format a 0-1 ratio as a percentage. Returns "-" for zero or missing,
 * treating zero as absent data. Use `fmtProb` instead when zero is a
 * meaningful value (e.g. a genuine 0.0% probability).
 */
export function fmtPct(v: Num): string {
  const x = n(v);
  if (!x || Number.isNaN(x)) return "-";
  return (x * 100).toFixed(1) + "%";
}

export function pctWidth(nanos: Num): number {
  const v = n(nanos);
  if (Number.isNaN(v)) return 0;
  return Math.max(0, Math.min(100, Math.round(v / 1e7)));
}

export function dollars(nanos: Num): string {
  const v = n(nanos);
  if (!v || Number.isNaN(v)) return "0";
  return Math.round(v / 1e9).toLocaleString();
}

export function moneySigned(v: Num): string {
  const x = n(v) || 0;
  const sign = x >= 0 ? "+$" : "-$";
  return sign + Math.abs(x).toLocaleString(undefined, { maximumFractionDigits: 0 });
}

export function dollarsFloat(v: Num): string {
  const x = n(v);
  if (!x || Number.isNaN(x)) return "0";
  return x.toLocaleString(undefined, { maximumFractionDigits: 0 });
}

export function fmtInt(v: Num): string {
  const x = n(v);
  return (Number.isNaN(x) ? 0 : x).toLocaleString();
}

export function shortRoot(root: string | null | undefined): string {
  if (!root) return "...";
  return String(root).slice(0, 8);
}

export function shortTime(value: number | string | null | undefined): string {
  if (!value) return "-";
  const d = new Date(value);
  if (Number.isNaN(d.getTime())) return String(value);
  return d.toLocaleString(undefined, {
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}
