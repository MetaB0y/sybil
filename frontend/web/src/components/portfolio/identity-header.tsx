"use client";

/**
 * Identity row at the top of /portfolio: 4×4 deterministic avatar tile
 * derived from the compressed pubkey, alias generated from accountId,
 * and a shortened address chip. Matches the handoff `IdentityStrip`.
 */

const ALIAS_LEFT = [
  "anon",
  "fuzz",
  "wraith",
  "echo",
  "halo",
  "drift",
  "lurk",
  "warp",
  "myth",
  "void",
];
const ALIAS_RIGHT = [
  "vega",
  "atlas",
  "nova",
  "polaris",
  "hydra",
  "cygnus",
  "lyra",
  "aurora",
  "pulse",
  "specter",
];

export function IdentityHeader({
  accountId,
  publicKeyHex,
}: {
  accountId: number;
  publicKeyHex: string;
}) {
  const alias = aliasFromAccountId(accountId);
  const address = shortAddress(publicKeyHex);

  return (
    <header
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-3)",
      }}
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
        <h1
          style={{
            margin: 0,
            fontFamily: "var(--font-display)",
            fontWeight: 600,
            fontSize: "var(--fs-32)",
            lineHeight: "var(--lh-32)",
            letterSpacing: "var(--track-tight)",
            color: "var(--fg-1)",
          }}
        >
          Portfolio
        </h1>
        <p
          style={{
            margin: 0,
            fontFamily: "var(--font-mono)",
            fontSize: 11,
            color: "var(--fg-4)",
            letterSpacing: "var(--track-wide)",
          }}
        >
          {"// "}positions · orders · history for {address}
        </p>
      </div>

      <div
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: 12,
        }}
      >
        <AvatarTile publicKeyHex={publicKeyHex} />
        <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
          <span
            className="tabular"
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: 14,
              color: "var(--fg-1)",
            }}
          >
            {address}
          </span>
          <span
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: 10,
              color: "var(--fg-3)",
              letterSpacing: "var(--track-wide)",
              textTransform: "uppercase",
            }}
          >
            {alias}
          </span>
        </div>
      </div>
    </header>
  );
}

function AvatarTile({ publicKeyHex }: { publicKeyHex: string }) {
  // 4x4 grid of squares. For each cell, hash a slice of the pubkey to a bit
  // and a hue; off-cells are bg-2, on-cells are accent variants.
  const bytes = hexToBytes(publicKeyHex);
  const cells: Array<{ on: boolean; tone: number }> = [];
  for (let i = 0; i < 16; i++) {
    const b = bytes[i % bytes.length] ?? 0;
    cells.push({ on: (b & 1) === 1, tone: b });
  }
  const CELL = 9;
  const GAP = 1;
  const SIZE = CELL * 4 + GAP * 3;

  return (
    <div
      aria-hidden
      style={{
        width: SIZE + 4,
        height: SIZE + 4,
        padding: 2,
        border: "1px solid var(--border-1)",
        borderRadius: 4,
        background: "var(--bg-2)",
        display: "grid",
        gridTemplateColumns: `repeat(4, ${CELL}px)`,
        gridTemplateRows: `repeat(4, ${CELL}px)`,
        gap: GAP,
      }}
    >
      {cells.map((c, i) => (
        <div
          key={i}
          style={{
            background: c.on
              ? `color-mix(in srgb, var(--accent) ${30 + (c.tone % 60)}%, transparent)`
              : "transparent",
            borderRadius: 1,
          }}
        />
      ))}
    </div>
  );
}

function aliasFromAccountId(accountId: number): string {
  const left = ALIAS_LEFT[accountId % ALIAS_LEFT.length]!;
  const right =
    ALIAS_RIGHT[Math.floor(accountId / ALIAS_LEFT.length) % ALIAS_RIGHT.length]!;
  return `${left}-${right}`;
}

function shortAddress(hex: string): string {
  if (hex.length < 12) return `0x${hex}`;
  const head = hex.slice(0, 4).toUpperCase();
  const tail = hex.slice(-4).toUpperCase();
  return `0x${head}···${tail}`;
}

function hexToBytes(hex: string): number[] {
  const out: number[] = [];
  for (let i = 0; i + 1 < hex.length; i += 2) {
    out.push(parseInt(hex.slice(i, i + 2), 16));
  }
  return out;
}
