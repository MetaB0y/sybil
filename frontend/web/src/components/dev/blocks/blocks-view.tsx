"use client";

import { useState, type CSSProperties } from "react";

import { BlockBarChart } from "@/components/dev/block-bar-chart";
import { Panel, PanelBody, PanelHead } from "@/components/dev/primitives/panel";
import { Stat, StatGrid } from "@/components/dev/primitives/stat";
import { dollars, shortRoot } from "@/lib/dev/format";
import { useDevRecentBlocks } from "@/lib/dev/use-recent-blocks";
import type { DevBlock } from "@/lib/dev/types";

/**
 * Local port of the console's `blockSummary` (index.html:1549-1560). Not
 * exported from lib/dev/derive.ts, so it lives here.
 */
function blockSummary(b: DevBlock | null): string {
  if (!b) return "";
  return JSON.stringify(
    {
      height: b.height,
      timestamp_ms: b.timestamp_ms,
      state_root: b.state_root,
      parent_hash: b.parent_hash,
      clearing_prices: Object.keys(b.clearing_prices_nanos ?? {}).length,
      system_events: (b.system_events ?? []).length,
      bridge: b.bridge ?? {},
    },
    null,
    2,
  );
}

const emptyStyle: CSSProperties = {
  padding: "32px 12px",
  textAlign: "center",
  color: "var(--fg-4)",
  fontSize: 12,
};

const preStyle: CSSProperties = {
  whiteSpace: "pre-wrap",
  wordBreak: "break-word",
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  color: "var(--fg-3)",
  margin: "0 0 4px",
  background: "var(--surface-2)",
  border: "1px solid var(--border-2)",
  borderRadius: 6,
  padding: "8px 10px",
  overflow: "auto",
};

const sectionTitleStyle: CSSProperties = {
  margin: "14px 0 8px",
  fontSize: 11,
  fontWeight: 650,
  letterSpacing: 0.4,
  textTransform: "uppercase",
  color: "var(--fg-3)",
};

export function BlocksView() {
  const { blocks, latestBlock } = useDevRecentBlocks();
  const [selectedBlock, setSelectedBlock] = useState<DevBlock | null>(null);

  // Effective selection: explicit click, else the latest block.
  const selected = selectedBlock ?? latestBlock;

  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "minmax(0,1.4fr) minmax(360px,0.8fr)",
        gap: 12,
      }}
    >
      <Panel>
        <PanelHead title="Chain Blocks" />
        <PanelBody>
          <BlockBarChart blocks={blocks} metric="fills" height={180} />
          <div
            style={{
              marginTop: 12,
              maxHeight: 520,
              overflow: "auto",
              border: "1px solid var(--border-2)",
              borderRadius: 6,
            }}
          >
            {blocks.length === 0 ? (
              <div style={emptyStyle}>Waiting for block history...</div>
            ) : (
              [...blocks].reverse().map((b) => {
                const active = selected != null && selected.height === b.height;
                return (
                  <div
                    key={b.height}
                    onClick={() => setSelectedBlock(b)}
                    style={{
                      display: "grid",
                      gridTemplateColumns: "72px 68px 90px 1fr",
                      gap: 10,
                      padding: "8px 10px",
                      borderBottom: "1px solid var(--border-2)",
                      cursor: "pointer",
                      fontSize: 12,
                      background: active ? "var(--accent-faint)" : undefined,
                    }}
                  >
                    <span style={{ color: "var(--accent)" }}>
                      {"#" + b.height}
                    </span>
                    <span
                      style={{
                        color:
                          (b.fill_count ?? 0) > 0
                            ? "var(--yes)"
                            : "var(--fg-4)",
                      }}
                    >
                      {(b.fill_count ?? 0) + " fills"}
                    </span>
                    <span>{"$" + dollars(b.total_volume_nanos)}</span>
                    <span style={{ color: "var(--fg-4)" }}>
                      {shortRoot(b.state_root) +
                        " / " +
                        (b.order_count ?? 0) +
                        " orders"}
                    </span>
                  </div>
                );
              })
            )}
          </div>
        </PanelBody>
      </Panel>

      <Panel>
        <PanelHead
          title="Selected Block"
          actions={
            <span style={{ color: "var(--fg-3)", fontSize: 12 }}>
              {selected ? "#" + selected.height : "none"}
            </span>
          }
        />
        <PanelBody>
          {selected == null ? (
            <div style={emptyStyle}>
              Select a block to inspect fills, rejections, prices, and roots.
            </div>
          ) : (
            <>
              <StatGrid columns={3}>
                <Stat label="Orders" value={selected.order_count ?? 0} />
                <Stat
                  label="Fills"
                  value={selected.fill_count ?? 0}
                  tone={(selected.fill_count ?? 0) > 0 ? "yes" : "no"}
                />
                <Stat
                  label="Volume"
                  value={"$" + dollars(selected.total_volume_nanos)}
                  tone="accent"
                />
              </StatGrid>
              <h3 style={sectionTitleStyle}>Root And Prices</h3>
              <pre style={preStyle}>{blockSummary(selected)}</pre>
              <h3 style={sectionTitleStyle}>Fills</h3>
              <pre style={preStyle}>
                {JSON.stringify((selected.fills ?? []).slice(0, 12), null, 2)}
              </pre>
              <h3 style={sectionTitleStyle}>Rejections</h3>
              <pre style={preStyle}>
                {JSON.stringify(
                  (selected.rejections ?? []).slice(0, 12),
                  null,
                  2,
                )}
              </pre>
            </>
          )}
        </PanelBody>
      </Panel>
    </div>
  );
}
