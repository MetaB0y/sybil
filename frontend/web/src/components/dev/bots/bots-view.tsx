"use client";

import { useState, type CSSProperties } from "react";

import { DataTable, Td, Th } from "@/components/dev/primitives/data-table";
import { Panel, PanelBody, PanelHead } from "@/components/dev/primitives/panel";
import { Pill } from "@/components/dev/primitives/pill";
import { Stat, StatGrid } from "@/components/dev/primitives/stat";
import type { Tone } from "@/components/dev/primitives/color-text";
import {
  articleLabel,
  articleUrl,
  formatOrder,
  orderList,
  articleList,
} from "@/lib/dev/derive";
import { useDevBots } from "@/lib/dev/fetchers";
import {
  dollarsFloat,
  fmtInt,
  fmtProb,
  moneySigned,
  shortTime,
} from "@/lib/dev/format";

const controlStyle: CSSProperties = {
  border: "1px solid var(--border-2)",
  background: "var(--surface-1)",
  color: "var(--fg-1)",
  borderRadius: 6,
  padding: "7px 9px",
  fontFamily: "inherit",
  fontSize: 12,
  width: "100%",
};

const truncCell: CSSProperties = {
  padding: "7px 9px",
  borderBottom: "1px solid var(--border-2)",
  verticalAlign: "top",
  maxWidth: 240,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
};

const mutedSpan: CSSProperties = { color: "var(--fg-3)", fontSize: 12 };

const emptyMsg: CSSProperties = {
  padding: "16px 4px",
  textAlign: "center",
  color: "var(--fg-4)",
  fontSize: 12,
};

const chipRow: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  gap: 6,
  marginTop: 8,
};

const articleLinkStyle: CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  padding: "2px 6px",
  borderRadius: 999,
  fontSize: 10,
  whiteSpace: "nowrap",
  overflow: "hidden",
  textOverflow: "ellipsis",
  maxWidth: 320,
  border: "1px solid var(--border-2)",
  color: "var(--accent)",
  textDecoration: "none",
};

/** Loose object view for order elements arriving as `unknown`. */
type LooseRecord = Record<string, unknown>;

/**
 * Structural shape accepted by `articleUrl` / `articleLabel`. `articleList`
 * returns `unknown[]`; the derive helpers narrow internally, so we widen
 * each element to this union rather than reaching for `any`.
 */
type ArticleArg =
  | string
  | { url?: string; source?: string; title?: string }
  | null
  | undefined;

export function BotsView() {
  const { data } = useDevBots();
  const [botFilter, setBotFilter] = useState<string>("");

  const dbAvailable = data?.db_available === true;
  const dbError = data?.error || "Arena decision database is not mounted.";
  const stats = data?.stats ?? {};
  const summaries = data?.summaries ?? [];
  const decisions = data?.decisions ?? [];

  const botTraderNames = summaries
    .map((b) => b.trader_name)
    .filter((name): name is string => Boolean(name));
  const filteredDecisions = botFilter
    ? decisions.filter((d) => d.trader_name === botFilter)
    : decisions;

  return (
    <div>
      {/* (1) Top StatGrid */}
      <StatGrid columns={6}>
        <Stat
          label="Decisions"
          value={fmtInt(stats.decisions)}
          tone="accent"
          sub={
            stats.latest_decision_timestamp
              ? "latest " + shortTime(stats.latest_decision_timestamp)
              : "no decisions yet"
          }
        />
        <Stat
          label="Traders"
          value={fmtInt(stats.traders)}
          sub="native arena SQLite feed"
        />
        <Stat
          label="Articles"
          value={fmtInt(stats.articles)}
          tone="cyan"
          sub="logged news inputs"
        />
        <Stat
          label="Snapshots"
          value={fmtInt(stats.snapshots)}
          sub="portfolio records"
        />
        <Stat
          label="LLM Calls"
          value={fmtInt(stats.token_usage)}
          tone="warn"
          sub="token usage rows"
        />
        <Stat
          label="Visible"
          value={fmtInt(filteredDecisions.length)}
          sub="recent reasoning cards"
        />
      </StatGrid>

      {/* (2) Two-column row */}
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "minmax(0,1fr) minmax(0,1fr)",
          gap: 12,
          marginTop: 12,
        }}
      >
        {/* Left — Bot Summaries */}
        <Panel>
          <PanelHead title="Bot Summaries" />
          <PanelBody>
            <div style={{ marginBottom: 10 }}>
              <select
                value={botFilter}
                onChange={(e) => setBotFilter(e.target.value)}
                style={controlStyle}
              >
                <option value="">All bots</option>
                {botTraderNames.map((name) => (
                  <option key={name} value={name}>
                    {name}
                  </option>
                ))}
              </select>
            </div>
            {!dbAvailable ? (
              <div style={emptyMsg}>{dbError}</div>
            ) : summaries.length === 0 ? (
              <div style={emptyMsg}>
                No bot decisions have been logged yet.
              </div>
            ) : (
              <DataTable maxHeight={520}>
                <thead>
                  <tr>
                    <Th>Bot</Th>
                    <Th align="right">Decisions</Th>
                    <Th align="right">Avg Edge</Th>
                    <Th>Latest Market</Th>
                    <Th align="right">FV</Th>
                    <Th align="right">Mkt</Th>
                    <Th align="right">PnL</Th>
                    <Th align="right">Orders/Fills</Th>
                  </tr>
                </thead>
                <tbody>
                  {summaries.map((b) => {
                    const avgEdge = Number(b.avg_edge) || 0;
                    const pnl = b.pnl || 0;
                    return (
                      <tr
                        key={b.trader_name}
                        onClick={() => setBotFilter(b.trader_name)}
                        style={{ cursor: "pointer" }}
                      >
                        <td style={truncCell} title={b.trader_name}>
                          {b.trader_name}
                        </td>
                        <Td mono align="right">
                          {fmtInt(b.decision_count)}
                        </Td>
                        <Td
                          mono
                          tone={avgEdge >= 0.1 ? "warn" : "dim"}
                          align="right"
                        >
                          {fmtProb(b.avg_edge)}
                        </Td>
                        <td
                          style={truncCell}
                          title={b.latest_market_name || "-"}
                        >
                          {b.latest_market_name || "-"}
                        </td>
                        <Td mono tone="yes" align="right">
                          {fmtProb(b.latest_fair_value)}
                        </Td>
                        <Td mono tone="accent" align="right">
                          {fmtProb(b.latest_market_price)}
                        </Td>
                        <Td
                          mono
                          tone={pnl >= 0 ? "yes" : "no"}
                          align="right"
                        >
                          {moneySigned(pnl)}
                        </Td>
                        <Td mono align="right">
                          {fmtInt(b.total_orders) + " / " + fmtInt(b.total_fills)}
                        </Td>
                      </tr>
                    );
                  })}
                </tbody>
              </DataTable>
            )}
          </PanelBody>
        </Panel>

        {/* Right — Recent Reasoning */}
        <Panel>
          <PanelHead
            title="Recent Reasoning"
            actions={
              <span style={mutedSpan}>{botFilter || "all bots"}</span>
            }
          />
          <PanelBody>
            <div
              style={{
                maxHeight: "calc(100vh - 330px)",
                minHeight: 420,
                overflow: "auto",
                display: "grid",
                gap: 10,
              }}
            >
              {!dbAvailable ? (
                <div style={emptyMsg}>{dbError}</div>
              ) : filteredDecisions.length === 0 ? (
                <div style={emptyMsg}>No matching bot decisions.</div>
              ) : (
                filteredDecisions.map((d) => {
                  const edge = d.edge || 0;
                  const orders = orderList(d);
                  const articles = articleList(d);
                  return (
                    <div
                      key={d.id}
                      style={{
                        border: "1px solid var(--border-2)",
                        background: "var(--surface-2)",
                        borderRadius: 8,
                        padding: 10,
                      }}
                    >
                      <div
                        style={{
                          display: "flex",
                          justifyContent: "space-between",
                          alignItems: "flex-start",
                          gap: 8,
                        }}
                      >
                        <div style={{ minWidth: 0 }}>
                          <div
                            title={d.market_name}
                            style={{
                              fontWeight: 650,
                              fontSize: 13,
                              overflow: "hidden",
                              textOverflow: "ellipsis",
                              whiteSpace: "nowrap",
                            }}
                          >
                            {d.market_name || "Market #" + d.market_id}
                          </div>
                          <div style={mutedSpan}>
                            {d.trader_name + " / " + shortTime(d.timestamp)}
                          </div>
                        </div>
                        <Pill tone={edge >= 0.1 ? "warn" : "accent"}>
                          {"edge " + fmtProb(d.edge)}
                        </Pill>
                      </div>

                      <div style={chipRow}>
                        <Pill tone="yes">{"FV " + fmtProb(d.fair_value)}</Pill>
                        <Pill tone="accent">
                          {"market " + fmtProb(d.market_price)}
                        </Pill>
                        <Pill>{"balance $" + dollarsFloat(d.balance)}</Pill>
                        <Pill>
                          {fmtInt(d.yes_pos) + " YES / " + fmtInt(d.no_pos) + " NO"}
                        </Pill>
                        <Pill>
                          {d.llm_duration_s
                            ? d.llm_duration_s.toFixed(1) + "s LLM"
                            : "LLM time -"}
                        </Pill>
                      </div>

                      {d.motivation ? (
                        <div
                          style={{
                            marginTop: 8,
                            fontSize: 12,
                            color: "var(--fg-3)",
                          }}
                        >
                          {d.motivation}
                        </div>
                      ) : null}
                      {d.analysis ? (
                        <div
                          style={{
                            marginTop: 8,
                            fontSize: 12,
                            color: "var(--fg-2)",
                            whiteSpace: "pre-wrap",
                            overflowWrap: "anywhere",
                          }}
                        >
                          {d.analysis}
                        </div>
                      ) : null}

                      {orders.length ? (
                        <div style={chipRow}>
                          {orders.map((o, idx) => {
                            const order = (o ?? {}) as LooseRecord;
                            const side = String(order.side || "");
                            const tone: Tone = side.includes("Yes")
                              ? "yes"
                              : "no";
                            return (
                              <Pill key={idx} tone={tone}>
                                {formatOrder(order)}
                              </Pill>
                            );
                          })}
                        </div>
                      ) : null}

                      {articles.length ? (
                        <div style={chipRow}>
                          {articles.map((a, idx) => {
                            const article = a as ArticleArg;
                            return (
                              <a
                                key={idx}
                                href={articleUrl(article)}
                                target="_blank"
                                rel="noreferrer"
                                title={articleLabel(article)}
                                style={articleLinkStyle}
                              >
                                {articleLabel(article)}
                              </a>
                            );
                          })}
                        </div>
                      ) : null}
                    </div>
                  );
                })
              )}
            </div>
          </PanelBody>
        </Panel>
      </div>
    </div>
  );
}
