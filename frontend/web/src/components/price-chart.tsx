"use client";

import {
  AreaSeries,
  createChart,
  type IChartApi,
  type ISeriesApi,
  type UTCTimestamp,
} from "lightweight-charts";
import { useEffect, useRef } from "react";
import type { components } from "@/lib/api/schema";
import { useStore } from "@/lib/store";

type PricePoint = components["schemas"]["PricePointResponse"];

type Props = {
  marketId: number;
  history: PricePoint[];
};

/**
 * PriceChart — TradingView Lightweight Charts (v5).
 *
 * Imperative chart lifecycle managed via refs; subscribes to the Zustand
 * store outside React's render path so each block update goes straight to
 * series.update() without re-rendering this component.
 */
export function PriceChart({ marketId, history }: Props) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const seriesRef = useRef<ISeriesApi<"Area"> | null>(null);
  const lastTimeRef = useRef<number>(0);

  // 1. Create the chart once.
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const chart = createChart(el, {
      layout: {
        background: { color: "transparent" },
        textColor: "rgba(245,245,242,0.52)",
        fontFamily:
          "JetBrains Mono, ui-monospace, 'SF Mono', Menlo, monospace",
        fontSize: 11,
      },
      grid: {
        vertLines: { color: "rgba(255,255,255,0.04)" },
        horzLines: { color: "rgba(255,255,255,0.04)" },
      },
      rightPriceScale: {
        borderColor: "transparent",
        scaleMargins: { top: 0.08, bottom: 0.08 },
      },
      timeScale: {
        borderColor: "transparent",
        timeVisible: true,
        secondsVisible: false,
      },
      crosshair: {
        vertLine: { color: "rgba(63,182,217,0.3)", width: 1 },
        horzLine: { color: "rgba(63,182,217,0.3)", width: 1 },
      },
      width: el.clientWidth,
      height: el.clientHeight || 280,
    });

    const series = chart.addSeries(AreaSeries, {
      lineColor: "#3FB6D9",
      topColor: "rgba(63,182,217,0.24)",
      bottomColor: "rgba(63,182,217,0)",
      lineWidth: 2,
      priceFormat: {
        type: "custom",
        minMove: 0.1,
        formatter: (v: number) => `${v.toFixed(1)}%`,
      },
    });

    chartRef.current = chart;
    seriesRef.current = series;

    // ResizeObserver to keep the chart sized correctly.
    const ro = new ResizeObserver(() => {
      const r = el.getBoundingClientRect();
      chart.applyOptions({ width: r.width, height: r.height });
    });
    ro.observe(el);

    return () => {
      ro.disconnect();
      chart.remove();
      chartRef.current = null;
      seriesRef.current = null;
    };
  }, []);

  // 2. Seed historical data when it arrives.
  useEffect(() => {
    const series = seriesRef.current;
    if (!series || history.length === 0) return;
    const data = history
      .map((p) => ({
        time: Math.floor(p.timestamp_ms / 1000) as UTCTimestamp,
        value: nanosToPercent(p.yes_price_nanos),
      }))
      .sort((a, b) => (a.time as number) - (b.time as number))
      // Lightweight Charts rejects duplicate timestamps — dedupe by time,
      // keeping the last value (most recent block at that wall-clock second).
      .reduce<{ time: UTCTimestamp; value: number }[]>((acc, pt) => {
        const prev = acc[acc.length - 1];
        if (prev && prev.time === pt.time) {
          prev.value = pt.value;
        } else {
          acc.push(pt);
        }
        return acc;
      }, []);
    series.setData(data);
    if (data.length > 0) {
      const lastEntry = data[data.length - 1];
      if (lastEntry) lastTimeRef.current = lastEntry.time as number;
    }
    chartRef.current?.timeScale().fitContent();
  }, [history]);

  // 3. Subscribe to the store for live ticks. Outside React's render path —
  //    selector returns the latest block; we react to changes manually.
  useEffect(() => {
    const unsubscribe = useStore.subscribe((state, prevState) => {
      if (state.latestBlock === prevState.latestBlock) return;
      const block = state.latestBlock;
      const series = seriesRef.current;
      if (!block || !series) return;
      const arr = block.clearing_prices_nanos?.[String(marketId)];
      const yesStr = arr?.[0];
      if (!yesStr) return;
      const t = Math.floor(block.timestamp_ms / 1000);
      // Skip if not newer than what we already plotted.
      if (t < lastTimeRef.current) return;
      // Same-second update: overwrite the last point.
      const time = t as UTCTimestamp;
      const value = nanosToPercent(yesStr);
      series.update({ time, value });
      lastTimeRef.current = t;
    });
    return unsubscribe;
  }, [marketId]);

  return (
    <div
      style={{
        position: "relative",
        width: "100%",
        height: 280,
      }}
    >
      <div ref={containerRef} style={{ position: "absolute", inset: 0 }} />
    </div>
  );
}

function nanosToPercent(nanos: string | bigint | number): number {
  if (typeof nanos === "number") return nanos / 1e7;
  return Number(BigInt(nanos)) / 1e7;
}
