// Five page-level variations of the Fed-rate market detail.
// Each is exported as Vn({ market, series, history, comments, secs }) returning a full page.
// All assume FED_SERIES is also globally available for sparkline lookup.

const PAGE_W = 1280;
const PAGE_H = 1500;

// Compute series from FED_SERIES which is shape [outcome][step]
function getSeriesByOutcome(market) {
  return market.outcomes.map((_, i) => window.FED_SERIES[i]);
}

// ─────────────────────────────────────────────────────────────────────────
// V1 — Conventional Polymarket-shape with Lite/Pro mode toggle.
// Default = Lite. Pro reveals: indicative price, IEV, imbalance, last-N stats.
// ─────────────────────────────────────────────────────────────────────────
function V1Conventional({ market, series, history, comments, secs }) {
  const [mode, setMode] = React.useState('lite'); // lite | pro
  const [outcomeId, setOutcomeId] = React.useState(market.outcomes[0].id);
  const [range, setRange] = React.useState('1W');
  const outcome = market.outcomes.find(o => o.id === outcomeId);
  const isPro = mode === 'pro';
  return (
    <div style={{ width: PAGE_W, minHeight: PAGE_H, background:'var(--bg-1)', color:'var(--fg-1)', fontFamily:'var(--font-sans)' }}>
      <TopNav secs={secs}/>
      <MarketHeader market={market} rightSlot={<ModeToggle mode={mode} setMode={setMode}/>} />
      <div style={{ display:'grid', gridTemplateColumns:'1fr 360px', gap:24, padding:'20px 24px 36px' }}>
        {/* LEFT — chart, rules, comments */}
        <div style={{ display:'flex', flexDirection:'column', gap:24 }}>
          <Card>
            <div style={{ display:'flex', justifyContent:'space-between', alignItems:'center', marginBottom:14 }}>
              <OutcomeLegend outcomes={market.outcomes} />
              <RangeBar value={range} onChange={setRange} />
            </div>
            <StackedAreaChart outcomes={market.outcomes} series={series} h={300} />
          </Card>

          <Card>
            <RulesCard market={market} embedded />
          </Card>

          <Card>
            <CommentsThread comments={comments} />
          </Card>
        </div>

        {/* RIGHT — outcome picker + (pro: batch info) + buy */}
        <div style={{ display:'flex', flexDirection:'column', gap:14 }}>
          <Card>
            <SectionLabel>pick an outcome</SectionLabel>
            <OutcomePicker outcomes={market.outcomes} value={outcomeId} onChange={setOutcomeId} />
          </Card>

          <Card style={{ padding:'14px 16px' }}>
            <BatchCountdown secs={secs} size="sm" />
          </Card>

          {isPro && (
            <>
              <Card>
                <CurrentBatchCard outcome={outcome} history={history} />
              </Card>
              <Card>
                <LastNStats history={history} />
              </Card>
            </>
          )}

          <Card>
            <SectionLabel>place batch order</SectionLabel>
            <BuyBox outcome={outcome} secs={secs} />
          </Card>

          {!isPro && (
            <button onClick={() => setMode('pro')} style={{
              padding:'10px', border:'1px dashed var(--border-2)', background:'transparent', borderRadius:6, cursor:'pointer',
              color:'var(--fg-3)', fontFamily:'var(--font-mono)', fontSize:11, letterSpacing:'0.04em', textTransform:'uppercase',
            }}>+ show batch internals (pro)</button>
          )}
        </div>
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────
// V2 — Batch theater. Right rail anchored by huge live batch clock.
// Indicative price / IEV / imbalance feed into it. The batch IS the hero.
// ─────────────────────────────────────────────────────────────────────────
function V2BatchTheater({ market, series, history, comments, secs }) {
  const [outcomeId, setOutcomeId] = React.useState(market.outcomes[0].id);
  const outcome = market.outcomes.find(o => o.id === outcomeId);
  const [range, setRange] = React.useState('1W');
  const [mode, setMode] = React.useState('degen');
  return (
    <div style={{ width: PAGE_W, minHeight: PAGE_H, background:'var(--bg-1)', color:'var(--fg-1)', fontFamily:'var(--font-sans)' }}>
      <TopNav secs={secs}/>
      <MarketHeader market={market} />
      <div style={{ display:'grid', gridTemplateColumns:'1fr 420px', gap:24, padding:'20px 24px 36px' }}>
        <div style={{ display:'flex', flexDirection:'column', gap:24 }}>
          <Card>
            <div style={{ display:'flex', justifyContent:'space-between', alignItems:'center', marginBottom:14 }}>
              <OutcomeLegend outcomes={market.outcomes} />
              <RangeBar value={range} onChange={setRange} />
            </div>
            <StackedAreaChart outcomes={market.outcomes} series={series} h={300} />
          </Card>
          <Card><RulesCard market={market} embedded /></Card>
          <Card><CommentsThread comments={comments} /></Card>
        </div>

        {/* RIGHT — mode-switched rail */}
        <div style={{ display:'flex', flexDirection:'column', gap:14 }}>
          <ModeTabs value={mode} onChange={setMode} />

          {mode === 'degen' && <DegenRail market={market} secs={secs} />}
          {mode === 'pro' && <ProRail market={market} history={history} outcomeId={outcomeId} setOutcomeId={setOutcomeId} outcome={outcome} secs={secs} />}
        </div>
      </div>
    </div>
  );
}

// Pro rail = the original V2 right-side content
function ProRail({ market, history, outcomeId, setOutcomeId, outcome, secs }) {
  return (
    <div style={{ display:'flex', flexDirection:'column', gap:14 }}>
          {/* HERO: live batch */}
          <div style={{
            background:'linear-gradient(180deg, rgba(63,182,217,0.10), rgba(63,182,217,0.02))',
            border:'1px solid rgba(63,182,217,0.30)', borderRadius:8, padding:'20px 22px',
            position:'relative', overflow:'hidden',
          }}>
            <div style={{ position:'absolute', top:14, right:16, fontFamily:'var(--font-mono)', fontSize:9.5, color:'var(--accent)', textTransform:'uppercase', letterSpacing:'0.06em' }}>● live batch</div>
            <BatchCountdown secs={secs} size="lg"/>
            <div style={{ height:1, background:'var(--border-1)', margin:'18px 0 14px' }} />
            <div style={{ display:'flex', flexDirection:'column', gap:10 }}>
              <SubStat label={<Glossary term="Indicative price">indicative price</Glossary>} value={`${outcome.yes}¢`} valueColor={OUT_COLORS[outcome.id]} secondary={`for ${outcome.label}`} />
              <SubStat label={<Glossary term="IEV">indicative volume</Glossary>} value={`$${(10+outcome.yes/3).toFixed(1)}K`} secondary="would clear at indicative" />
              <SubStat label={<Glossary term="Imbalance">imbalance</Glossary>} value={
                <span style={{ color: history[0].imbalance==='YES'?'var(--yes)':'var(--no)' }}>
                  {history[0].imbalance==='YES' ? '↑ buy-side' : '↓ sell-side'}
                </span>
              } secondary="net unmatched orders" />
            </div>
            {/* Pulsing ring of last 12 batches as ticks */}
            <div style={{ marginTop:14 }}>
              <BatchHistoryBars history={history} n={24} />
            </div>
            <div style={{ marginTop:6, fontFamily:'var(--font-mono)', fontSize:9, color:'var(--fg-4)', textTransform:'uppercase', letterSpacing:'0.04em' }}>last 24 batches · matched volume + side</div>
          </div>

          <Card>
            <SectionLabel>pick an outcome</SectionLabel>
            <OutcomePicker outcomes={market.outcomes} value={outcomeId} onChange={setOutcomeId} compact />
          </Card>

          <Card>
            <SectionLabel>place batch order</SectionLabel>
            <BuyBox outcome={outcome} secs={secs} />
          </Card>

          {/* Collapsible: deeper stats one tap away */}
          <Disclosure label="last batches · stats">
            <LastNStats history={history} />
          </Disclosure>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────
// V3 — Order-flow tape. A horizontal "tape" of recent batches sits BETWEEN
// the chart and the comments. Right rail kept simple and friendly.
// ─────────────────────────────────────────────────────────────────────────
function V3OrderFlowTape({ market, series, history, comments, secs }) {
  const [outcomeId, setOutcomeId] = React.useState(market.outcomes[0].id);
  const outcome = market.outcomes.find(o => o.id === outcomeId);
  const [range, setRange] = React.useState('1W');
  const [tapeN, setTapeN] = React.useState(20);
  return (
    <div style={{ width: PAGE_W, minHeight: PAGE_H, background:'var(--bg-1)', color:'var(--fg-1)', fontFamily:'var(--font-sans)' }}>
      <TopNav secs={secs}/>
      <MarketHeader market={market} />
      <div style={{ display:'grid', gridTemplateColumns:'1fr 340px', gap:24, padding:'20px 24px 36px' }}>
        <div style={{ display:'flex', flexDirection:'column', gap:24 }}>
          <Card>
            <div style={{ display:'flex', justifyContent:'space-between', alignItems:'center', marginBottom:14 }}>
              <OutcomeLegend outcomes={market.outcomes} />
              <RangeBar value={range} onChange={setRange} />
            </div>
            <StackedAreaChart outcomes={market.outcomes} series={series} h={280} />
          </Card>

          {/* THE TAPE */}
          <Card>
            <div style={{ display:'flex', justifyContent:'space-between', alignItems:'baseline', marginBottom:12 }}>
              <div style={{ display:'flex', alignItems:'baseline', gap:10 }}>
                <h3 style={{ fontFamily:'var(--font-sans)', fontSize:14, fontWeight:600, color:'var(--fg-1)', margin:0 }}>Batch tape</h3>
                <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)' }}>last {tapeN} · oldest → newest →</span>
              </div>
              <div style={{ display:'flex', gap:1, padding:1, background:'var(--bg-2)', border:'1px solid var(--border-1)', borderRadius:3 }}>
                {[10,20,50,100].map(n => (
                  <button key={n} onClick={() => setTapeN(n)} style={{
                    padding:'2px 8px', borderRadius:2, border:0, cursor:'pointer',
                    background: tapeN===n ? 'var(--surface-2)' : 'transparent',
                    color: tapeN===n ? 'var(--fg-1)' : 'var(--fg-3)',
                    fontFamily:'var(--font-mono)', fontSize:10,
                  }}>{n}</button>
                ))}
              </div>
            </div>
            <Tape history={history.slice(0, tapeN).reverse()} />
            <div style={{ marginTop:10, display:'flex', gap:18, fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)' }}>
              <span><span style={{ display:'inline-block', width:8, height:8, background:'var(--yes)', marginRight:6, borderRadius:1 }}/>matched (buy lean)</span>
              <span><span style={{ display:'inline-block', width:8, height:8, background:'var(--no)', marginRight:6, borderRadius:1 }}/>matched (sell lean)</span>
              <span><span style={{ display:'inline-block', width:8, height:8, background:'var(--border-3)', marginRight:6, borderRadius:1 }}/>unmatched</span>
              <span style={{ marginLeft:'auto' }}>height = batch volume</span>
            </div>
          </Card>

          <Card><RulesCard market={market} embedded /></Card>
          <Card><CommentsThread comments={comments} /></Card>
        </div>

        {/* RIGHT — minimal, friendly */}
        <div style={{ display:'flex', flexDirection:'column', gap:14 }}>
          <Card style={{ padding:'14px 16px' }}>
            <BatchCountdown secs={secs} size="sm" />
          </Card>
          <Card>
            <SectionLabel>pick an outcome</SectionLabel>
            <OutcomePicker outcomes={market.outcomes} value={outcomeId} onChange={setOutcomeId} compact />
          </Card>
          <Card>
            <SectionLabel>place batch order</SectionLabel>
            <BuyBox outcome={outcome} secs={secs} />
          </Card>
          <Disclosure label="indicative price · iev · imbalance">
            <CurrentBatchCard outcome={outcome} history={history} />
          </Disclosure>
        </div>
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────
// V4 — Outcome-first cards. Replaces the single chart with stacked
// per-outcome cards, each its own "row". Buy is inline. Right rail is a
// thin global confirm tray. Best for normies.
// ─────────────────────────────────────────────────────────────────────────
function V4OutcomeCards({ market, series, history, comments, secs }) {
  const [outcomeId, setOutcomeId] = React.useState(null);
  const [amount, setAmount] = React.useState('');
  const outcome = market.outcomes.find(o => o.id === outcomeId);
  return (
    <div style={{ width: PAGE_W, minHeight: PAGE_H, background:'var(--bg-1)', color:'var(--fg-1)', fontFamily:'var(--font-sans)' }}>
      <TopNav secs={secs}/>
      <MarketHeader market={market} />
      <div style={{ display:'grid', gridTemplateColumns:'1fr 320px', gap:24, padding:'20px 24px 36px' }}>
        <div style={{ display:'flex', flexDirection:'column', gap:24 }}>
          {/* Compact context chart */}
          <Card>
            <div style={{ display:'flex', justifyContent:'space-between', alignItems:'baseline', marginBottom:10 }}>
              <h3 style={{ fontFamily:'var(--font-sans)', fontSize:14, fontWeight:600, color:'var(--fg-1)', margin:0 }}>Probability over time</h3>
              <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)' }}>last 1 week</span>
            </div>
            <StackedAreaChart outcomes={market.outcomes} series={series} h={180} />
            <OutcomeLegend outcomes={market.outcomes} />
          </Card>

          {/* Per-outcome cards */}
          <div>
            <h3 style={{ fontFamily:'var(--font-sans)', fontSize:14, fontWeight:600, color:'var(--fg-1)', margin:'0 0 10px' }}>
              Outcomes <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', fontWeight:400 }}>· tap to bet</span>
            </h3>
            <div style={{ display:'flex', flexDirection:'column', gap:10 }}>
              {market.outcomes.map((o, i) => (
                <OutcomeRowCard key={o.id} outcome={o} series={series[i]} active={outcomeId===o.id} onPick={() => setOutcomeId(o.id)} />
              ))}
            </div>
          </div>

          <Card><RulesCard market={market} embedded /></Card>
          <Card><CommentsThread comments={comments} /></Card>
        </div>

        {/* RIGHT — sticky confirm tray */}
        <div>
          <div style={{ position:'sticky', top: 72, display:'flex', flexDirection:'column', gap:14 }}>
            <Card style={{ padding:'14px 16px' }}>
              <BatchCountdown secs={secs} size="sm"/>
            </Card>
            {!outcome && (
              <Card>
                <div style={{ textAlign:'center', padding:'24px 8px', color:'var(--fg-3)', fontFamily:'var(--font-sans)', fontSize:13 }}>
                  <div style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-4)', textTransform:'uppercase', letterSpacing:'0.04em', marginBottom:8 }}>no outcome selected</div>
                  Tap an outcome row to set up a batch order.
                </div>
              </Card>
            )}
            {outcome && (
              <>
                <Card>
                  <SectionLabel>order summary</SectionLabel>
                  <BuyBox outcome={outcome} secs={secs} />
                </Card>
                <Disclosure label="this batch · indicative">
                  <CurrentBatchCard outcome={outcome} history={history} />
                </Disclosure>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function OutcomeRowCard({ outcome, series, active, onPick }) {
  const accent = OUT_COLORS[outcome.id];
  return (
    <div onClick={onPick} style={{
      background: active ? 'var(--surface-2)' : 'var(--surface-1)',
      border:'1px solid', borderColor: active ? accent : 'var(--border-1)',
      borderRadius: 8, padding:'14px 18px', cursor:'pointer', transition:'border-color 120ms, background 120ms',
      display:'grid', gridTemplateColumns:'auto 1fr 200px 160px', gap:18, alignItems:'center',
    }}>
      <div style={{
        width:14, height:14, borderRadius:'50%', border:'1.5px solid', borderColor: active ? accent : 'var(--border-3)',
        display:'flex', alignItems:'center', justifyContent:'center',
      }}>{active && <div style={{ width:6, height:6, borderRadius:'50%', background: accent }}/>}</div>
      <div style={{ display:'flex', flexDirection:'column', gap:4 }}>
        <span style={{ fontFamily:'var(--font-sans)', fontSize:15, fontWeight:600, color:'var(--fg-1)' }}>{outcome.label}</span>
        <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)' }}>vol ${outcome.vol} · {outcome.traders} traders · liq ${outcome.liq}</span>
      </div>
      <Spark data={series} w={200} h={36} stroke={accent}/>
      <div style={{ display:'flex', flexDirection:'column', alignItems:'flex-end' }}>
        <div style={{ display:'flex', alignItems:'baseline', gap:8 }}>
          <span style={{ fontFamily:'var(--font-mono)', fontSize:24, color: accent, fontVariantNumeric:'tabular-nums' }}>{outcome.yes}¢</span>
          <span style={{ fontFamily:'var(--font-mono)', fontSize:11, color: outcome.delta24>=0?'var(--yes)':'var(--no)' }}>{outcome.delta24>=0?'+':''}{outcome.delta24}%</span>
        </div>
        <span style={{ fontFamily:'var(--font-mono)', fontSize:9.5, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em' }}>indicative · this batch</span>
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────
// V5 — Terminal layout. Bloomberg-grid for pros. Has Lite toggle that
// collapses everything back to V1's shape. Default = pro density.
// ─────────────────────────────────────────────────────────────────────────
function V5Terminal({ market, series, history, comments, secs }) {
  const [mode, setMode] = React.useState('pro');
  const [outcomeId, setOutcomeId] = React.useState(market.outcomes[0].id);
  const outcome = market.outcomes.find(o => o.id === outcomeId);
  if (mode === 'lite') return <V1Conventional market={market} series={series} history={history} comments={comments} secs={secs} />;
  return (
    <div style={{ width: PAGE_W, minHeight: PAGE_H, background:'var(--bg-0)', color:'var(--fg-1)', fontFamily:'var(--font-sans)' }}>
      <TopNav secs={secs}/>
      <MarketHeader market={market} rightSlot={<ModeToggle mode={mode} setMode={setMode}/>}/>
      <div style={{ padding:'14px 16px 28px', display:'flex', flexDirection:'column', gap:10 }}>
        {/* Top row: chart | batch | last-N */}
        <div style={{ display:'grid', gridTemplateColumns:'1fr 320px 320px', gap:10 }}>
          <TerminalPanel title="price · stacked probabilities" right={<RangeBar value="1W" onChange={()=>{}} />}>
            <StackedAreaChart outcomes={market.outcomes} series={series} h={260} />
            <OutcomeLegend outcomes={market.outcomes} />
          </TerminalPanel>
          <TerminalPanel title="this batch · live">
            <BatchCountdown secs={secs} size="sm" />
            <div style={{ height:10 }} />
            <CurrentBatchCard outcome={outcome} history={history} />
          </TerminalPanel>
          <TerminalPanel title="batch history · last n">
            <LastNStats history={history} />
          </TerminalPanel>
        </div>

        {/* Mid row: outcome ladder | book-like batch tape | order */}
        <div style={{ display:'grid', gridTemplateColumns:'1fr 1fr 360px', gap:10 }}>
          <TerminalPanel title="outcomes">
            <OutcomePicker outcomes={market.outcomes} value={outcomeId} onChange={setOutcomeId} />
          </TerminalPanel>
          <TerminalPanel title="batch tape · last 30">
            <Tape history={history.slice(0, 30).reverse()} />
            <div style={{ marginTop:8, fontFamily:'var(--font-mono)', fontSize:9.5, color:'var(--fg-3)', display:'flex', justifyContent:'space-between' }}>
              <span>oldest</span><span>now →</span>
            </div>
          </TerminalPanel>
          <TerminalPanel title="place batch order">
            <BuyBox outcome={outcome} secs={secs} />
          </TerminalPanel>
        </div>

        {/* Bottom row: rules | comments */}
        <div style={{ display:'grid', gridTemplateColumns:'360px 1fr', gap:10 }}>
          <TerminalPanel title="rules · oracle"><RulesCard market={market} embedded /></TerminalPanel>
          <TerminalPanel title="discussion"><CommentsThread comments={comments} /></TerminalPanel>
        </div>
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────
// Helpers used inside the variations
// ─────────────────────────────────────────────────────────────────────────
function Card({ children, style }) {
  return (
    <div style={{
      background:'var(--surface-1)', border:'1px solid var(--border-1)', borderRadius:8,
      padding:'16px 18px', boxShadow:'inset 0 1px 0 rgba(255,255,255,0.04)', ...style,
    }}>{children}</div>
  );
}

function SectionLabel({ children }) {
  return <div style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em', marginBottom:10 }}>{children}</div>;
}

function ModeToggle({ mode, setMode }) {
  return (
    <div style={{ display:'flex', alignItems:'center', gap:8, padding:'4px', background:'var(--bg-2)', border:'1px solid var(--border-1)', borderRadius:9999 }}>
      {[
        { v:'lite', label:'Lite', sub:'normie' },
        { v:'pro', label:'Pro', sub:'show batch internals' },
      ].map(o => (
        <button key={o.v} onClick={() => setMode(o.v)} title={o.sub} style={{
          padding:'5px 13px', border:0, borderRadius:9999, cursor:'pointer',
          background: mode===o.v ? 'var(--surface-2)' : 'transparent',
          color: mode===o.v ? 'var(--fg-1)' : 'var(--fg-3)',
          fontFamily:'var(--font-sans)', fontSize:11.5, fontWeight: mode===o.v ? 600 : 500,
        }}>{o.label}</button>
      ))}
    </div>
  );
}

function Disclosure({ label, children }) {
  const [open, setOpen] = React.useState(false);
  return (
    <div style={{ background:'var(--surface-1)', border:'1px solid var(--border-1)', borderRadius:8, overflow:'hidden' }}>
      <button onClick={() => setOpen(o => !o)} style={{
        width:'100%', display:'flex', justifyContent:'space-between', alignItems:'center',
        padding:'12px 16px', background:'transparent', border:0, cursor:'pointer',
        color:'var(--fg-2)', fontFamily:'var(--font-mono)', fontSize:10.5, textTransform:'uppercase', letterSpacing:'0.04em',
      }}>
        <span>{label}</span>
        <span style={{ color:'var(--fg-4)' }}>{open ? '–' : '+'}</span>
      </button>
      {open && <div style={{ padding:'4px 16px 16px' }}>{children}</div>}
    </div>
  );
}

function SubStat({ label, value, valueColor, secondary }) {
  return (
    <div style={{ display:'flex', justifyContent:'space-between', alignItems:'center' }}>
      <div style={{ display:'flex', flexDirection:'column' }}>
        <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em' }}>{label}</span>
        <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-4)', marginTop:1 }}>{secondary}</span>
      </div>
      <span style={{ fontFamily:'var(--font-mono)', fontSize:18, color: valueColor || 'var(--fg-1)', fontVariantNumeric:'tabular-nums' }}>{value}</span>
    </div>
  );
}

// Batch tape — visual blocks; one bar per batch, height = matched vol, color = imbalance
function Tape({ history }) {
  const max = Math.max(...history.map(h => parseFloat(h.volMatched)));
  return (
    <div style={{ display:'flex', alignItems:'flex-end', gap:2, height:80 }}>
      {history.map((b, i) => {
        const h = (parseFloat(b.volMatched) / max) * 70 + 4;
        const placedH = (parseFloat(b.volPlaced) / max) * 70 + 4;
        const color = b.imbalance==='YES' ? 'var(--yes)' : 'var(--no)';
        return (
          <div key={i} title={`batch #${b.i} · matched $${b.volMatched} · placed $${b.volPlaced}`}
            style={{ flex:'1 1 0', display:'flex', flexDirection:'column', justifyContent:'flex-end', position:'relative' }}>
            {/* unmatched (placed-matched) ghost */}
            <div style={{ height: placedH, background:'rgba(255,255,255,0.06)', borderRadius:'1px 1px 0 0' }} />
            {/* matched on top */}
            <div style={{ position:'absolute', bottom:0, left:0, right:0, height: h, background: color, opacity:0.8, borderRadius:'1px 1px 0 0' }}/>
          </div>
        );
      })}
    </div>
  );
}

function TerminalPanel({ title, right, children }) {
  return (
    <div style={{ background:'var(--surface-1)', border:'1px solid var(--border-1)', borderRadius:4, display:'flex', flexDirection:'column' }}>
      <div style={{ display:'flex', justifyContent:'space-between', alignItems:'center', padding:'8px 12px', borderBottom:'1px solid var(--border-1)' }}>
        <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em' }}>// {title}</span>
        {right}
      </div>
      <div style={{ padding:'12px 14px', flex:1 }}>{children}</div>
    </div>
  );
}

Object.assign(window, {
  V1Conventional, V2BatchTheater, V3OrderFlowTape, V4OutcomeCards, V5Terminal,
  PAGE_W, PAGE_H, getSeriesByOutcome,
});
