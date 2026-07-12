"use client";

import { useCallback, useEffect, useRef } from "react";

import type { DevBlock } from "@/lib/dev/types";

interface BlockBarChartProps {
  blocks: DevBlock[];
  metric: "volume" | "fills" | "orders";
  height?: number;
}

// Canvas can't read CSS custom properties directly, so the palette is resolved
// from the live theme tokens at draw time (see `draw`). Each metric maps to a
// design token; the hex is only a fallback for SSR / a missing var. A redraw is
// triggered on theme flips via a MutationObserver on <html>'s data-theme.
const BAR_TOKEN: Record<BlockBarChartProps["metric"], string> = {
  volume: "--accent",
  orders: "--warn",
  fills: "--yes",
};
const BAR_FALLBACK: Record<BlockBarChartProps["metric"], string> = {
  volume: "#3FB6D9",
  orders: "#E8B447",
  fills: "#5BD99A",
};

export function BlockBarChart({
  blocks,
  metric,
  height = 240,
}: BlockBarChartProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  const draw = useCallback(() => {
    // Ported from the console's drawActivityChart (index.html lines 1409-1459):
    // the console reads the canvas by id; here we use the ref instead.
    const canvas = canvasRef.current;
    if (!canvas || !blocks.length) return;
    const dpr = window.devicePixelRatio || 1;
    const rect = canvas.getBoundingClientRect();
    if (!rect.width || !rect.height) return;
    canvas.width = Math.floor(rect.width * dpr);
    canvas.height = Math.floor(rect.height * dpr);
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.scale(dpr, dpr);
    const w = rect.width;
    const h = rect.height;
    // Resolve the palette from the live theme tokens (canvas can't read CSS
    // vars) so the chart tracks light/dark like the rest of the app. The hex is
    // only a last-resort fallback if a var doesn't resolve.
    const cs = getComputedStyle(canvas);
    const tok = (name: string, fallback: string) =>
      cs.getPropertyValue(name).trim() || fallback;
    const chartBg = tok("--bg-1", "#0A0E12");
    const gridline = tok("--border-2", "#161C24");
    const zeroBar = tok("--border-3", "#1D242E");
    const labelText = tok("--fg-3", "#8b93a6");
    const emptyText = tok("--fg-4", "#5C6578");
    const barColor = tok(BAR_TOKEN[metric], BAR_FALLBACK[metric]);
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = chartBg;
    ctx.fillRect(0, 0, w, h);
    const pad = { l: 46, r: 14, t: 18, b: 28 };
    const plotW = w - pad.l - pad.r;
    const plotH = h - pad.t - pad.b;
    const values = blocks.map((b) => {
      if (metric === "volume") return Number(b.total_volume_nanos || 0) / 1e9;
      if (metric === "orders") return Number(b.order_count || 0);
      return Number(b.fill_count || 0);
    });
    const max = Math.max(...values, 1);
    ctx.strokeStyle = gridline;
    ctx.lineWidth = 1;
    for (let i = 0; i <= 4; i++) {
      const y = pad.t + (plotH * i) / 4;
      ctx.beginPath();
      ctx.moveTo(pad.l, y);
      ctx.lineTo(w - pad.r, y);
      ctx.stroke();
    }
    const barW = Math.max(2, plotW / values.length - 1);
    values.forEach((v, i) => {
      const x = pad.l + i * (plotW / values.length);
      const bh = Math.max(1, (v / max) * plotH);
      ctx.fillStyle = v === 0 ? zeroBar : barColor;
      ctx.fillRect(x, pad.t + plotH - bh, barW, bh);
    });
    ctx.fillStyle = labelText;
    ctx.font = "11px ui-monospace, monospace";
    ctx.fillText(
      metric +
        " max " +
        (metric === "volume" ? "$" + max.toFixed(0) : max.toFixed(0)),
      10,
      14,
    );
    const first = blocks[0]?.height;
    const last = blocks[blocks.length - 1]?.height;
    ctx.fillText("#" + first, pad.l, h - 8);
    ctx.textAlign = "right";
    ctx.fillText("#" + last, w - pad.r, h - 8);
    ctx.textAlign = "left";
    if (values.every((v) => v === 0)) {
      ctx.fillStyle = emptyText;
      ctx.fillText("no " + metric + " in this window", pad.l + 10, pad.t + 22);
    }
  }, [blocks, metric]);

  useEffect(() => {
    draw();
    const canvas = canvasRef.current;
    const parent = canvas?.parentElement;
    const resizeObserver = parent ? new ResizeObserver(() => draw()) : null;
    if (parent && resizeObserver) resizeObserver.observe(parent);
    // Redraw when the theme flips (data-theme on <html>): the palette is read
    // from CSS tokens at draw time, so re-reading repaints it for light/dark.
    const themeObserver = new MutationObserver(() => draw());
    themeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme"],
    });
    return () => {
      resizeObserver?.disconnect();
      themeObserver.disconnect();
    };
  }, [draw, height]);

  return (
    <canvas
      ref={canvasRef}
      style={{ width: "100%", height, display: "block" }}
    />
  );
}
