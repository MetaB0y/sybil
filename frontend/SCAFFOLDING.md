# Sybil Frontend — Scaffolding Plan

> Reference doc. Decisions locked in during planning + revised after independent DevOps / FE / PM review.
> If you're picking this up cold, read **Decisions** and **Where the app lives** first, then jump to the highest unchecked box in **Status**.

## Status

- [ ] Step 1 — Project created + Node/pm pinned
- [ ] Step 2 — Runtime deps installed
- [ ] Step 3 — Dev deps installed
- [ ] Step 4 — shadcn/ui initialized
- [ ] Step 5 — Design tokens wired (selective `@theme`)
- [ ] Step 6 — Fonts wired
- [ ] Step 7 — Env files created (`.env.local` + `.env.example`)
- [ ] Step 8 — REST types generated and committed
- [ ] Step 9 — Nanos helper written + u64 mapping verified
- [ ] Step 10 — Providers shell mounted (QueryClient + RealtimeProvider placeholder)
- [ ] Step 11 — Smoke page renders typed REST + formatted bigint + WS first envelope
- [ ] Step 12 — Frontend CI job added + `pnpm build` green

Scaffolding is done when all 12 boxes are checked. After that: resolve **Pending decisions** below, then proceed to **Next milestones A → B → C**, then start on the market-detail page.

---

## Decisions

| Concern | Choice |
|---|---|
| Framework | **Next.js 15** (App Router) + TypeScript |
| Styling | **Tailwind v4** — map only color / spacing / radius / font scales via `@theme`. Use raw `var(--…)` for the long tail (shadows, motion durations, blurs). Don't try to expose all 80 tokens as utilities. |
| UI primitives | **shadcn/ui** (Radix under the hood) — components copy into repo, so heavy restyle is fine. See pending decisions if you want to drop shadcn and use Radix directly. |
| State store | **Zustand** (WS reducer in milestone B) |
| REST cache | **TanStack Query** (`QueryClientProvider` wired in scaffolding, Step 10) |
| REST types | **openapi-typescript** + **openapi-fetch** against `${NEXT_PUBLIC_API_BASE}/openapi.json` |
| Real-time | Native `WebSocket` to `/v1/blocks/ws` (milestones A/B) |
| Price chart | **TradingView Lightweight Charts** — wired when market-detail page ships, NOT during scaffolding |
| Sparklines | Hand-rolled inline SVG |
| Forms | **React Hook Form** + **Zod** — when order form ships |
| Animation | **Framer Motion**. ⚠ At 2s batch cadence, the batch-theater clock must use linear easing keyed to `block.height`, not wall-clock spring physics — Framer springs jank at this cadence. |
| Money math | All `*_nanos` fields as **`bigint`** via `parseNanos()`. NOT raw `number` — corrupts above 2^53. See [KNOWN_ISSUES.md #1](./KNOWN_ISSUES.md) for the workaround in place and the pending backend fix. |
| Signed orders | **borsh@2.0.0** — NOT installed in scaffolding. Add when wallet/auth ships. Lift schemas from `crates/sybil-api/static/trade.html`. ⚠ JS borsh and `borsh-rs` are NOT automatically wire-compatible — first integration must include a round-trip test against `/v1/orders/signed` with a Rust-generated fixture. |
| Package manager | **pnpm 9.x** (pinned via `packageManager` field in `package.json`) |
| Node | **20 LTS** (pinned via `.nvmrc` + `engines`) |
| TypeScript | strict mode + `noUncheckedIndexedAccess` + `exactOptionalPropertyTypes` |
| Linting | Next's ESLint defaults + Prettier (added via shadcn init) |
| Deploy | Vercel (deferred). `vercel.json` with `rootDirectory: frontend/web` checked in during scaffolding so preview deploys work day-one when flipped on. |
| Backend | `https://172-104-31-54.nip.io` (Rust REST + WS, no BFF) — see pending decisions re: real domain |

---

## Where the app lives

```
/Users/r/pr/Sybil/frontend/
├── handoff/        ← design source of truth (DO NOT MODIFY)
├── SCAFFOLDING.md  ← this file
└── web/            ← Next.js app (created in Step 1)
```

---

## Prerequisites

- Node 20+: `node -v` → v20.x or higher (`nvm install 20` if missing)
- pnpm 9+: `npm i -g pnpm`
- Backend reachable: `curl -sk https://172-104-31-54.nip.io/v1/health` → `{"status":"ok",...}`
- OpenAPI reachable: `curl -sk https://172-104-31-54.nip.io/openapi.json | head -c 100` → starts with a JSON spec

If either backend probe fails, **stop and resolve** — Steps 8/11 depend on both.

---

## Step-by-step

### Step 1 — Create project + pin Node and package manager

```bash
cd /Users/r/pr/Sybil/frontend
pnpm create next-app@latest web \
  --typescript --tailwind --app --eslint \
  --src-dir --import-alias "@/*" --turbopack
cd web
```

Then, in `web/`:

```bash
echo "20" > .nvmrc
```

Edit `package.json`:
- Add `"packageManager": "pnpm@9.15.0"` (or current 9.x)
- Add `"engines": { "node": ">=20.0.0", "pnpm": ">=9.0.0" }`
- **Pin Next.js to an exact patch** (no `^`) — Turbopack dev regressions in 15.x have bitten people. Whatever version `create-next-app` installed, lock it.

Edit `tsconfig.json` `compilerOptions`:
```json
"strict": true,
"noUncheckedIndexedAccess": true,
"exactOptionalPropertyTypes": true
```

### Step 2 — Runtime deps

```bash
pnpm add zustand @tanstack/react-query \
  framer-motion react-hook-form zod @hookform/resolvers
```

(Lightweight Charts and borsh deferred — see Decisions.)

### Step 3 — Dev deps

```bash
pnpm add -D openapi-typescript openapi-fetch prettier
```

### Step 4 — Initialize shadcn/ui

```bash
pnpm dlx shadcn@latest init
```

Prompts will vary by shadcn version; accept the **New York** style and **Zinc** base color (we override with Sybil tokens). Enable CSS variables. **Don't add any components yet** — pull them per-page when needed.

### Step 5 — Wire design tokens (selective `@theme`)

The handoff token file (`frontend/handoff/tokens/colors_and_type.css`, 321 lines) is the source. Two parts:

**(a)** Import the handoff CSS unchanged into `src/app/globals.css`:
```css
@import "../../../handoff/tokens/colors_and_type.css";
@import "tailwindcss";
```

**(b)** Below those imports, add a **selective** `@theme` block that exposes only the token groups you'll write as Tailwind utility names. Recommended scope:
- `--color-bg-*`, `--color-surface-*`, `--color-fg-*`, `--color-border-*`, `--color-accent*`, `--color-yes*`, `--color-no*`, `--color-warn*`, `--color-info*`
- `--font-display`, `--font-sans`, `--font-mono`
- `--spacing-*` (the 4–96px scale)
- `--radius-sm/md/lg/xl`

For everything else (shadows, motion durations/easings, custom letter-spacing, blurs), use raw `var(--…)` in CSS or `style={{}}`. Trying to map all 80 tokens to Tailwind utilities is where the v4 papercuts live.

⚠ Also: the handoff CSS contains `@import "https://fonts.googleapis.com/..."` lines. **Delete those** — Step 6 owns font loading, and the duplicate `@import` causes FOUT.

### Step 6 — Wire fonts

In `src/app/layout.tsx`, load Syne / Inter / JetBrains Mono via `next/font/google`:

```ts
import { Syne, Inter, JetBrains_Mono } from "next/font/google";

const display = Syne({
  subsets: ["latin"],
  variable: "--font-display",
  axes: ["wght"], // Syne is variable on weight 400-800
});
const sans = Inter({ subsets: ["latin"], variable: "--font-sans" });
const mono = JetBrains_Mono({ subsets: ["latin"], variable: "--font-mono" });
```

Apply all three variable classes on `<html>`. After running `pnpm dev`, verify in DevTools that `var(--font-display)` resolves to a Syne fallback chain.

### Step 7 — Environment files

Create both files in `frontend/web/`:

`.env.example` (committed):
```
NEXT_PUBLIC_API_BASE=https://172-104-31-54.nip.io
NEXT_PUBLIC_WS_BASE=wss://172-104-31-54.nip.io
NEXT_PUBLIC_SENTRY_DSN=
```

`.env.local` (gitignored, copy of `.env.example` with real values).

Confirm `.env.local` is in `.gitignore` (Next default already does this).

### Step 8 — REST types generated + committed

Add to `package.json` scripts:
```json
"types:generate": "openapi-typescript \"${NEXT_PUBLIC_API_BASE:-https://172-104-31-54.nip.io}/openapi.json\" -o src/lib/api/schema.d.ts"
```

Run:
```bash
pnpm types:generate
```

**Commit `src/lib/api/schema.d.ts`** to git. CI and Vercel builds must work without the demo VM being reachable.

Create `src/lib/api/client.ts`:
```ts
import createClient from "openapi-fetch";
import type { paths } from "./schema";

export const api = createClient<paths>({
  baseUrl: process.env.NEXT_PUBLIC_API_BASE!,
});
```

### Step 9 — Nanos helper + verify u64 mapping

`openapi-typescript` by default emits `number` for u64. JavaScript `Number` loses precision above 2^53. Verify and fix:

```bash
grep -E "_nanos.*:\s*number" src/lib/api/schema.d.ts | head
```

**If matches appear**, one of these must happen before Step 11:
- (preferred) backend: configure `utoipa` on the Rust side to serialize u64 as `string` and regenerate. Then `_nanos` fields become `string` in TS and we parse to `bigint` at the boundary.
- (fallback) add a post-process script `scripts/patch-bigints.mjs` that rewrites `_nanos: number` → `_nanos: string` in `schema.d.ts` after generation, called via `pnpm types:generate`.

Decide which path, then create `src/lib/format/nanos.ts`:
```ts
export const NANOS_PER_UNIT = 1_000_000_000n;

export const parseNanos = (s: string | bigint): bigint =>
  typeof s === "bigint" ? s : BigInt(s);

export const formatDollars = (nanos: bigint, opts?: { decimals?: number }): string => {
  const decimals = opts?.decimals ?? 2;
  const whole = nanos / NANOS_PER_UNIT;
  const frac = nanos % NANOS_PER_UNIT;
  const fracStr = frac.toString().padStart(9, "0").slice(0, decimals);
  return `$${whole.toString()}.${fracStr}`;
};

export const formatPercent = (nanos: bigint): string => {
  // 0..1e9 nanos = 0..100% probability
  const pct = Number(nanos) / 1e7; // safe — bounded 0–100
  return `${pct.toFixed(1)}%`;
};
```

### Step 10 — Providers shell

Create `src/app/providers.tsx`:
```tsx
"use client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";

// Placeholder — becomes the WS singleton owner in milestone B
function RealtimeProvider({ children }: { children: React.ReactNode }) {
  return <>{children}</>;
}

export function Providers({ children }: { children: React.ReactNode }) {
  const [qc] = useState(() => new QueryClient());
  return (
    <QueryClientProvider client={qc}>
      <RealtimeProvider>{children}</RealtimeProvider>
    </QueryClientProvider>
  );
}
```

Wrap `{children}` in `app/layout.tsx` with `<Providers>`. Wiring the shape now means milestone B is a no-op edit, not a layout refactor.

Also add `app/error.tsx`, `app/loading.tsx`, `app/not-found.tsx` — Next 15 expects these for sane UX.

### Step 11 — Smoke page (deeper)

Replace `src/app/page.tsx` with a `"use client"` page that proves the whole stack end-to-end:

1. Calls `api.GET("/v1/health")` via the typed client — renders `height` formatted via `nanos.ts` helpers (height is u64 too)
2. Calls `api.GET("/v1/markets/summary")` — renders `data.length` market count
3. Opens a `WebSocket` to `${NEXT_PUBLIC_WS_BASE}/v1/blocks/ws` — on first envelope, renders "WS OK · v1 · live", then closes
4. Background `var(--bg-1)`, "Sybil" heading in Syne color `var(--accent)`, all numbers in mono

If all four panels populate within a few seconds, the stack is end-to-end healthy. This page gets replaced when real `/markets` work starts.

### Step 12 — CI + verify

Add a GitHub Actions job (new file `.github/workflows/frontend.yml`) that runs on changes to `frontend/web/**`:

```yaml
name: Frontend CI
on:
  push:
    paths: ['frontend/web/**', '.github/workflows/frontend.yml']
  pull_request:
    paths: ['frontend/web/**']
jobs:
  build:
    runs-on: ubuntu-latest
    defaults: { run: { working-directory: frontend/web } }
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
        with: { version: 9 }
      - uses: actions/setup-node@v4
        with: { node-version: 20, cache: pnpm, cache-dependency-path: frontend/web/pnpm-lock.yaml }
      - run: pnpm install --frozen-lockfile
      - run: pnpm tsc --noEmit
      - run: pnpm lint
      - run: pnpm build
```

Locally verify:
```bash
pnpm dev      # all four smoke panels populate at localhost:3000
pnpm tsc --noEmit
pnpm lint
pnpm build
```

Commit `frontend/web/` (including `schema.d.ts` and `pnpm-lock.yaml`) to git.

---

## Target file structure after scaffolding

```
frontend/web/
├── src/
│   ├── app/
│   │   ├── layout.tsx          # fonts + <Providers>
│   │   ├── providers.tsx       # QueryClient + RealtimeProvider placeholder
│   │   ├── page.tsx            # smoke now, markets index later
│   │   ├── error.tsx           # Next 15 boundary
│   │   ├── loading.tsx
│   │   ├── not-found.tsx
│   │   ├── m/[id]/page.tsx     # added in market-detail milestone
│   │   ├── activity/page.tsx   # added later
│   │   ├── portfolio/page.tsx  # added later
│   │   └── globals.css         # handoff tokens + tailwind + @theme
│   ├── components/
│   │   └── ui/                 # shadcn primitives, added per-page
│   ├── lib/
│   │   ├── api/
│   │   │   ├── schema.d.ts     # generated, committed
│   │   │   └── client.ts       # openapi-fetch wrapper
│   │   ├── ws/                 # milestone A
│   │   ├── store/              # milestone B
│   │   └── format/
│   │       └── nanos.ts        # bigint helpers
│   └── ...
├── public/
├── .env.local                  # gitignored
├── .env.example                # committed
├── .nvmrc                      # "20"
├── next.config.ts
├── package.json                # engines + packageManager + pinned next
├── pnpm-lock.yaml              # committed
├── tsconfig.json               # strict + 2 extra flags
└── vercel.json                 # rootDirectory: frontend/web
```

---

## Out of scope for scaffolding

These come AFTER scaffolding, in named milestones:

- WS singleton + reconnect + visibility-change handling (milestone A)
- Zustand store + reducer + wiring into `RealtimeProvider` (milestone B)
- REST hydration + height handshake with WS replay (milestone C)
- Global nav, batch pill (first page milestone)
- All four production pages
- TradingView Lightweight Charts (market-detail milestone)
- Order entry form (market-detail milestone)
- `borsh@2.0.0` + signed-order serialization (auth/wallet milestone)
- Auth / wallet model
- Sentry / observability beyond placeholder env var
- Playwright tests (add after first stable page)
- Vercel deployment

---

## Pending decisions — resolve before page work

These are blocking decisions for the *first real page*, not for scaffolding itself. Park them in this file, settle before milestone A finishes:

1. **Batch cadence reconciliation.** Designs assume 60s; production is 2s. Several handoff copy lines hardcode "every 60s". Decide: slow blocks for UX, redesign the batch theater for a 2s strobe feel, or some hybrid. Affects all four pages.
2. **Real domain for the backend.** `172-104-31-54.nip.io` is IP-pinned — cert and DNS both tied to the current Linode IP. Strongly recommend a CNAME like `api.sybil.exchange` before frontend hardcodes the URL in env files / Vercel project. Otherwise an IP rotation rewrites every preview deploy's env.
3. **shadcn vs. raw Radix.** Sybil's design system is strict (yes/no semantics, tight radii, no gradients, no emoji, tabular nums everywhere). shadcn is fine because components copy into the repo, but a senior FE reviewer pushed for using `@radix-ui/*` primitives directly with CVA + tailwind-merge. Decide before installing your first component.
4. **u64 mapping fix path** (from Step 9). If `_nanos: number` matches: fix Rust `utoipa` to emit `string`, OR add a TS post-process script. Pick one; document it in the project.
5. **Logo file.** Handoff only ships `sybil-mark.png` (raster). Vector SVG needed before nav extraction in the first page milestone.
6. **`account_id` lifecycle.** Deferred to wallet milestone, but: the in-memory backend resets state on container restart. A localStorage `account_id` survives the wipe and becomes a ghost identity. Stamp it with backend's boot epoch (e.g. `state-root` from boot) and invalidate on mismatch.

---

## Next milestones (split into A / B / C)

The original plan bundled WS client + reconnect + visibility + Zustand reducer + REST hydration + provider into one milestone. Reviewer feedback: that's three milestones. Split:

### Milestone A — WS client + reconnect state machine
- `src/lib/ws/client.ts` — singleton `WebSocket` to `/v1/blocks/ws`
- Envelope dispatch: `Block` / `ReplayComplete` / `Lagged` per `docs/architecture/WebSocket Block Stream.md`
- Reconnect with `?from_block=lastSeenHeight+1`; fallback to fresh on "block not found"
- 30s server pings handled automatically by browser; 90s idle timeout respected
- `document.visibilitychange` listener: reconnect on `visible` if stale > 30s
- Exposes a minimal event emitter; no React/Zustand coupling yet

### Milestone B — Zustand store + RealtimeProvider wiring
- `src/lib/store/index.ts` — slices for `blocks`, `markets`, `orders`, `fills`, `connection`
- Reducer over WS envelopes — updates prices, appends fills, drops pending, patches positions
- `RealtimeProvider` (currently a no-op stub from Step 10) becomes the owner: opens WS, pipes envelopes into the store, manages lifecycle
- Components consume via Zustand selectors only

### Milestone C — REST hydration + height handshake
- On mount: TanStack Query fetches `/v1/state-root`, `/v1/markets/summary`, `/v1/markets/prices`, `/v1/accounts/:id/portfolio` (if account known)
- Capture `H₀ = state_root.height`
- WS subscribes with `from_block = H₀ + 1` only after snapshot lands
- Buffer incoming `block` events until `ReplayComplete`, then commit

After C is green: first real page is **`/m/[id]` (market detail)** — batch theater is the architecture stress test, everything else is a simpler variation.

---

## Conversation context (so future-Claude knows what's been decided and why)

- Backend live at `https://172-104-31-54.nip.io` — single Linode VM, docker-compose, in-memory state by design. Real deploy mechanism is `just deploy-*` (scp + compose up), NOT Kamal — DEPLOY.md is partially stale.
- Full API surface in `crates/sybil-api/src/app.rs`. OpenAPI at `/openapi.json`. CORS permissive.
- Block cadence 2s (`SYBIL_BLOCK_INTERVAL_MS=2000`). Handoff designs assume 60s — pending decision #1.
- WS contract in `docs/architecture/WebSocket Block Stream.md`. 64-block per-subscriber buffer; 100-block in-memory replay history. Tab backgrounded >2 min risks `Lagged`; offline >3 min risks "block not found".
- Frontend handoff (`frontend/handoff/`) is **design source of truth, not shippable code**. Babel-in-browser HTML previews of 4 pages. Lift tokens, layouts, copy 1:1; rewrite the JSX into Next idioms.
- Real-time architecture: ONE WS to `/v1/blocks/ws` feeds a Zustand reducer; every component reads store slices. Never subscribe per-component. Never put the WS in a React Context that re-renders on every message.
- All `*_nanos` fields are u64 → `bigint` in TS. Verified in Step 9.
- Auth deferred — stash `account_id` in `localStorage` for now (with the lifecycle caveat in pending decision #6).
- BFF deferred — call Rust API directly. Next.js Route Handlers available if a real reason appears.
- This plan was independently reviewed by DevOps, FE, and PM agents and revised before execution. Verdicts: "ship with fixes" — those fixes are now incorporated above.
