"use client";

import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api/client";
import type { HistoryEvent } from "@/lib/account/use-account-history";
import {
  formatWithdrawalCountdown,
  pendingWithdrawals,
  withdrawalCancelState,
  type BridgeWithdrawal,
} from "@/lib/account/withdrawals";
import { formatDollars, parseNanos } from "@/lib/format/nanos";

export function WithdrawalEtas({
  accountId,
  events,
}: {
  accountId: number;
  events: HistoryEvent[];
}) {
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);

  const blockHeights = useMemo(
    () =>
      [...new Set(events.filter((e) => e.type === "withdrawal").map((e) => e.blockHeight))]
        .filter((height) => Number.isFinite(height))
        .sort((a, b) => b - a),
    [events],
  );

  const q = useQuery({
    enabled: blockHeights.length > 0,
    queryKey: ["account", accountId, "withdrawal-leaves", blockHeights],
    queryFn: async (): Promise<BridgeWithdrawal[]> => {
      const blocks = await Promise.all(
        blockHeights.map(async (height) => {
          const { data, error } = await api.GET("/v1/blocks/{height}", {
            params: { path: { height } },
          });
          if (error || !data) throw new Error("fetch withdrawal block failed");
          return data;
        }),
      );
      return blocks.flatMap((block) =>
        (block.bridge?.withdrawal_leaves ?? []).filter(
          (leaf) => leaf.account_id === accountId,
        ),
      );
    },
    staleTime: 30_000,
    refetchOnWindowFocus: false,
  });

  const rows = useMemo(
    () =>
      pendingWithdrawals(q.data ?? [], nowMs).sort(
        (a, b) => a.withdrawal_id - b.withdrawal_id,
      ),
    [q.data, nowMs],
  );

  if (blockHeights.length === 0 || rows.length === 0) return null;

  return (
    <section
      style={{
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-lg)",
        boxShadow: "var(--shadow-inset-top)",
        overflow: "hidden",
      }}
    >
      <div
        className="withdrawal-section-head"
        style={{
          padding: "12px 16px",
          borderBottom: "1px solid var(--border-1)",
          display: "flex",
          alignItems: "baseline",
          justifyContent: "space-between",
          gap: 12,
        }}
      >
        <span className="eyebrow">Pending withdrawals</span>
        <span className="text-annotation">
          {q.isPending ? "checking bridge status" : `${rows.length} pending`}
        </span>
      </div>
      <div style={{ display: "flex", flexDirection: "column" }}>
        {rows.map((withdrawal) => (
          <WithdrawalRow
            key={withdrawal.withdrawal_id}
            withdrawal={withdrawal}
            nowMs={nowMs}
          />
        ))}
      </div>
    </section>
  );
}

function WithdrawalRow({
  withdrawal,
  nowMs,
}: {
  withdrawal: BridgeWithdrawal;
  nowMs: number;
}) {
  const countdown = formatWithdrawalCountdown(
    nowMs,
    withdrawal.l1_executable_at_unix,
  );
  const state = withdrawalCancelState(withdrawal, nowMs);
  const absolute =
    withdrawal.l1_executable_at_unix == null
      ? "L1 executable time not observed yet"
      : new Date(withdrawal.l1_executable_at_unix * 1000).toLocaleString();
  const amount = formatDollars(parseNanos(withdrawal.amount_nanos), {
    decimals: 2,
  });
  return (
    <div
      className="withdrawal-row"
      style={{
        display: "grid",
        gridTemplateColumns: "minmax(0, 1fr) auto",
        gap: 12,
        padding: "12px 16px",
        borderBottom: "1px solid var(--border-1)",
        alignItems: "center",
      }}
    >
      <div style={{ minWidth: 0 }}>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            flexWrap: "wrap",
          }}
        >
          <span
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: 13,
              color: "var(--fg-1)",
            }}
          >
            #{withdrawal.withdrawal_id} · {amount}
          </span>
          <StatePill state={state} />
        </div>
        <div
          title={absolute}
          style={{
            marginTop: 4,
            fontFamily: "var(--font-mono)",
            fontSize: 11,
            color: "var(--fg-3)",
          }}
        >
          executable at {absolute}
        </div>
      </div>
      <div
        className="tabular"
        title={absolute}
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 14,
          color: countdown.expired ? "var(--yes)" : "var(--accent)",
          whiteSpace: "nowrap",
        }}
      >
        {countdown.label}
      </div>
    </div>
  );
}

function StatePill({ state }: { state: ReturnType<typeof withdrawalCancelState> }) {
  const color =
    state === "cancel-window-open"
      ? "var(--warn)"
      : state === "executable"
        ? "var(--yes)"
        : "var(--fg-3)";
  const label =
    state === "cancel-window-open"
      ? "cancel window"
      : state === "not-requested"
        ? "not requested"
        : state;
  return (
    <span
      style={{
        padding: "2px 7px",
        borderRadius: "var(--radius-pill)",
        color,
        background: `color-mix(in srgb, ${color} 12%, transparent)`,
        fontFamily: "var(--font-mono)",
        fontSize: 10,
        letterSpacing: "var(--track-wide)",
        textTransform: "uppercase",
        whiteSpace: "nowrap",
      }}
    >
      {label}
    </span>
  );
}
