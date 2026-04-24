import React, { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  createAccount,
  createWizardDraft,
  discover,
  getState,
  importSources,
  nanosPct,
  pct,
  publishWizardDraft,
  proposeTrade,
  quoteOnce,
  searchExplorer,
  seedDemo,
  submitTrade,
} from "./api";
import type { DemoState, Formula, Instrument, SearchResult, TradeProposal, WizardDraft } from "./types";
import "./styles.css";

function App() {
  const [state, setState] = useState<DemoState | null>(null);
  const [selectedId, setSelectedId] = useState("");
  const [query, setQuery] = useState("I want a basket around 2028 election outcomes and macro conditions.");
  const [agentAnswer, setAgentAnswer] = useState("");
  const [rankedIds, setRankedIds] = useState<string[]>([]);
  const [proposal, setProposal] = useState<TradeProposal | null>(null);
  const [draftPrompt, setDraftPrompt] = useState("ETH between 3000 and 6000 by end of 2026.");
  const [draft, setDraft] = useState<WizardDraft | null>(null);
  const [explorerQuery, setExplorerQuery] = useState("ETH");
  const [domain, setDomain] = useState("");
  const [objectKind, setObjectKind] = useState("");
  const [measurementKind, setMeasurementKind] = useState("");
  const [measurementId, setMeasurementId] = useState("");
  const [predicateOp, setPredicateOp] = useState("");
  const [showDevControls, setShowDevControls] = useState(false);
  const [searchResult, setSearchResult] = useState<SearchResult | null>(null);
  const [accountId, setAccountId] = useState<number | null>(() => {
    const raw = localStorage.getItem("compositionDemoAccount");
    return raw ? Number(raw) : null;
  });
  const [busy, setBusy] = useState<string | null>(null);
  const [toast, setToast] = useState("");

  async function refresh() {
    setState(await getState());
  }

  useEffect(() => {
    refresh().catch((e) => setToast(String(e)));
    const timer = setInterval(() => refresh().catch(() => undefined), 4000);
    return () => clearInterval(timer);
  }, []);

  const instruments = state?.instruments || [];
  const measurements = state?.measurements || [];
  const allObjects = [...measurements, ...instruments] as Instrument[];
  const selected =
    allObjects.find((item) => item.id === selectedId) ||
    searchResult?.items[0] ||
    instruments.find((item) => item.kind === "proposition" || item.kind === "composition") ||
    instruments[0];
  const propositions = instruments.filter((item) => item.kind === "proposition" || item.kind === "composition");
  const conditions = instruments.filter((item) => item.kind === "condition" || item.kind === "atom");
  const selectedEdges = (state?.implication_edges || []).filter((edge) => edge.from === selected?.id || edge.to === selected?.id);

  const ranked = useMemo(() => {
    if (!rankedIds.length) return searchResult?.items.slice(0, 8) || propositions.slice(0, 8);
    const byId = new Map(instruments.map((item) => [item.id, item]));
    return rankedIds.map((id) => byId.get(id)).filter(Boolean) as Instrument[];
  }, [rankedIds, instruments, searchResult, propositions]);

  useEffect(() => {
    const timer = setTimeout(() => {
      searchExplorer({
        query: explorerQuery,
        domain,
        object_kind: objectKind,
        measurement_kind: measurementKind,
        measurement_id: measurementId,
        predicate_op: predicateOp,
        limit: 80,
      })
        .then((result) => {
          setSearchResult(result);
          if (!selectedId && result.items[0]) setSelectedId(result.items[0].id);
        })
        .catch(() => undefined);
    }, 220);
    return () => clearTimeout(timer);
  }, [explorerQuery, domain, objectKind, measurementKind, measurementId, predicateOp, selectedId]);

  async function runAgent() {
    setBusy("agent");
    try {
      const result = await discover(query);
      setAgentAnswer(result.answer);
      setRankedIds(result.ranked_ids);
      if (result.recommendation_id) setSelectedId(result.recommendation_id);
    } catch (e) {
      setToast(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function runSeed() {
    setBusy("seed");
    try {
      setState(await seedDemo());
      setToast("Seeded graph markets");
    } catch (e) {
      setToast(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function runImport() {
    setBusy("import");
    try {
      setState(await importSources(false, 110));
      setToast("Built measurement graph + source aliases");
    } catch (e) {
      setToast(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function runQuote() {
    setBusy("quote");
    try {
      const result = await quoteOnce();
      setToast(`Submitted ${result.orders} MM quote orders`);
      await refresh();
    } catch (e) {
      setToast(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function createDemoAccount() {
    setBusy("account");
    try {
      const id = await createAccount(500);
      localStorage.setItem("compositionDemoAccount", String(id));
      setAccountId(id);
      setToast(`Created demo account #${id}`);
    } catch (e) {
      setToast(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function askTrade(side?: string) {
    if (!selected) return;
    setBusy("trade");
    try {
      setProposal(await proposeTrade({ instrument_id: selected.id, intent: query, side }));
    } catch (e) {
      setToast(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function confirmTrade() {
    if (!proposal || !accountId) return;
    setBusy("confirm");
    try {
      await submitTrade(accountId, proposal);
      setToast("Trade submitted to Sybil");
      await refresh();
    } catch (e) {
      setToast(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function makeDraft() {
    setBusy("draft");
    try {
      setDraft(await createWizardDraft(draftPrompt));
    } catch (e) {
      setToast(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function approveDraft() {
    if (!draft) return;
    setBusy("approve");
    try {
      const next = await publishWizardDraft(draft);
      setState(next);
      setSelectedId(next.instruments[next.instruments.length - 1].id);
      setDraft(null);
      setToast("Published proposition market");
    } catch (e) {
      setToast(String(e));
    } finally {
      setBusy(null);
    }
  }

  return (
    <main>
      <div className="ambient ambient-a" />
      <div className="ambient ambient-b" />
      {toast && <button className="toast" onClick={() => setToast("")}>{toast}</button>}
      <header className="hero">
        <div>
          <div className="eyebrow">Sybil composition engine MVP</div>
          <h1>Trade the definition, not a vague headline.</h1>
          <p>
            A measurement-first UI for creating predicate markets, composing conditions, and checking no-arb relationships before liquidity follows.
          </p>
        </div>
        <div className="hero-actions">
          <button className="primary" onClick={makeDraft} disabled={!!busy}>{busy === "draft" ? "Drafting..." : "Start from prompt"}</button>
          <button onClick={() => setShowDevControls((value) => !value)}>Demo controls</button>
        </div>
      </header>
      {showDevControls && (
        <section className="dev-controls">
          <button onClick={runImport} disabled={!!busy}>{busy === "import" ? "Building..." : "Rebuild demo graph"}</button>
          <button onClick={runSeed} disabled={!!busy}>{busy === "seed" ? "Creating..." : "Create Sybil markets"}</button>
          <button onClick={runQuote} disabled={!!busy}>Submit demo quotes</button>
          <span>These are local demo maintenance actions. Normal market creation starts in the wizard.</span>
        </section>
      )}

      <section className="stats-strip">
        <Metric label="Measurements" help="Observable variables such as ETH/USD spot, US unemployment, or an NBA stat." value={state?.instrument_counts?.measurements ?? measurements.length} />
        <Metric label="Conditions" help="One yes/no statement about one measurement, such as ETH > 6000." value={state?.instrument_counts?.conditions ?? conditions.length} />
        <Metric label="Definitions" help="A tradable formula made from conditions. This used to be labeled proposition." value={state?.instrument_counts?.propositions ?? propositions.length} />
        <Metric label="Live markets" help="Definitions or conditions already created in sybil-api and ready for quotes/trades." value={state?.instrument_counts?.seeded ?? 0} />
      </section>

      <section className="shell">
        <aside className="panel agent-panel">
          <div className="panel-label">Discovery Agent</div>
          <textarea value={query} onChange={(e) => setQuery(e.target.value)} />
          <button className="primary" onClick={runAgent} disabled={!!busy}>
            {busy === "agent" ? "Thinking..." : "Find the right proposition"}
          </button>
          <div className="agent-answer">{agentAnswer || "Ask in natural language. The agent ranks definitions and explains tradeoffs."}</div>
          <div className="rank-list">
            {ranked.slice(0, 4).map((item) => (
              <button
                key={item.id}
                className={item.id === selected?.id ? "rank-card active" : "rank-card"}
                onClick={() => setSelectedId(item.id)}
              >
                <span>{item.short_name}</span>
                <strong>{nanosPct(item.market?.yes_price_nanos) || pct(item.model_value)}</strong>
              </button>
            ))}
          </div>

          <div className="creator">
            <div className="panel-label">Market Creation Wizard</div>
            <textarea value={draftPrompt} onChange={(e) => setDraftPrompt(e.target.value)} />
            <button onClick={makeDraft} disabled={!!busy}>{busy === "draft" ? "Drafting..." : "Draft proposition"}</button>
            {draft && (
              <div className="draft">
                <h3>{draft.short_name || draft.title}</h3>
                <p>{draft.description}</p>
                <FormulaView formula={draft.formula || null} instruments={instruments} />
                {draft.validation && (
                  <div className="validation">
                    <span>{draft.validation.valid ? "Valid formula" : draft.validation.errors.join("; ")}</span>
                    {draft.validation.duplicate && <span>Duplicate proposition exists</span>}
                    {draft.validation.warnings?.map((warning) => <span key={warning}>{warning}</span>)}
                  </div>
                )}
                <button className="primary" onClick={approveDraft} disabled={!!busy}>
                  Publish and create market
                </button>
              </div>
            )}
          </div>
        </aside>

        <section className="panel main-panel">
          {selected ? (
            <>
              <div className="instrument-header">
                <div>
                  <div className="panel-label">{displayKind(selected)}</div>
                  <h2>{selected.title}</h2>
                  <p>{selected.description}</p>
                </div>
                <div className="price-orb" title="Estimated probability before live trading. Live market price appears here after quotes clear.">
                  <span>{selected.market ? "MARKET" : "ESTIMATE"}</span>
                  <strong>{selected.object_kind === "measurement" ? "input" : nanosPct(selected.market?.yes_price_nanos) || pct(selected.model_value)}</strong>
                </div>
              </div>

              <div className="metrics">
                <Metric label="Live market" help="The sybil-api market id. If absent, this is only a definition in the demo registry." value={selected.market_id ?? "not created"} />
                <Metric label="Probability" help="Demo fair-value estimate used for initial quoting. It is not an oracle result." value={selected.object_kind === "measurement" ? "-" : pct(selected.model_value ?? selected.fair_value)} />
                <Metric label="Object" help="Measurement = input variable. Condition = yes/no predicate. Market definition = formula over conditions." value={displayKind(selected)} />
                <Metric label="Volume" help="Matched volume reported by sybil-api for the live market." value={`$${Math.round((selected.market?.volume_nanos || 0) / 1_000_000_000)}`} />
              </div>

              <div className="metadata-grid core-grid">
                <Meta label="Domain" help="Topic area used for browsing." value={selected.domain || "-"} />
                <Meta label="Measurement" help="The observable input variable behind this condition." value={selected.measurement?.subject || selected.subject || selected.measurement_kind || "-"} />
                <Meta label="Window" help="When the measurement is observed." value={selected.time_window || selected.aggregation_semantics || "-"} />
                <Meta label="Predicate" help="The yes/no rule applied to a measurement." value={predicateText(selected)} />
                <Meta label="Data source" help="Where the demo expects the measurement to come from." value={sourceText(selected)} />
                <Meta label="Market state" help="Created means sybil-api has a tradable market. Draft means only the demo registry knows about it." value={selected.market?.status || (selected.market_id ? "created" : "draft only")} />
              </div>

              {selected.object_kind !== "measurement" && (
                <div className="formula-card">
                  <div className="panel-label">Market Definition</div>
                  <FormulaView formula={selected.formula || null} instruments={instruments} />
                </div>
              )}
              {selectedEdges.length > 0 && (
                <div className="formula-card">
                  <div className="panel-label">No-Arb Relationships</div>
                  {selectedEdges.slice(0, 6).map((edge) => (
                    <div className="edge-row" key={`${edge.from}-${edge.to}`}>
                      <span>{explainEdge(edge, instruments)}</span>
                      <strong>{edge.no_arb}</strong>
                    </div>
                  ))}
                </div>
              )}
              <details className="technical-details">
                <summary>Technical details</summary>
                <div className="metadata-grid">
                  <Meta label="Quality" help="seed means curated demo data; source_matched means an external market alias matched it." value={qualityText(selected.quality)} />
                  <Meta label="Registry source" help="graph means this came from the curated demo ontology, not directly from a source-market title." value={sourceText(selected)} />
                  <Meta label="Resolver" help="Future oracle primitive that would resolve this object." value={selected.resolver_primitive || selected.oracle_path || "-"} />
                  <Meta label="Canonical key" help="Structural identity used to deduplicate equivalent objects." value={selected.canonical_key || selected.id} long />
                  <Meta label="Params" help="Raw predicate/formula metadata." value={selected.params ? JSON.stringify(selected.params) : "-"} long />
                  <Meta label="Aliases" help="External source markets matched as evidence, not ontology roots." value={selected.aliases?.length || 0} />
                </div>
              </details>

              <div className="explorer">
                <div className="explorer-head">
                  <div>
                    <div className="panel-label">Explorer</div>
                    <h3>{searchResult?.total ?? conditions.length} matches</h3>
                  </div>
                  <input value={explorerQuery} onChange={(e) => setExplorerQuery(e.target.value)} placeholder="Search measurements, conditions, propositions." />
                </div>
                <div className="filters">
                  <Select label="Show" value={objectKind} setValue={setObjectKind} values={["", "measurement", "condition", "proposition"]} />
                  <Select label="Domain" value={domain} setValue={setDomain} values={["", ...(state?.facets?.domains || searchResult?.facets.domains || [])]} />
                  <Select label="Measurement type" value={measurementKind} setValue={setMeasurementKind} values={["", ...(state?.facets?.measurement_kinds || searchResult?.facets.measurement_kinds || [])]} />
                  <Select label="Predicate" value={predicateOp} setValue={setPredicateOp} values={["", ...(state?.facets?.predicate_ops || searchResult?.facets.predicate_ops || [])]} />
                </div>
                <div className="atom-list">
                  {(searchResult?.items || conditions.slice(0, 80)).map((item) => (
                  <button
                    key={item.id}
                    className={selected.leaf_ids?.includes(item.id) || item.id === selected.id ? "atom used" : "atom"}
                    onClick={() => setSelectedId(item.id)}
                  >
                    <span>{item.short_name || item.title}</span>
                    <strong>{item.object_kind === "measurement" ? "input" : nanosPct(item.market?.yes_price_nanos) || pct(item.fair_value)}</strong>
                    <small>{displayKind(item)} / {item.domain || "unknown"}</small>
                    <small>{item.question || item.description}</small>
                  </button>
                  ))}
                </div>
              </div>
            </>
          ) : (
            <div className="empty">Seed the demo markets to begin.</div>
          )}
        </section>

        <aside className="panel trade-panel">
          <div className="panel-label">Trade Proposal</div>
          <div className="account">
            {accountId ? (
              <span>Demo account #{accountId}</span>
            ) : (
              <button onClick={createDemoAccount} disabled={!!busy}>Create demo account</button>
            )}
          </div>
          <div className="trade-buttons">
            <button onClick={() => askTrade("BUY_YES")} disabled={!selected || !isTradable(selected) || !!busy}>Propose YES</button>
            <button onClick={() => askTrade("BUY_NO")} disabled={!selected || !isTradable(selected) || !!busy}>Propose NO</button>
          </div>
          {proposal ? (
            <div className="ticket">
              <h3>{proposal.side.replace("_", " ")}</h3>
              <div className="ticket-row"><span>Limit</span><strong>{pct(proposal.limit_price)}</strong></div>
              <div className="ticket-row"><span>Quantity</span><strong>{proposal.quantity}</strong></div>
              <div className="ticket-row"><span>Notional</span><strong>${proposal.notional}</strong></div>
              <p>{proposal.rationale}</p>
              <button className="primary" disabled={!accountId || !!busy} onClick={confirmTrade}>
                Confirm and submit
              </button>
            </div>
          ) : (
            <p className="muted">The agent can propose a trade after you pick a condition or proposition. Nothing submits without confirmation.</p>
          )}

          <div className="definitions">
            <div className="panel-label">Known Propositions</div>
            {propositions.slice(0, 18).map((item) => (
              <button key={item.id} onClick={() => setSelectedId(item.id)} className={item.id === selected?.id ? "def active" : "def"}>
                <span>{item.short_name}</span>
                <small>{item.source || item.author}</small>
              </button>
            ))}
          </div>
        </aside>
      </section>
    </main>
  );
}

function Metric({ label, value, help }: { label: string; value: React.ReactNode; help?: string }) {
  return (
    <div className="metric">
      <span>{label}{help && <Info text={help} />}</span>
      <strong>{value}</strong>
    </div>
  );
}

function Meta({ label, value, help, long }: { label: string; value: React.ReactNode; help?: string; long?: boolean }) {
  return (
    <div className={long ? "meta long" : "meta"}>
      <span>{label}{help && <Info text={help} />}</span>
      <strong title={String(value)}>{value}</strong>
    </div>
  );
}

function Info({ text }: { text: string }) {
  return <b className="info" title={text}>?</b>;
}

function Select({
  label,
  value,
  setValue,
  values,
}: {
  label: string;
  value: string;
  setValue: (value: string) => void;
  values: string[];
}) {
  return (
    <label>
      <span>{label}</span>
      <select value={value} onChange={(e) => setValue(e.target.value)}>
        {Array.from(new Set(values)).map((item) => (
          <option key={item || "all"} value={item}>
            {item || "all"}
          </option>
        ))}
      </select>
    </label>
  );
}

function FormulaView({ formula, instruments }: { formula: Formula | null; instruments: Instrument[] }) {
  if (!formula) return <div className="formula-node atom-node">Single condition</div>;
  const conditionId = formula.condition || formula.atom;
  if (conditionId) {
    const condition = instruments.find((item) => item.id === conditionId);
    return <div className="formula-node atom-node">{condition?.short_name || conditionId}</div>;
  }
  return (
    <div className="formula-node">
      <div className="op">{formula.op}{formula.op === "K_OF_N" ? `:${formula.k}` : ""}</div>
      <div className="children">
        {(formula.args || []).map((arg, idx) => (
          <FormulaView key={idx} formula={arg} instruments={instruments} />
        ))}
      </div>
    </div>
  );
}

function displayKind(item?: Instrument | null): string {
  if (!item) return "-";
  if (item.object_kind === "measurement") return "Measurement";
  if (item.kind === "condition" || item.object_kind === "condition") return "Condition";
  if (item.kind === "proposition" || item.kind === "composition" || item.object_kind === "proposition") return "Market definition";
  return item.object_kind || item.kind || "Object";
}

function isTradable(item?: Instrument | null): boolean {
  return !!item && item.object_kind !== "measurement";
}

function predicateText(item: Instrument): string {
  const predicate = (item.predicate || item.params?.predicate) as Record<string, unknown> | undefined;
  if (!predicate) return item.object_kind === "measurement" ? "not a yes/no statement" : "formula";
  if (predicate.op === "between") return `${predicate.low} < value < ${predicate.high}`;
  if (predicate.threshold !== undefined) return `value ${predicate.op} ${predicate.threshold}`;
  if (predicate.value !== undefined) return `value ${predicate.op} ${predicate.value}`;
  return String(predicate.op || "-");
}

function sourceText(item: Instrument): string {
  if (item.object_kind === "measurement" && item.feed_ids?.length) return item.feed_ids.join(", ");
  if (item.measurement?.feed_ids?.length) return item.measurement.feed_ids.join(", ");
  if (item.source === "graph") return "curated graph";
  return item.source || "-";
}

function qualityText(value?: string): string {
  if (value === "seed") return "curated seed";
  if (value === "source_matched") return "matched to source market";
  if (value === "wizard_published") return "created in wizard";
  if (value === "user_draft") return "user draft";
  return value || "-";
}

function explainEdge(edge: { from: string; to: string }, instruments: Instrument[]): string {
  const from = instruments.find((item) => item.id === edge.from)?.short_name || edge.from;
  const to = instruments.find((item) => item.id === edge.to)?.short_name || edge.to;
  return `If "${from}" is YES, then "${to}" must also be YES.`;
}

createRoot(document.getElementById("root")!).render(<App />);
