"use client";

import { useStore } from "@/lib/store";
import { shortRoot } from "@/lib/dev/format";

export function DevStatusLine() {
  const latestBlock = useStore((s) => s.latestBlock);
  const connection = useStore((s) => s.connection);

  const live = connection.state === "live";

  return (
    <div
      style={{
        display: "flex",
        gap: 16,
        alignItems: "center",
        fontFamily: "var(--font-mono)",
        fontSize: 12,
        color: "var(--fg-3)",
        whiteSpace: "nowrap",
        flexWrap: "wrap",
      }}
    >
      <span
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: 6,
          padding: "3px 7px",
          borderRadius: 999,
          border: "1px solid var(--border-2)",
        }}
      >
        <span
          style={{
            width: 7,
            height: 7,
            borderRadius: 999,
            background: live ? "var(--yes)" : "var(--fg-4)",
            boxShadow: live ? "0 0 8px var(--yes)" : undefined,
          }}
        />
        {live ? "live" : "connecting"}
      </span>
      <span>
        block{" "}
        <strong style={{ color: "var(--fg-1)" }}>
          {latestBlock?.height ?? "..."}
        </strong>
      </span>
      <span>
        root{" "}
        <strong style={{ color: "var(--fg-1)" }}>
          {shortRoot(latestBlock?.state_root)}
        </strong>
      </span>
    </div>
  );
}
