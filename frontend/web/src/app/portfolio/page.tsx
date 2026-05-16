"use client";

/**
 * /portfolio — handoff Variant Classic layout:
 *   identity header
 *   ┌─────────── hero ────────────┐ ┌─── equity chart ───┐
 *   │ big number · 4-stat grid    │ │ svg line + labels  │
 *   └─────────────────────────────┘ └────────────────────┘
 *   tab strip
 *   active-tab panel
 */

import { useMemo, useState } from "react";
import { ActivityList } from "@/components/portfolio/activity-list";
import { EquityChart } from "@/components/portfolio/equity-chart";
import { HistoryList } from "@/components/portfolio/history-list";
import {
  IdentityHeader,
  IdentityStrip,
} from "@/components/portfolio/identity-header";
import { OpenOrdersList } from "@/components/portfolio/open-orders-list";
import { PortfolioHero } from "@/components/portfolio/portfolio-hero";
import {
  PortfolioTabs,
  type PortfolioTab,
} from "@/components/portfolio/portfolio-tabs";
import { PositionsList } from "@/components/portfolio/positions-list";
import { RangeTabs } from "@/components/portfolio/range-tabs";
import {
  useAccountHydrated,
  useAccountSession,
  useSetConnectModalOpen,
} from "@/lib/account/use-account";
import { useAccountFills } from "@/lib/account/use-account-fills";
import { useAccountOrders } from "@/lib/account/use-account-orders";
import { useTrackedCancels } from "@/lib/account/use-cancelled-orders";
import { useClosedPositions } from "@/lib/account/use-closed-positions";
import {
  useEquityCurve,
  type EquityRange,
} from "@/lib/account/use-equity-curve";
import { usePnlSplit } from "@/lib/account/use-pnl-split";
import { usePortfolio } from "@/lib/account/use-portfolio";
import { parseNanos } from "@/lib/format/nanos";
import { useMarketsList } from "@/lib/markets/use-markets";

const FILLS_PAGE = 200;
const RANGE_COPY: Record<EquityRange, string> = {
  "24H": "past 24 hours",
  "7D": "past 7 days",
  "30D": "past 30 days",
  ALL: "since first deposit",
};

export default function PortfolioPage() {
  const session = useAccountSession();
  const hydrated = useAccountHydrated();

  if (!hydrated) {
    return (
      <Shell>
        <Hint>loading…</Hint>
      </Shell>
    );
  }
  if (!session) {
    return (
      <Shell>
        <Disconnected />
      </Shell>
    );
  }
  return (
    <Shell>
      <Connected
        accountId={session.accountId}
        publicKeyHex={session.publicKeyHex}
      />
    </Shell>
  );
}

function Connected({
  accountId,
  publicKeyHex,
}: {
  accountId: number;
  publicKeyHex: string;
}) {
  const portfolio = usePortfolio(accountId);
  const orders = useAccountOrders(accountId);
  const fills = useAccountFills(accountId, { limit: FILLS_PAGE });
  const cancels = useTrackedCancels(accountId);
  const markets = useMarketsList();

  const fillsData = fills.data ?? [];
  const ordersData = orders.data ?? [];
  const portfolioData = portfolio.data ?? null;
  const closed = useClosedPositions(fillsData, portfolioData);
  const pnlSplit = usePnlSplit(portfolioData);

  const marketsById = useMemo(
    () => markets.bundle?.byId ?? new Map(),
    [markets.bundle],
  );

  const [range, setRange] = useState<EquityRange>("ALL");
  const [tab, setTab] = useState<PortfolioTab>("positions");

  const currentValue = portfolioData
    ? Number(parseNanos(portfolioData.portfolio_value_nanos)) / 1e9
    : 0;
  const baselineDeposits = portfolioData
    ? Number(parseNanos(portfolioData.total_deposited_nanos)) / 1e9
    : 0;

  const curve = useEquityCurve({
    accountId,
    range,
    currentValueDollars: currentValue,
    baselineDepositsDollars: baselineDeposits,
  });

  const tradeCount = fillsData.length;
  const tradeCountCapped = tradeCount >= FILLS_PAGE;

  const counts: Record<PortfolioTab, number> = {
    positions: portfolioData?.positions.length ?? 0,
    orders: ordersData.length,
    history: closed.length,
    activity: fillsData.length + cancels.length,
  };

  return (
    <>
      <IdentityHeader publicKeyHex={publicKeyHex} />

      <section
        style={{
          display: "flex",
          flexDirection: "column",
          gap: "var(--space-4)",
          paddingBottom: "var(--space-5)",
          borderBottom: "1px solid var(--border-1)",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: "var(--space-4)",
          }}
        >
          <IdentityStrip accountId={accountId} publicKeyHex={publicKeyHex} />
          <RangeTabs value={range} onChange={setRange} />
        </div>

        <div
          style={{
            display: "grid",
            gridTemplateColumns: "minmax(0, 0.85fr) minmax(0, 1.15fr)",
            gap: 48,
            alignItems: "start",
          }}
        >
          <PortfolioHero
            portfolio={portfolioData}
            pnlSplit={pnlSplit}
            curve={curve}
            tradeCount={tradeCount}
            tradeCountCapped={tradeCountCapped}
            rangeLabel={RANGE_COPY[range]}
          />
          <EquityChart curve={curve} />
        </div>
      </section>

      <PortfolioTabs value={tab} onChange={setTab} counts={counts} />

      {tab === "positions" && (
        <PositionsList
          positions={portfolioData?.positions ?? []}
          fills={fillsData}
          marketsById={marketsById}
        />
      )}
      {tab === "orders" && (
        <OpenOrdersList
          accountId={accountId}
          publicKeyHex={publicKeyHex}
          orders={ordersData}
          marketsById={marketsById}
        />
      )}
      {tab === "history" && (
        <HistoryList closed={closed} marketsById={marketsById} />
      )}
      {tab === "activity" && (
        <ActivityList
          fills={fillsData}
          cancels={cancels}
          marketsById={marketsById}
        />
      )}
    </>
  );
}

function Disconnected() {
  const openModal = useSetConnectModalOpen();
  return (
    <div
      style={{
        padding: "48px 24px",
        background: "var(--surface-1)",
        border: "1px dashed var(--border-1)",
        borderRadius: 10,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        gap: 16,
        textAlign: "center",
      }}
    >
      <div
        style={{
          fontFamily: "var(--font-display)",
          fontSize: 18,
          color: "var(--fg-1)",
        }}
      >
        Connect to view your portfolio
      </div>
      <p
        style={{
          margin: 0,
          color: "var(--fg-3)",
          fontFamily: "var(--font-sans)",
          fontSize: 13,
          maxWidth: 400,
          lineHeight: 1.5,
        }}
      >
        Create a demo account in your browser — preview &middot; wallet auth
        coming soon. Your keys are stored only in this browser.
      </p>
      <button
        type="button"
        onClick={() => openModal(true)}
        style={{
          padding: "10px 18px",
          background: "var(--accent)",
          border: 0,
          borderRadius: 8,
          color: "var(--bg-1)",
          fontFamily: "var(--font-sans)",
          fontWeight: 600,
          fontSize: 14,
          cursor: "pointer",
        }}
      >
        Connect
      </button>
    </div>
  );
}

function Shell({ children }: { children: React.ReactNode }) {
  return (
    <main
      style={{
        width: "100%",
        padding: "var(--space-6) var(--space-5) var(--space-9)",
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-4)",
      }}
    >
      {children}
    </main>
  );
}

function Hint({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        color: "var(--fg-4)",
        fontFamily: "var(--font-mono)",
        fontSize: 12,
      }}
    >
      {children}
    </div>
  );
}
