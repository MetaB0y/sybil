// ─────────────────────────────────────────────────────────────────────────
// Right-rail mode switcher: Degen / Trader / Pro
// Pro = the existing rich rail (V2's batch hero + outcome picker + buy box)
// Degen = stripped-down "feels like betting" UI
// Trader = TBD placeholder
// ─────────────────────────────────────────────────────────────────────────

const MODES = [
  { id: 'degen',  label: 'Degen',  sub: 'tap & win' },
  { id: 'pro',    label: 'Pro',    sub: 'full depth' },
];

function ModeTabs({ value, onChange }) {
  return (
    <div role="tablist" style={{
      display:'grid', gridTemplateColumns:'1fr 1fr', gap:4,
      padding:4, background:'var(--bg-2)', border:'1px solid var(--border-1)', borderRadius:6,
    }}>
      {MODES.map(m => {
        const a = value === m.id;
        return (
          <button key={m.id} role="tab" aria-selected={a} onClick={() => onChange(m.id)} style={{
            display:'flex', flexDirection:'column', alignItems:'center', justifyContent:'center', gap:1,
            padding:'8px 6px', borderRadius:4, border:0, cursor:'pointer',
            background: a ? 'var(--surface-2)' : 'transparent',
            boxShadow: a ? 'inset 0 0 0 1px var(--border-3)' : 'none',
            color: a ? 'var(--fg-1)' : 'var(--fg-3)',
            transition:'background 120ms, color 120ms',
          }}>
            <span style={{ fontFamily:'var(--font-sans)', fontSize:13, fontWeight:600, lineHeight:1 }}>{m.label}</span>
            <span style={{ fontFamily:'var(--font-mono)', fontSize:9, color: a?'var(--fg-3)':'var(--fg-4)', textTransform:'uppercase', letterSpacing:'0.05em' }}>{m.sub}</span>
          </button>
        );
      })}
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────
// Big "next batch in" pill — degen-friendly clock without the math
// ─────────────────────────────────────────────────────────────────────────
function NextBatchBanner({ secs }) {
  const pct = (secs/60)*100;
  // Live participant counter — drifts up/down to fire FOMO
  const [traders, setTraders] = React.useState(127);
  React.useEffect(() => {
    const id = setInterval(() => {
      setTraders(t => Math.max(40, t + Math.round((Math.random()-0.35)*6)));
    }, 1400);
    return () => clearInterval(id);
  }, []);
  return (
    <div style={{
      position:'relative', overflow:'hidden',
      background:'linear-gradient(135deg, rgba(63,182,217,0.12), rgba(63,182,217,0.02))',
      border:'1px solid rgba(63,182,217,0.35)', borderRadius:8, padding:'14px 16px',
      display:'flex', alignItems:'center', gap:14,
    }}>
      <div style={{
        width:48, height:48, borderRadius:'50%',
        background:'rgba(63,182,217,0.10)', border:'1.5px solid var(--accent)',
        display:'flex', alignItems:'center', justifyContent:'center',
        fontFamily:'var(--font-mono)', fontSize:14, fontWeight:600, color:'var(--accent)',
        fontVariantNumeric:'tabular-nums', flexShrink:0,
      }}>
        0:{secs.toString().padStart(2,'0')}
      </div>
      <div style={{ flex:1, minWidth:0 }}>
        <div style={{ fontFamily:'var(--font-mono)', fontSize:9.5, color:'var(--accent)', textTransform:'uppercase', letterSpacing:'0.06em', marginBottom:3 }}>● next trade in</div>
        <div style={{ fontFamily:'var(--font-sans)', fontSize:17, fontWeight:600, color:'var(--fg-1)', letterSpacing:'-0.01em', lineHeight:1.2, fontVariantNumeric:'tabular-nums' }}>
          {secs} {secs===1?'second':'seconds'}
        </div>
        <div style={{ marginTop:4, display:'flex', alignItems:'center', gap:5, fontFamily:'var(--font-mono)', fontSize:11, color:'var(--fg-3)', fontVariantNumeric:'tabular-nums' }}>
          <span style={{ width:5, height:5, borderRadius:'50%', background:'var(--yes)', boxShadow:'0 0 6px var(--yes)' }} />
          <span style={{ color:'var(--fg-1)', fontWeight:600 }}>{traders}</span>
          <span>traders joined</span>
        </div>
      </div>
      <div style={{ position:'absolute', left:0, right:0, bottom:0, height:2, background:'rgba(63,182,217,0.16)' }}>
        <div style={{ width:`${pct}%`, height:'100%', background:'var(--accent)', transition:'width 1000ms linear' }} />
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────
// Single-outcome card with dropdown — selected outcome is shown big,
// the rest collapse under a "switch" dropdown.
// ─────────────────────────────────────────────────────────────────────────
function DegenOutcomePicker({ outcomes, value, onChange }) {
  const [open, setOpen] = React.useState(false);
  const ref = React.useRef(null);
  React.useEffect(() => {
    const close = (e) => { if (ref.current && !ref.current.contains(e.target)) setOpen(false); };
    document.addEventListener('mousedown', close);
    return () => document.removeEventListener('mousedown', close);
  }, []);
  const sel = outcomes.find(o => o.id === value) || outcomes[0];
  const c = OUT_COLORS[sel.id];
  const others = outcomes.filter(o => o.id !== sel.id);
  return (
    <div ref={ref} style={{ position:'relative' }}>
      <div style={{
        display:'flex', alignItems:'center', gap:12,
        padding:'14px 16px', borderRadius:6,
        background: `${c}1A`, border:'1px solid', borderColor: c,
      }}>
        <span style={{
          width:14, height:14, borderRadius:'50%',
          border:'2px solid', borderColor: c,
          display:'flex', alignItems:'center', justifyContent:'center', flexShrink:0,
        }}><span style={{ width:6, height:6, borderRadius:'50%', background:c }} /></span>
        <span style={{ flex:1, fontFamily:'var(--font-sans)', fontSize:15, fontWeight:600, color:'var(--fg-1)' }}>{sel.label}</span>
        <span style={{ fontFamily:'var(--font-sans)', fontSize:18, fontWeight:600, color:c, fontVariantNumeric:'tabular-nums' }}>{sel.yes}¢</span>
      </div>
      <button onClick={() => setOpen(o => !o)} style={{
        marginTop:6, width:'100%',
        display:'flex', alignItems:'center', justifyContent:'space-between',
        padding:'8px 14px', borderRadius:4,
        background:'transparent', border:'1px solid var(--border-1)',
        color:'var(--fg-3)', fontFamily:'var(--font-sans)', fontSize:11.5, cursor:'pointer',
      }}>
        <span>switch outcome ({others.length} more)</span>
        <svg width="10" height="10" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5"
             style={{ transform: open?'rotate(180deg)':'none', transition:'transform 120ms' }}>
          <path d="m3 4.5 3 3 3-3"/>
        </svg>
      </button>
      {open && (
        <div style={{
          position:'absolute', top:'calc(100% + 4px)', left:0, right:0, zIndex:30,
          background:'var(--surface-2)', border:'1px solid var(--border-2)', borderRadius:6,
          padding:4, boxShadow:'var(--shadow-popover)',
          display:'flex', flexDirection:'column', gap:2,
        }}>
          {others.map(o => {
            const oc = OUT_COLORS[o.id];
            return (
              <button key={o.id} onClick={() => { onChange(o.id); setOpen(false); }} style={{
                display:'flex', alignItems:'center', gap:10,
                padding:'10px 12px', borderRadius:4,
                background:'transparent', border:0, cursor:'pointer', textAlign:'left',
              }}
                onMouseEnter={(e) => e.currentTarget.style.background='var(--bg-2)'}
                onMouseLeave={(e) => e.currentTarget.style.background='transparent'}>
                <span style={{ width:8, height:8, borderRadius:'50%', background:oc, flexShrink:0 }} />
                <span style={{ flex:1, fontFamily:'var(--font-sans)', fontSize:13, color:'var(--fg-1)' }}>{o.label}</span>
                <span style={{ fontFamily:'var(--font-sans)', fontSize:13, fontWeight:600, color:oc, fontVariantNumeric:'tabular-nums' }}>{o.yes}¢</span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────
// Big YES / NO segmented toggle
// ─────────────────────────────────────────────────────────────────────────
function YesNoToggle({ value, onChange }) {
  return (
    <div style={{ display:'grid', gridTemplateColumns:'1fr 1fr', gap:6 }}>
      {[
        { id:'YES', label:'Yes', color:'var(--yes)' },
        { id:'NO',  label:'No',  color:'var(--no)' },
      ].map(s => {
        const a = value === s.id;
        return (
          <button key={s.id} onClick={() => onChange(s.id)} style={{
            padding:'14px 0', borderRadius:6, cursor:'pointer',
            background: a ? s.color : 'var(--bg-2)',
            border:'1px solid', borderColor: a ? s.color : 'var(--border-1)',
            color: a ? '#0A0E12' : 'var(--fg-2)',
            fontFamily:'var(--font-sans)', fontSize:17, fontWeight:700, letterSpacing:'-0.005em',
            transition:'background 120ms, color 120ms, border-color 120ms',
          }}>{s.label}</button>
        );
      })}
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────
// Amount input + live "to win up to" line
// ─────────────────────────────────────────────────────────────────────────
function DegenAmount({ amount, setAmount, priceCents, side }) {
  // For YES at p¢: paying p, win 100. Multiplier = 100/p.
  // For NO at p¢: paying (100-p), win 100. Multiplier = 100/(100-p).
  const effPrice = side==='YES' ? priceCents : (100 - priceCents);
  const mult = effPrice > 0 ? (100/effPrice) : 0;
  const num = parseFloat(amount) || 0;
  const win = num * mult;

  const chips = [10, 25, 100, 500];

  return (
    <div style={{ display:'flex', flexDirection:'column', gap:10 }}>
      <div style={{
        display:'flex', alignItems:'center', gap:10,
        background:'var(--bg-2)', border:'1px solid var(--border-2)', borderRadius:6,
        padding:'14px 16px',
      }}>
        <span style={{ fontFamily:'var(--font-sans)', fontSize:24, fontWeight:500, color:'var(--fg-3)', lineHeight:1 }}>$</span>
        <input
          type="text" inputMode="decimal" value={amount}
          onChange={(e) => setAmount(e.target.value.replace(/[^0-9.]/g,''))}
          style={{
            flex:1, minWidth:0, background:'transparent', border:0, outline:'none',
            color:'var(--fg-1)', fontFamily:'var(--font-sans)', fontSize:30, fontWeight:600,
            letterSpacing:'-0.01em', padding:0, fontVariantNumeric:'tabular-nums',
          }}
          placeholder="0"
        />
      </div>

      <div style={{ display:'grid', gridTemplateColumns:'repeat(4, 1fr)', gap:6 }}>
        {chips.map(c => (
          <button key={c} onClick={() => setAmount(String(c))} style={{
            padding:'8px 0', background:'var(--bg-2)', border:'1px solid var(--border-1)', borderRadius:4,
            color:'var(--fg-2)', fontFamily:'var(--font-mono)', fontSize:11, cursor:'pointer',
          }}>+${c}</button>
        ))}
      </div>

      {/* To-win readout */}
      <div style={{
        background:'linear-gradient(135deg, rgba(76,175,80,0.10), rgba(76,175,80,0.02))',
        border:'1px solid rgba(76,175,80,0.30)', borderRadius:6,
        padding:'12px 14px',
        display:'flex', alignItems:'baseline', justifyContent:'space-between', gap:10,
      }}>
        <div style={{ display:'flex', flexDirection:'column', gap:2 }}>
          <span style={{ fontFamily:'var(--font-mono)', fontSize:9.5, color:'var(--yes)', textTransform:'uppercase', letterSpacing:'0.06em' }}>to win up to</span>
          <span style={{ fontFamily:'var(--font-sans)', fontSize:22, fontWeight:600, color:'var(--yes)', letterSpacing:'-0.01em', fontVariantNumeric:'tabular-nums' }}>
            ${win.toFixed(4)}
          </span>
        </div>
        <span style={{ fontFamily:'var(--font-mono)', fontSize:14, fontWeight:600, color:'var(--yes)', fontVariantNumeric:'tabular-nums' }}>
          {mult.toFixed(2)}×
        </span>
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────
// "why waiting?" — tooltip explaining FBA in plain language
// ─────────────────────────────────────────────────────────────────────────
function WhyWaiting() {
  const [open, setOpen] = React.useState(false);
  const ref = React.useRef(null);
  React.useEffect(() => {
    const close = (e) => { if (ref.current && !ref.current.contains(e.target)) setOpen(false); };
    document.addEventListener('mousedown', close);
    return () => document.removeEventListener('mousedown', close);
  }, []);
  return (
    <div ref={ref} style={{ position:'relative', display:'flex', justifyContent:'center' }}>
      <button onClick={() => setOpen(o=>!o)} style={{
        background:'transparent', border:0, color:'var(--fg-3)',
        fontFamily:'var(--font-sans)', fontSize:11.5, cursor:'pointer',
        display:'inline-flex', alignItems:'center', gap:6,
      }}>
        <span style={{
          width:14, height:14, borderRadius:'50%', border:'1px solid var(--border-3)',
          display:'inline-flex', alignItems:'center', justifyContent:'center',
          fontFamily:'var(--font-mono)', fontSize:9, color:'var(--fg-3)',
        }}>?</span>
        <span style={{ textDecoration:'underline', textUnderlineOffset:2, textDecorationColor:'var(--border-3)' }}>why am I waiting?</span>
      </button>
      {open && (
        <div style={{
          position:'absolute', bottom:'calc(100% + 8px)', left:0, right:0,
          background:'var(--surface-3)', border:'1px solid var(--border-2)', borderRadius:6,
          padding:'12px 14px', boxShadow:'var(--shadow-popover)', zIndex:20,
          fontFamily:'var(--font-sans)', fontSize:12, lineHeight:'17px', color:'var(--fg-2)',
        }}>
          <div style={{ fontFamily:'var(--font-mono)', fontSize:9.5, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.06em', marginBottom:6 }}>frequent batch auction</div>
          Every 60 seconds, all orders settle together at one fair price — so a whale can't jump the queue and bots can't snipe you. Your order joins the next batch and clears with everyone else's.
          <div style={{ marginTop:8, paddingTop:8, borderTop:'1px solid var(--border-1)', fontSize:11, color:'var(--fg-3)' }}>
            tl;dr — same price for everyone, no front-running.
          </div>
        </div>
      )}
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────
// DEGEN mode body
// ─────────────────────────────────────────────────────────────────────────
function DegenRail({ market, secs }) {
  const [outcomeId, setOutcomeId] = React.useState(market.outcomes[0].id);
  const [side, setSide] = React.useState('YES');
  const [amount, setAmount] = React.useState('100');
  const outcome = market.outcomes.find(o => o.id === outcomeId);
  return (
    <div style={{ display:'flex', flexDirection:'column', gap:12 }}>
      <NextBatchBanner secs={secs} />

      <div>
        <div style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.06em', marginBottom:8 }}>pick outcome</div>
        <DegenOutcomePicker outcomes={market.outcomes} value={outcomeId} onChange={setOutcomeId} />
      </div>

      <div>
        <div style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.06em', marginBottom:8 }}>
          will it happen?
        </div>
        <YesNoToggle value={side} onChange={setSide} />
      </div>

      <div>
        <div style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.06em', marginBottom:8 }}>your bet</div>
        <DegenAmount amount={amount} setAmount={setAmount} priceCents={outcome.yes} side={side} />
      </div>

      <button style={{
        marginTop:4, padding:'16px 0', borderRadius:6, border:0, cursor:'pointer',
        background: side==='YES' ? 'var(--yes)' : 'var(--no)',
        color:'#0A0E12', fontFamily:'var(--font-sans)', fontSize:15, fontWeight:700, letterSpacing:'-0.005em',
      }}>
        Bet ${parseFloat(amount)||0} on {side} · {outcome.label}
      </button>

      <WhyWaiting />
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────
// TRADER mode — combo of Degen (timer + simple outcome pick) + Pro buy box
// ─────────────────────────────────────────────────────────────────────────
function TraderRail({ market, secs, history }) {
  const [outcomeId, setOutcomeId] = React.useState(market.outcomes[0].id);
  const outcome = market.outcomes.find(o => o.id === outcomeId);
  const last = history && history[0];
  const ind = outcome.yes;
  const iev = '$' + (10 + outcome.yes/3).toFixed(1) + 'K';
  const imb = (last && last.imbalance) || 'YES';
  return (
    <div style={{ display:'flex', flexDirection:'column', gap:14 }}>
      <NextBatchBanner secs={secs} />

      {/* Indicative price / volume / imbalance — compact */}
      <div style={{
        background:'var(--bg-2)', border:'1px solid var(--border-1)', borderRadius:6,
        padding:'12px 14px', display:'grid', gridTemplateColumns:'1fr 1fr 1fr', gap:12,
      }}>
        <div>
          <div style={{ fontFamily:'var(--font-mono)', fontSize:9, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em', marginBottom:3 }}>
            <Glossary term="Indicative price">ind. price</Glossary>
          </div>
          <div style={{ fontFamily:'var(--font-sans)', fontSize:18, fontWeight:600, color: OUT_COLORS[outcome.id], fontVariantNumeric:'tabular-nums' }}>{ind}¢</div>
        </div>
        <div>
          <div style={{ fontFamily:'var(--font-mono)', fontSize:9, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em', marginBottom:3 }}>
            <Glossary term="IEV">ind. volume</Glossary>
          </div>
          <div style={{ fontFamily:'var(--font-sans)', fontSize:18, fontWeight:600, color:'var(--fg-1)', fontVariantNumeric:'tabular-nums' }}>{iev}</div>
        </div>
        <div>
          <div style={{ fontFamily:'var(--font-mono)', fontSize:9, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em', marginBottom:3 }}>
            <Glossary term="Imbalance">imbalance</Glossary>
          </div>
          <div style={{ fontFamily:'var(--font-sans)', fontSize:14, fontWeight:600, color: imb==='YES'?'var(--yes)':'var(--no)' }}>
            {imb==='YES'?'↑ buy':'↓ sell'}
          </div>
        </div>
      </div>

      {/* Pick outcome — degen-style single + dropdown */}
      <div>
        <div style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.06em', marginBottom:8 }}>pick outcome</div>
        <DegenOutcomePicker outcomes={market.outcomes} value={outcomeId} onChange={setOutcomeId} />
      </div>

      {/* Place order — pro buy box */}
      <div>
        <div style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.06em', marginBottom:8 }}>place batch order</div>
        <BuyBox outcome={outcome} secs={secs} />
      </div>
    </div>
  );
}

Object.assign(window, {
  ModeTabs, DegenRail, TraderRail,
});
