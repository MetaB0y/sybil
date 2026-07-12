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
import {
  TradesList,
  tradeOrderCount,
} from "@/components/portfolio/trades-list";
import { DisconnectedAccountPrompt } from "@/components/auth/disconnected-account-prompt";
import { EquityChart } from "@/components/portfolio/equity-chart";
import { HistoryFeed } from "@/components/portfolio/history-feed";
import { IdentityHeader } from "@/components/portfolio/identity-header";
import { L1DepositGuide } from "@/components/portfolio/l1-deposit-guide";
import { OpenOrdersList } from "@/components/portfolio/open-orders-list";
import { PortfolioHero } from "@/components/portfolio/portfolio-hero";
import {
  PortfolioTabs,
  type PortfolioTab,
} from "@/components/portfolio/portfolio-tabs";
import { PositionsList } from "@/components/portfolio/positions-list";
import { RealizedPnlPanel } from "@/components/portfolio/realized-pnl-panel";
import { RangeTabs } from "@/components/portfolio/range-tabs";
import { WithdrawalEtas } from "@/components/portfolio/withdrawal-etas";
import {
  AuthenticatedReadState,
  authenticatedReadStatus,
} from "@/components/portfolio/authenticated-read-state";
import {
  useAccountHydrated,
  useAccountSession,
  useSetConnectModalOpen,
} from "@/lib/account/use-account";
import { useAccountFills } from "@/lib/account/use-account-fills";
import {
  useAccountHistory,
  fillAggByOrder,
} from "@/lib/account/use-account-history";
import { useAccountOrders } from "@/lib/account/use-account-orders";
import {
  useEquityCurve,
  type EquityRange,
} from "@/lib/account/use-equity-curve";
import { usePnlSplit } from "@/lib/account/use-pnl-split";
import { usePortfolio } from "@/lib/account/use-portfolio";
import { parseNanos } from "@/lib/format/nanos";
import { useEventQuestions } from "@/lib/markets/use-event-raw";
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
  const ordersData = useMemo(() => orders.data ?? [], [orders.data]);
  const portfolioData = portfolio.data ?? null;
  const pnlSplit = usePnlSplit(portfolioData);

  const marketsById = useMemo(
    () => markets.bundle?.byId ?? new Map(),
    [markets.bundle],
  );
  const history = useAccountHistory(accountId);

  const touchedMarketIds = useMemo(() => {
    const ids = new Set<number>();
    for (const position of portfolioData?.positions ?? []) {
      ids.add(position.market_id);
    }
    for (const order of ordersData) ids.add(order.market_id);
    for (const event of history.events) {
      if (event.marketId != null) ids.add(event.marketId);
    }
    return ids;
  }, [portfolioData, ordersData, history.events]);

  const eventIds = useMemo(() => {
    const ids = new Set<string>();
    for (const marketId of touchedMarketIds) {
      const market = marketsById.get(marketId);
      if (market?.event_id && market.polymarket_condition_id) {
        ids.add(market.event_id);
      }
    }
    return [...ids];
  }, [touchedMarketIds, marketsById]);

  const questionByCondition = useEventQuestions(eventIds);
  const titleByMarket = useMemo(() => {
    const titles = new Map<number, string>();
    for (const marketId of touchedMarketIds) {
      const market = marketsById.get(marketId);
      const question = market?.polymarket_condition_id
        ? questionByCondition.get(market.polymarket_condition_id)
        : undefined;
      titles.set(marketId, question ?? market?.name ?? `#${marketId}`);
    }
    return titles;
  }, [touchedMarketIds, marketsById, questionByCondition]);

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

  // Trades are reconstructed from the history feed inside TradesList (one row
  // per executed order — fills grouped by order_id). The badge mirrors that via
  // the shared counter so it can't drift from the list.
  const tradesCount = useMemo(
    () => tradeOrderCount(history.events),
    [history.events],
  );

  // Per-order fill count + avg price for the Open Orders "Avg fill" column,
  // derived from the durable history log (the `/fills` endpoint is empty in
  // prod, which is why this used to read "— / 0 fills").
  const fillsByOrder = useMemo(
    () => fillAggByOrder(history.events),
    [history.events],
  );

  const privateReadStatus = authenticatedReadStatus([
    portfolio,
    orders,
    fills,
    history,
    { isPending: curve.isLoading, error: curve.error },
  ]);
  const privateReadsRetrying =
    portfolio.isFetching ||
    orders.isFetching ||
    fills.isFetching ||
    history.isFetching ||
    curve.isFetching;

  const retryPrivateReads = () => {
    void Promise.all([
      portfolio.refetch(),
      orders.refetch(),
      fills.refetch(),
      history.refetch(),
      curve.refetch(),
    ]);
  };

  if (privateReadStatus === "loading") {
    return (
      <>
        <IdentityHeader publicKeyHex={publicKeyHex} />
        <AuthenticatedReadState
          status="loading"
          title="Loading your portfolio"
          message="Checking your private balances, positions, orders, and account history."
        />
      </>
    );
  }

  if (privateReadStatus === "error") {
    return (
      <>
        <IdentityHeader publicKeyHex={publicKeyHex} />
        <AuthenticatedReadState
          status="error"
          title="Portfolio unavailable"
          message="We could not load all of your private account data. Balances and activity are hidden instead of being shown as zero."
          onRetry={retryPrivateReads}
          retrying={privateReadsRetrying}
        />
      </>
    );
  }

  const counts: Record<PortfolioTab, number> = {
    positions: portfolioData?.positions.length ?? 0,
    orders: ordersData.length,
    trades: tradesCount,
    pnl: 0, // badge hidden for P&L (chart tab, not a row count)
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

      <section className="portfolio-hero-grid" style={{}}>
        {/* Left: portfolio hero. */}
        <PortfolioHero
          portfolio={portfolioData}
          pnlSplit={pnlSplit}
          curve={curve}
          rangeLabel={RANGE_COPY[range]}
        />

        {/* Right: the equity chart fills the row height, range tabs in its
            header so the selector sits directly above the curve. */}
        <EquityChart
          curve={curve}
          headerRight={<RangeTabs value={range} onChange={setRange} />}
        />
      </section>

      <L1DepositGuide accountId={accountId} />

      <WithdrawalEtas accountId={accountId} />

      {tab === "positions" && (
        <PositionsList
          tabs={tabsStrip}
          positions={portfolioData?.positions ?? []}
          fills={fillsData}
          marketsById={marketsById}
          titleByMarket={titleByMarket}
        />
      )}
      {tab === "orders" && (
        <OpenOrdersList
          tabs={tabsStrip}
          accountId={accountId}
          publicKeyHex={publicKeyHex}
          orders={ordersData}
          fillsByOrder={fillsByOrder}
          marketsById={marketsById}
          titleByMarket={titleByMarket}
        />
      )}
      {tab === "trades" && (
        <TradesList
          tabs={tabsStrip}
          events={history.events}
          marketsById={marketsById}
          titleByMarket={titleByMarket}
        />
      )}
      {tab === "pnl" && (
        <RealizedPnlPanel tabs={tabsStrip} events={history.events} />
      )}
      {tab === "history" && (
        <HistoryFeed
          tabs={tabsStrip}
          events={history.events}
          marketsById={marketsById}
          titleByMarket={titleByMarket}
        />
      )}
    </>
  );
}

function Disconnected() {
  const openModal = useSetConnectModalOpen();
  return (
    <DisconnectedAccountPrompt
      title="Connect to view your portfolio"
      message={
        <>
          Create a demo account with a passkey or a local browser key. Your key
          material stays on this device.
        </>
      }
      onConnect={() => openModal(true)}
    />
  );
}

function Shell({ children }: { children: React.ReactNode }) {
  return (
    <main
      className="sybil-page-pad"
      style={{
        width: "100%",
        // +36px = markets ClearingTicker height, so the title aligns
        // with /'s "All markets" across pages
        paddingTop: "calc(var(--space-6) + 36px)",
        paddingBottom: "var(--space-9)",
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
