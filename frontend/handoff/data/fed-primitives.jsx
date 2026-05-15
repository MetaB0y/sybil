// Shared atoms for all Fed-market variations.
// Pure presentation; data comes from data.jsx (FED_MARKET, FED_SERIES, FED_BATCH_HISTORY, FED_COMMENTS).

const OUT_COLORS = {
  'fed-25d': '#5BD99A',
  'fed-hold': '#3FB6D9',
  'fed-50d': '#E8AA4A',
  'fed-25u': '#E8556C',
};

// ───────────────────────────────────────────────────────────────────────
// Glossary — small "?" badge with hover tooltip explaining novel terms.
// ───────────────────────────────────────────────────────────────────────
const GLOSSARY = {
  'Indicative price': 'The price the current batch would clear at if it ran right now. Updates as new orders come in. Not final until the batch closes.',
  'IEV': 'Indicative Executable Volume — how much $ would actually trade at the indicative price. High IEV = a thick batch; low = thin.',
  'Imbalance': 'Net direction of unmatched orders. Buy = more demand than supply at current price; sell = the reverse. Tells you which side is leaning.',
  'Batch': 'Sybil clears all orders in 60-second windows at one uniform price. No order has time priority within a batch — front-running becomes geometrically harder.',
  'Uniform clearing': 'Every order in a batch trades at the same single price. Eliminates the "sniper\u2019s tax" continuous order books leak to fast actors.',
  'FBA': 'Frequent Batch Auction. The market mechanism Sybil uses instead of a continuous limit order book.',
};
function Glossary({ term, children, side='top' }) {
  const [open, setOpen] = React.useState(false);
  const content = GLOSSARY[term] || '';
  return (
    <span style={{ position:'relative', display:'inline-flex', alignItems:'center', gap:4 }}
      onMouseEnter={() => setOpen(true)} onMouseLeave={() => setOpen(false)} onFocus={() => setOpen(true)} onBlur={() => setOpen(false)}>
      {children}
      <button tabIndex={0} aria-label={`What is ${term}?`} onClick={(e) => { e.preventDefault(); setOpen(o => !o); }}
        style={{
          width:13, height:13, borderRadius:'50%', border:'1px solid var(--border-3)', background:'transparent',
          color:'var(--fg-3)', fontFamily:'var(--font-mono)', fontSize:9, lineHeight:'11px', padding:0, cursor:'help',
          display:'inline-flex', alignItems:'center', justifyContent:'center',
        }}>?</button>
      {open && (
        <span style={{
          position:'absolute', zIndex:50,
          [side==='top'?'bottom':'top']: 'calc(100% + 6px)',
          left:0, width:240,
          background:'var(--surface-3)', border:'1px solid var(--border-2)', borderRadius:4, padding:'10px 12px',
          fontFamily:'var(--font-sans)', fontSize:12, lineHeight:'17px', color:'var(--fg-2)',
          boxShadow:'var(--shadow-popover)',
        }}>
          <span style={{ display:'block', fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em', marginBottom:4 }}>{term}</span>
          {content}
        </span>
      )}
    </span>
  );
}

// ───────────────────────────────────────────────────────────────────────
// Live batch clock — tick down 0:60 → 0:00. Used by all variations.
// ───────────────────────────────────────────────────────────────────────
function useBatchSecs() {
  const [s, setS] = React.useState(42);
  React.useEffect(() => {
    const id = setInterval(() => setS(v => v > 0 ? v - 1 : 60), 1000);
    return () => clearInterval(id);
  }, []);
  return s;
}

// ───────────────────────────────────────────────────────────────────────
// Sparkline & stacked area chart
// ───────────────────────────────────────────────────────────────────────
function Spark({ data, w=120, h=28, stroke='#3FB6D9', fill=true }) {
  if (!data || data.length === 0) return null;
  const max = Math.max(...data), min = Math.min(...data); const r = max-min || 1;
  const step = w/(data.length-1);
  const pts = data.map((v,i) => [i*step, (1-(v-min)/r)*h]);
  const path = pts.map((p,i) => (i===0?'M':'L')+p[0].toFixed(1)+' '+p[1].toFixed(1)).join(' ');
  return (
    <svg viewBox={`0 0 ${w} ${h}`} width={w} height={h} style={{ display:'block' }}>
      {fill && <path d={path + ` L ${w} ${h} L 0 ${h} Z`} fill={stroke} fillOpacity="0.14"/>}
      <path d={path} fill="none" stroke={stroke} strokeWidth="1.25" strokeLinejoin="round"/>
    </svg>
  );
}

// Stacked-area chart of all 4 outcome probabilities. Polymarket-style.
function StackedAreaChart({ outcomes, series, w=720, h=260, hoverEnabled=true }) {
  const N = series[0].length;
  const stepX = w / (N-1);
  // For each timestep, cumulative tops
  const stacks = [];
  for (let i=0; i<N; i++) {
    let acc = 0; const col = [];
    for (let k=0; k<series.length; k++) { col.push(acc); acc += series[k][i]; col.push(acc); }
    stacks.push(col);
  }
  const yOf = (v) => (1 - v) * h; // v in [0..1]
  const layers = outcomes.map((o, k) => {
    let path = '';
    for (let i=0; i<N; i++) path += (i===0?'M':'L') + (i*stepX).toFixed(1) + ' ' + yOf(stacks[i][k*2+1]).toFixed(1) + ' ';
    for (let i=N-1; i>=0; i--) path += 'L' + (i*stepX).toFixed(1) + ' ' + yOf(stacks[i][k*2]).toFixed(1) + ' ';
    path += 'Z';
    let line = '';
    for (let i=0; i<N; i++) line += (i===0?'M':'L') + (i*stepX).toFixed(1) + ' ' + yOf(stacks[i][k*2+1]).toFixed(1) + ' ';
    return { fill: path, line, color: OUT_COLORS[o.id] };
  });

  const [hover, setHover] = React.useState(null);
  const ref = React.useRef(null);
  const onMove = (e) => {
    if (!hoverEnabled) return;
    const r = ref.current.getBoundingClientRect();
    const x = e.clientX - r.left;
    const idx = Math.max(0, Math.min(N-1, Math.round(x / r.width * (N-1))));
    setHover(idx);
  };
  const labelAt = (idx) => {
    // batches ago label
    const ago = (N-1 - idx);
    if (ago === 0) return 'now';
    return `${ago} batch${ago===1?'':'es'} ago`;
  };

  return (
    <div ref={ref} style={{ position:'relative' }}
      onMouseMove={onMove} onMouseLeave={() => setHover(null)}>
      <svg viewBox={`0 0 ${w} ${h}`} width="100%" height={h} preserveAspectRatio="none" style={{ display:'block' }}>
        {/* y-grid */}
        {[0, 0.25, 0.5, 0.75, 1].map(y => (
          <line key={y} x1="0" x2={w} y1={yOf(y)} y2={yOf(y)} stroke="rgba(255,255,255,0.05)" strokeDasharray={y===0||y===1?'':'2 3'} />
        ))}
        {layers.map((l, k) => (
          <g key={k}>
            <path d={l.fill} fill={l.color} fillOpacity="0.34" />
            <path d={l.line} fill="none" stroke={l.color} strokeWidth="1.25" />
          </g>
        ))}
        {hover != null && (
          <g>
            <line x1={hover*stepX} x2={hover*stepX} y1="0" y2={h} stroke="rgba(255,255,255,0.4)" strokeDasharray="2 3" />
          </g>
        )}
      </svg>
      {/* y-axis labels */}
      <div style={{ position:'absolute', top:0, right:0, height:h, width:32, pointerEvents:'none', display:'flex', flexDirection:'column', justifyContent:'space-between', fontFamily:'var(--font-mono)', fontSize:9, color:'var(--fg-4)' }}>
        <span>100%</span><span>75%</span><span>50%</span><span>25%</span><span>0%</span>
      </div>
      {hover != null && hoverEnabled && (
        <div style={{
          position:'absolute', top:8, left: Math.min(w-200, Math.max(8, hover*stepX*(ref.current?.clientWidth||w)/w + 12)),
          background:'var(--surface-3)', border:'1px solid var(--border-2)', borderRadius:4, padding:'8px 10px',
          fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-2)', minWidth:170, pointerEvents:'none',
          boxShadow:'var(--shadow-popover)',
        }}>
          <div style={{ color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em', marginBottom:6, fontSize:9 }}>{labelAt(hover)}</div>
          {outcomes.map((o, k) => (
            <div key={o.id} style={{ display:'flex', justifyContent:'space-between', gap:12, lineHeight:'15px' }}>
              <span style={{ display:'flex', alignItems:'center', gap:6, color:'var(--fg-2)' }}>
                <span style={{ width:6, height:6, background:OUT_COLORS[o.id], borderRadius:1 }} />{o.label}
              </span>
              <span style={{ color:'var(--fg-1)' }}>{Math.round(series[k][hover]*100)}¢</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────
// Top global nav (matches markets-page.html — Markets/Activity/Account/Docs
// + Dev zone dropdown + search + batch pill + wallet)
// ───────────────────────────────────────────────────────────────────────
function TopNav({ secs, active='Markets' }) {
  const pct = (secs/60)*100;
  const [devOpen, setDevOpen] = React.useState(false);
  const devRef = React.useRef(null);
  React.useEffect(() => {
    const close = (e) => { if (devRef.current && !devRef.current.contains(e.target)) setDevOpen(false); };
    document.addEventListener('mousedown', close);
    return () => document.removeEventListener('mousedown', close);
  }, []);
  return (
    <div style={{
      position:'sticky', top:0, zIndex:40, height:56,
      background:'rgba(10,14,18,0.84)', backdropFilter:'var(--blur-nav)', WebkitBackdropFilter:'var(--blur-nav)',
      borderBottom:'1px solid var(--border-1)',
      display:'flex', alignItems:'center', padding:'0 24px', gap:18,
    }}>
      <div style={{ display:'flex', alignItems:'center', gap:8 }}>
        <img src="../assets/sybil-mark.png" width="22" height="22" style={{ borderRadius:3 }} alt="" />
        <span style={{ fontFamily:'var(--font-display)', fontWeight:700, fontSize:15, letterSpacing:'-0.01em', textTransform:'uppercase' }}>SYBIL</span>
      </div>
      <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--warn)', padding:'2px 7px', background:'var(--warn-soft)', borderRadius:9999, letterSpacing:'0.04em', textTransform:'uppercase' }}>testnet</span>
      <div style={{ display:'flex', gap:2, alignItems:'center' }}>
        {['Markets','Activity','Portfolio','Docs'].map((l) => (
          <span key={l} style={{
            display:'inline-flex', alignItems:'center', height:26,
            padding:'0 10px', borderRadius:3,
            color: active===l ? 'var(--fg-1)' : 'var(--fg-3)',
            background: active===l ? 'var(--surface-1)' : 'transparent',
            fontSize:12, fontWeight:500, cursor:'pointer', lineHeight:1,
          }}>{l}</span>
        ))}
        <div style={{ position:'relative', display:'inline-flex' }} ref={devRef}>
          <button onClick={() => setDevOpen(o => !o)} style={{
            background:'transparent', border:0, padding:'0 10px', height:26, borderRadius:3,
            color:'var(--fg-3)', fontFamily:'var(--font-sans)', fontSize:12, fontWeight:500, cursor:'pointer',
            display:'inline-flex', alignItems:'center', gap:6, lineHeight:1,
          }}>
            Dev zone
            <svg width="9" height="9" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5"><path d="m3 4.5 3 3 3-3"/></svg>
          </button>
          {devOpen && (
            <div style={{
              position:'absolute', top:'calc(100% + 6px)', left:0,
              background:'var(--surface-1)', border:'1px solid var(--border-2)', borderRadius:4,
              padding:4, minWidth:160, boxShadow:'0 10px 30px rgba(0,0,0,0.35)',
            }}>
              <div style={{ fontFamily:'var(--font-mono)', fontSize:9, color:'var(--fg-4)', textTransform:'uppercase', letterSpacing:'0.04em', padding:'6px 10px 4px' }}>Dev zone</div>
              {['Overview','Blocks','Trading'].map(l => (
                <div key={l} style={{ padding:'7px 10px', borderRadius:3, color:'var(--fg-2)', fontSize:12, cursor:'pointer' }}>{l}</div>
              ))}
            </div>
          )}
        </div>
      </div>
      <div style={{ marginLeft:'auto', display:'flex', alignItems:'center', gap:10 }}>
        <div style={{ display:'flex', alignItems:'center', gap:8, padding:'4px 10px', background:'var(--accent-soft)', borderRadius:4, fontFamily:'var(--font-mono)', fontSize:11, color:'var(--accent)' }}>
          <span style={{ width:6, height:6, borderRadius:'50%', background:'var(--accent)' }} />
          <span style={{ letterSpacing:'0.04em', textTransform:'uppercase', fontSize:10, color:'rgba(63,182,217,0.7)' }}>batch</span>
          <span>0:{secs.toString().padStart(2,'0')}</span>
          <div style={{ width:48, height:2, background:'rgba(63,182,217,0.16)', borderRadius:1, overflow:'hidden' }}>
            <div style={{ width:`${pct}%`, height:'100%', background:'var(--accent)', transition:'width 1000ms linear' }} />
          </div>
        </div>
        <div style={{ display:'flex', alignItems:'center', gap:8, background:'var(--surface-1)', border:'1px solid var(--border-2)', borderRadius:3, padding:'4px 10px', width:240 }}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" style={{ color:'var(--fg-3)' }}><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>
          <span style={{ flex:1, color:'var(--fg-4)', fontSize:12 }}>Search events, markets</span>
          <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)' }}>⌘K</span>
        </div>
        <button style={{ background:'transparent', border:'1px solid var(--border-2)', borderRadius:3, padding:'5px 10px', color:'var(--fg-1)', fontFamily:'var(--font-sans)', fontSize:12, fontWeight:500, cursor:'pointer' }}>0xA17F···c92E</button>
      </div>
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────
// Breadcrumb / title block
// ───────────────────────────────────────────────────────────────────────
function MarketHeader({ market, rightSlot }) {
  return (
    <div style={{ padding:'20px 24px 16px', display:'flex', alignItems:'flex-start', gap:16, borderBottom:'1px solid var(--border-1)' }}>
      <div style={{
        width:56, height:56, borderRadius:6, flexShrink:0,
        display:'flex', alignItems:'center', justifyContent:'center',
        background:'linear-gradient(135deg, rgba(232,170,74,0.20), rgba(232,170,74,0.06))',
        border:'1px solid rgba(232,170,74,0.30)', color:'#E8AA4A', fontFamily:'var(--font-display)', fontSize:28, lineHeight:1,
      }}>%</div>
      <div style={{ flex:1, minWidth:0 }}>
        <div style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', letterSpacing:'0.04em', textTransform:'uppercase', marginBottom:6, display:'flex', gap:10 }}>
          <span style={{ color:'var(--fg-4)' }}>Markets</span><span style={{ color:'var(--fg-4)' }}>/</span>
          <span><span style={{ width:6, height:6, borderRadius:'50%', background:'#E8AA4A', display:'inline-block', marginRight:6, verticalAlign:'1px' }} />Economy</span>
          <span style={{ color:'var(--fg-4)' }}>/</span>
          <span style={{ color:'var(--fg-3)' }}>resolves {market.resolves}</span>
        </div>
        <h2 style={{ fontFamily:'var(--font-sans)', fontWeight:600, fontSize:22, lineHeight:'28px', letterSpacing:'-0.01em', color:'var(--fg-1)', margin:0 }}>
          {market.title}
        </h2>
        <div style={{ marginTop:10, display:'flex', gap:18, fontFamily:'var(--font-mono)', fontSize:11, color:'var(--fg-3)' }}>
          <span><span style={{ color:'var(--fg-4)' }}>vol </span><span style={{ color:'var(--fg-2)' }}>${market.vol}</span></span>
          <span><span style={{ color:'var(--fg-4)' }}>24h </span><span style={{ color:'var(--fg-2)' }}>${market.vol24}</span></span>
          <span><span style={{ color:'var(--fg-4)' }}>traders </span><span style={{ color:'var(--fg-2)' }}>{market.traders.toLocaleString()}</span></span>
          <span><span style={{ color:'var(--fg-4)' }}>liq </span><span style={{ color:'var(--fg-2)' }}>${market.liq}</span></span>
          <span><span style={{ color:'var(--fg-4)' }}>batches cleared </span><span style={{ color:'var(--fg-2)' }}>{market.batches.toLocaleString()}</span></span>
        </div>
      </div>
      {rightSlot}
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────
// Outcome legend (used above stacked chart)
// ───────────────────────────────────────────────────────────────────────
function OutcomeLegend({ outcomes }) {
  return (
    <div style={{ display:'flex', gap:18, flexWrap:'wrap', alignItems:'center' }}>
      {outcomes.map(o => (
        <span key={o.id} style={{ display:'flex', alignItems:'center', gap:8, fontFamily:'var(--font-sans)', fontSize:12, color:'var(--fg-2)' }}>
          <span style={{ width:8, height:8, background:OUT_COLORS[o.id], borderRadius:1 }} />
          <span>{o.label}</span>
          <span style={{ fontFamily:'var(--font-mono)', color: o.delta24>=0?'var(--yes)':'var(--no)' }}>{o.yes}¢</span>
          <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)' }}>{o.delta24>=0?'+':''}{o.delta24}%</span>
        </span>
      ))}
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────
// Time range selector
// ───────────────────────────────────────────────────────────────────────
function RangeBar({ value, onChange, ranges=['1H','6H','1D','1W','1M','ALL'] }) {
  return (
    <div style={{ display:'flex', gap:2, padding:2, background:'var(--bg-2)', border:'1px solid var(--border-1)', borderRadius:4 }}>
      {ranges.map(r => {
        const a = value === r;
        return (
          <button key={r} onClick={() => onChange && onChange(r)} style={{
            padding:'4px 9px', borderRadius:3, border:0, cursor:'pointer',
            background: a ? 'var(--surface-2)' : 'transparent',
            color: a ? 'var(--fg-1)' : 'var(--fg-3)',
            fontFamily:'var(--font-mono)', fontSize:10, letterSpacing:'0.04em',
          }}>{r}</button>
        );
      })}
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────
// Comments thread — full visible, simple
// ───────────────────────────────────────────────────────────────────────
function CommentsThread({ comments }) {
  return (
    <div>
      <div style={{ display:'flex', alignItems:'baseline', gap:12, marginBottom:14 }}>
        <h3 style={{ fontFamily:'var(--font-sans)', fontSize:16, fontWeight:600, color:'var(--fg-1)', margin:0 }}>Discussion</h3>
        <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)' }}>{comments.length} comments</span>
      </div>
      <div style={{ display:'flex', gap:8, marginBottom:18 }}>
        <div style={{ width:32, height:32, borderRadius:16, background:'#3FB6D9', flexShrink:0 }} />
        <textarea placeholder="Add to the thread. No shilling, no spam." style={{
          flex:1, minHeight:60, resize:'vertical',
          background:'var(--bg-2)', border:'1px solid var(--border-1)', borderRadius:4,
          color:'var(--fg-1)', fontFamily:'var(--font-sans)', fontSize:13, padding:'10px 12px',
        }}/>
      </div>
      <div style={{ display:'flex', flexDirection:'column' }}>
        {comments.map((c, i) => (
          <div key={i} style={{ display:'flex', gap:10, padding:'14px 0', borderTop:i?'1px solid var(--border-1)':'0' }}>
            <div style={{ width:32, height:32, borderRadius:16, background: c.avatar, flexShrink:0, marginTop:2 }} />
            <div style={{ flex:1, minWidth:0 }}>
              <div style={{ display:'flex', gap:10, alignItems:'baseline', marginBottom:4 }}>
                <span style={{ fontFamily:'var(--font-sans)', fontSize:13, fontWeight:600, color:'var(--fg-1)' }}>{c.user}</span>
                <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-4)' }}>{c.time}</span>
                {c.yes && (
                  <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', padding:'2px 7px', background:'var(--bg-2)', borderRadius:9999, border:'1px solid var(--border-1)' }}>
                    holds <span style={{ color:'var(--fg-1)' }}>{c.yes}</span> · {c.stake}
                  </span>
                )}
              </div>
              <div style={{ fontFamily:'var(--font-sans)', fontSize:13, lineHeight:'19px', color:'var(--fg-2)' }}>{c.body}</div>
              <div style={{ marginTop:8, display:'flex', gap:14, fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)' }}>
                <span style={{ cursor:'pointer' }}>↑ 24</span>
                <span style={{ cursor:'pointer' }}>↓ 1</span>
                <span style={{ cursor:'pointer' }}>reply</span>
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────
// Rules card
// ───────────────────────────────────────────────────────────────────────
function RulesCard({ market, embedded=false }) {
  return (
    <div style={{
      background: embedded ? 'transparent' : 'var(--surface-1)',
      border: embedded ? '0' : '1px solid var(--border-1)',
      borderRadius: 8, padding: embedded ? 0 : '16px 18px',
    }}>
      <div style={{ display:'flex', justifyContent:'space-between', alignItems:'baseline', marginBottom:10 }}>
        <h3 style={{ fontFamily:'var(--font-sans)', fontSize:14, fontWeight:600, color:'var(--fg-1)', margin:0 }}>Market rules</h3>
        <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em' }}>oracle · standard</span>
      </div>
      <ol style={{ margin:0, padding:0, listStyle:'none', display:'flex', flexDirection:'column', gap:8, fontFamily:'var(--font-sans)', fontSize:12, lineHeight:'18px', color:'var(--fg-2)' }}>
        {market.rules.map((r, i) => (
          <li key={i} style={{ display:'flex', gap:8 }}>
            <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-4)', minWidth:14 }}>{(i+1).toString().padStart(2,'0')}</span>
            <span>{r}</span>
          </li>
        ))}
      </ol>
    </div>
  );
}

Object.assign(window, {
  OUT_COLORS, GLOSSARY, Glossary, useBatchSecs, Spark, StackedAreaChart,
  TopNav, MarketHeader, OutcomeLegend, RangeBar, CommentsThread, RulesCard,
});
