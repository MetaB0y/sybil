"use client";

/**
 * Sybil account key — the account's 32-byte routing key.
 *
 * This used to be buried in the portfolio's "L1 deposits" panel alongside
 * bridge-domain and quarantine guidance. Deposits are not something a user does
 * from this app, but the key is durable account identity, so it lives here with
 * the rest of the account's keys and the deposit apparatus is gone.
 */

import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api/client";
import { Panel, PanelBody, PanelHead } from "@/components/dev/primitives/panel";
import { SettingsSectionReadState } from "./settings-view";

export function AccountKeySection({ accountId }: { accountId: number }) {
  const bridgeKey = useQuery({
    queryKey: ["account", accountId, "bridge-key"],
    queryFn: async () => {
      const { data, error } = await api.GET("/v1/accounts/{id}/bridge-key", {
        params: { path: { id: accountId } },
      });
      if (error || !data) throw new Error("fetch account key failed");
      return data;
    },
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });

  if (!bridgeKey.isSuccess) {
    return (
      <SettingsSectionReadState
        title="Sybil account key"
        status={bridgeKey.isError ? "error" : "loading"}
        loadingMessage="Loading your account key…"
        errorMessage="Your account key could not be read. It is hidden rather than shown unverified."
        onRetry={() => void bridgeKey.refetch()}
        retrying={bridgeKey.isFetching}
      />
    );
  }

  return (
    <AccountKeyPanel
      accountId={accountId}
      sybilAccountKeyHex={bridgeKey.data.sybil_account_key_hex}
    />
  );
}

export function AccountKeyPanel({
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
    <Panel>
      <PanelHead title="Sybil account key" />
      <PanelBody style={{ display: "flex", flexDirection: "column", gap: 14 }}>
        <p style={{ ...bodyText, margin: 0 }}>
          The 32-byte routing key that identifies account #{accountId} on Sybil.
          It is public and safe to share, and it is{" "}
          <strong>not</strong> your passkey, signing key, or an API token.
        </p>
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
      </PanelBody>
    </Panel>
  );
}

const bodyText: React.CSSProperties = {
  fontFamily: "var(--font-sans)",
  fontSize: 13,
  color: "var(--fg-3)",
  lineHeight: 1.5,
};
