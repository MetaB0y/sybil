// Three portfolio variants. Each is a self-contained "page" rendered inside a DCArtboard.
//
// A · Classic equity-first  — hero number + sparkline equity curve + tabs of holdings.
// B · Research / terminal   — denser, methodology callouts, batch-emphasis, ASCII-feel rows.
// C · Two-column sticky     — left rail with identity + summary stats, right column = holdings.

// Density helper
function densityRow(d) { return d === 'dense' ? 44 : 56; }

// ── Holdings views (shared across variants, parametrized by density) ──────
function PositionsTable({ density='balanced' }) {
  const rowH = densityRow(density);
  return (
    <div style={{ background:'var(--surface-1)', border:'1px solid var(--border-1)', borderRadius:6, overflow:'hidden' }}>
      <div className="row" style={{
        display:'grid',
        gridTemplateColumns:'14px minmax(0, 1.6fr) 64px 80px 60px 60px 96px 100px 110px 80px',
        gap:14, padding:'0 18px', height:32, alignItems:'center',
        fontFamily:'var(--font-mono)', fontSize:9.5, textTransform:'uppercase', letterSpacing:'0.06em', color:'var(--fg-3)',
        borderBottom:'1px solid var(--border-1)',
      }}>
        <span/><span>Market</span><span>Side</span>
        <span style={{ textAlign:'right' }}>Shares</span>
        <span style={{ textAlign:'right' }}>Entry</span>
        <span style={{ textAlign:'right' }}>Mark</span>
        <span>7d</span>
        <span style={{ textAlign:'right' }}>Value</span>
        <span style={{ textAlign:'right' }}>Unrealized</span>
        <span style={{ textAlign:'right' }}>Resolves</span>
      </div>
      {OPEN_POSITIONS.map(p => {
        const isUp = p.pnl >= 0;
        const color = isUp ? 'var(--yes)' : 'var(--no)';
        return (
          <div key={p.id} className="row-hover" style={{
            display:'grid',
            gridTemplateColumns:'14px minmax(0, 1.6fr) 64px 80px 60px 60px 96px 100px 110px 80px',
            gap:14, padding:'0 18px', height:rowH, alignItems:'center',
            borderBottom:'1px solid var(--border-1)', cursor:'pointer',
            transition:'background var(--dur-fast) var(--ease-standard)',
          }}>
            <CategoryDot cat={p.category}/>
            <span style={{ fontFamily:'var(--font-sans)', fontSize:13, color:'var(--fg-1)', overflow:'hidden', textOverflow:'ellipsis', whiteSpace:'nowrap' }}>{p.title}</span>
            <SidePill side={p.side}/>
            <span style={{ fontFamily:'var(--font-mono)', fontSize:12, color:'var(--fg-2)', textAlign:'right', fontVariantNumeric:'tabular-nums' }}>{fmtShares(p.shares)}</span>
            <span style={{ fontFamily:'var(--font-mono)', fontSize:12, color:'var(--fg-3)', textAlign:'right', fontVariantNumeric:'tabular-nums' }}>{p.entry}¢</span>
            <span style={{ fontFamily:'var(--font-mono)', fontSize:12, color:'var(--fg-1)', textAlign:'right', fontVariantNumeric:'tabular-nums' }}>{p.mark}¢</span>
            <MiniSpark data={p.series} w={86} h={22}/>
            <span style={{ fontFamily:'var(--font-mono)', fontSize:12, color:'var(--fg-1)', textAlign:'right', fontVariantNumeric:'tabular-nums' }}>{fmtMoney(p.value)}</span>
            <div style={{ display:'flex', flexDirection:'column', gap:1, alignItems:'flex-end' }}>
              <span style={{ fontFamily:'var(--font-mono)', fontSize:12, color, fontVariantNumeric:'tabular-nums' }}>{fmtMoney(p.pnl, { sign:true })}</span>
              <span style={{ fontFamily:'var(--font-mono)', fontSize:9.5, color, fontVariantNumeric:'tabular-nums' }}>{fmtPct(p.pnlPct)}</span>
            </div>
            <span style={{ fontFamily:'var(--font-mono)', fontSize:11, color:'var(--fg-2)', textAlign:'right', fontVariantNumeric:'tabular-nums' }}>{p.resolves}<span style={{ color:'var(--fg-4)', marginLeft:6 }}>{p.horizonDays}d</span></span>
          </div>
        );
      })}
    </div>
  );
}

function PositionCards() {
  return (
    <div style={{ display:'grid', gridTemplateColumns:'repeat(auto-fill, minmax(290px, 1fr))', gap:12 }}>
      {OPEN_POSITIONS.map(p => {
        const isUp = p.pnl >= 0;
        const color = isUp ? 'var(--yes)' : 'var(--no)';
        return (
          <div key={p.id} style={{
            background:'var(--surface-1)', border:'1px solid var(--border-1)', borderRadius:6,
            padding:'14px 16px', display:'flex', flexDirection:'column', gap:12,
            boxShadow:'var(--shadow-inset-top)',
          }}>
            <div style={{ display:'flex', alignItems:'center', justifyContent:'space-between', gap:10 }}>
              <div style={{ display:'flex', alignItems:'center', gap:6 }}>
                <CategoryDot cat={p.category}/>
                <span style={{ fontFamily:'var(--font-mono)', fontSize:9.5, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em' }}>{p.category}</span>
              </div>
              <SidePill side={p.side}/>
            </div>
            <div style={{ fontFamily:'var(--font-sans)', fontSize:13, fontWeight:500, lineHeight:1.32, color:'var(--fg-1)', minHeight:34, textWrap:'pretty' }}>{p.title}</div>
            <MiniSpark data={p.series} w={260} h={32}/>
            <div style={{ display:'grid', gridTemplateColumns:'1fr 1fr', gap:10, paddingTop:6, borderTop:'1px solid var(--border-1)' }}>
              <div style={{ display:'flex', flexDirection:'column', gap:1 }}>
                <span className="eyebrow">Value</span>
                <span style={{ fontFamily:'var(--font-mono)', fontSize:14, color:'var(--fg-1)', fontVariantNumeric:'tabular-nums' }}>{fmtMoney(p.value)}</span>
              </div>
              <div style={{ display:'flex', flexDirection:'column', gap:1, alignItems:'flex-end' }}>
                <span className="eyebrow">Unrealized</span>
                <span style={{ fontFamily:'var(--font-mono)', fontSize:14, color, fontVariantNumeric:'tabular-nums' }}>{fmtMoney(p.pnl, { sign:true })}<span style={{ marginLeft:6, fontSize:10 }}>{fmtPct(p.pnlPct)}</span></span>
              </div>
            </div>
            <div style={{ display:'flex', alignItems:'center', justifyContent:'space-between', fontFamily:'var(--font-mono)', fontSize:9.5, color:'var(--fg-3)', textTransform:'uppercase', letterSpacing:'0.04em' }}>
              <span>{fmtShares(p.shares)} sh · {p.entry}¢ → {p.mark}¢</span>
              <span>{p.resolves}</span>
            </div>
          </div>
        );
      })}
    </div>
  );
}

function OpenOrders({ batchDetail='standard' }) {
  const cols = '14px minmax(0, 1.3fr) 56px 54px 118px 56px 84px 76px 100px 60px';
  return (
    <div style={{ background:'var(--surface-1)', border:'1px solid var(--border-1)', borderRadius:6, overflow:'hidden' }}>
      <div style={{
        display:'grid', gridTemplateColumns: cols,
        gap:14, padding:'0 18px', height:32, alignItems:'center',
        fontFamily:'var(--font-mono)', fontSize:9.5, textTransform:'uppercase', letterSpacing:'0.06em', color:'var(--fg-3)',
        borderBottom:'1px solid var(--border-1)',
      }}>
        <span/><span>Market</span><span>Action</span><span>Side</span>
        <span>Filled / Size</span>
        <span style={{ textAlign:'right' }}>Limit</span>
        <span style={{ textAlign:'right' }}>Value</span>
        <span>TIF</span>
        <span style={{ textAlign:'right' }}>{batchDetail==='minimal'?'Queued':'Clears in batch'}</span>
        <span/>
      </div>
      {OPEN_ORDERS.map(o => {
        const fillPct = Math.min(100, (o.filled / o.shares) * 100);
        const partial = o.filled > 0;
        return (
          <div key={o.id} className="row-hover" style={{
            display:'grid', gridTemplateColumns: cols,
            gap:14, padding:'0 18px', height:52, alignItems:'center',
            borderBottom:'1px solid var(--border-1)', transition:'background var(--dur-fast) var(--ease-standard)',
          }}>
            <CategoryDot cat={o.category}/>
            <span style={{ fontFamily:'var(--font-sans)', fontSize:13, color:'var(--fg-1)', overflow:'hidden', textOverflow:'ellipsis', whiteSpace:'nowrap' }}>{o.title}</span>
            <span style={{ fontFamily:'var(--font-mono)', fontSize:10.5, color: o.action==='BUY'?'var(--yes)':'var(--no)', textTransform:'uppercase', letterSpacing:'0.04em' }}>{o.action}</span>
            <SidePill side={o.side}/>

            {/* Filled / Size with progress bar */}
            <div style={{ display:'flex', flexDirection:'column', gap:4, minWidth:0 }}>
              <div style={{ display:'flex', alignItems:'baseline', gap:6, fontFamily:'var(--font-mono)', fontSize:11.5, fontVariantNumeric:'tabular-nums' }}>
                <span style={{ color: partial ? 'var(--accent)' : 'var(--fg-4)' }}>{fmtShares(o.filled)}</span>
                <span style={{ color:'var(--fg-4)' }}>/</span>
                <span style={{ color:'var(--fg-1)' }}>{fmtShares(o.shares)}</span>
                {partial && (
                  <span style={{ marginLeft:'auto', fontSize:9, color:'var(--accent)', textTransform:'uppercase', letterSpacing:'0.04em' }}>
                    {fillPct.toFixed(0)}%
                  </span>
                )}
              </div>
              <div style={{ height:2, background:'var(--border-1)', borderRadius:1, overflow:'hidden' }}>
                <div style={{ width: fillPct + '%', height:'100%', background: partial ? 'var(--accent)' : 'transparent' }}/>
              </div>
            </div>

            <span style={{ fontFamily:'var(--font-mono)', fontSize:12, color:'var(--fg-1)', textAlign:'right', fontVariantNumeric:'tabular-nums' }}>{o.limit}¢</span>
            <span style={{ fontFamily:'var(--font-mono)', fontSize:12, color:'var(--fg-1)', textAlign:'right', fontVariantNumeric:'tabular-nums' }}>{fmtMoney(o.value)}</span>

            {/* Time-in-force */}
            <div style={{ display:'flex', flexDirection:'column', gap:1 }}>
              <span style={{
                fontFamily:'var(--font-mono)', fontSize:10.5, fontWeight:500,
                color: o.tif === 'GTC' ? 'var(--accent)' : 'var(--fg-1)',
                textTransform:'uppercase', letterSpacing:'0.04em',
              }}>{o.tif}</span>
              {o.tifRemaining != null && o.tif !== '1 batch' && (
                <span style={{ fontFamily:'var(--font-mono)', fontSize:9, color:'var(--fg-4)', textTransform:'uppercase', letterSpacing:'0.04em' }}>
                  {o.tifRemaining} left
                </span>
              )}
              {o.tif === 'GTC' && (
                <span style={{ fontFamily:'var(--font-mono)', fontSize:9, color:'var(--fg-4)', textTransform:'uppercase', letterSpacing:'0.04em' }}>
                  till cancel
                </span>
              )}
            </div>

            <div style={{ display:'flex', flexDirection:'column', gap:1, alignItems:'flex-end' }}>
              {batchDetail==='minimal' ? (
                <span style={{ fontFamily:'var(--font-mono)', fontSize:11, color:'var(--fg-3)' }}>{o.queuedAgo}</span>
              ) : (
                <React.Fragment>
                  <span style={{ fontFamily:'var(--font-mono)', fontSize:11, color:'var(--accent)', fontVariantNumeric:'tabular-nums' }}>#{o.queuedFor.toLocaleString()}</span>
                  <span style={{ fontFamily:'var(--font-mono)', fontSize:9, color:'var(--fg-4)', textTransform:'uppercase', letterSpacing:'0.04em' }}>queued {o.queuedAgo}</span>
                </React.Fragment>
              )}
            </div>
            <button style={{
              background:'transparent', border:'1px solid var(--border-2)', color:'var(--fg-3)', cursor:'pointer',
              borderRadius:2, padding:'3px 8px', fontFamily:'var(--font-mono)', fontSize:9.5, textTransform:'uppercase', letterSpacing:'0.04em',
            }}>cancel</button>
          </div>
        );
      })}
    </div>
  );
}

function HistoryList() {
  return (
    <div style={{ background:'var(--surface-1)', border:'1px solid var(--border-1)', borderRadius:6, overflow:'hidden' }}>
      <div style={{
        display:'grid', gridTemplateColumns:'14px minmax(0, 1.7fr) 64px 80px 60px 60px 100px 110px 90px',
        gap:14, padding:'0 18px', height:32, alignItems:'center',
        fontFamily:'var(--font-mono)', fontSize:9.5, textTransform:'uppercase', letterSpacing:'0.06em', color:'var(--fg-3)',
        borderBottom:'1px solid var(--border-1)',
      }}>
        <span/><span>Market</span><span>Side</span>
        <span style={{ textAlign:'right' }}>Shares</span>
        <span style={{ textAlign:'right' }}>Entry</span>
        <span style={{ textAlign:'right' }}>Exit</span>
        <span style={{ textAlign:'right' }}>Realized</span>
        <span style={{ textAlign:'right' }}>Outcome</span>
        <span style={{ textAlign:'right' }}>Closed</span>
      </div>
      {CLOSED_POSITIONS.map(c => {
        const isUp = c.pnl >= 0;
        const color = isUp ? 'var(--yes)' : 'var(--no)';
        return (
          <div key={c.id} className="row-hover" style={{
            display:'grid', gridTemplateColumns:'14px minmax(0, 1.7fr) 64px 80px 60px 60px 100px 110px 90px',
            gap:14, padding:'0 18px', height:48, alignItems:'center',
            borderBottom:'1px solid var(--border-1)', transition:'background var(--dur-fast) var(--ease-standard)',
          }}>
            <CategoryDot cat={c.category}/>
            <span style={{ fontFamily:'var(--font-sans)', fontSize:13, color:'var(--fg-1)', overflow:'hidden', textOverflow:'ellipsis', whiteSpace:'nowrap' }}>{c.title}</span>
            <SidePill side={c.side}/>
            <span style={{ fontFamily:'var(--font-mono)', fontSize:12, color:'var(--fg-2)', textAlign:'right', fontVariantNumeric:'tabular-nums' }}>{fmtShares(c.shares)}</span>
            <span style={{ fontFamily:'var(--font-mono)', fontSize:12, color:'var(--fg-3)', textAlign:'right', fontVariantNumeric:'tabular-nums' }}>{c.entry}¢</span>
            <span style={{ fontFamily:'var(--font-mono)', fontSize:12, color:'var(--fg-1)', textAlign:'right', fontVariantNumeric:'tabular-nums' }}>{c.exit}¢</span>
            <div style={{ display:'flex', flexDirection:'column', gap:1, alignItems:'flex-end' }}>
              <span style={{ fontFamily:'var(--font-mono)', fontSize:12, color, fontVariantNumeric:'tabular-nums' }}>{fmtMoney(c.pnl, { sign:true })}</span>
              <span style={{ fontFamily:'var(--font-mono)', fontSize:9.5, color, fontVariantNumeric:'tabular-nums' }}>{fmtPct(c.pnlPct)}</span>
            </div>
            <span style={{ fontFamily:'var(--font-mono)', fontSize:9.5, textAlign:'right', textTransform:'uppercase', letterSpacing:'0.04em', color: c.outcome === 'resolved' ? 'var(--accent)' : 'var(--fg-3)' }}>{c.outcome}</span>
            <span style={{ fontFamily:'var(--font-mono)', fontSize:11, color:'var(--fg-3)', textAlign:'right', textTransform:'uppercase', letterSpacing:'0.04em' }}>{c.closedAgo}</span>
          </div>
        );
      })}
    </div>
  );
}

function ActivityList() {
  return (
    <div style={{ background:'var(--surface-1)', border:'1px solid var(--border-1)', borderRadius:6, overflow:'hidden' }}>
      {RECENT_FILLS.map((f, i) => (
        <div key={f.id} style={{
          display:'grid', gridTemplateColumns:'70px 60px 60px 60px minmax(0, 1fr) 80px 90px 100px',
          gap:14, padding:'10px 18px', alignItems:'center',
          borderBottom: i < RECENT_FILLS.length - 1 ? '1px solid var(--border-1)' : 0,
        }}>
          <span style={{ fontFamily:'var(--font-mono)', fontSize:9.5, padding:'2px 6px', borderRadius:2,
            color: f.kind === 'fill' ? 'var(--yes)' : 'var(--fg-3)',
            background: f.kind === 'fill' ? 'var(--yes-soft)' : 'rgba(255,255,255,0.04)',
            textTransform:'uppercase', letterSpacing:'0.04em', textAlign:'center', justifySelf:'start' }}>{f.kind === 'fill' ? 'filled' : 'cancelled'}</span>
          <span style={{ fontFamily:'var(--font-mono)', fontSize:10.5, color: f.action==='BUY'?'var(--yes)':'var(--no)', textTransform:'uppercase', letterSpacing:'0.04em' }}>{f.action}</span>
          <SidePill side={f.side}/>
          <span style={{ fontFamily:'var(--font-mono)', fontSize:12, color:'var(--fg-1)', fontVariantNumeric:'tabular-nums' }}>{fmtShares(f.shares)}</span>
          <span style={{ fontFamily:'var(--font-sans)', fontSize:12, color:'var(--fg-2)', overflow:'hidden', textOverflow:'ellipsis', whiteSpace:'nowrap' }}>{f.market}</span>
          <span style={{ fontFamily:'var(--font-mono)', fontSize:12, color:'var(--fg-1)', textAlign:'right', fontVariantNumeric:'tabular-nums' }}>@ {f.price}¢</span>
          <span style={{ fontFamily:'var(--font-mono)', fontSize:12, color:'var(--fg-1)', textAlign:'right', fontVariantNumeric:'tabular-nums' }}>{fmtMoney(f.amount)}</span>
          <div style={{ display:'flex', flexDirection:'column', gap:1, alignItems:'flex-end' }}>
            <span style={{ fontFamily:'var(--font-mono)', fontSize:10.5, color:'var(--accent)', fontVariantNumeric:'tabular-nums' }}>#{f.batch.toLocaleString()}</span>
            <span style={{ fontFamily:'var(--font-mono)', fontSize:9, color:'var(--fg-4)', textTransform:'uppercase', letterSpacing:'0.04em' }}>{f.ago}</span>
          </div>
        </div>
      ))}
    </div>
  );
}

// Tabbed holdings shell
function HoldingsTabs({ tab, setTab, layout, density, batchDetail, showActivity=true }) {
  const tabs = [
    { id:'positions', label:'Positions', count: OPEN_POSITIONS.length },
    { id:'orders',    label:'Open orders', count: OPEN_ORDERS.length },
    { id:'history',   label:'History',   count: CLOSED_POSITIONS.length },
  ];
  if (showActivity) tabs.push({ id:'activity', label:'Activity', count: RECENT_FILLS.length });
  return (
    <section>
      <div style={{ display:'flex', alignItems:'center', justifyContent:'space-between', borderBottom:'1px solid var(--border-1)', marginBottom:14 }}>
        <div style={{ display:'flex', gap:0 }}>
          {tabs.map(tt => (
            <button key={tt.id} onClick={() => setTab(tt.id)} className={'tab-btn' + (tab === tt.id ? ' active':'')}>
              {tt.label}<span style={{ color:'var(--fg-4)', fontFamily:'var(--font-mono)', marginLeft:5 }}>{tt.count}</span>
            </button>
          ))}
        </div>
        <div style={{ display:'flex', alignItems:'center', gap:10 }}>
          <span className="anno">
            {tab==='positions' && 'open positions · marked at last batch'}
            {tab==='orders'    && 'queued for next clear'}
            {tab==='history'   && 'closed · realized P&L'}
            {tab==='activity'  && 'recent fills and cancels'}
          </span>
          <button style={{
            background:'transparent', border:'1px solid var(--border-2)', borderRadius:3, padding:'4px 9px',
            color:'var(--fg-2)', fontFamily:'var(--font-mono)', fontSize:10, cursor:'pointer',
            textTransform:'uppercase', letterSpacing:'0.04em',
          }}>export →</button>
        </div>
      </div>
      {tab==='positions' && (layout==='cards' ? <PositionCards/> : <PositionsTable density={density}/>)}
      {tab==='orders'    && <OpenOrders batchDetail={batchDetail}/>}
      {tab==='history'   && <HistoryList/>}
      {tab==='activity'  && <ActivityList/>}
    </section>
  );
}

// ─────────────────────────────────────────────────────────────────────────
// VARIANT A · Classic equity-first
// ─────────────────────────────────────────────────────────────────────────
function VariantClassic({ t }) {
  const [tab, setTab] = React.useState('positions');
  const [range, setRange] = React.useState('all');
  const p = PORTFOLIO;
  const delta = { '24h':{v:p.pnl24h,pct:p.pnlPct24h}, '7d':{v:p.pnl7d,pct:p.pnlPct7d}, '30d':{v:p.pnl30d,pct:p.pnlPct30d}, 'all':{v:p.totalPnL,pct:p.pnlPct} }[range];
  const isUp = delta.v >= 0;
  const upClr = 'var(--yes)';
  return (
    <div className="pf-shell">
      <GlobalNav/>
      <div style={{ padding:'18px 24px 6px', display:'flex', alignItems:'baseline', gap:14 }}>
        <h1 style={{ fontFamily:'var(--font-sans)', fontSize:20, fontWeight:600, letterSpacing:'-0.01em', margin:0 }}>Portfolio</h1>
        <span className="anno">positions · orders · history for {TRADER.short}</span>
      </div>

      <section style={{ padding:'14px 24px 22px', borderBottom:'1px solid var(--border-1)' }}>
        <div style={{ display:'flex', alignItems:'center', justifyContent:'space-between', gap:24, paddingBottom:18 }}>
          <IdentityStrip/>
          <RangePicker range={range} setRange={setRange}/>
        </div>
        <div style={{ display:'grid', gridTemplateColumns: t.showEquityChart ? 'minmax(0, 0.85fr) minmax(0, 1.15fr)' : '1fr', gap:48, alignItems:'start' }}>
          <div style={{ display:'flex', flexDirection:'column', gap:6 }}>
            <span className="eyebrow">Portfolio value</span>
            <div style={{ fontFamily:'var(--font-sans)', fontWeight:600, fontSize:'clamp(46px, 4.6vw, 64px)', lineHeight:0.95, letterSpacing:'-0.02em', color:'var(--fg-1)', fontVariantNumeric:'tabular-nums' }}>
              {fmtMoney(p.totalValue)}
            </div>
            <div style={{ display:'flex', alignItems:'baseline', gap:14, paddingTop:6 }}>
              <span style={{ fontFamily:'var(--font-mono)', fontSize:16, color: isUp?upClr:'var(--no)', fontVariantNumeric:'tabular-nums' }}>
                {isUp?'▲':'▼'} {fmtMoney(Math.abs(delta.v), { sign:false })}
              </span>
              <span style={{ fontFamily:'var(--font-mono)', fontSize:13, color: isUp?upClr:'var(--no)', fontVariantNumeric:'tabular-nums' }}>{fmtPct(delta.pct)}</span>
              <span className="eyebrow">{range==='all' ? 'since first deposit' : 'last ' + range}</span>
            </div>
            <div style={{ display:'grid', gridTemplateColumns:'1fr 1fr', columnGap:32, rowGap:18, paddingTop:22, marginTop:6, borderTop:'1px solid var(--border-1)' }}>
              <Kv label="Positions value" value={fmtMoney(p.positionsValue)} sub={`${p.openPositions} open`} size={20}/>
              <Kv label="Cash" value={fmtMoney(p.cash)} sub="available" size={20}/>
              <Kv label="Unrealized P&L" value={fmtMoney(p.unrealizedPnL,{sign:true})} accent={p.unrealizedPnL>=0?upClr:'var(--no)'} sub="open positions" size={20}/>
              <Kv label="Realized P&L" value={fmtMoney(p.realizedPnL,{sign:true})} accent={p.realizedPnL>=0?upClr:'var(--no)'} sub={`${p.closedTrades} trades`} size={20}/>
            </div>
          </div>
          {t.showEquityChart && (
            <div style={{ display:'flex', flexDirection:'column', gap:10 }}>
              <div style={{ display:'flex', alignItems:'baseline', gap:10 }}>
                <span className="eyebrow">Equity curve</span>
                <span className="anno">marked-to-batch · dashed = net deposits</span>
              </div>
              <div style={{ background:'var(--surface-1)', border:'1px solid var(--border-1)', borderRadius:6, padding:'14px 12px 6px' }}>
                <EquityChart data={EQUITY_CURVE} w={620} h={150} range={range==='all'?'all':range}/>
              </div>
            </div>
          )}
        </div>
      </section>

      {t.showAllocation && (
        <section style={{ padding:'18px 24px 4px' }}>
          <AllocationStrip/>
        </section>
      )}

      <div style={{ padding:'10px 24px 36px' }}>
        <HoldingsTabs tab={tab} setTab={setTab}
          layout={t.positionsLayout} density={t.density} batchDetail={t.batchDetail} />
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────
// VARIANT B · Research / terminal
// Methodology callouts, bordered ASCII feel, no large hero.
// ─────────────────────────────────────────────────────────────────────────
function VariantTerminal({ t }) {
  const [tab, setTab] = React.useState('positions');
  const [range, setRange] = React.useState('all');
  const p = PORTFOLIO;
  const isUp = p.totalPnL >= 0;
  return (
    <div className="pf-shell">
      <GlobalNav/>
      <div style={{ padding:'16px 26px 0' }}>
        <div style={{ display:'flex', alignItems:'baseline', gap:14 }}>
          <span className="eyebrow" style={{ color:'var(--accent)' }}>portfolio · v0.3.1</span>
          <span className="anno">marked at batch <span style={{ color:'var(--fg-1)' }}>#9412</span> · {TRADER.short}</span>
        </div>
        <h1 style={{ fontFamily:'var(--font-display)', fontSize:32, fontWeight:600, letterSpacing:'-0.02em', margin:'8px 0 4px', color:'var(--fg-1)' }}>
          {fmtMoney(p.totalValue)} <span style={{ fontFamily:'var(--font-mono)', fontSize:14, color: isUp?'var(--yes)':'var(--no)', marginLeft:10, fontWeight:500 }}>{fmtMoney(p.totalPnL,{sign:true})} · {fmtPct(p.pnlPct)}</span>
        </h1>
        <div className="anno" style={{ paddingTop:2 }}>
          // {p.openPositions} open positions · {p.openOrders} orders queued · {p.closedTrades} closed trades · {p.winRate.toFixed(1)}% win rate
        </div>
      </div>

      {/* Stats strip — 5 cells, terminal style */}
      <div style={{ margin:'18px 26px 0', border:'1px solid var(--border-1)', borderRadius:6, background:'var(--surface-1)', display:'grid', gridTemplateColumns:'repeat(5, 1fr)' }}>
        {[
          { l:'Positions value', v: fmtMoney(p.positionsValue), s: `${p.openPositions} open` },
          { l:'Cash',            v: fmtMoney(p.cash),           s: 'available' },
          { l:'Unrealized',      v: fmtMoney(p.unrealizedPnL,{sign:true}), c: p.unrealizedPnL>=0?'var(--yes)':'var(--no)', s: 'open' },
          { l:'Realized',        v: fmtMoney(p.realizedPnL,{sign:true}), c: p.realizedPnL>=0?'var(--yes)':'var(--no)', s: `${p.closedTrades} trades` },
          { l:'Open orders',     v: p.openOrders,               s: 'next batch #9413' },
        ].map((k,i) => (
          <div key={k.l} style={{ padding:'14px 16px', borderRight: i<4 ? '1px solid var(--border-1)' : 0 }}>
            <div className="eyebrow" style={{ marginBottom:6 }}>{k.l}</div>
            <div style={{ fontFamily:'var(--font-mono)', fontSize:20, color: k.c || 'var(--fg-1)', letterSpacing:'-0.01em', fontVariantNumeric:'tabular-nums', lineHeight:1 }}>{k.v}</div>
            <div className="anno" style={{ marginTop:4 }}>{k.s}</div>
          </div>
        ))}
      </div>

      {/* Equity chart, sparkline-style */}
      {t.showEquityChart && (
        <div style={{ margin:'14px 26px 0' }}>
          <div style={{ display:'flex', alignItems:'baseline', gap:14, paddingBottom:8 }}>
            <span className="eyebrow">Equity</span>
            <span className="anno">142 d · 3 deposits · cyan = portfolio · dashed = net deposits baseline</span>
            <div style={{ marginLeft:'auto' }}><RangePicker range={range} setRange={setRange}/></div>
          </div>
          <div style={{ background:'var(--surface-1)', border:'1px solid var(--border-1)', borderRadius:6, padding:'12px 14px 4px' }}>
            <EquityChart data={EQUITY_CURVE} w={1280} h={120} range={range==='all'?'all':range}/>
          </div>
        </div>
      )}

      {/* Methodology callout (only for FBA-curious) */}
      {t.batchDetail !== 'minimal' && (
        <details style={{ margin:'14px 26px 0', background:'var(--accent-faint)', border:'1px solid var(--accent-soft)', borderRadius:6, padding:'10px 14px' }}>
          <summary style={{ cursor:'pointer', listStyle:'none', display:'flex', alignItems:'center', gap:8 }}>
            <span style={{ fontFamily:'var(--font-mono)', fontSize:11, color:'var(--accent)', textTransform:'uppercase', letterSpacing:'0.06em' }}>// methodology</span>
            <span className="anno" style={{ flex:1 }}>how marks are computed · LP / market-maker views</span>
            <span style={{ fontFamily:'var(--font-mono)', fontSize:10, color:'var(--fg-3)' }}>expand</span>
          </summary>
          <div style={{ paddingTop:10, fontFamily:'var(--font-mono)', fontSize:11, color:'var(--fg-2)', lineHeight:1.6 }}>
            Marks reflect the uniform clearing price at the most recent batch (#9412). Unrealized P&amp;L is (mark − entry) · shares, gross of fees. Realized P&amp;L includes resolved markets credited at $0 / $1 per share. LP views (depth, inventory, imbalance per batch) are hidden — toggle "market-maker" in account settings to enable.
          </div>
        </details>
      )}

      {t.showAllocation && (
        <div style={{ margin:'18px 26px 0' }}>
          <AllocationStrip/>
        </div>
      )}

      <div style={{ padding:'18px 26px 36px' }}>
        <HoldingsTabs tab={tab} setTab={setTab}
          layout={t.positionsLayout} density={t.density} batchDetail={t.batchDetail} />
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────
// VARIANT C · Two-column sticky
// Left rail: identity, summary stats, allocation. Right: holdings.
// ─────────────────────────────────────────────────────────────────────────
function VariantTwoCol({ t }) {
  const [tab, setTab] = React.useState('positions');
  const [range, setRange] = React.useState('all');
  const p = PORTFOLIO;
  const isUp = p.totalPnL >= 0;
  return (
    <div className="pf-shell">
      <GlobalNav/>
      <div style={{ display:'grid', gridTemplateColumns:'320px 1fr', gap:0, alignItems:'start' }}>
        {/* Left rail */}
        <aside style={{ borderRight:'1px solid var(--border-1)', padding:'22px 22px', display:'flex', flexDirection:'column', gap:22, minHeight:'calc(100vh - 52px)' }}>
          <IdentityStrip size="sm"/>
          <div>
            <span className="eyebrow">Portfolio value</span>
            <div style={{ fontFamily:'var(--font-sans)', fontSize:40, fontWeight:600, letterSpacing:'-0.02em', lineHeight:1, marginTop:6, fontVariantNumeric:'tabular-nums' }}>{fmtMoney(p.totalValue)}</div>
            <div style={{ display:'flex', alignItems:'baseline', gap:8, paddingTop:6 }}>
              <span style={{ fontFamily:'var(--font-mono)', fontSize:13, color: isUp?'var(--yes)':'var(--no)', fontVariantNumeric:'tabular-nums' }}>
                {isUp?'▲':'▼'} {fmtMoney(Math.abs(p.totalPnL),{sign:false})}
              </span>
              <span style={{ fontFamily:'var(--font-mono)', fontSize:12, color: isUp?'var(--yes)':'var(--no)', fontVariantNumeric:'tabular-nums' }}>{fmtPct(p.pnlPct)}</span>
              <span className="eyebrow" style={{ marginLeft:'auto' }}>since deposit</span>
            </div>
          </div>
          {t.showEquityChart && (
            <div>
              <div style={{ display:'flex', justifyContent:'space-between', alignItems:'baseline', paddingBottom:6 }}>
                <span className="eyebrow">Equity</span>
                <RangePicker range={range} setRange={setRange} sizes={['7d','30d','all']}/>
              </div>
              <div style={{ background:'var(--surface-1)', border:'1px solid var(--border-1)', borderRadius:6, padding:'10px 8px 2px' }}>
                <EquityChart data={EQUITY_CURVE} w={272} h={92} range={range} showAxes={false} showDeposits={false}/>
              </div>
            </div>
          )}
          <div style={{ display:'grid', gridTemplateColumns:'1fr 1fr', gap:16 }}>
            <Kv label="Positions" value={fmtMoney(p.positionsValue)} sub={`${p.openPositions} open`} size={16}/>
            <Kv label="Cash" value={fmtMoney(p.cash)} sub="available" size={16}/>
            <Kv label="Unrealized" value={fmtMoney(p.unrealizedPnL,{sign:true})} accent={p.unrealizedPnL>=0?'var(--yes)':'var(--no)'} sub="open" size={16}/>
            <Kv label="Realized" value={fmtMoney(p.realizedPnL,{sign:true})} accent={p.realizedPnL>=0?'var(--yes)':'var(--no)'} sub={`${p.closedTrades} trades`} size={16}/>
            <Kv label="Open orders" value={p.openOrders} sub="queued" size={16}/>
            <Kv label="Net deposits" value={fmtMoney(p.netDeposits)} sub="lifetime" size={16}/>
          </div>
          {t.showAllocation && (
            <div>
              <span className="eyebrow" style={{ marginBottom:8, display:'block' }}>Allocation</span>
              <AllocationStrip compact/>
            </div>
          )}
          <div style={{ marginTop:'auto', paddingTop:14, borderTop:'1px solid var(--border-1)', display:'flex', flexDirection:'column', gap:6 }}>
            <span className="anno">batch #9412 · clears next at 14:30 UTC</span>
            <div style={{ display:'flex', gap:6 }}>
              <button style={{ flex:1, background:'var(--accent)', border:0, color:'var(--fg-on-accent)', padding:'8px 10px', borderRadius:3, fontFamily:'var(--font-sans)', fontSize:12, fontWeight:600, cursor:'pointer' }}>deposit</button>
              <button style={{ flex:1, background:'transparent', border:'1px solid var(--border-2)', color:'var(--fg-1)', padding:'8px 10px', borderRadius:3, fontFamily:'var(--font-sans)', fontSize:12, fontWeight:500, cursor:'pointer' }}>withdraw</button>
            </div>
          </div>
        </aside>

        {/* Right column */}
        <main style={{ padding:'22px 26px 36px' }}>
          <div style={{ display:'flex', alignItems:'baseline', gap:14, paddingBottom:14 }}>
            <h1 style={{ fontFamily:'var(--font-sans)', fontSize:20, fontWeight:600, letterSpacing:'-0.01em', margin:0 }}>Holdings</h1>
            <span className="anno">7 open · 4 queued · 38 closed</span>
          </div>
          <Collapsible title="Positions" anno={`${OPEN_POSITIONS.length} open · marked at last batch`} defaultOpen>
            {t.positionsLayout === 'cards' ? <PositionCards/> : <PositionsTable density={t.density}/>}
          </Collapsible>
          <Collapsible title="Open orders" anno={`${OPEN_ORDERS.length} queued for batch #9413`} defaultOpen>
            <OpenOrders batchDetail={t.batchDetail}/>
          </Collapsible>
          <Collapsible title="History" anno={`${CLOSED_POSITIONS.length} closed positions · realized P&L`} defaultOpen={false}>
            <HistoryList/>
          </Collapsible>
          <Collapsible title="Activity" anno={`recent fills and cancels · ${RECENT_FILLS.length} entries`} defaultOpen={false}>
            <ActivityList/>
          </Collapsible>
        </main>
      </div>
    </div>
  );
}

Object.assign(window, {
  PositionsTable, PositionCards, OpenOrders, HistoryList, ActivityList, HoldingsTabs,
  VariantClassic, VariantTerminal, VariantTwoCol,
});
