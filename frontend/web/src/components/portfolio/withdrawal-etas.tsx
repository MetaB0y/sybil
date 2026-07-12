"use client";

import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api/client";
import {
  formatWithdrawalCountdown,
  pendingWithdrawals,
  withdrawalCancelState,
  type BridgeWithdrawal,
} from "@/lib/account/withdrawals";
import { formatDollars, parseNanos } from "@/lib/format/nanos";
import { selectLatestBlock, useStore } from "@/lib/store";
import { AuthenticatedReadState } from "./authenticated-read-state";

export function WithdrawalEtas({ accountId }: { accountId: number }) {
  const [nowMs, setNowMs] = useState(() => Date.now());
  const queryClient = useQueryClient();
  const latest = useStore(selectLatestBlock);
  useEffect(() => {
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);

  useEffect(() => {
    queryClient.invalidateQueries({
      queryKey: ["account", accountId, "withdrawals"],
    });
  }, [accountId, latest?.height, queryClient]);

  const q = useQuery({
    queryKey: ["account", accountId, "withdrawals"],
    queryFn: async (): Promise<BridgeWithdrawal[]> => {
      const { data, error } = await api.GET("/v1/accounts/{id}/withdrawals", {
        params: { path: { id: accountId } },
      });
      if (error || !data) throw new Error("fetch active withdrawals failed");
      return data;
    },
    staleTime: 0,
    refetchOnWindowFocus: false,
  });

  const rows = useMemo(
    () =>
      pendingWithdrawals(q.data ?? [], nowMs).sort(
        (a, b) => a.withdrawal_id - b.withdrawal_id,
      ),
    [q.data, nowMs],
  );

  if (q.isPending) {
    return (
      <AuthenticatedReadState
        status="loading"
        title="Loading withdrawal status"
        message="Checking the current bridge status for this account."
      />
    );
  }

  if (q.error) {
    return (
      <AuthenticatedReadState
        status="error"
        title="Withdrawal status unavailable"
        message="We could not verify your active withdrawals. They are hidden instead of being shown as an empty list."
        onRetry={() => void q.refetch()}
        retrying={q.isFetching}
      />
    );
  }

  return <WithdrawalStatusPanel rows={rows} nowMs={nowMs} />;
}

export function WithdrawalStatusPanel({
  rows,
  nowMs,
}: {
  rows: readonly BridgeWithdrawal[];
  nowMs: number;
}) {
  return (
    <section
      aria-labelledby="normal-withdrawals-title"
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
        <span id="normal-withdrawals-title" className="eyebrow">
          Normal withdrawals
        </span>
        <span className="text-annotation">{rows.length} active</span>
      </div>
      {rows.length === 0 ? (
        <div
          style={{
            padding: "12px 16px",
            color: "var(--fg-3)",
            fontFamily: "var(--font-sans)",
            fontSize: 13,
          }}
        >
          No active withdrawal leaves.
        </div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column" }}>
          {rows.map((withdrawal) => (
            <WithdrawalRow
              key={withdrawal.withdrawal_id}
              withdrawal={withdrawal}
              nowMs={nowMs}
            />
          ))}
        </div>
      )}
      <div
        style={{
          padding: "10px 16px",
          borderTop: "1px solid var(--border-1)",
          background: "var(--bg-2)",
          color: "var(--fg-4)",
          fontFamily: "var(--font-sans)",
          fontSize: 12,
          lineHeight: 1.5,
        }}
      >
        Status only on this private devnet. New withdrawal requests are not
        enabled in the web app: the signed API is service-gated and currently
        creates a Sybil withdrawal leaf and debits available cash, but does not
        by itself release L1 funds. The proof-backed vault claim path is still
        incomplete.
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

function StatePill({
  state,
}: {
  state: ReturnType<typeof withdrawalCancelState>;
}) {
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
