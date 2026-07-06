import { describe, expect, it } from "vitest";
import {
  estimateTokenCost,
  extractStrategy,
  strategyRows,
  summarizeBots,
} from "./derive";
import type { ArenaBotSummary, ArenaTokenUsage } from "./use-arena-feed";

describe("arena derivations", () => {
  it("classifies bot strategy names", () => {
    expect(extractStrategy("US macro (Kelly)")).toBe("Kelly");
    expect(extractStrategy("US macro (Flat)")).toBe("Flat");
    expect(extractStrategy("Noise-03")).toBe("Noise");
    expect(extractStrategy("Legacy analyst")).toBe("Legacy");
  });

  it("aggregates bot roster totals and strategy rows", () => {
    const bots: ArenaBotSummary[] = [
      bot("A (Kelly)", 5, 110, 10, 4, 3, 0.12),
      bot("B (Kelly)", 4, 90, -10, 2, 1, 0.08),
      bot("C (Flat)", 2, 104, 4, 1, 1, null),
    ];

    expect(summarizeBots(bots)).toEqual({
      portfolioValue: 304,
      pnl: 4,
      orders: 7,
      fills: 5,
    });

    expect(strategyRows(bots)).toEqual([
      {
        strategy: "Kelly",
        traders: 2,
        totalPnl: 0,
        avgPnl: 0,
        avgEdge: 0.1,
        totalOrders: 6,
        totalFills: 4,
      },
      {
        strategy: "Flat",
        traders: 1,
        totalPnl: 4,
        avgPnl: 4,
        avgEdge: null,
        totalOrders: 1,
        totalFills: 1,
      },
    ]);
  });

  it("estimates token cost from prompt and completion tokens", () => {
    const rows: ArenaTokenUsage[] = [
      usage("A", 1, 600_000, 200_000),
      usage("B", 2, 100_000, 100_000),
    ];
    expect(estimateTokenCost(rows)).toBeCloseTo(0.7);
  });
});

function bot(
  trader_name: string,
  decision_count: number,
  portfolio_value: number,
  pnl: number,
  total_orders: number,
  total_fills: number,
  avg_edge: number | null,
): ArenaBotSummary {
  return {
    trader_name,
    decision_count,
    portfolio_value,
    pnl,
    total_orders,
    total_fills,
    avg_edge,
  };
}

function usage(
  trader_name: string,
  calls: number,
  prompt_tokens: number,
  completion_tokens: number,
): ArenaTokenUsage {
  return {
    trader_name,
    calls,
    prompt_tokens,
    completion_tokens,
    avg_latency_s: null,
    latest_model: null,
  };
}
