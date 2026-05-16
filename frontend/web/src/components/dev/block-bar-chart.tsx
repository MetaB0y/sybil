"use client";

import { useCallback, useEffect, useRef } from "react";

import type { DevBlock } from "@/lib/dev/types";

interface BlockBarChartProps {
  blocks: DevBlock[];
  metric: "volume" | "fills" | "orders";
  height?: number;
}

const BAR_COLOR: Record<BlockBarChartProps["metric"], string> = {
  volume: "#3FB6D9",
  orders: "#E8B447",
  fills: "#5BD99A",
};

const ZERO_BAR_COLOR = "#1D242E";
const GRIDLINE_COLOR = "#161C24";
const CHART_BG_COLOR = "#0A0E12";
const LABEL_TEXT_COLOR = "#8b93a6";
const EMPTY_TEXT_COLOR = "#5C6578";

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
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = CHART_BG_COLOR;
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
    ctx.strokeStyle = GRIDLINE_COLOR;
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
      ctx.fillStyle = BAR_COLOR[metric];
      if (v === 0) ctx.fillStyle = ZERO_BAR_COLOR;
      ctx.fillRect(x, pad.t + plotH - bh, barW, bh);
    });
    ctx.fillStyle = LABEL_TEXT_COLOR;
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
      ctx.fillStyle = EMPTY_TEXT_COLOR;
      ctx.fillText("no " + metric + " in this window", pad.l + 10, pad.t + 22);
    }
  }, [blocks, metric]);

  useEffect(() => {
    draw();
    const canvas = canvasRef.current;
    const parent = canvas?.parentElement;
    if (!parent) return;
    const observer = new ResizeObserver(() => draw());
    observer.observe(parent);
    return () => observer.disconnect();
  }, [draw, height]);

  return (
    <canvas
      ref={canvasRef}
      style={{ width: "100%", height, display: "block" }}
    />
  );
}
