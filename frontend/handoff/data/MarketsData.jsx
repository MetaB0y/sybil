// Shared mock data + tiny components for the all-markets explorations.
// Each variation imports these and renders its own page composition.

const MARKETS_V2 = [
  { id:1, category:'macro',    title:'Will the Fed cut rates by ≥50bps before year-end?',           vol:'1.2M',  vol24:'184K', resolves:'Dec 31',  yes:64, delta24:+2.4, traders:1284, batches:9412, bid:63, ask:65, spread:2, liq:'420K' },
  { id:2, category:'crypto',   title:'Will Bitcoin trade above $150,000 at any point in 2026?',     vol:'847K',  vol24:'92K',  resolves:'Dec 31',  yes:38, delta24:-1.1, traders: 612, batches:5183, bid:37, ask:39, spread:2, liq:'310K' },
  { id:3, category:'politics', title:'Will US presidential turnout exceed 160M?',                   vol:'412K',  vol24:'48K',  resolves:'Nov 03',  yes:52, delta24:+0.3, traders: 401, batches:2914, bid:51, ask:53, spread:2, liq:'180K' },
  { id:4, category:'tech',     title:'Will OpenAI release a model >90% on SWE-bench by Q3?',        vol:'318K',  vol24:'71K',  resolves:'Sep 30',  yes:71, delta24:+4.8, traders: 287, batches:1801, bid:70, ask:72, spread:2, liq:'140K' },
  { id:5, category:'science',  title:'Will an AI system author a NeurIPS-accepted paper?',          vol:'124K',  vol24:'12K',  resolves:'Dec 12',  yes:22, delta24:-0.6, traders: 142, batches: 612, bid:21, ask:23, spread:2, liq: '42K' },
  { id:6, category:'macro',    title:'Will US Q4 GDP growth exceed 2.5%?',                          vol:'108K',  vol24:'18K',  resolves:'Jan 30',  yes:44, delta24:+1.2, traders: 121, batches: 514, bid:43, ask:45, spread:2, liq: '38K' },
  { id:7, category:'crypto',   title:'Will Ethereum staking ratio exceed 35% by year-end?',         vol: '94K',  vol24:'8K',   resolves:'Dec 31',  yes:58, delta24:+0.4, traders:  98, batches: 401, bid:57, ask:59, spread:2, liq: '29K' },
  { id:8, category:'tech',     title:'Will Apple announce an AR headset price cut in 2026?',        vol: '72K',  vol24:'4K',   resolves:'Dec 31',  yes:31, delta24:-2.1, traders:  83, batches: 312, bid:30, ask:33, spread:3, liq: '21K' },
];

// Fake price series, one per market, deterministic-ish
function genSeries(seed, n=48) {
  let s = seed;
  const arr = [0.5];
  for (let i = 1; i < n; i++) {
    s = (s * 9301 + 49297) % 233280;
    const r = s / 233280;
    arr.push(Math.max(0.05, Math.min(0.95, arr[i-1] + (r - 0.5) * 0.06)));
  }
  return arr;
}
const SERIES_BY_ID = Object.fromEntries(MARKETS_V2.map(m => [m.id, genSeries(m.id * 1234, 48)]));

// Sparkline — small inline chart, baseline midline
function Sparkline({ data, w=120, h=28, stroke='#3FB6D9', fill=true }) {
  if (!data || !data.length) return null;
  const max = Math.max(...data), min = Math.min(...data);
  const range = max - min || 1;
  const step = w / (data.length - 1);
  const pts = data.map((v,i) => [i*step, (1 - (v-min)/range) * h]);
  const path = pts.map((p,i) => (i===0?'M':'L') + p[0].toFixed(1) + ' ' + p[1].toFixed(1)).join(' ');
  const area = path + ` L ${w} ${h} L 0 ${h} Z`;
  return (
    <svg viewBox={`0 0 ${w} ${h}`} width={w} height={h} style={{ display:'block', overflow:'visible' }}>
      {fill && <path d={area} fill={stroke} fillOpacity="0.14"/>}
      <path d={path} fill="none" stroke={stroke} strokeWidth="1.25" strokeLinejoin="round"/>
    </svg>
  );
}

// Mini order-book — tiny stacked bid/ask viz
function MiniBook({ bid, ask, depth=[8,5,3], maxW=60 }) {
  const max = Math.max(...depth);
  return (
    <div style={{ display:'flex', flexDirection:'column', gap:2, fontFamily:'var(--font-mono)', fontSize:10, lineHeight:'12px', fontVariantNumeric:'tabular-nums' }}>
      {depth.slice().reverse().map((d,i) => (
        <div key={'a'+i} style={{ display:'flex', alignItems:'center', justifyContent:'flex-end', gap:6, color:'var(--no)' }}>
          <span style={{ color:'rgba(245,245,242,0.52)' }}>{d}K</span>
          <span>{ask + i + 1}¢</span>
          <div style={{ width: (d/max)*maxW, height:6, background:'rgba(232,85,108,0.32)' }} />
        </div>
      ))}
      <div style={{ height:1, background:'rgba(255,255,255,0.06)', margin:'2px 0' }} />
      {depth.map((d,i) => (
        <div key={'b'+i} style={{ display:'flex', alignItems:'center', justifyContent:'flex-end', gap:6, color:'var(--yes)' }}>
          <span style={{ color:'rgba(245,245,242,0.52)' }}>{d}K</span>
          <span>{bid - i}¢</span>
          <div style={{ width: (d/max)*maxW, height:6, background:'rgba(91,217,154,0.32)' }} />
        </div>
      ))}
    </div>
  );
}

// Global batch clock pill — used in nav and on cards
function BatchPill({ secs, total=60, dense=false }) {
  const pct = (secs/total)*100;
  return (
    <div style={{
      display:'flex', alignItems:'center', gap:8,
      padding: dense ? '3px 8px' : '5px 10px',
      background:'var(--accent-soft)', borderRadius:4,
      fontFamily:'var(--font-mono)', fontSize: dense ? 11 : 12, color:'var(--accent)',
      fontVariantNumeric:'tabular-nums',
    }}>
      <span style={{ width:6, height:6, borderRadius:'50%', background:'var(--accent)' }} />
      <span style={{ letterSpacing:'0.04em', textTransform:'uppercase', fontSize: dense ? 9 : 10, color:'rgba(63,182,217,0.7)' }}>batch</span>
      <span>0:{secs.toString().padStart(2,'0')}</span>
      <div style={{ width:48, height:2, background:'rgba(63,182,217,0.16)', borderRadius:1, overflow:'hidden' }}>
        <div style={{ width:`${pct}%`, height:'100%', background:'var(--accent)', transition:'width 1000ms linear' }} />
      </div>
    </div>
  );
}

// Top-strip ticker for recent clearing prices
function ClearingTicker({ items, secs }) {
  return (
    <div style={{
      display:'flex', alignItems:'center', gap:0, height:36,
      background:'var(--bg-1)', borderTop:'1px solid var(--border-1)', borderBottom:'1px solid var(--border-1)',
      overflow:'hidden', fontFamily:'var(--font-mono)', fontSize:12,
    }}>
      <div style={{
        padding:'0 14px', height:'100%', display:'flex', alignItems:'center', gap:8,
        background:'var(--accent-soft)', color:'var(--accent)', borderRight:'1px solid var(--border-1)', flexShrink:0,
        textTransform:'uppercase', letterSpacing:'0.04em', fontSize:11,
      }}>
        <span style={{ width:6, height:6, borderRadius:'50%', background:'var(--accent)' }} />
        Last batch · #{9412 - Math.floor(secs/10)}
      </div>
      <div style={{ display:'flex', gap:0, overflow:'hidden', flex:1 }}>
        {items.map((m,i) => (
          <div key={i} style={{ padding:'0 14px', height:36, display:'flex', alignItems:'center', gap:8, borderRight:'1px solid var(--border-1)', flexShrink:0 }}>
            <span style={{ color:'var(--fg-3)', maxWidth:200, overflow:'hidden', textOverflow:'ellipsis', whiteSpace:'nowrap' }}>{m.title}</span>
            <span style={{ color: m.delta24 >= 0 ? 'var(--yes)' : 'var(--no)', fontVariantNumeric:'tabular-nums' }}>{m.yes}¢</span>
            <span style={{ color: m.delta24 >= 0 ? 'var(--yes)' : 'var(--no)', fontSize:11 }}>{m.delta24 >= 0 ? '+' : ''}{m.delta24}%</span>
          </div>
        ))}
      </div>
    </div>
  );
}

Object.assign(window, { MARKETS_V2, SERIES_BY_ID, Sparkline, MiniBook, BatchPill, ClearingTicker });
