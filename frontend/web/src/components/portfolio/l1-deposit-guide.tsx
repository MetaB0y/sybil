"use client";

import { useQuery } from "@tanstack/react-query";
import Link from "next/link";
import { api } from "@/lib/api/client";

export function L1DepositGuide({ accountId }: { accountId: number }) {
  const bridgeKey = useQuery({
    queryKey: ["account", accountId, "bridge-key"],
    queryFn: async () => {
      const { data, error } = await api.GET("/v1/accounts/{id}/bridge-key", {
        params: { path: { id: accountId } },
      });
      if (error || !data) throw new Error("fetch bridge key failed");
      return data;
    },
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });

  if (bridgeKey.isPending) {
    return (
      <DepositGuideShell>
        <div role="status" aria-busy="true" className="text-annotation">
          Loading your L1 deposit key…
        </div>
      </DepositGuideShell>
    );
  }

  if (bridgeKey.error) {
    return (
      <DepositGuideShell>
        <div
          role="alert"
          style={{ display: "flex", gap: 12, flexWrap: "wrap" }}
        >
          <span style={{ ...bodyText, color: "var(--no)" }}>
            Your deposit key is unavailable. Do not send a deposit until it can
            be verified.
          </span>
          <button
            type="button"
            onClick={() => void bridgeKey.refetch()}
            disabled={bridgeKey.isFetching}
            style={retryButtonStyle}
          >
            {bridgeKey.isFetching ? "Retrying…" : "Retry"}
          </button>
        </div>
      </DepositGuideShell>
    );
  }

  return (
    <DepositGuideShell>
      <L1DepositGuidance
        accountId={accountId}
        sybilAccountKeyHex={bridgeKey.data.sybil_account_key_hex}
      />
    </DepositGuideShell>
  );
}

export function L1DepositGuidance({
  accountId,
  sybilAccountKeyHex,
}: {
  accountId: number;
  sybilAccountKeyHex: string;
}) {
  const displayKey = sybilAccountKeyHex.startsWith("0x")
    ? sybilAccountKeyHex
    : `0x${sybilAccountKeyHex}`;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <p style={{ ...bodyText, margin: 0 }}>
        For the devnet vault flow, this is the 32-byte routing key the deposit
        call expects. It belongs to account #{accountId} and is different from
        your passkey or signing public key.
      </p>

      <div>
        <div className="eyebrow" style={{ marginBottom: 6 }}>
          Sybil account key
        </div>
        <code
          style={{
            display: "block",
            padding: "9px 10px",
            borderRadius: 6,
            border: "1px solid var(--border-1)",
            background: "var(--bg-2)",
            color: "var(--fg-1)",
            fontFamily: "var(--font-mono)",
            fontSize: 12,
            lineHeight: 1.45,
            overflowWrap: "anywhere",
            userSelect: "all",
          }}
        >
          {displayKey}
        </code>
      </div>

      <div
        style={{
          padding: "10px 12px",
          borderRadius: 6,
          border:
            "1px solid color-mix(in srgb, var(--warn) 35%, var(--border-1))",
          background: "color-mix(in srgb, var(--warn) 7%, transparent)",
        }}
      >
        <div
          className="eyebrow"
          style={{ color: "var(--warn)", marginBottom: 5 }}
        >
          Deposit missing?
        </div>
        <p style={{ ...bodyText, margin: 0 }}>
          If this key was not resolvable when the L1 deposit was indexed, the
          value is held in Sybil&apos;s committed quarantine ledger and is not
          included in your portfolio yet. Sybil automatically claims the full
          parked value when the matching account is created with its initial
          signing key, or when any later signing key is registered for it in{" "}
          <Link href="/settings" style={{ color: "var(--accent)" }}>
            Settings
          </Link>
          .
        </p>
        <p style={{ ...bodyText, margin: "8px 0 0", color: "var(--fg-4)" }}>
          This page cannot confirm a specific quarantined deposit because the
          API currently exposes aggregate quarantine totals only. There is no L1
          refund flow today.
        </p>
      </div>
    </div>
  );
}

function DepositGuideShell({ children }: { children: React.ReactNode }) {
  return (
    <section
      aria-labelledby="l1-deposit-guide-title"
      style={{
        background: "var(--surface-1)",
        border: "1px solid var(--border-1)",
        borderRadius: "var(--radius-lg)",
        boxShadow: "var(--shadow-inset-top)",
        overflow: "hidden",
      }}
    >
      <div
        style={{
          padding: "12px 16px",
          borderBottom: "1px solid var(--border-1)",
          display: "flex",
          alignItems: "baseline",
          justifyContent: "space-between",
          gap: 12,
        }}
      >
        <span id="l1-deposit-guide-title" className="eyebrow">
          L1 deposits
        </span>
        <span className="text-annotation">routing &amp; recovery guide</span>
      </div>
      <div style={{ padding: "14px 16px" }}>{children}</div>
    </section>
  );
}

const bodyText: React.CSSProperties = {
  fontFamily: "var(--font-sans)",
  fontSize: 13,
  color: "var(--fg-3)",
  lineHeight: 1.5,
};

const retryButtonStyle: React.CSSProperties = {
  padding: "5px 10px",
  borderRadius: 6,
  border: "1px solid var(--border-1)",
  background: "var(--bg-2)",
  color: "var(--fg-1)",
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  cursor: "pointer",
};
