// FBA right-panel building blocks: outcome picker, batch countdown, last-N stats,
// current-batch indicative card, buy box.
// Used by V1, V2, V5 (with different layouts).

// ───────────────────────────────────────────────────────────────────────
// Outcome picker — radio list with sparkline + price.
// ───────────────────────────────────────────────────────────────────────
function OutcomePicker({ outcomes, value, onChange, compact=false }) {
  return (
    <div style={{ display:'flex', flexDirection:'column', gap:6 }}>
      {outcomes.map(o => {
        const a = value === o.id;
        return (
          <button key={o.id} onClick={() => onChange(o.id)} style={{
            display:'grid', gridTemplateColumns: compact ? '14px 1fr auto auto' : '14px 1fr 60px auto',
            alignItems:'center', gap:10,
            padding: compact ? '7px 10px' : '10px 12px',
            background: a ? 'var(--surface-2)' : 'var(--bg-2)',
            border:'1px solid', borderColor: a ? OUT_COLORS[o.id] : 'var(--border-1)',
            borderRadius:4, cursor:'pointer', textAlign:'left',
            boxShadow: a ? `inset 0 0 0 1px ${OUT_COLORS[o.id]}33` : 'none',
            transition:'background 120ms, border-color 120ms',
          }}>
            <span style={{
              width:12, height:12, borderRadius:'50%',
              border:'1.5px solid', borderColor: a ? OUT_COLORS[o.id] : 'var(--border-3)',
              display:'flex', alignItems:'center', justifyContent:'center',
            }}>{a && <span style={{ width:5, height:5, borderRadius:'50%', background: OUT_COLORS[o.id] }} />}</span>
            <div style={{ display:'flex', flexDirection:'column', gap:2, minWidth:0 }}>
              <span style={{ fontFamily:'var(--font-sans)', fontSize: compact?12:13, fontWeight:500, color:'var(--fg-1)', whiteSpace:'nowrap' }}>{o.label}</span>
              {!compact && <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)' }}>vol ${o.vol} · {o.traders} traders</span>}
            </div>
            {!compact && <Spark data={(window.FED_SERIES && window.FED_SERIES[outcomes.indexOf(o)]) || []} w={56} h={20} stroke={OUT_COLORS[o.id]} fill={false}/>}
            <div style={{ display:'flex', flexDirection:'column', alignItems:'flex-end', gap:2 }}>
              <span style={{ fontFamily:'var(--font-mono)', fontSize: compact?13:15, color: OUT_COLORS[o.id], fontVariantNumeric:'tabular-nums' }}>{o.yes}¢</span>
              {!compact && <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color: o.delta24>=0?'var(--yes)':'var(--no)' }}>{o.delta24>=0?'+':''}{o.delta24}%</span>}
            </div>
          </button>
        );
      })}
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────
// Batch countdown — circular ring + numeric. Variants:
//   size='sm' (right-rail strip)  size='lg' (V2 hero)
// ───────────────────────────────────────────────────────────────────────
function BatchCountdown({ secs, size='sm' }) {
  const r = size==='lg' ? 80 : 36;
  const stroke = size==='lg' ? 8 : 4;
  const C = 2 * Math.PI * r;
  const pct = secs/60;
  const dim = (r+stroke)*2;
  // Live participant counter for the lg variant — drifts to feel real
  const [traders, setTraders] = React.useState(127);
  React.useEffect(() => {
    if (size !== 'lg') return;
    const id = setInterval(() => {
      setTraders(t => Math.max(40, t + Math.round((Math.random()-0.35)*6)));
    }, 1400);
    return () => clearInterval(id);
  }, [size]);
  return (
    <div style={{ display:'flex', alignItems:'center', gap: size==='lg'? 18 : 10 }}>
      <svg width={dim} height={dim} viewBox={`0 0 ${dim} ${dim}`}>
        <circle cx={dim/2} cy={dim/2} r={r} fill="none" stroke="rgba(63,182,217,0.16)" strokeWidth={stroke} />
        <circle cx={dim/2} cy={dim/2} r={r} fill="none" stroke="var(--accent)" strokeWidth={stroke}
          strokeDasharray={C} strokeDashoffset={C*(1-pct)} strokeLinecap="round"
          transform={`rotate(-90 ${dim/2} ${dim/2})`}
          style={{ transition:'stroke-dashoffset 1000ms linear' }} />
        <text x={dim/2} y={dim/2 + (size==='lg'?10:4)} textAnchor="middle"
          fill="var(--fg-1)" fontFamily="var(--font-mono)" fontSize={size==='lg'?32:14} fontWeight="500">
          0:{secs.toString().padStart(2,'0')}
        </text>
      </svg>
      <div style={{ display:'flex', flexDirection:'column', gap:2 }}>
        <span style={{ fontFamily:'var(--font-mono)', fontSize: size==='lg'?11:9, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.06em' }}>
          <Glossary term="Batch">next batch clears in</Glossary>
        </span>
        <span style={{ fontFamily:'var(--font-sans)', fontSize: size==='lg'?14:11, color:'var(--fg-2)' }}>
          batch <span style={{ fontFamily:'var(--font-mono)', color:'var(--fg-1)' }}>#9413</span>
        </span>
        {size==='lg' && (
          <span style={{ marginTop:2, display:'inline-flex', alignItems:'center', gap:6, fontFamily:'var(--font-mono)', fontSize:11, color:'var(--fg-3)', fontVariantNumeric:'tabular-nums' }}>
            <span style={{ width:5, height:5, borderRadius:'50%', background:'var(--yes)', boxShadow:'0 0 6px var(--yes)' }} />
            <span style={{ color:'var(--fg-1)', fontWeight:600 }}>{traders}</span>
            <span>traders joined</span>
          </span>
        )}
      </div>
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────
// Last-N batch summary — "last 1/5/10/100" tabs, totals.
// ───────────────────────────────────────────────────────────────────────
function LastNStats({ history }) {
  const [n, setN] = React.useState(1);
  const slice = history.slice(0, n);
  const sum = (k) => slice.reduce((a,b) => a + b[k], 0);
  const sumK = (k) => slice.reduce((a,b) => a + parseFloat(b[k]), 0);
  const opts = [1, 5, 10, 100];
  return (
    <div>
      <div style={{ display:'flex', alignItems:'baseline', justifyContent:'space-between', marginBottom:8 }}>
        <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em' }}>last batches</span>
        <div style={{ display:'flex', gap:1, padding:1, background:'var(--bg-2)', border:'1px solid var(--border-1)', borderRadius:3 }}>
          {opts.map(o => (
            <button key={o} onClick={() => setN(o)} style={{
              padding:'2px 8px', borderRadius:2, border:0, cursor:'pointer',
              background: n===o ? 'var(--surface-2)' : 'transparent',
              color: n===o ? 'var(--fg-1)' : 'var(--fg-3)',
              fontFamily:'var(--font-mono)', fontSize:10,
            }}>{o}</button>
          ))}
        </div>
      </div>
      <div style={{ display:'grid', gridTemplateColumns:'1fr 1fr', gap:1, background:'var(--border-1)', border:'1px solid var(--border-1)', borderRadius:4 }}>
        {[
          { l:'traders placed', v: sum('placed') },
          { l:'traders matched', v: sum('matched') },
          { l:'volume placed', v: '$'+sumK('volPlaced').toFixed(1)+'K' },
          { l:'volume matched', v: '$'+sumK('volMatched').toFixed(1)+'K' },
        ].map((s, i) => (
          <div key={i} style={{ background:'var(--surface-1)', padding:'10px 12px' }}>
            <div style={{ fontFamily:'var(--font-mono)', fontSize:9.5, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em', marginBottom:4 }}>{s.l}</div>
            <div style={{ fontFamily:'var(--font-mono)', fontSize:16, color:'var(--fg-1)', fontVariantNumeric:'tabular-nums' }}>{s.v}</div>
          </div>
        ))}
      </div>
      {/* match-rate progress */}
      <div style={{ marginTop:8, display:'flex', alignItems:'center', gap:8, fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)' }}>
        <span>match rate</span>
        <div style={{ flex:1, height:3, background:'var(--bg-2)', borderRadius:2, overflow:'hidden' }}>
          <div style={{ width: `${(sum('matched')/Math.max(1,sum('placed'))*100).toFixed(0)}%`, height:'100%', background:'var(--accent)' }} />
        </div>
        <span style={{ color:'var(--fg-1)' }}>{(sum('matched')/Math.max(1,sum('placed'))*100).toFixed(0)}%</span>
      </div>
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────
// Current batch indicative card — IEV, indicative price, imbalance
// ───────────────────────────────────────────────────────────────────────
function CurrentBatchCard({ outcome, history }) {
  const last = history[0];
  const indPrice = Math.round(last.cleared * 100);
  const iev = '$' + (10 + (outcome.yes/3)).toFixed(1) + 'K';
  const imbalance = last.imbalance;
  const imbMag = 0.62; // visual mock
  return (
    <div style={{ background:'var(--bg-2)', border:'1px solid var(--border-1)', borderRadius:6, padding:'12px 14px' }}>
      <div style={{ display:'flex', justifyContent:'space-between', alignItems:'baseline', marginBottom:10 }}>
        <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em' }}>this batch · {outcome.label}</span>
        <span style={{ fontFamily:'var(--font-mono)', fontSize:9, color:'var(--accent)' }}>● live</span>
      </div>
      <div style={{ display:'grid', gridTemplateColumns:'1fr 1fr', gap:14 }}>
        <div>
          <div style={{ fontFamily:'var(--font-mono)', fontSize:9, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em', marginBottom:4 }}>
            <Glossary term="Indicative price">indicative price</Glossary>
          </div>
          <div style={{ fontFamily:'var(--font-mono)', fontSize:22, color: OUT_COLORS[outcome.id] }}>{outcome.yes}¢</div>
          <div style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', marginTop:2 }}>was {outcome.yes-1}¢ · last batch</div>
        </div>
        <div>
          <div style={{ fontFamily:'var(--font-mono)', fontSize:9, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em', marginBottom:4 }}>
            <Glossary term="IEV">iev</Glossary>
          </div>
          <div style={{ fontFamily:'var(--font-mono)', fontSize:22, color:'var(--fg-1)' }}>{iev}</div>
          <div style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', marginTop:2 }}>would clear at indicative</div>
        </div>
      </div>
      <div style={{ marginTop:12 }}>
        <div style={{ display:'flex', justifyContent:'space-between', alignItems:'baseline', marginBottom:4 }}>
          <span style={{ fontFamily:'var(--font-mono)', fontSize:9, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em' }}>
            <Glossary term="Imbalance">imbalance</Glossary>
          </span>
          <span style={{ fontFamily:'var(--font-mono)', fontSize:11, color: imbalance==='YES'?'var(--yes)':'var(--no)' }}>
            {imbalance === 'YES' ? '↑ buy-side' : '↓ sell-side'}
          </span>
        </div>
        {/* divergent bar — center is balanced, length = magnitude */}
        <div style={{ position:'relative', height:6, background:'var(--surface-1)', borderRadius:3, overflow:'hidden' }}>
          <div style={{ position:'absolute', top:0, bottom:0, left:'50%', width:1, background:'var(--border-2)' }} />
          <div style={{
            position:'absolute', top:0, bottom:0,
            left: imbalance==='YES' ? '50%' : `${50 - imbMag*50}%`,
            width: `${imbMag*50}%`,
            background: imbalance==='YES' ? 'var(--yes)' : 'var(--no)', opacity:0.85,
          }}/>
        </div>
        <div style={{ display:'flex', justifyContent:'space-between', marginTop:4, fontFamily:'var(--font-mono)', fontSize:9, color:'var(--fg-4)' }}>
          <span>more sells</span><span>balanced</span><span>more buys</span>
        </div>
      </div>
    </div>
  );
}

// ───────────────────────────────────────────────────────────────────────
// Buy box — market order with optional Advanced (limit) accordion.
// ───────────────────────────────────────────────────────────────────────
function BuyBox({ outcome, secs, advancedAllowed=true, layout='stacked' }) {
  const [side, setSide] = React.useState('buy');
  const [unit, setUnit] = React.useState('usd'); // 'usd' or 'shares'
  const [amount, setAmount] = React.useState('25');
  const [shares, setShares] = React.useState('100');
  const [limit, setLimit] = React.useState(outcome.yes);
  // Sync limit input text with slider
  const [limitText, setLimitText] = React.useState(String(outcome.yes));
  React.useEffect(() => { setLimitText(String(limit)); }, [limit]);
  const [ttl, setTtl] = React.useState('1 batch');
  const accent = side==='buy' ? OUT_COLORS[outcome.id] : 'var(--no)';

  // Derived numbers
  const limitDec = Math.max(1, Math.min(99, parseFloat(limit) || outcome.yes)) / 100;
  const usd = parseFloat(amount) || 0;
  const sh = parseFloat(shares) || 0;
  // If $: at limit price, shares = usd/limit (min); could clear better → "≥"
  // If shares: shares is firm; max cost = sh * limit; payout (if win) = sh; payout ≥ sh always
  const sharesIfUsd = usd / limitDec;
  const maxCostIfShares = sh * limitDec;

  return (
    <div style={{ display:'flex', flexDirection:'column', gap:10 }}>
      {/* Buy/Sell toggle */}
      <div style={{ display:'flex', background:'var(--bg-2)', border:'1px solid var(--border-1)', borderRadius:4, padding:2, gap:2 }}>
        {['buy','sell'].map(s => (
          <button key={s} onClick={() => setSide(s)} style={{
            flex:1, padding:'7px 0', border:0, borderRadius:3, cursor:'pointer',
            background: side===s ? 'var(--surface-2)' : 'transparent',
            color: side===s ? 'var(--fg-1)' : 'var(--fg-3)',
            fontFamily:'var(--font-sans)', fontSize:12, fontWeight: side===s?600:500, textTransform:'capitalize',
          }}>{s}</button>
        ))}
      </div>
      {/* Selected outcome reminder */}
      <div style={{ display:'flex', justifyContent:'space-between', alignItems:'center', padding:'8px 10px', background:'var(--bg-2)', border:'1px solid var(--border-1)', borderRadius:4 }}>
        <span style={{ display:'flex', alignItems:'center', gap:8, fontFamily:'var(--font-sans)', fontSize:12, color:'var(--fg-2)' }}>
          <span style={{ width:8, height:8, background:OUT_COLORS[outcome.id], borderRadius:1 }} />
          {outcome.label}
        </span>
        <span style={{ fontFamily:'var(--font-mono)', fontSize:13, color:OUT_COLORS[outcome.id] }}>{outcome.yes}¢</span>
      </div>

      {/* Order in $ vs shares */}
      <div>
        <div style={{ display:'flex', alignItems:'baseline', justifyContent:'space-between', marginBottom:5 }}>
          <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em' }}>order in</span>
          <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-4)' }}>
            {unit==='usd' ? 'balance $1,242' : 'available '+(1242/limitDec).toFixed(0)+' sh'}
          </span>
        </div>
        <div style={{ display:'flex', gap:4, marginBottom:6 }}>
          {[{id:'usd',l:'$ amount'},{id:'shares',l:'shares'}].map(o => (
            <button key={o.id} onClick={() => setUnit(o.id)} style={{
              flex:1, padding:'6px 0', borderRadius:3, cursor:'pointer',
              background: unit===o.id ? 'var(--surface-2)' : 'var(--bg-2)',
              border:'1px solid', borderColor: unit===o.id ? 'var(--border-3)' : 'var(--border-1)',
              color: unit===o.id ? 'var(--fg-1)' : 'var(--fg-3)',
              fontFamily:'var(--font-mono)', fontSize:10.5,
            }}>{o.l}</button>
          ))}
        </div>
        <div style={{ display:'flex', alignItems:'center', background:'var(--bg-2)', border:'1px solid var(--border-1)', borderRadius:4, padding:'6px 10px' }}>
          <span style={{ fontFamily:'var(--font-mono)', fontSize:18, color:'var(--fg-3)' }}>{unit==='usd'?'$':'#'}</span>
          <input
            value={unit==='usd' ? amount : shares}
            onChange={e => unit==='usd' ? setAmount(e.target.value) : setShares(e.target.value)}
            style={{
              flex:1, background:'transparent', border:0, outline:0, padding:'4px 4px',
              color:'var(--fg-1)', fontFamily:'var(--font-mono)', fontSize:18, fontVariantNumeric:'tabular-nums',
            }}/>
          <div style={{ display:'flex', gap:4 }}>
            {(unit==='usd'?['+10','+50','MAX']:['+10','+100','MAX']).map(p => (
              <button key={p} style={{
                padding:'2px 7px', borderRadius:3, border:'1px solid var(--border-1)', background:'transparent',
                color:'var(--fg-3)', fontFamily:'var(--font-mono)', fontSize:9.5, cursor:'pointer',
              }}>{p}</button>
            ))}
          </div>
        </div>
      </div>

      {/* Limit price — typed input + slider, indicative as default */}
      <div>
        <div style={{ display:'flex', justifyContent:'space-between', alignItems:'baseline', marginBottom:5 }}>
          <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em' }}>limit price</span>
          <button onClick={() => setLimit(outcome.yes)} style={{
            background:'transparent', border:0, padding:0, cursor:'pointer',
            color: limit===outcome.yes ? 'var(--fg-3)' : 'var(--accent)',
            fontFamily:'var(--font-mono)', fontSize:10, textDecoration:'underline', textUnderlineOffset:2,
          }}>set indicative {outcome.yes}¢</button>
        </div>
        <div style={{ display:'flex', alignItems:'center', background:'var(--bg-2)', border:'1px solid var(--border-1)', borderRadius:4, padding:'6px 10px', marginBottom:8 }}>
          <input
            value={limitText}
            onChange={(e) => {
              const v = e.target.value.replace(/[^0-9.]/g,'');
              setLimitText(v);
              const n = parseFloat(v);
              if (!isNaN(n)) setLimit(Math.max(1, Math.min(99, n)));
            }}
            style={{
              flex:1, background:'transparent', border:0, outline:0, padding:'2px 0',
              color:'var(--fg-1)', fontFamily:'var(--font-mono)', fontSize:16, fontVariantNumeric:'tabular-nums',
            }}/>
          <span style={{ fontFamily:'var(--font-mono)', fontSize:14, color:'var(--fg-3)' }}>¢</span>
        </div>
        <input type="range" min={1} max={99} value={limit} onChange={e => setLimit(+e.target.value)} style={{ width:'100%' }}/>
        <div style={{ display:'flex', justifyContent:'space-between', fontFamily:'var(--font-mono)', fontSize:9, color:'var(--fg-4)' }}>
          <span>1¢</span>
          <span>indicative {outcome.yes}¢</span>
          <span>99¢</span>
        </div>
      </div>

      {/* Order longevity */}
      <div>
        <div style={{ marginBottom:5, fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em' }}>good for</div>
        <div style={{ display:'flex', gap:4 }}>
          {['1 batch','5 batches','until cancel'].map(t => (
            <button key={t} onClick={() => setTtl(t)} style={{
              flex:1, padding:'6px 0', borderRadius:3, cursor:'pointer',
              background: ttl===t ? 'var(--surface-2)' : 'var(--bg-2)',
              border:'1px solid', borderColor: ttl===t ? 'var(--border-3)' : 'var(--border-1)',
              color: ttl===t ? 'var(--fg-1)' : 'var(--fg-3)',
              fontFamily:'var(--font-mono)', fontSize:10,
            }}>{t}</button>
          ))}
        </div>
      </div>

      {/* Receipt — semantics depend on unit:
          $:    shares ≥ X, payout ≥ Y    (price could clear better)
          shares: shares = X (firm), payout ≥ Y */}
      <div style={{ display:'flex', flexDirection:'column', gap:5, padding:'10px 12px', background:'var(--bg-2)', border:'1px dashed var(--border-1)', borderRadius:4, fontFamily:'var(--font-mono)', fontSize:11 }}>
        {unit==='usd' ? (
          <>
            <div style={{ display:'flex', justifyContent:'space-between', color:'var(--fg-2)' }}>
              <span>cost</span>
              <span style={{ color:'var(--fg-1)' }}>${usd.toFixed(2)}</span>
            </div>
            <div style={{ display:'flex', justifyContent:'space-between', color:'var(--fg-2)' }}>
              <span>shares (if matched)</span>
              <span style={{ color:'var(--fg-1)' }}>≥ {sharesIfUsd.toFixed(1)}</span>
            </div>
            <div style={{ display:'flex', justifyContent:'space-between', color:'var(--fg-2)' }}>
              <span>max payout</span>
              <span style={{ color:'var(--fg-1)' }}>≥ ${sharesIfUsd.toFixed(2)}</span>
            </div>
          </>
        ) : (
          <>
            <div style={{ display:'flex', justifyContent:'space-between', color:'var(--fg-2)' }}>
              <span>max cost</span>
              <span style={{ color:'var(--fg-1)' }}>≤ ${maxCostIfShares.toFixed(2)}</span>
            </div>
            <div style={{ display:'flex', justifyContent:'space-between', color:'var(--fg-2)' }}>
              <span>shares (if matched)</span>
              <span style={{ color:'var(--fg-1)' }}>{sh.toFixed(0)}</span>
            </div>
            <div style={{ display:'flex', justifyContent:'space-between', color:'var(--fg-2)' }}>
              <span>max payout</span>
              <span style={{ color:'var(--fg-1)' }}>≥ ${sh.toFixed(2)}</span>
            </div>
          </>
        )}
        <div style={{ display:'flex', justifyContent:'space-between', color:'var(--fg-3)', borderTop:'1px solid var(--border-1)', paddingTop:5, marginTop:2 }}>
          <span>queued for batch</span><span style={{ color:'var(--accent)' }}>#9413 · 0:{secs.toString().padStart(2,'0')}</span>
        </div>
      </div>

      {/* CTA */}
      <button style={{
        marginTop:2, padding:'12px 0', border:0, borderRadius:4, cursor:'pointer',
        background: accent, color: 'var(--fg-on-accent)',
        fontFamily:'var(--font-sans)', fontSize:14, fontWeight:600, letterSpacing:'0.01em',
      }}>queue {side} → batch #9413</button>
      <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', textAlign:'center' }}>
        clears at the uniform price · could fill better than your limit
      </span>
    </div>
  );
}

function BatchHistoryBars({ history, n=24 }) {
  const [hover, setHover] = React.useState(null);
  const slice = history.slice(0, n).reverse();
  return (
    <div style={{ position:'relative' }}>
      <div style={{ display:'flex', gap:2, alignItems:'flex-end', height:22 }}
           onMouseLeave={() => setHover(null)}>
        {slice.map((b, i) => {
          const h = 6 + (b.matched / 50) * 16;
          const isHover = hover && hover.i === i;
          return (
            <div key={i}
              onMouseEnter={() => setHover({ i, b })}
              style={{
                width:`calc(100%/${n} - 2px)`, height:h,
                background: b.imbalance==='YES'?'var(--yes)':'var(--no)',
                opacity: isHover ? 1 : 0.35 + (i/n)*0.5,
                outline: isHover ? '1px solid var(--fg-1)' : 'none',
                borderRadius:1, cursor:'pointer', transition:'opacity 80ms',
              }} />
          );
        })}
      </div>
      {hover && (
        <div style={{
          position:'absolute', bottom:'calc(100% + 6px)',
          left:`calc(${(hover.i + 0.5) * (100/n)}% - 90px)`,
          width:180, zIndex:30,
          background:'var(--surface-3)', border:'1px solid var(--border-2)', borderRadius:6,
          padding:'10px 12px', boxShadow:'var(--shadow-popover)',
          fontFamily:'var(--font-mono)', fontSize:10.5, color:'var(--fg-2)',
          pointerEvents:'none',
        }}>
          <div style={{ display:'flex', justifyContent:'space-between', marginBottom:5, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em', fontSize:9 }}>
            <span>batch #{9413 - (n - 1 - hover.i)}</span>
            <span style={{ color: hover.b.imbalance==='YES'?'var(--yes)':'var(--no)' }}>
              {hover.b.imbalance==='YES'?'↑ buy-side':'↓ sell-side'}
            </span>
          </div>
          <div style={{ display:'flex', justifyContent:'space-between', color:'var(--fg-3)' }}>
            <span>volume matched</span><span style={{ color:'var(--fg-1)' }}>${hover.b.volMatched}K</span>
          </div>
          <div style={{ display:'flex', justifyContent:'space-between', color:'var(--fg-3)' }}>
            <span>traders matched</span><span style={{ color:'var(--fg-1)' }}>{hover.b.matched}</span>
          </div>
          <div style={{ display:'flex', justifyContent:'space-between', color:'var(--fg-3)' }}>
            <span>unmatched orders</span><span style={{ color:'var(--fg-1)' }}>{Math.max(0, hover.b.placed - hover.b.matched)}</span>
          </div>
          <div style={{ display:'flex', justifyContent:'space-between', color:'var(--fg-3)' }}>
            <span>cleared price</span><span style={{ color:'var(--fg-1)' }}>{Math.round(hover.b.cleared*100)}¢</span>
          </div>
        </div>
      )}
    </div>
  );
}

Object.assign(window, { BatchHistoryBars });
