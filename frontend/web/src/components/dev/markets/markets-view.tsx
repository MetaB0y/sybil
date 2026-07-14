"use client";

import { useState, type CSSProperties } from "react";

import { DataTable, Td, Th } from "@/components/dev/primitives/data-table";
import { Panel, PanelBody, PanelHead } from "@/components/dev/primitives/panel";
import { Pill } from "@/components/dev/primitives/pill";
import {
  filterMarkets,
  pendingCount,
  pendingIndex,
  priceGap,
  priceState,
  priceStateClass,
} from "@/lib/dev/derive";
import {
  useDevGroups,
  useDevMarkets,
  useDevPendingOrders,
} from "@/lib/dev/fetchers";
import { dollars, fmtPct, fmtPrice, pctWidth } from "@/lib/dev/format";

const STATE_OPTIONS: ReadonlyArray<readonly [string, string]> = [
  ["all", "All states"],
  ["marked", "Has Sybil mark"],
  ["ref", "Reference only"],
  ["none", "No Sybil mark"],
  ["pending", "Has pending"],
  ["mismatch", "Large ref mismatch"],
];

const controlStyle: CSSProperties = {
  width: "100%",
  border: "1px solid var(--border-2)",
  background: "var(--surface-1)",
  color: "var(--fg-1)",
  borderRadius: 6,
  padding: "7px 9px",
  fontFamily: "inherit",
};

export function MarketsView() {
  const [marketSearch, setMarketSearch] = useState<string>("");
  const [selectedGroup, setSelectedGroup] = useState<string>("");
  const [marketStateFilter, setMarketStateFilter] = useState<string>("all");

  const markets = useDevMarkets().data ?? [];
  const groups = useDevGroups().data ?? [];
  const pendingOrders = useDevPendingOrders().data ?? [];

  const pendingIdx = pendingIndex(pendingOrders);
  const rows = filterMarkets(
    markets,
    { search: marketSearch, group: selectedGroup, state: marketStateFilter },
    pendingIdx,
    groups,
  );

  return (
    <Panel>
      <PanelHead
        title="Market Analytics"
        actions={
          <span style={{ color: "var(--fg-3)", fontSize: 12 }}>
            {rows.length + " shown / " + markets.length + " total"}
          </span>
        }
      />
      <PanelBody>
        <div
          style={{
            display: "grid",
            gridTemplateColumns:
              "minmax(180px,1.4fr) minmax(160px,0.8fr) minmax(140px,0.6fr)",
            gap: 10,
            marginBottom: 12,
          }}
        >
          <input
            value={marketSearch}
            onChange={(e) => setMarketSearch(e.target.value)}
            placeholder="Search markets, IDs, names"
            style={controlStyle}
          />
          <select
            value={selectedGroup}
            onChange={(e) => setSelectedGroup(e.target.value)}
            style={controlStyle}
          >
            <option value="">All groups</option>
            {groups.map((g) => (
              <option key={g.name} value={g.name}>
                {g.name + " (" + g.market_ids.length + ")"}
              </option>
            ))}
          </select>
          <select
            value={marketStateFilter}
            onChange={(e) => setMarketStateFilter(e.target.value)}
            style={controlStyle}
          >
            {STATE_OPTIONS.map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </div>

        <DataTable maxHeight="calc(100vh - 260px)">
          <thead>
            <tr>
              <Th>#</Th>
              <Th>Market</Th>
              <Th>State</Th>
              <Th align="right">Ref</Th>
              <Th align="right">Yes</Th>
              <Th align="right">No</Th>
              <Th />
              <Th align="right">Volume</Th>
              <Th align="right">Pending</Th>
              <Th align="right">Gap</Th>
            </tr>
          </thead>
          <tbody>
            {rows.map((m) => {
              const gap = priceGap(m);
              return (
                <tr key={m.market_id}>
                  <Td mono tone="dim">
                    {m.market_id}
                  </Td>
                  <td
                    style={{
                      padding: "7px 9px",
                      borderBottom: "1px solid var(--border-2)",
                      verticalAlign: "top",
                      maxWidth: 520,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                    title={m.name}
                  >
                    {m.name}
                  </td>
                  <Td>
                    <Pill tone={priceStateClass(m)}>{priceState(m)}</Pill>
                  </Td>
                  <Td mono tone="accent" align="right">
                    {fmtPrice(m.reference_price_nanos)}
                  </Td>
                  <Td mono tone="yes" align="right">
                    {fmtPrice(m.yes_price_nanos)}
                  </Td>
                  <Td mono tone="no" align="right">
                    {fmtPrice(m.no_price_nanos)}
                  </Td>
                  <Td>
                    <span
                      style={{
                        display: "inline-block",
                        width: 86,
                        height: 5,
                        borderRadius: 99,
                        background: "var(--surface-3)",
                        overflow: "hidden",
                      }}
                    >
                      <span
                        style={{
                          display: "block",
                          height: "100%",
                          borderRadius: 99,
                          background: "var(--accent)",
                          width:
                            pctWidth(
                              m.reference_price_nanos ?? m.yes_price_nanos,
                            ) + "%",
                        }}
                      />
                    </span>
                  </Td>
                  <Td mono align="right">
                    {"$" + dollars(m.volume_nanos)}
                  </Td>
                  <Td mono tone="warn" align="right">
                    {pendingCount(pendingIdx, m.market_id)}
                  </Td>
                  <Td mono tone={gap > 0.1 ? "no" : "dim"} align="right">
                    {fmtPct(gap)}
                  </Td>
                </tr>
              );
            })}
          </tbody>
        </DataTable>
      </PanelBody>
    </Panel>
  );
}
