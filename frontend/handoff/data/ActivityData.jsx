// Mock data for the Sybil Activity page.

// ── Top-level stats ────────────────────────────────────────────────────────
const ACTIVITY_24H = {
  matchedVolume:    '$4.82M',
  matchedVolumeDelta: +12.4,
  traders:          3_214,
  tradersDelta:     +6.1,
  ordersPlaced:     58_912,
  ordersMatched:    47_209,
  ordersUnmatched:  11_703,
  // sparklines
  volumeSeries:     genSeries(11, 48, 0.5,  0.22),
  tradersSeries:    genSeries(19, 48, 0.55, 0.18),
  ordersSeries:     genSeries(7,  48, 0.55, 0.18),
};

const ACTIVITY_ALLTIME = {
  matchedVolume:    '$487.2M',
  traders:          18_402,
  ordersPlaced:     2_104_877,
  ordersMatched:    1_682_711,
  ordersUnmatched:    422_166,
  totalBatches:     9_412,
  liveMarkets:      142,
  uptime:           '99.97%',
  genesisAge:       '6 mo 17 d',
  weeklyVolume:     genSeries(3,  32, 0.45, 0.30),
  weeklyTraders:    genSeries(13, 32, 0.40, 0.28),
  weeklyOrders:     genSeries(23, 32, 0.50, 0.25),
};

// ── Batches: header rows + on-demand detail ────────────────────────────────
function genBatches(n = 60) {
  const out = [];
  let baseTime = Date.now();
  for (let i = 0; i < n; i++) {
    const id = 9412 - i;
    const t  = new Date(baseTime - i * 60_000);
    const seed = id * 13 + 7;
    const r = (k) => {
      const x = Math.sin(seed * 99 + k * 17) * 10000;
      return x - Math.floor(x);
    };
    const markets        = 18 + Math.floor(r(1) * 14);
    const matchedVolume  = 1500 + r(3) * 6500;        // $K
    const traders        = 80 + Math.floor(r(4) * 240);
    const ordersPlaced   = 220 + Math.floor(r(5) * 480);
    const matchRate      = 0.62 + r(6) * 0.34;
    const ordersMatched  = Math.floor(ordersPlaced * matchRate);
    const ordersUnmatched= ordersPlaced - ordersMatched;
    out.push({
      id, ts: t,
      markets,
      matchedVolume,
      traders,
      ordersPlaced, ordersMatched, ordersUnmatched,
      detailSeed: seed,
    });
  }
  return out;
}
const BATCHES = genBatches(60);

function detailFor(batch) {
  const r = (k) => {
    const x = Math.sin(batch.detailSeed * 41 + k * 23) * 10000;
    return x - Math.floor(x);
  };
  // Build up to N market rows; we'll display 6 by default and let user expand.
  const N = batch.markets;
  const allRows = [];
  for (let i = 0; i < N; i++) {
    const m = MARKETS_V2[i % MARKETS_V2.length];
    const clearPrice = Math.max(1, Math.min(99, Math.round(m.yes + (r(i+1) - 0.5) * 8)));
    const buys = 12 + Math.floor(r(i*2+1) * 60);
    const sells = 8 + Math.floor(r(i*2+2) * 50);
    const placed = buys + sells;
    const matched = Math.min(placed, Math.floor(placed * (0.65 + r(i*3+5) * 0.3)));
    const matchedVol = (matched * (clearPrice/100) * (3 + r(i*3) * 12)).toFixed(1);
    allRows.push({
      id: m.id + '-' + i,
      title: m.title,
      category: m.category,
      clearPrice,
      delta: Math.round((r(i*5) - 0.5) * 40)/10,
      placed, matched,
      matchedVol,
      buys, sells,
    });
  }
  const hex = (n) => Math.floor(r(n) * 16**8).toString(16).padStart(8,'0');
  return {
    txHash: '0x' + hex(99) + hex(98) + '···' + hex(97),
    clearingMs: 180 + Math.round(r(50) * 240),
    sequencer:  '0x4f2c···7a91',
    blockNum:   18_902_412 + (9412 - batch.id),
    marketRows: allRows,
  };
}

// ── Helpers ─────────────────────────────────────────────────────────────────
function genSeries(seed, n=48, mid=0.5, amp=0.18) {
  let s = seed;
  const arr = [mid];
  for (let i = 1; i < n; i++) {
    s = (s * 9301 + 49297) % 233280;
    const r = s / 233280;
    arr.push(Math.max(0.05, Math.min(0.95, arr[i-1] + (r - 0.5) * amp)));
  }
  return arr;
}
function fmtMoneyK(k) {
  if (k >= 1000) return '$' + (k/1000).toFixed(2).replace(/\.?0+$/,'') + 'M';
  return '$' + k.toFixed(1).replace(/\.0$/,'') + 'K';
}
function fmtNum(n) {
  if (n >= 1_000_000) return (n/1_000_000).toFixed(2).replace(/\.?0+$/,'') + 'M';
  if (n >= 1000) return (n/1000).toFixed(1).replace(/\.0$/,'') + 'K';
  return '' + n;
}
function fmtRelTime(d) {
  const s = Math.max(0, Math.floor((Date.now() - d.getTime())/1000));
  if (s < 60) return s + 's ago';
  if (s < 3600) return Math.floor(s/60) + 'm ago';
  if (s < 86400) return Math.floor(s/3600) + 'h ago';
  return Math.floor(s/86400) + 'd ago';
}
function fmtClock(d) { return d.toTimeString().slice(0,8); }

Object.assign(window, {
  ACTIVITY_24H, ACTIVITY_ALLTIME, BATCHES,
  detailFor,
  fmtMoneyK, fmtNum, fmtRelTime, fmtClock,
});
