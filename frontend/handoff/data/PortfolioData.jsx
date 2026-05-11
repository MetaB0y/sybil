// Mock data for the Sybil Portfolio page.
// Trader: 0xA17F···c92E (alias: anon-vega)

const TRADER = {
  address: '0xA17Fb5C2De8E1c44778Ba9E8c92E',
  short:   '0xA17F···c92E',
  alias:   'anon-vega',
  joined:  '142 d ago',
  tier:    'B',           // A–E
  rank:    187,
  ofTotal: 18_402,
  pctile:  98.9,          // top X%
};

// ── Headline numbers ──────────────────────────────────────────────────────
const PORTFOLIO = {
  totalValue:        12_847.42,        // current $ value of all open positions + cash
  cash:                 3_104.18,
  positionsValue:      9_743.24,
  netDeposits:         8_500.00,
  unrealizedPnL:        +1_842.66,
  realizedPnL:          +2_504.76,
  totalPnL:             +4_347.42,
  pnlPct:               +51.1,         // vs. net deposits
  // 24h
  pnl24h:                 +184.22,
  pnlPct24h:              +1.46,
  // 7d
  pnl7d:                  +612.40,
  pnlPct7d:               +5.01,
  // 30d
  pnl30d:                +1_204.18,
  pnlPct30d:             +10.34,
  // counts
  openPositions:           7,
  openOrders:              4,
  closedTrades:           38,
  winRate:                63.2,        // %
  avgHoldDays:            11.4,
  bestTrade:           +     842.10,   // single best closed trade
  worstTrade:          -     318.40,
};

// ── P&L equity curve over time ─────────────────────────────────────────────
// Builds a deterministic-ish curve from $8,500 deposit baseline up to current value.
function genEquityCurve(n = 142, start = 8500, end = 12847) {
  const arr = [start];
  let s = 7;
  for (let i = 1; i < n; i++) {
    s = (s * 9301 + 49297) % 233280;
    const r = s / 233280;
    // gentle drift toward end with noise
    const target = start + ((end - start) * (i / (n - 1)));
    const noise  = (r - 0.5) * 180;
    const drift  = (target - arr[i-1]) * 0.18;
    const next = Math.max(start * 0.85, arr[i-1] + drift + noise);
    arr.push(next);
  }
  arr[arr.length - 1] = end;
  return arr;
}
const EQUITY_CURVE = genEquityCurve(142, 8500, 12847.42);

// Deposit/withdraw markers (index into EQUITY_CURVE)
const DEPOSITS = [
  { i: 0,   amount: +5000, label: 'deposit' },
  { i: 38,  amount: +2500, label: 'deposit' },
  { i: 92,  amount: +1000, label: 'deposit' },
];

// ── Open positions ─────────────────────────────────────────────────────────
// Each: market, side (YES/NO), shares, avg entry (¢), current mark (¢),
//       cost ($), value ($), unrealized P&L ($), P&L %, 7d series.
function genSeries(seed, n = 32, start = 0.5, amp = 0.08) {
  let s = seed;
  const arr = [start];
  for (let i = 1; i < n; i++) {
    s = (s * 9301 + 49297) % 233280;
    const r = s / 233280;
    arr.push(Math.max(0.02, Math.min(0.98, arr[i-1] + (r - 0.5) * amp)));
  }
  return arr;
}

const OPEN_POSITIONS = [
  {
    id: 1, marketId: 1, category: 'macro',
    title:    'Will the Fed cut rates by ≥50bps before year-end?',
    side:     'YES',
    shares:   1_840,
    entry:    52,    // ¢
    mark:     64,    // ¢
    cost:     956.80,
    value:  1_177.60,
    pnl:    +220.80,
    pnlPct:   +23.1,
    series:   genSeries(11, 32, 0.52, 0.06),
    resolves: 'Dec 31',
    horizonDays: 38,
  },
  {
    id: 2, marketId: 4, category: 'tech',
    title:    'Will OpenAI release a model >90% on SWE-bench by Q3?',
    side:     'YES',
    shares:   2_400,
    entry:    58,
    mark:     71,
    cost:   1_392.00,
    value:  1_704.00,
    pnl:    +312.00,
    pnlPct:   +22.4,
    series:   genSeries(41, 32, 0.58, 0.06),
    resolves: 'Sep 30',
    horizonDays: 142,
  },
  {
    id: 3, marketId: 2, category: 'crypto',
    title:    'Will Bitcoin trade above $150,000 at any point in 2026?',
    side:     'NO',
    shares:   3_100,
    entry:    65,    // entry on NO side
    mark:     62,    // current NO mark (= 100 - YES 38)
    cost:   2_015.00,
    value:  1_922.00,
    pnl:     -93.00,
    pnlPct:    -4.6,
    series:   genSeries(73, 32, 0.62, 0.05),
    resolves: 'Dec 31',
    horizonDays: 280,
  },
  {
    id: 4, marketId: 3, category: 'politics',
    title:    'Will US presidential turnout exceed 160M?',
    side:     'YES',
    shares:   1_200,
    entry:    44,
    mark:     52,
    cost:     528.00,
    value:    624.00,
    pnl:     +96.00,
    pnlPct:   +18.2,
    series:   genSeries(31, 32, 0.48, 0.05),
    resolves: 'Nov 03',
    horizonDays: 80,
  },
  {
    id: 5, marketId: 6, category: 'macro',
    title:    'Will US Q4 GDP growth exceed 2.5%?',
    side:     'NO',
    shares:     900,
    entry:    52,
    mark:     56,    // NO mark = 100 - YES 44
    cost:     468.00,
    value:    504.00,
    pnl:     +36.00,
    pnlPct:    +7.7,
    series:   genSeries(53, 32, 0.54, 0.04),
    resolves: 'Jan 30',
    horizonDays: 168,
  },
  {
    id: 6, marketId: 7, category: 'crypto',
    title:    'Will Ethereum staking ratio exceed 35% by year-end?',
    side:     'YES',
    shares:   2_800,
    entry:    49,
    mark:     58,
    cost:   1_372.00,
    value:  1_624.00,
    pnl:    +252.00,
    pnlPct:   +18.4,
    series:   genSeries(67, 32, 0.55, 0.05),
    resolves: 'Dec 31',
    horizonDays: 38,
  },
  {
    id: 7, marketId: 5, category: 'science',
    title:    'Will an AI system author a NeurIPS-accepted paper?',
    side:     'NO',
    shares:   2_700,
    entry:    74,
    mark:     78,    // NO mark = 100 - YES 22
    cost:   1_998.00,
    value:  2_106.00,
    pnl:    +108.00,
    pnlPct:    +5.4,
    series:   genSeries(89, 32, 0.74, 0.04),
    resolves: 'Dec 12',
    horizonDays: 218,
  },
];

// ── Open orders (queued for next batch) ────────────────────────────────────
// `tif` is time-in-force: "1 batch" (clears next batch only), "N batches" (rolls
// for N batches at this limit), or "GTC" (good-till-cancelled). `filled` is
// shares already matched on prior batches — non-zero means a partial fill.
const OPEN_ORDERS = [
  {
    id: 'ord-9412-44',
    marketId: 1, category: 'macro',
    title: 'Will the Fed cut rates by ≥50bps before year-end?',
    side: 'YES', action: 'BUY',
    shares: 500, filled: 0, limit: 62, value: 310.00,
    tif: '1 batch', tifRemaining: 1,
    queuedFor: 9413, queuedAgo: '8s ago',
  },
  {
    id: 'ord-9412-45',
    marketId: 4, category: 'tech',
    title: 'Will OpenAI release a model >90% on SWE-bench by Q3?',
    side: 'YES', action: 'SELL',
    shares: 800, filled: 320, limit: 73, value: 584.00,
    tif: '5 batches', tifRemaining: 3,
    queuedFor: 9413, queuedAgo: '14s ago',
  },
  {
    id: 'ord-9412-46',
    marketId: 8, category: 'tech',
    title: 'Will Apple announce an AR headset price cut in 2026?',
    side: 'NO', action: 'BUY',
    shares: 1_200, filled: 0, limit: 68, value: 816.00,
    tif: 'GTC', tifRemaining: null,
    queuedFor: 9413, queuedAgo: '22s ago',
  },
  {
    id: 'ord-9412-47',
    marketId: 2, category: 'crypto',
    title: 'Will Bitcoin trade above $150,000 at any point in 2026?',
    side: 'NO', action: 'BUY',
    shares: 600, filled: 180, limit: 64, value: 384.00,
    tif: '5 batches', tifRemaining: 4,
    queuedFor: 9413, queuedAgo: '37s ago',
  },
];

// ── Closed positions (history) ─────────────────────────────────────────────
const CLOSED_POSITIONS = [
  { id: 101, category:'crypto',   title:'Will Solana ETF be approved before May 2026?',           side:'YES', shares:1800, entry:42, exit:71, pnl:+522.00, pnlPct:+69.0, closedAgo:'3 d ago',  outcome:'sold' },
  { id: 102, category:'macro',    title:'Will US CPI print above 3.2% in March?',                side:'NO',  shares:2400, entry:58, exit:84, pnl:+624.00, pnlPct:+44.8, closedAgo:'5 d ago',  outcome:'resolved' },
  { id: 103, category:'sports',   title:'Will any NBA team win 70+ regular season games?',       side:'NO',  shares:1100, entry:81, exit: 0, pnl:+891.00, pnlPct:+99.9, closedAgo:'8 d ago',  outcome:'resolved' },
  { id: 104, category:'tech',     title:'Will Anthropic release a model larger than 500B?',      side:'YES', shares:1500, entry:38, exit:18, pnl:-300.00, pnlPct:-52.6, closedAgo:'11 d ago', outcome:'sold' },
  { id: 105, category:'politics', title:'Will UK general election occur before July 2025?',      side:'YES', shares: 900, entry:48, exit:97, pnl:+441.00, pnlPct:+102.0,closedAgo:'18 d ago', outcome:'resolved' },
  { id: 106, category:'crypto',   title:'Will Coinbase report >$2B revenue in Q1?',              side:'YES', shares:2200, entry:54, exit:42, pnl:-264.00, pnlPct:-22.2, closedAgo:'24 d ago', outcome:'sold' },
  { id: 107, category:'science',  title:'Will SpaceX complete an orbital Starship flight in Q2?',side:'NO',  shares:1300, entry:72, exit:18, pnl:-702.00, pnlPct:-75.0, closedAgo:'29 d ago', outcome:'resolved' },
  { id: 108, category:'macro',    title:'Will the ECB hold rates through May meeting?',          side:'YES', shares:1600, entry:62, exit:91, pnl:+464.00, pnlPct:+46.8, closedAgo:'34 d ago', outcome:'resolved' },
];

// ── Recent activity (your fills + cancels) ─────────────────────────────────
const RECENT_FILLS = [
  { id: 1, kind:'fill',   batch:9412, ago:'just now', side:'YES', action:'BUY',  shares:340, price:64, market:'Will the Fed cut rates by ≥50bps before year-end?',           amount:217.60 },
  { id: 2, kind:'fill',   batch:9412, ago:'just now', side:'YES', action:'SELL', shares:120, price:71, market:'Will OpenAI release a model >90% on SWE-bench by Q3?',         amount: 85.20 },
  { id: 3, kind:'cancel', batch:9411, ago:'1m ago',   side:'NO',  action:'BUY',  shares:800, price:60, market:'Will Bitcoin trade above $150,000 at any point in 2026?',     amount:480.00 },
  { id: 4, kind:'fill',   batch:9410, ago:'2m ago',   side:'NO',  action:'BUY',  shares:600, price:62, market:'Will Bitcoin trade above $150,000 at any point in 2026?',     amount:372.00 },
  { id: 5, kind:'fill',   batch:9409, ago:'3m ago',   side:'YES', action:'BUY',  shares:200, price:58, market:'Will Ethereum staking ratio exceed 35% by year-end?',          amount:116.00 },
  { id: 6, kind:'fill',   batch:9407, ago:'5m ago',   side:'YES', action:'BUY',  shares:480, price:51, market:'Will US presidential turnout exceed 160M?',                    amount:244.80 },
  { id: 7, kind:'cancel', batch:9405, ago:'7m ago',   side:'NO',  action:'SELL', shares:300, price:79, market:'Will an AI system author a NeurIPS-accepted paper?',           amount:237.00 },
  { id: 8, kind:'fill',   batch:9402, ago:'10m ago',  side:'NO',  action:'BUY',  shares:540, price:78, market:'Will an AI system author a NeurIPS-accepted paper?',           amount:421.20 },
];

// ── Allocation by category ─────────────────────────────────────────────────
const ALLOCATION = (() => {
  const by = {};
  for (const p of OPEN_POSITIONS) {
    by[p.category] = (by[p.category] || 0) + p.value;
  }
  const total = Object.values(by).reduce((a,b) => a+b, 0);
  return Object.entries(by)
    .map(([cat, val]) => ({ cat, val, pct: val / total * 100 }))
    .sort((a, b) => b.val - a.val);
})();

const CAT_COLORS = {
  macro:    '#A988E0',
  crypto:   '#F4A058',
  politics: '#E8556C',
  tech:     '#3FB6D9',
  science:  '#5BD99A',
  sports:   '#E8B447',
};

// ── Helpers ────────────────────────────────────────────────────────────────
function fmtMoney(n, opts = {}) {
  const { sign = false, decimals = 2 } = opts;
  const abs = Math.abs(n);
  const str = abs.toLocaleString('en-US', {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
  const s = (n < 0 ? '−' : (sign ? '+' : '')) + '$' + str;
  return s;
}
function fmtPct(n, opts = {}) {
  const { sign = true, decimals = 2 } = opts;
  const s = (n >= 0 && sign ? '+' : '') + n.toFixed(decimals) + '%';
  return s;
}
function fmtShares(n) {
  return n.toLocaleString('en-US');
}

Object.assign(window, {
  TRADER, PORTFOLIO, EQUITY_CURVE, DEPOSITS,
  OPEN_POSITIONS, OPEN_ORDERS, CLOSED_POSITIONS, RECENT_FILLS,
  ALLOCATION, CAT_COLORS,
  fmtMoney, fmtPct, fmtShares,
});
