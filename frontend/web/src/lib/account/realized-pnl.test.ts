import { describe, expect, it } from "vitest";
import type { HistoryEvent, HistoryEventType } from "./use-account-history";
import { cumulativeRealizedPnl, totalRealizedPnl } from "./realized-pnl";

function ev(
  partial: Partial<HistoryEvent> & { id: string; timestampMs: number },
): HistoryEvent {
  return {
    type: "filled" as HistoryEventType,
    blockHeight: 1,
    ...partial,
  };
}

describe("cumulativeRealizedPnl", () => {
  it("returns an empty series when nothing was realized", () => {
    const events: HistoryEvent[] = [
      ev({ id: "1.0", timestampMs: 100, type: "placed" }),
      ev({ id: "1.1", timestampMs: 200, type: "filled" }), // no realized field
    ];
    expect(cumulativeRealizedPnl(events)).toEqual([]);
    expect(totalRealizedPnl(cumulativeRealizedPnl(events))).toBe(0n);
  });

  it("accumulates realized PnL in chronological order (running sum)", () => {
    // Provided newest-first (as the history feed arrives) to prove we re-sort.
    const events: HistoryEvent[] = [
      ev({ id: "3.0", timestampMs: 300, blockHeight: 3, realizedPnlNanos: -1_000_000_000n }),
      ev({ id: "2.0", timestampMs: 200, blockHeight: 2, realizedPnlNanos: 5_000_000_000n }),
      ev({ id: "1.0", timestampMs: 100, blockHeight: 1, realizedPnlNanos: 2_000_000_000n }),
    ];
    const pts = cumulativeRealizedPnl(events);
    expect(pts).toEqual([
      { t: 100, cumNanos: 2_000_000_000n },
      { t: 200, cumNanos: 7_000_000_000n },
      { t: 300, cumNanos: 6_000_000_000n }, // 7 + (-1)
    ]);
    expect(totalRealizedPnl(pts)).toBe(6_000_000_000n);
  });

  it("includes settlement (resolved) and partial fills, ignores side", () => {
    const events: HistoryEvent[] = [
      ev({ id: "1.0", timestampMs: 100, type: "partial_fill", side: "SELL", realizedPnlNanos: 1_000_000_000n }),
      ev({ id: "1.1", timestampMs: 150, type: "partial_fill", side: "BUY", realizedPnlNanos: 500_000_000n }),
      ev({ id: "2.0", timestampMs: 200, type: "resolved", realizedPnlNanos: 3_000_000_000n }),
    ];
    expect(totalRealizedPnl(cumulativeRealizedPnl(events))).toBe(4_500_000_000n);
  });

  it("breaks timestamp ties deterministically by block height then id", () => {
    const events: HistoryEvent[] = [
      ev({ id: "b", timestampMs: 100, blockHeight: 2, realizedPnlNanos: 10n }),
      ev({ id: "a", timestampMs: 100, blockHeight: 1, realizedPnlNanos: 100n }),
    ];
    const pts = cumulativeRealizedPnl(events);
    // block 1 first → cumulative 100 then 110.
    expect(pts.map((p) => p.cumNanos)).toEqual([100n, 110n]);
  });
});
