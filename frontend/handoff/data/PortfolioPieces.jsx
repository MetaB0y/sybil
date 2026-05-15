// Shared portfolio pieces — chrome, charts, hero stat blocks.
// Loaded as window globals.

function useBatchClock() {
  const [secs, setSecs] = React.useState(42);
  React.useEffect(() => {
    const id = setInterval(() => setSecs(s => s > 0 ? s - 1 : 60), 1000);
    return () => clearInterval(id);
  }, []);
  return secs;
}

function GlobalNav({ activeTop='Portfolio' }) {
  const secs = useBatchClock();
  const pct = (secs/60)*100;
  return (
    <div style={{
      position:'sticky', top:0, zIndex:50, height:52,
      background:'rgba(10,14,18,0.84)', backdropFilter:'var(--blur-nav)', WebkitBackdropFilter:'var(--blur-nav)',
      borderBottom:'1px solid var(--border-1)',
      display:'flex', alignItems:'center', padding:'0 22px', gap:18,
    }}>
      <div style={{ display:'flex', alignItems:'center', gap:8 }}>
        <img src="../assets/sybil-mark.png" width="20" height="20" style={{ borderRadius:3 }} alt="" />
        <span style={{ fontFamily:'var(--font-display)', fontWeight:700, fontSize:14, letterSpacing:'-0.01em', textTransform:'uppercase' }}>SYBIL</span>
      </div>
      <span style={{ fontFamily:'var(--font-mono)', fontSize:9, color:'var(--warn)', padding:'2px 7px', background:'var(--warn-soft)', borderRadius:9999, letterSpacing:'0.04em', textTransform:'uppercase' }}>testnet</span>
      <div style={{ display:'flex', gap:2 }}>
        {['Markets','Activity','Portfolio','Docs'].map(l => (
          <button key={l} style={{
            background: l===activeTop?'var(--surface-1)':'transparent', border:0, padding:'5px 10px', borderRadius:3,
            color: l===activeTop ? 'var(--fg-1)' : 'var(--fg-3)',
            fontFamily:'var(--font-sans)', fontSize:12, fontWeight:500, cursor:'pointer',
          }}>{l}</button>
        ))}
      </div>
      <div style={{ marginLeft:'auto', display:'flex', alignItems:'center', gap:10 }}>
        <div style={{ display:'flex', alignItems:'center', gap:8, padding:'4px 10px', background:'var(--accent-soft)', borderRadius:3, fontFamily:'var(--font-mono)', fontSize:11, color:'var(--accent)', fontVariantNumeric:'tabular-nums' }}>
          <span style={{ width:5, height:5, borderRadius:'50%', background:'var(--accent)' }} />
          <span style={{ letterSpacing:'0.04em', textTransform:'uppercase', fontSize:9, color:'rgba(63,182,217,0.7)' }}>batch</span>
          <span>0:{secs.toString().padStart(2,'0')}</span>
          <div style={{ width:36, height:2, background:'rgba(63,182,217,0.16)', borderRadius:1, overflow:'hidden' }}>
            <div style={{ width:`${pct}%`, height:'100%', background:'var(--accent)', transition:'width 1000ms linear' }} />
          </div>
        </div>
        <div style={{ display:'flex', alignItems:'center', gap:8, background:'var(--surface-1)', border:'1px solid var(--border-2)', borderRadius:3, padding:'4px 10px', width:200 }}>
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" style={{ color:'var(--fg-3)' }}><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>
          <span style={{ flex:1, color:'var(--fg-4)', fontSize:11 }}>Search markets</span>
          <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)' }}>⌘K</span>
        </div>
        <button style={{ background:'transparent', border:'1px solid var(--border-2)', borderRadius:3, padding:'4px 10px', color:'var(--fg-1)', fontFamily:'var(--font-mono)', fontSize:11, fontWeight:500, cursor:'pointer' }}>{TRADER.short}</button>
      </div>
    </div>
  );
}

// Mini sparkline
function MiniSpark({ data, w=100, h=28, force }) {
  const max = Math.max(...data), min = Math.min(...data);
  const r = max - min || 1;
  const step = w / (data.length - 1);
  const pts = data.map((v,i) => [i*step, (1-(v-min)/r)*h]);
  const path = pts.map((p,i) => (i===0?'M':'L')+p[0].toFixed(1)+' '+p[1].toFixed(1)).join(' ');
  const isUp = force ? force === 'up' : data[data.length-1] >= data[0];
  const color = isUp ? 'var(--yes)' : 'var(--no)';
  return (
    <svg viewBox={`0 0 ${w} ${h}`} width={w} height={h} style={{ display:'block', overflow:'visible' }}>
      <path d={path + ` L ${w} ${h} L 0 ${h} Z`} fill={color} fillOpacity="0.10"/>
      <path d={path} fill="none" stroke={color} strokeWidth="1.25" strokeLinejoin="round"/>
    </svg>
  );
}

// Equity curve area chart
function EquityChart({ data, w=720, h=160, range='all', deposits=DEPOSITS, showAxes=true, showDeposits=true }) {
  const map = { '24h': 24, '7d': 30, '30d': 60, 'all': data.length };
  const n = map[range] || data.length;
  const slice = data.slice(-n);
  const max = Math.max(...slice), min = Math.min(...slice);
  const pad = (max - min) * 0.12 || 1;
  const yMax = max + pad, yMin = min - pad;
  const r = yMax - yMin || 1;
  const step = w / (slice.length - 1);
  const pts = slice.map((v,i) => [i*step, (1-(v-yMin)/r)*h]);
  const path = pts.map((p,i) => (i===0?'M':'L')+p[0].toFixed(1)+' '+p[1].toFixed(1)).join(' ');
  const baseline = 8500;
  const baseY = (1 - (baseline - yMin) / r) * h;
  const last = pts[pts.length - 1];
  const ticks = [yMax, (yMax+yMin)/2, yMin].map(v => ({ v, y: (1-(v-yMin)/r)*h }));
  const startIdx = data.length - n;
  return (
    <svg viewBox={`0 0 ${w} ${h+18}`} width="100%" preserveAspectRatio="none" style={{ display:'block', overflow:'visible' }}>
      {showAxes && ticks.map((t,i) => (
        <line key={i} x1="0" x2={w} y1={t.y} y2={t.y} stroke="rgba(255,255,255,0.04)" strokeDasharray="2 4" />
      ))}
      {baseline >= yMin && baseline <= yMax && (
        <line x1="0" x2={w} y1={baseY} y2={baseY} stroke="var(--fg-4)" strokeDasharray="3 3" strokeWidth="1" opacity="0.6" />
      )}
      <defs>
        <linearGradient id={`eq-${w}-${h}`} x1="0" x2="0" y1="0" y2="1">
          <stop offset="0%" stopColor="var(--accent)" stopOpacity="0.26"/>
          <stop offset="100%" stopColor="var(--accent)" stopOpacity="0"/>
        </linearGradient>
      </defs>
      <path d={path + ` L ${w} ${h} L 0 ${h} Z`} fill={`url(#eq-${w}-${h})`} />
      <path d={path} fill="none" stroke="var(--accent)" strokeWidth="1.5" strokeLinejoin="round" />
      {showDeposits && deposits.filter(d => d.i >= startIdx).map(d => {
        const lx = (d.i - startIdx) * step;
        return (
          <g key={d.i}>
            <line x1={lx} x2={lx} y1="0" y2={h} stroke="var(--fg-4)" strokeDasharray="2 3" strokeWidth="0.75" opacity="0.45"/>
            <circle cx={lx} cy={h - 4} r="2.5" fill="var(--bg-1)" stroke="var(--fg-3)" strokeWidth="1"/>
          </g>
        );
      })}
      {last && (
        <g>
          <circle cx={last[0]} cy={last[1]} r="3.5" fill="var(--bg-1)" stroke="var(--accent)" strokeWidth="1.5"/>
          <circle cx={last[0]} cy={last[1]} r="8" fill="var(--accent)" opacity="0.18"/>
        </g>
      )}
      {showAxes && ticks.map((t,i) => (
        <text key={i} x={w - 4} y={t.y - 3} textAnchor="end" style={{ fontFamily:'var(--font-mono)', fontSize:9, fill:'var(--fg-4)', fontVariantNumeric:'tabular-nums' }}>
          ${(t.v/1000).toFixed(1)}K
        </text>
      ))}
      {baseline >= yMin && baseline <= yMax && showAxes && (
        <text x="4" y={baseY - 3} style={{ fontFamily:'var(--font-mono)', fontSize:9, fill:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em' }}>net deposits</text>
      )}
    </svg>
  );
}

function SidePill({ side }) {
  const isYes = side === 'YES';
  return <span className={'pill ' + (isYes ? 'pill-yes' : 'pill-no')}>{side}</span>;
}
function CategoryDot({ cat }) {
  return <span style={{ width:6, height:6, borderRadius:'50%', background: CAT_COLORS[cat] || '#888', display:'inline-block', flexShrink:0 }} />;
}

// Range selector
function RangePicker({ range, setRange, sizes=['24h','7d','30d','all'] }) {
  return (
    <div style={{ display:'flex', alignItems:'center', gap:4, padding:2, background:'var(--surface-1)', border:'1px solid var(--border-2)', borderRadius:4 }}>
      {sizes.map(r => (
        <button key={r} onClick={() => setRange(r)} style={{
          background: range===r ? 'var(--surface-2)' : 'transparent', border:0, padding:'3px 10px', borderRadius:3, cursor:'pointer',
          color: range===r ? 'var(--fg-1)' : 'var(--fg-3)',
          fontFamily:'var(--font-mono)', fontSize:10, textTransform:'uppercase', letterSpacing:'0.04em',
        }}>{r}</button>
      ))}
    </div>
  );
}

// KV
function Kv({ label, value, sub, accent='var(--fg-1)', size=22 }) {
  return (
    <div style={{ display:'flex', flexDirection:'column', gap:4, minWidth:0 }}>
      <span className="eyebrow">{label}</span>
      <span style={{ fontFamily:'var(--font-sans)', fontSize:size, fontWeight:600, color:accent, fontVariantNumeric:'tabular-nums', letterSpacing:'-0.01em', lineHeight:1 }}>{value}</span>
      {sub && <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em' }}>{sub}</span>}
    </div>
  );
}

// Identity strip
function IdentityStrip({ size='md' }) {
  const t = TRADER;
  const compact = size === 'sm';
  return (
    <div style={{ display:'flex', alignItems:'center', gap:12 }}>
      <div style={{
        width: compact?28:36, height: compact?28:36, borderRadius:4,
        background:'var(--surface-1)', border:'1px solid var(--border-2)',
        display:'grid', gridTemplateColumns:'repeat(4, 1fr)', padding:3, gap:1,
      }}>
        {Array.from({length:16}).map((_,i) => {
          const on = ((parseInt(t.address.slice(2 + i, 4 + i), 16) || 0) % 7) > 2;
          return <span key={i} style={{ background: on ? 'var(--accent)' : 'transparent', borderRadius:1 }} />;
        })}
      </div>
      <div style={{ display:'flex', alignItems:'center', gap:10, minWidth:0, whiteSpace:'nowrap' }}>
        <span style={{ fontFamily:'var(--font-mono)', fontSize: compact?12:13, color:'var(--fg-1)' }}>{t.short}</span>
        <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em' }}>{t.alias}</span>
      </div>
    </div>
  );
}

// Allocation bar
function AllocationStrip({ compact=false }) {
  const total = ALLOCATION.reduce((a,b) => a + b.val, 0);
  return (
    <div>
      {!compact && (
        <div style={{ display:'flex', alignItems:'baseline', gap:14, paddingBottom:10 }}>
          <span className="eyebrow">Allocation</span>
          <span className="anno">by category · {fmtMoney(total)} deployed across {OPEN_POSITIONS.length} positions</span>
        </div>
      )}
      <div style={{ display:'flex', height:6, borderRadius:1, overflow:'hidden', background:'var(--surface-1)', border:'1px solid var(--border-1)' }}>
        {ALLOCATION.map(a => (
          <div key={a.cat} style={{ width: a.pct + '%', background: CAT_COLORS[a.cat] || '#888', opacity: 0.85 }} title={`${a.cat} · ${a.pct.toFixed(1)}%`} />
        ))}
      </div>
      <div style={{ display:'flex', flexWrap:'wrap', gap:14, paddingTop:10 }}>
        {ALLOCATION.map(a => (
          <div key={a.cat} style={{ display:'flex', alignItems:'center', gap:6 }}>
            <span style={{ width:7, height:7, background: CAT_COLORS[a.cat] || '#888', borderRadius:1 }} />
            <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em' }}>{a.cat}</span>
            <span style={{ fontFamily:'var(--font-mono)', fontSize:11, color:'var(--fg-1)', fontVariantNumeric:'tabular-nums' }}>{a.pct.toFixed(1)}%</span>
          </div>
        ))}
      </div>
    </div>
  );
}

// Collapsible section
function Collapsible({ title, defaultOpen=true, anno, right, children }) {
  const [open, setOpen] = React.useState(defaultOpen);
  return (
    <section style={{ marginTop:18 }}>
      <div style={{ display:'flex', alignItems:'center', justifyContent:'space-between', borderBottom:'1px solid var(--border-1)', padding:'0 0 8px', marginBottom: open?14:0 }}>
        <button onClick={() => setOpen(o => !o)} style={{ background:'transparent', border:0, color:'var(--fg-1)', cursor:'pointer', display:'flex', alignItems:'center', gap:8, padding:0 }}>
          <svg width="10" height="10" viewBox="0 0 10 10" style={{ transform:`rotate(${open?90:0}deg)`, transition:'transform 120ms', color:'var(--fg-3)' }}><path d="M3 1l4 4-4 4" fill="none" stroke="currentColor" strokeWidth="1.5"/></svg>
          <span style={{ fontFamily:'var(--font-sans)', fontSize:13, fontWeight:600, color:'var(--fg-1)', textTransform:'uppercase', letterSpacing:'0.06em' }}>{title}</span>
          {anno && <span className="anno" style={{ marginLeft:6 }}>{anno}</span>}
        </button>
        {right && <div>{right}</div>}
      </div>
      {open && children}
    </section>
  );
}

Object.assign(window, { useBatchClock, GlobalNav, MiniSpark, EquityChart, SidePill, CategoryDot, RangePicker, Kv, IdentityStrip, AllocationStrip, Collapsible });
