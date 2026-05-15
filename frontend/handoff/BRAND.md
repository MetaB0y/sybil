# Sybil Design System

> The first prediction market built on frequent batch auctions.
> No front-running. No MEV. Fair pricing.

Sybil is a prediction market exchange currently in testnet. The product replaces the continuous limit order book that Polymarket and Kalshi use with **frequent batch auctions** — orders accumulate in short windows and clear at a single uniform price, eliminating the "sniper's tax" that lets sophisticated actors pick off stale orders the moment new information lands.

The brand voice is academic, dry, technical. Think arXiv preprint x trading-desk terminal. Sybil publishes original research (spike analysis, agentic-research dashboards) as a wedge into the space — they're earning the right to redesign the market by proving they understand the existing one in painful detail.

## Surfaces

Sybil has two distinct surfaces today, plus a third planned product:

| Surface | Status | What it is |
|---|---|---|
| **Marketing / Coming Soon** | Live | One-page landing at sybil.exchange. Wordmark, tagline, links to research + socials. |
| **Research dashboards** | Live | Long-form data essays — spike-analysis, agentic-research. Heavy on tables, monospace, methodology callouts. |
| **The Exchange (web app)** | Planned | All-markets index, market detail/trade page, activity, profile, portfolio, docs. This is what we're building toward. |

## Sources used to build this system

The user did not attach a codebase or Figma file. Everything in this design system was reverse-engineered from the public web presence. If you have access, the source material is:

- **Landing**: https://www.sybil.exchange/
- **Spike Analysis essay**: https://www.sybil.exchange/spike-analysis
- **Agentic Research dashboard**: https://www.sybil.exchange/agentic-research
- **Substack** (long-form): https://sybilpm.substack.com — "the sniper's tax" essay
- **GitHub**: https://github.com/SybilPM/prediction-markets-are-fisher-markets — math primer
- **Twitter**: https://x.com/sybil_pm

The user explicitly said "let's do everything from scratch" — meaning the system below is a synthesized direction grounded in the existing voice + visual hints, not a literal copy of the live site.

## Index

| File / folder | What's in it |
|---|---|
| `README.md` | This file. Brand context, content fundamentals, visual foundations, iconography. |
| `SKILL.md` | Agent skill manifest. How to use this system in Claude Code or here. |
| `colors_and_type.css` | All design tokens — colors, type, spacing, radii, shadows. Import once and you're set. |
| `fonts/` | Webfont files (with substitution notes). |
| `assets/` | Logos, marks, OG images. |
| `preview/` | Design-system spec cards (typography, color, spacing, components). |
| `ui_kits/exchange/` | High-fidelity UI kit for the exchange web app. Click-thru prototype with markets index, market detail (chart + batch clock + order panel), activity feed, portfolio, docs. |
| `ui_kits/marketing/` | Recreation of the live marketing site (hero + research grid + footer). |
| `SKILL.md` | Agent-skill manifest — load this in Claude Code or invoke as a skill. |
| `screenshots/` | Verification screenshots taken during build. |

## Caveats and asks

- **No codebase or Figma was attached** — everything below was reverse-engineered from sybil.exchange + the two research pages + the substack/github links. The user explicitly said "do everything from scratch", so the system is a synthesized direction, not a literal copy.
- **Substituted icon set**: Lucide. The live site barely uses icons; if the team has chosen a different set (Phosphor, Tabler), flag it and I'll swap.
- **Substituted body + mono fonts**: Inter and JetBrains Mono. Syne is confirmed from the live site. The other two are choices that match the personality you picked. All three load from Google Fonts.
- **The exchange app does not exist yet** — the UI kit is a directional recreation of what it _should_ look like, anchored in the brand voice + visual hints from the live site. Once a real exchange surface ships, this kit should be re-grounded in actual code/design.
- **Wordmark-only logo file** is not yet copied — only the spike-mark exists in `assets/`. If a vector wordmark file exists, share it.
- **Batch cadence** (60s) in the kit is a placeholder. Real Sybil might use 250ms / 1s / 60s windows depending on market depth.

---

## CONTENT FUNDAMENTALS

### Voice

Sybil writes like a quant who reads a lot. Dry, precise, slightly academic. Confident without being loud. Never breathless. Never crypto-bro.

**Three adjectives**: precise · dry · transparent

### Casing

**Sentence case by default.** Headers, page titles, body copy use standard sentence capitalization ("All markets", "Place batch order", "Order queued. Clears at 14:30 UTC."). Title Case is acceptable for formal research titles ("Deep Dive into Market Maker Losses on Polymarket"). UPPERCASE is reserved for:

1. The wordmark (`SYBIL`)
2. Hard taxonomic labels in data UIs — tier grades (`A`–`E`), status enums (`LIVE`, `CLOSED`, `RESOLVED`), eyebrow / section identifiers above headlines.

The `//` annotation glyph and short mono labels in chrome may stay lowercase to read as code-comment voice ("// methodology").

### Pronouns

**No "we", no "you", no "I".** Sybil writes in the third person about the market, the trader, the data. When a pronoun is unavoidable, "we" is acceptable for editorial / research authorship ("we identify three classes of sniper"). Avoid "you" entirely in product copy — say "trader" or just describe the action ("place a batch order", not "you place a batch order").

### Numbers, units, ranges

- **Always use real numbers.** "$311K total sniped" is more Sybil than "millions sniped".
- Decimals over fractions. `1.38%` not "about one and a half percent".
- Use `K` / `M` / `B` (no period). Currency symbol attached: `$311K`.
- Ranges use en-dash with no spaces: `5–8 minutes`, `tier A–E`.
- Probabilities shown as percent with no decimal unless ≤1%: `64%`, `0.4%`.
- Time always 24h in product UI: `14:30 UTC`. Marketing copy can use natural language.

### Punctuation

- **Em dashes** for asides, not parentheses where avoidable. Set tight: `Sybil—the first FBA-based market—is in testnet`.
- **Slashes** for shorthand in data UIs: `Loss/Vol ratio`, `Yes/No`.
- **`//`** prefix for code-comment-style annotations and methodology asides. Borrowed from the spike-analysis page (`// why this formula`).
- **`→`** for forward links and CTAs. Not `>` and not `›`. Always after a space.
- **`↗`** for external links that leave the property.
- **`⚠`** for warnings and caveats. Used sparingly.

### Emoji

**No emoji.** Anywhere. Not in marketing, not in product, not in error states. The OG image is a line graph, not a 📈.

### Sample copy — taken from / written in the Sybil voice

| Where | Copy |
|---|---|
| Hero (live) | `the first prediction market built on frequent batch auctions` |
| Hero sub | `no front-running. no MEV. fair pricing.` |
| CTA | `join early access →` |
| Empty state | `no markets match these filters. broaden the date range or clear categories.` |
| Methodology callout | `// snipers are wallets that bought within 90s of a price spike >5% and exited within 24h.` |
| Disclaimer | `agent-generated research. treat findings accordingly.` |
| Status pill | `testnet · v0.3.1` |
| Confirmation | `order queued. clears at 14:30:00 UTC in the next batch.` |

### What to avoid

- "🚀", "💎", "wagmi", "ser", "anon", "GM" — no crypto-vernacular. Sybil is crypto-native by *infrastructure*, not by tone.
- "Game-changing", "revolutionary", "the future of" — empty marketing.
- Exclamation points in any product chrome.
- Title Case for buttons (`Place Order`) — write `place order`.
- Sentence-final periods on standalone UI labels and one-line headers.

---

## VISUAL FOUNDATIONS

### Theme

**Dark primary, single theme.** Light theme is not in scope. The aesthetic is reading-room-at-3am: deep ink background, off-white primary text, color used surgically.

### Color philosophy

A near-black foundation (`#0A0E12` — "deep ocean", named on the live site) with a graphite mid-layer for cards/surfaces. Text is warm-white at 92% luminance — full white is too clinical. Accents are restrained:

- **Cyan** as the brand spotlight (`#3FB6D9`) — used for links, focus rings, and the wordmark on hover. Echoes the line-graph mark.
- **Yes-green / No-red** as the only loud colors in the system, and *only* in trading contexts. Yes is `#5BD99A` (mint-leaning), No is `#E8556C` (coral-leaning, not stop-light red). They're tuned to feel like a Bloomberg terminal, not a casino.
- **Amber** (`#E8B447`) for warnings, testnet status, and the `⚠` glyph.

No gradients on backgrounds. No gradients on buttons. Gradients allowed only as **protection scrims** at the top/bottom of scrollable content (10% black → transparent) and inside the line-chart fill under a price series (cyan @ 24% → 0%).

### Typography

Three families, no more:

1. **Syne** (Variable) — display + wordmark. Confirmed from the live site. Used at 72px+ for the hero wordmark and at 24–40px for section headers.
2. **Inter** (Variable) — body, UI chrome, navigation, table cells, button labels. The workhorse.
3. **JetBrains Mono** (Variable) — all numerics, prices, addresses, code, methodology callouts, the `//` annotations. Tabular figures only.

Note: `Syne` is the live-site font — confirmed. `Inter` and `JetBrains Mono` are deliberate choices that match the "clean / minimal / Stripe-ish + crypto-native / monospace" personality the user picked. All three are on Google Fonts; substitution is unlikely to be needed.

### Spacing scale

8-point grid. Tokens in `colors_and_type.css`:

```
--space-1: 4px    --space-5: 24px
--space-2: 8px    --space-6: 32px
--space-3: 12px   --space-7: 48px
--space-4: 16px   --space-8: 64px   --space-9: 96px
```

### Corner radii

Tight. The brand reads as engineered, not friendly.

```
--radius-sm: 2px   (chips, badges, micro-buttons)
--radius-md: 4px   (buttons, inputs, dropdowns)
--radius-lg: 8px   (cards, modals, surfaces)
--radius-xl: 12px  (largest container — used rarely)
```

Pills exist (`9999px`) but only for status indicators (`testnet`, `live`, `resolved`) — never for buttons.

### Borders

Borders are how Sybil draws the world. They do more work than shadow.

- **Hairline divider** — `1px solid rgba(255,255,255,0.06)` — the default surface separator. Used between table rows, around cards, in toolbars.
- **Active border** — `1px solid rgba(255,255,255,0.14)` — for hover states on cards and selected nav items.
- **Focus border** — `1px solid #3FB6D9` + `box-shadow: 0 0 0 3px rgba(63,182,217,0.18)` — for keyboard focus on inputs and buttons.

### Shadows

Used minimally; the dark UI doesn't need to shout depth.

- **Surface lift** — `0 1px 0 rgba(255,255,255,0.04) inset` on cards and rows. Inner top hairline. This is the signature elevation move: light from above, drawn as a 1px inner highlight.
- **Floating** — `0 16px 32px -16px rgba(0,0,0,0.6)` for menus, popovers, modals. Soft, no spread.
- **Press** — no shadow change. The surface darkens by 4% instead.

### Backgrounds

- The brand mark uses a subtle **graph-paper grid texture** (visible in the OG image, see `assets/og-image.png`). Use it sparingly — header backgrounds, the all-markets hero, the empty-state region of charts. Never under text-heavy surfaces.
- No full-bleed photography. No illustration. The dataset *is* the imagery — sparkline charts, order book heatmaps, network graphs.
- No noise/grain overlay. The grid does that work.

### Animation

Restrained. Functional, not delightful.

- **Easing**: `cubic-bezier(0.2, 0, 0, 1)` (standard) for most transitions. `cubic-bezier(0.4, 0, 0.6, 1)` (in-out) for color crossfades.
- **Durations**: 120ms (state changes — hover, press), 200ms (panel open/close), 320ms (page-level transitions). Nothing over 400ms.
- **Bounce / spring**: never.
- **Number tickers**: digits roll smoothly when prices update. Use a 200ms tween on the changed digit only — *not* on the whole string.
- **Batch clock**: in market views, a thin 1px progress bar at the top of the trade panel ticks down to the next batch clear. Linear, no easing — it's a clock, not a vibe.
- **Page enters**: 8px upward translate + opacity 0→1 over 200ms. Stagger sibling elements by 24ms.

### Hover & press

- **Hover on text/buttons**: text color steps from `--fg-2` (75%) to `--fg-1` (92%). No underline animation. Cursor changes only when actually clickable.
- **Hover on cards**: border steps from 6%→14% white. No scale, no shadow.
- **Hover on links**: cyan accent appears as a 1px underline, 2px below baseline, full opacity.
- **Press**: surface darkens 4%, no scale-down. (Scale-down feels iOS, which is wrong here.)

### Transparency & blur

- **Backdrop blur** is used on **fixed overlays only**: the top nav (`backdrop-filter: blur(12px)` over a `rgba(10,14,18,0.72)` fill) and dropdown panels. Never used decoratively.
- Translucent fills on the body or cards: never. Cards are solid surfaces.

### Layout rules

- **Top nav is fixed**, 56px tall, full-width, hairline bottom border + backdrop blur.
- **Side rails are sticky**, not fixed — they scroll with the page until they hit the nav, then stick.
- **Content max-width**: 1440px for the exchange, 720px for research essays.
- **Tables fill their container**; no awkward fixed-width columns.
- **Numbers right-align** in tables. Always. Tabular figures.

### Cards

The default card is:

```
background: var(--surface-1)        /* #11161C */
border: 1px solid var(--border-1)   /* rgba(255,255,255,0.06) */
border-radius: var(--radius-lg)     /* 8px */
box-shadow: 0 1px 0 rgba(255,255,255,0.04) inset
padding: 20px 24px
```

That's it. No tilt, no gradient header, no colored accent border. The card is a quiet container; the *data inside* is the design.

---

## ICONOGRAPHY

Sybil's iconography is a means, not a feature. Two principles:

1. **Line icons over filled.** 1.5px stroke, square caps where it reads cleanly, round caps for organic shapes. Match the line weight of the wordmark mark itself.
2. **Currency-colored icons are forbidden.** Icons are always `currentColor`, inheriting the surrounding text color. Yes-green and no-red colors live on numbers and bars, never on icons.

### What's used

- **Lucide** (`lucide.dev`) — confirmed match for the brand's stroke-weight and minimal style. Linked from CDN. **This is the primary icon set.**
- **Custom marks**:
  - `assets/sybil-mark.png` — the line-graph spike on grid (logomark from the live site)
  - `assets/og-image.png` — same mark, used as social/OG
  - `assets/favicon.ico` — pulled from the live site
- **Glyph icons (single Unicode chars used as icons)**:
  - `→` forward link / CTA arrow (U+2192)
  - `↗` external link (U+2197)
  - `⚠` warning, testnet, methodology caveat (U+26A0)
  - `·` bullet separator in metadata strings (U+00B7) — `sybil.exchange · testnet · v0.3.1`
  - `//` text marker for code-style annotations (not a Unicode char — typed slashes, mono-styled)

### Emoji

Never. See Content Fundamentals.

### Iconography substitution flag

⚠ The live Sybil site does not appear to use a published icon set — there are very few icons on the marketing pages. **Lucide is a substitution** chosen to match the brand's visual stroke weight and minimalism. If Sybil internally uses a different set (Phosphor, Tabler, Heroicons), please flag it and I'll swap.

### Logo files

- `assets/sybil-mark.png` — 400×400, dark background, white spike. Use this on dark surfaces.
- `assets/og-image.png` — same artwork, 1200×630, social meta.
- `assets/favicon.ico` — multi-size favicon.

A wordmark-only file (the `SYBIL` text-only logo) is **not yet copied** — it would need to be either pulled from a vector source or rebuilt in Syne. Flagged below.

---
