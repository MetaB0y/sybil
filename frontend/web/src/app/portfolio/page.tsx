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
import { TradesList } from "@/components/portfolio/trades-list";
import { EquityChart } from "@/components/portfolio/equity-chart";
import { HistoryFeed } from "@/components/portfolio/history-feed";
import { IdentityHeader } from "@/components/portfolio/identity-header";
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
import { useAccountHistory } from "@/lib/account/use-account-history";
import { useAccountOrders } from "@/lib/account/use-account-orders";
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
  const markets = useMarketsList();

  const fillsData = fills.data ?? [];
  const ordersData = orders.data ?? [];
  const portfolioData = portfolio.data ?? null;
  const pnlSplit = usePnlSplit(portfolioData);

  const marketsById = useMemo(
    () => markets.bundle?.byId ?? new Map(),
    [markets.bundle],
  );
  const history = useAccountHistory(accountId);

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

  // Trades are reconstructed from the history feed inside TradesList (one row
  // per fill / partial fill). The count below mirrors that for the tab badge.
  const tradesCount = useMemo(() => {
    let n = 0;
    for (const e of history.events) {
      if (e.type === "filled" || e.type === "partial_fill") n += 1;
    }
    return n;
  }, [history.events]);

  const counts: Record<PortfolioTab, number> = {
    positions: portfolioData?.positions.length ?? 0,
    orders: ordersData.length,
    trades: tradesCount,
    history: history.events.length,
  };

  // The tab strip is rendered inside the active list's toolbar so tabs +
  // search + filters share one row (see PortfolioToolbar).
  const tabsStrip = (
    <PortfolioTabs value={tab} onChange={setTab} counts={counts} />
  );

  return (
    <>
      <IdentityHeader publicKeyHex={publicKeyHex} />

      <section
        style={{
          display: "grid",
          gridTemplateColumns: "minmax(0, 0.85fr) minmax(0, 1.15fr)",
          gap: 48,
          alignItems: "stretch",
          paddingBottom: "var(--space-5)",
          borderBottom: "1px solid var(--border-1)",
        }}
      >
        {/* Left: portfolio hero. */}
        <PortfolioHero
          portfolio={portfolioData}
          pnlSplit={pnlSplit}
          curve={curve}
          tradeCount={tradeCount}
          tradeCountCapped={tradeCountCapped}
          rangeLabel={RANGE_COPY[range]}
        />

        {/* Right: the equity chart fills the row height, range tabs in its
            header so the selector sits directly above the curve. */}
        <EquityChart
          curve={curve}
          headerRight={<RangeTabs value={range} onChange={setRange} />}
        />
      </section>

      {tab === "positions" && (
        <PositionsList
          tabs={tabsStrip}
          positions={portfolioData?.positions ?? []}
          fills={fillsData}
          marketsById={marketsById}
        />
      )}
      {tab === "orders" && (
        <OpenOrdersList
          tabs={tabsStrip}
          accountId={accountId}
          publicKeyHex={publicKeyHex}
          orders={ordersData}
          fills={fillsData}
          marketsById={marketsById}
        />
      )}
      {tab === "trades" && (
        <TradesList
          tabs={tabsStrip}
          events={history.events}
          marketsById={marketsById}
        />
      )}
      {tab === "history" && (
        <HistoryFeed
          tabs={tabsStrip}
          events={history.events}
          marketsById={marketsById}
          isMock={history.isMock}
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
        // +36px = markets ClearingTicker height, so the title aligns
        // with /'s "All markets" across pages
        padding: "calc(var(--space-6) + 36px) var(--space-5) var(--space-9)",
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
