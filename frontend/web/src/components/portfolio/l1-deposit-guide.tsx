"use client";

import { useQuery } from "@tanstack/react-query";
import Link from "next/link";
import { api } from "@/lib/api/client";
import type { components } from "@/lib/api/schema";

type BridgeDomain = components["schemas"]["BridgeDomainResponse"];

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
  const bridgeStatus = useQuery({
    queryKey: ["bridge", "status"],
    queryFn: async () => {
      const { data, error } = await api.GET("/v1/bridge/status");
      if (error || !data) throw new Error("fetch bridge status failed");
      return data;
    },
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });

  if (bridgeKey.isPending || bridgeStatus.isPending) {
    return (
      <DepositGuideShell>
        <div role="status" aria-busy="true" className="text-annotation">
          Loading verified L1 deposit instructions…
        </div>
      </DepositGuideShell>
    );
  }

  if (bridgeKey.error || bridgeStatus.error) {
    return (
      <DepositGuideShell>
        <div
          role="alert"
          style={{ display: "flex", gap: 12, flexWrap: "wrap" }}
        >
          <span style={{ ...bodyText, color: "var(--no)" }}>
            Deposit instructions are unavailable. The account key and configured
            chain, vault, and token must all be verified before sending
            anything.
          </span>
          <button
            type="button"
            onClick={() =>
              void Promise.all([bridgeKey.refetch(), bridgeStatus.refetch()])
            }
            disabled={bridgeKey.isFetching || bridgeStatus.isFetching}
            style={retryButtonStyle}
          >
            {bridgeKey.isFetching || bridgeStatus.isFetching
              ? "Retrying…"
              : "Retry"}
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
        domain={bridgeStatus.data.configured_domain ?? null}
      />
    </DepositGuideShell>
  );
}

export function L1DepositGuidance({
  accountId,
  sybilAccountKeyHex,
  domain,
}: {
  accountId: number;
  sybilAccountKeyHex: string;
  domain?: BridgeDomain | null;
}) {
  const displayKey = sybilAccountKeyHex.startsWith("0x")
    ? sybilAccountKeyHex
    : `0x${sybilAccountKeyHex}`;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <BridgeDomainGuidance domain={domain ?? null} />

      <p style={{ ...bodyText, margin: 0 }}>
        {domain
          ? "For the configured devnet vault, this is the 32-byte routing key the deposit call expects."
          : "This account routing key is shown for identity and recovery only while L1 deposit admission is unavailable."}{" "}
        It belongs to account #{accountId} and is different from your passkey or
        signing public key.
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

export function BridgeDomainGuidance({
  domain,
}: {
  domain: BridgeDomain | null;
}) {
  if (!domain) {
    return (
      <div role="alert" style={domainWarningStyle}>
        <div
          className="eyebrow"
          style={{ color: "var(--no)", marginBottom: 5 }}
        >
          Deposits unavailable
        </div>
        <p style={{ ...bodyText, margin: 0 }}>
          This API has no configured L1 chain, vault, and collateral token. Do
          not send tokens using the routing key below.
        </p>
      </div>
    );
  }

  const rows = [
    ["Network", bridgeChainLabel(domain.chain_id)],
    ["Vault", normalizeHexAddress(domain.vault_address_hex)],
    ["Token", normalizeHexAddress(domain.token_address_hex)],
  ] as const;
  return (
    <section aria-label="Configured L1 deposit domain" style={domainReadyStyle}>
      <div
        className="eyebrow"
        style={{ color: "var(--accent)", marginBottom: 7 }}
      >
        Configured bridge domain
      </div>
      <dl
        style={{
          display: "grid",
          gridTemplateColumns: "auto minmax(0, 1fr)",
          gap: "6px 12px",
          margin: 0,
        }}
      >
        {rows.map(([label, value]) => (
          <div key={label} style={{ display: "contents" }}>
            <dt style={{ ...bodyText, color: "var(--fg-4)" }}>{label}</dt>
            <dd
              style={{
                margin: 0,
                color: "var(--fg-1)",
                fontFamily: "var(--font-mono)",
                fontSize: 12,
                overflowWrap: "anywhere",
                userSelect: "all",
              }}
            >
              {value}
            </dd>
          </div>
        ))}
      </dl>
      <p style={{ ...bodyText, margin: "9px 0 0", color: "var(--fg-4)" }}>
        No in-browser wallet transaction is available. Verify the wallet
        network, vault, and token independently. This status does not attest the
        token&apos;s value or verifier safety; do not use real funds.
      </p>
    </section>
  );
}

export function bridgeChainLabel(chainId: number): string {
  return chainId === 11_155_111
    ? "Sepolia (chain 11155111)"
    : `Chain ${chainId}`;
}

function normalizeHexAddress(value: string): string {
  return value.startsWith("0x") || value.startsWith("0X")
    ? value
    : `0x${value}`;
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

const domainWarningStyle: React.CSSProperties = {
  padding: "10px 12px",
  borderRadius: 6,
  border: "1px solid color-mix(in srgb, var(--no) 35%, var(--border-1))",
  background: "color-mix(in srgb, var(--no) 7%, transparent)",
};

const domainReadyStyle: React.CSSProperties = {
  padding: "10px 12px",
  borderRadius: 6,
  border: "1px solid color-mix(in srgb, var(--accent) 30%, var(--border-1))",
  background: "color-mix(in srgb, var(--accent) 6%, transparent)",
};
