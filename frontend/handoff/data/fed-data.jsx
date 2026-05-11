// Fed-rate market data + reusable bits for the market detail page.

const FED_MARKET = {
  id: 'fed-mar',
  category: 'Economy',
  title: 'Fed rate decision · March FOMC',
  question: 'What will the FOMC announce at its March 19, 2026 meeting?',
  resolves: 'Mar 19, 2026',
  endsIn: '14d 03h 22m',
  vol: '2.4M', vol24: '312K', traders: 1842, batches: 9412, liq: '802K',
  outcomes: [
    { id:'fed-25d', label:'25bp cut',  yes:54, delta24:+3.2, vol24:'128K', vol:'984K', traders:712, liq:'410K', color:'#5BD99A' },
    { id:'fed-hold',label:'Hold',      yes:31, delta24:-2.1, vol24: '92K', vol:'682K', traders:512, liq:'280K', color:'#3FB6D9' },
    { id:'fed-50d', label:'50bp cut',  yes:11, delta24:-0.8, vol24: '48K', vol:'412K', traders:380, liq: '92K', color:'#E8AA4A' },
    { id:'fed-25u', label:'25bp hike', yes: 4, delta24:-0.3, vol24: '12K', vol: '98K', traders:148, liq: '21K', color:'#E8556C' },
  ],
  rules: [
    'Resolves YES for the option that matches the FOMC\'s announced target rate change at the March 19, 2026 meeting.',
    'Source of truth: the official FOMC statement published on federalreserve.gov.',
    'If the meeting is delayed past Mar 31, the market resolves to "Hold".',
    'No early resolution. Disputes follow the standard Sybil oracle process.',
  ],
};

// Stacked-area series: 4 outcomes, 60 points, summing to 1 each step.
function genStackedSeries(seeds, n=60) {
  const series = seeds.map(s => {
    let v = s.start; const a = [v]; let r = s.seed;
    for (let i=1; i<n; i++) {
      r = (r*9301+49297)%233280;
      v = Math.max(0.02, Math.min(0.95, v + ((r/233280)-0.5)*0.04));
      a.push(v);
    }
    return a;
  });
  // normalize each timestep to 1
  const out = seeds.map(()=>[]);
  for (let i=0; i<n; i++) {
    const s = series.reduce((acc, arr) => acc + arr[i], 0);
    series.forEach((arr, k) => out[k].push(arr[i]/s));
  }
  return out;
}
const FED_SERIES = genStackedSeries([
  { start: 0.46, seed: 71 },  // 25bp cut
  { start: 0.36, seed: 134 }, // hold
  { start: 0.14, seed: 203 }, // 50bp cut
  { start: 0.04, seed: 412 }, // 25bp hike
]);

// Recent batch ticks (last 100 batches), used for the FBA sparkline & stats
const FED_BATCH_HISTORY = (() => {
  let r = 12345;
  const arr = [];
  for (let i=0; i<100; i++) {
    r = (r*9301+49297)%233280;
    arr.push({
      i: 9412 - i,
      placed: 30 + Math.floor((r/233280)*50),
      matched: 18 + Math.floor((r/233280)*30),
      volPlaced: (8 + (r/233280)*22).toFixed(1) + 'K',
      volMatched: (5 + (r/233280)*15).toFixed(1) + 'K',
      cleared: 0.50 + ((r/233280)-0.5)*0.06,
      imbalance: (r/233280) > 0.5 ? 'YES' : 'NO',
    });
  }
  return arr;
})();

// Comments — for the discussion section
const FED_COMMENTS = [
  { user:'@vol_curve',    avatar:'#5BD99A', time:'2h',  body:'Dot plot shifted hawkish but PCE is cooperating. 25bp cut still the modal outcome but I\'m fading 54¢.', yes:'25bp cut', stake:'$2.4K' },
  { user:'@macroharper',  avatar:'#3FB6D9', time:'4h',  body:'Hold at 31¢ feels rich given the Q4 GDP print. Adding more on dips below 28.', yes:'Hold', stake:'$1.1K' },
  { user:'@kevx',         avatar:'#E8AA4A', time:'6h',  body:'Anyone else notice the indicative price spent the last hour bouncing between 53.8 and 54.2? Feels like a few large orders are anchoring it.', yes:null, stake:null },
  { user:'@oracle_pilled',avatar:'#B68FD9', time:'9h',  body:'Reminder: this resolves on the official statement, not the dot plot. If they cut 25 *and* signal pause, this is still 25bp cut.', yes:null, stake:null },
  { user:'@sniper_tax',   avatar:'#E8556C', time:'1d',  body:'FBA matters here — last meeting cycle the 5 batches before the announcement saw 3x normal volume. Plan accordingly.', yes:null, stake:null },
  { user:'@delta_one',    avatar:'#7DA3F0', time:'1d',  body:'Selling 50bp cut at 11¢. The macro narrative isn\'t there.', yes:'50bp cut', stake:'$840' },
];

Object.assign(window, { FED_MARKET, FED_SERIES, FED_BATCH_HISTORY, FED_COMMENTS });
