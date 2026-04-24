import React, { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  createAccount,
  createWizardDraft,
  discover,
  getGraph,
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
import type { DemoState, Formula, GraphNode, GraphProjection, Instrument, SearchResult, TradeProposal, WizardDraft } from "./types";
import "./styles.css";

const AGENT_MODES = [
  {
    id: "hedge",
    label: "Hedge exposure",
    prompt: "I am long ETH and worried about downside in 2026.",
    help: "Find markets that pay in the bad state for an existing position.",
  },
  {
    id: "news",
    label: "Trade news",
    prompt: "New Iran strike reports look underappreciated. What should reprice?",
    help: "Turn a headline into direct or proxy markets.",
  },
  {
    id: "interview",
    label: "Find bets",
    prompt: "I have opinions about macro and crypto. Interview me to find bets.",
    help: "Ask follow-up questions until opinions become measurable claims.",
  },
  {
    id: "alpha",
    label: "Monetize alpha",
    prompt: "I have alpha about BTC ETF flows. What is the best way to express it?",
    help: "Map an information edge to the tightest tradable condition or proxy.",
  },
  {
    id: "create",
    label: "Create market",
    prompt: "ETH above 3000 and BTC above 100000 in 2026.",
    help: "Draft a new market definition from existing graph conditions.",
  },
] as const;

type AgentMode = (typeof AGENT_MODES)[number]["id"];

function App() {
  const [state, setState] = useState<DemoState | null>(null);
  const [selectedId, setSelectedId] = useState("");
  const [agentMode, setAgentMode] = useState<AgentMode>("hedge");
  const [query, setQuery] = useState<string>(AGENT_MODES[0].prompt);
  const [agentAnswer, setAgentAnswer] = useState("");
  const [agentActions, setAgentActions] = useState<string[]>([]);
  const [agentQuestions, setAgentQuestions] = useState<string[]>([]);
  const [rankedIds, setRankedIds] = useState<string[]>([]);
  const [proposal, setProposal] = useState<TradeProposal | null>(null);
  const [draftPrompt, setDraftPrompt] = useState("ETH between 3000 and 6000 by end of 2026.");
  const [draft, setDraft] = useState<WizardDraft | null>(null);
  const [explorerQuery, setExplorerQuery] = useState("ETH");
  const [viewMode, setViewMode] = useState<"workspace" | "graph">("workspace");
  const [graphQuery, setGraphQuery] = useState("");
  const [graphDomain, setGraphDomain] = useState("");
  const [graphKind, setGraphKind] = useState("");
  const [graphDepth, setGraphDepth] = useState(2);
  const [graphProjection, setGraphProjection] = useState<GraphProjection | null>(null);
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
  const entities = state?.entities || [];
  const contexts = state?.contexts || [];
  const allObjects = [...entities, ...contexts, ...measurements, ...instruments] as Instrument[];
  const selected =
    allObjects.find((item) => item.id === selectedId) ||
    searchResult?.items[0] ||
    instruments.find((item) => item.kind === "proposition" || item.kind === "composition") ||
    instruments[0];
  const propositions = instruments.filter((item) => item.kind === "proposition" || item.kind === "composition");
  const conditions = instruments.filter((item) => item.kind === "condition" || item.kind === "atom");
  const selectedEdges = (state?.implication_edges || []).filter((edge) => edge.from === selected?.id || edge.to === selected?.id);
  const selectedMeasurementId = selected?.object_kind === "measurement" ? selected.id : selected?.measurement_id || "";
  const selectedCurve = (state?.threshold_curves || []).find((curve) => curve.measurement_id === selectedMeasurementId);
  const modeConfig = AGENT_MODES.find((item) => item.id === agentMode) || AGENT_MODES[0];
  const diagnostics = state?.ontology_diagnostics;

  const ranked = useMemo(() => {
    if (!rankedIds.length) return searchResult?.items.slice(0, 8) || propositions.slice(0, 8);
    const byId = new Map(allObjects.map((item) => [item.id, item]));
    return rankedIds.map((id) => byId.get(id)).filter(Boolean) as Instrument[];
  }, [rankedIds, allObjects, searchResult, propositions]);

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

  useEffect(() => {
    if (viewMode !== "graph") return;
    const timer = setTimeout(() => {
      getGraph({
        query: graphQuery,
        domain: graphDomain,
        kind: graphKind,
        focus_id: selected?.id || "",
        depth: graphDepth,
        limit: 260,
      })
        .then(setGraphProjection)
        .catch((e) => setToast(String(e)));
    }, 180);
    return () => clearTimeout(timer);
  }, [viewMode, graphQuery, graphDomain, graphKind, graphDepth, selected?.id]);

  async function runAgent() {
    setBusy("agent");
    try {
      const result = await discover(query, agentMode);
      setAgentAnswer(result.answer);
      setAgentActions(result.actions || []);
      setAgentQuestions(result.questions || []);
      setRankedIds(result.ranked_ids);
      if (result.recommendation_id) setSelectedId(result.recommendation_id);
      if (result.creation_prompt && agentMode === "create") setDraftPrompt(result.creation_prompt);
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
      setToast("Published market definition");
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
          <div className="eyebrow">Composition demo</div>
          <h1>Ask for the trade you actually want.</h1>
          <p>
            A copilot over a prediction knowledge graph: measurements become conditions, conditions become market definitions, and definitions become tradable Sybil markets.
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
          <span>
            Graph health: {diagnostics?.status || "loading"}.
            {diagnostics?.errors.length ? ` ${diagnostics.errors.length} ontology errors.` : " No ontology errors."}
          </span>
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
          <div className="panel-label">Agent Copilot</div>
          <div className="mode-tabs" role="tablist" aria-label="Agent mode">
            {AGENT_MODES.map((mode) => (
              <button
                key={mode.id}
                className={mode.id === agentMode ? "mode-tab active" : "mode-tab"}
                title={mode.help}
                onClick={() => {
                  setAgentMode(mode.id);
                  setQuery(mode.prompt);
                  setAgentAnswer("");
                  setAgentActions([]);
                  setAgentQuestions([]);
                }}
              >
                {mode.label}
              </button>
            ))}
          </div>
          <textarea value={query} onChange={(e) => setQuery(e.target.value)} placeholder={modeConfig.prompt} />
          <button className="primary" onClick={runAgent} disabled={!!busy}>
            {busy === "agent" ? "Thinking..." : modeConfig.label}
          </button>
          <div className="agent-answer">
            {agentAnswer || "Describe the exposure, news, opinion, or alpha. The copilot ranks existing markets and tells you when a new definition is needed."}
          </div>
          {(agentQuestions.length > 0 || agentActions.length > 0) && (
            <div className="agent-guidance">
              {agentQuestions.length > 0 && (
                <div>
                  <strong>Questions to tighten the trade</strong>
                  {agentQuestions.map((question) => <span key={question}>{question}</span>)}
                </div>
              )}
              {agentActions.length > 0 && (
                <div>
                  <strong>Next checks</strong>
                  {agentActions.map((action) => <span key={action}>{action}</span>)}
                </div>
              )}
            </div>
          )}
          <div className="rank-list">
            {ranked.slice(0, 4).map((item) => (
              <button
                key={item.id}
                className={item.id === selected?.id ? "rank-card active" : "rank-card"}
                onClick={() => setSelectedId(item.id)}
              >
                <span>
                  <b>{item.short_name || item.title}</b>
                  <small>{displayKind(item)} / {pathText(item) === "-" ? item.domain || "unknown" : pathText(item)}</small>
                </span>
                <strong>{item.object_kind === "measurement" ? "input" : nanosPct(item.market?.yes_price_nanos) || pct(item.model_value)}</strong>
              </button>
            ))}
          </div>

          <div className="creator">
            <div className="panel-label">Market Creation Wizard</div>
            <textarea value={draftPrompt} onChange={(e) => setDraftPrompt(e.target.value)} />
            <button onClick={makeDraft} disabled={!!busy}>{busy === "draft" ? "Drafting..." : "Draft definition"}</button>
            {draft && (
              <div className="draft">
                <h3>{draft.short_name || draft.title}</h3>
                <p>{draft.description}</p>
                <FormulaView formula={draft.formula || null} instruments={instruments} />
                {draft.validation && (
                  <div className="validation">
                    <span>{draft.validation.valid ? "Valid formula" : draft.validation.errors.join("; ")}</span>
                    {draft.validation.duplicate && <span>Duplicate definition exists</span>}
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
          <div className="view-switch">
            <button className={viewMode === "workspace" ? "active" : ""} onClick={() => setViewMode("workspace")}>Workspace</button>
            <button className={viewMode === "graph" ? "active" : ""} onClick={() => setViewMode("graph")}>Graph View</button>
          </div>
          {viewMode === "graph" ? (
            <GraphView
              projection={graphProjection}
              selectedId={selected?.id || ""}
              query={graphQuery}
              setQuery={setGraphQuery}
              domain={graphDomain}
              setDomain={setGraphDomain}
              kind={graphKind}
              setKind={setGraphKind}
              depth={graphDepth}
              setDepth={setGraphDepth}
              select={setSelectedId}
            />
          ) : selected ? (
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
                <Meta label="Path" help="Where this object sits in the prediction graph." value={pathText(selected)} long />
                <Meta label="Measurement" help="The observable input variable behind this condition." value={selected.measurement?.display_title || selected.display_title || selected.measurement?.subject || selected.subject || selected.measurement_kind || "-"} />
                <Meta label="Context" help="The event or time window that scopes the observation." value={selected.context?.title || selected.time_window || selected.aggregation_semantics || "-"} />
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
              {selectedCurve && <ThresholdCurveView curve={selectedCurve} select={setSelectedId} />}

              <GraphNavigator state={state} selected={selected} select={setSelectedId} />

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
                {diagnostics && (diagnostics.errors.length > 0 || diagnostics.warnings.length > 0) && (
                  <div className="diagnostics">
                    {[...diagnostics.errors, ...diagnostics.warnings].slice(0, 8).map((item) => <span key={item}>{item}</span>)}
                  </div>
                )}
              </details>

              <details className="explorer" open={false}>
                <summary>
                  <span>
                    <div className="panel-label">Advanced Graph Explorer</div>
                    <strong>{searchResult?.total ?? conditions.length} matches</strong>
                  </span>
                </summary>
                <div className="explorer-head">
                  <input value={explorerQuery} onChange={(e) => setExplorerQuery(e.target.value)} placeholder="Search measurements, conditions, definitions." />
                </div>
                <div className="filters">
                  <Select label="Show" value={objectKind} setValue={setObjectKind} values={["", "entity", "context", "measurement", "condition", "proposition"]} />
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
                    <span>{item.display_title || item.short_name || item.title}</span>
                    <strong>{item.object_kind === "measurement" ? "input" : item.object_kind === "entity" || item.object_kind === "context" ? "node" : nanosPct(item.market?.yes_price_nanos) || pct(item.fair_value)}</strong>
                    <small>{displayKind(item)} / {item.domain || "unknown"}</small>
                    <small>{item.question || item.description}</small>
                  </button>
                  ))}
                </div>
              </details>
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
            <p className="muted">The agent can propose a trade after you pick a condition or definition. Nothing submits without confirmation.</p>
          )}

          <div className="definitions">
            <div className="panel-label">Known Definitions</div>
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

function GraphView({
  projection,
  selectedId,
  query,
  setQuery,
  domain,
  setDomain,
  kind,
  setKind,
  depth,
  setDepth,
  select,
}: {
  projection: GraphProjection | null;
  selectedId: string;
  query: string;
  setQuery: (value: string) => void;
  domain: string;
  setDomain: (value: string) => void;
  kind: string;
  setKind: (value: string) => void;
  depth: number;
  setDepth: (value: number) => void;
  select: (id: string) => void;
}) {
  const [hovered, setHovered] = useState("");
  const nodes = projection?.nodes || [];
  const edges = projection?.edges || [];
  const layout = useMemo(() => layoutGraph(nodes), [nodes]);
  const width = 1180;
  const height = Math.max(720, ...Object.values(layout).map((item) => item.y + 80), 720);
  const neighborIds = useMemo(() => {
    if (!hovered) return new Set<string>();
    const ids = new Set<string>([hovered]);
    edges.forEach((edge) => {
      if (edge.from === hovered) ids.add(edge.to);
      if (edge.to === hovered) ids.add(edge.from);
    });
    return ids;
  }, [hovered, edges]);

  return (
    <div className="graph-view">
      <div className="graph-toolbar">
        <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search graph: ETH hedge, Iran, Tatum, recession..." />
        <Select label="Domain" value={domain} setValue={setDomain} values={["", ...(projection?.facets.domains || [])]} />
        <Select label="Kind" value={kind} setValue={setKind} values={["", ...(projection?.facets.kinds || [])]} />
        <label>
          <span>Depth</span>
          <select value={depth} onChange={(e) => setDepth(Number(e.target.value))}>
            {[1, 2, 3].map((value) => <option key={value} value={value}>{value}</option>)}
          </select>
        </label>
      </div>
      <div className="graph-legend">
        {["entity", "context", "measurement", "condition", "definition", "market"].map((item) => (
          <span key={item}><b className={`legend-dot ${item}`} />{graphKindLabel(item)}</span>
        ))}
      </div>
      <div className="graph-canvas-wrap">
        {!projection ? (
          <div className="empty">Loading graph...</div>
        ) : (
          <svg className="graph-canvas" viewBox={`0 0 ${width} ${height}`} role="img" aria-label="Composition knowledge graph">
            <defs>
              <marker id="arrow" markerWidth="10" markerHeight="10" refX="8" refY="3" orient="auto" markerUnits="strokeWidth">
                <path d="M0,0 L0,6 L9,3 z" />
              </marker>
            </defs>
            {edges.map((edge) => {
              const from = layout[edge.from];
              const to = layout[edge.to];
              if (!from || !to) return null;
              const active = !hovered || edge.from === hovered || edge.to === hovered;
              return (
                <line
                  key={`${edge.from}-${edge.to}-${edge.type}`}
                  className={active ? `graph-edge ${edge.type}` : "graph-edge dim"}
                  x1={from.x + 76}
                  y1={from.y}
                  x2={to.x - 76}
                  y2={to.y}
                  markerEnd="url(#arrow)"
                >
                  <title>{edge.label}</title>
                </line>
              );
            })}
            {nodes.map((node) => {
              const point = layout[node.id];
              if (!point) return null;
              const selected = node.id === selectedId || node.object_id === selectedId;
              const active = !hovered || neighborIds.has(node.id);
              return (
                <g
                  key={node.id}
                  className={active ? `graph-node ${node.kind} ${selected ? "selected" : ""}` : `graph-node ${node.kind} dim`}
                  transform={`translate(${point.x - 76}, ${point.y - 26})`}
                  onMouseEnter={() => setHovered(node.id)}
                  onMouseLeave={() => setHovered("")}
                  onClick={() => select(node.object_kind === "market" ? node.object_id : node.id)}
                >
                  <rect width="152" height="52" rx="8" />
                  <text x="10" y="20">{truncate(node.label, 24)}</text>
                  <text x="10" y="38" className="node-subtitle">{graphKindLabel(node.kind)} / {node.domain || "demo"}</text>
                  <title>{node.summary || node.label}</title>
                </g>
              );
            })}
          </svg>
        )}
      </div>
      {projection && (
        <div className="graph-summary">
          <span>{projection.nodes.length} nodes</span>
          <span>{projection.edges.length} edges</span>
          <span>{projection.matched_ids.length} search matches</span>
        </div>
      )}
    </div>
  );
}

function layoutGraph(nodes: GraphNode[]): Record<string, { x: number; y: number }> {
  const columns = ["entity", "context", "measurement", "condition", "definition", "market"];
  const grouped = new Map<string, GraphNode[]>();
  columns.forEach((column) => grouped.set(column, []));
  nodes.forEach((node) => grouped.get(node.kind)?.push(node));
  grouped.forEach((items) => items.sort((a, b) => `${a.domain}:${a.label}`.localeCompare(`${b.domain}:${b.label}`)));
  const layout: Record<string, { x: number; y: number }> = {};
  columns.forEach((column, columnIndex) => {
    const items = grouped.get(column) || [];
    const x = 90 + columnIndex * 195;
    items.forEach((node, rowIndex) => {
      layout[node.id] = { x, y: 64 + rowIndex * 74 };
    });
  });
  return layout;
}

function graphKindLabel(kind: string): string {
  if (kind === "definition") return "Definition";
  if (kind === "context") return "Event/window";
  return kind.charAt(0).toUpperCase() + kind.slice(1);
}

function truncate(value: string, max: number): string {
  return value.length > max ? `${value.slice(0, max - 1)}...` : value;
}

function ThresholdCurveView({
  curve,
  select,
}: {
  curve: NonNullable<DemoState["threshold_curves"]>[number];
  select: (id: string) => void;
}) {
  return (
    <div className="curve-card">
      <div className="curve-head">
        <div>
          <div className="panel-label">Threshold Curve</div>
          <h3>{curve.title}</h3>
        </div>
        <span>{curve.window} / {curve.aggregation}</span>
      </div>
      <div className="curve-rows">
        {curve.conditions.map((item) => (
          <button key={item.condition_id} onClick={() => select(item.condition_id)}>
            <span>
              <b>{item.short_name}</b>
              <small>{predicateRecordText(item.predicate)}</small>
            </span>
            <strong>{nanosPct(item.market_price) || pct(item.fair_value)}</strong>
          </button>
        ))}
      </div>
      <p>{curve.mm_note}</p>
    </div>
  );
}

function GraphNavigator({
  state,
  selected,
  select,
}: {
  state: DemoState | null;
  selected?: Instrument | null;
  select: (id: string) => void;
}) {
  const domains = state?.facets?.domains || [];
  const selectedDomain = selected?.domain || domains[0] || "";
  const [domain, setDomain] = useState(selectedDomain);
  const [anchorId, setAnchorId] = useState("");
  const activeDomain = domain || selectedDomain;
  const entities = (state?.entities || []).filter((item) => !activeDomain || item.domain === activeDomain);
  const contexts = (state?.contexts || []).filter((item) => !activeDomain || item.domain === activeDomain);
  const anchors = [
    ...entities.map((item) => ({ id: item.id, title: item.short_name || item.title, kind: "Entity" })),
    ...contexts.map((item) => ({ id: item.id, title: item.short_name || item.title, kind: "Event" })),
  ];
  const activeAnchor = anchorId || selected?.context_id || selected?.entity_ids?.[0] || "";
  const measurements = (state?.measurements || []).filter((item) => {
    if (activeDomain && item.domain !== activeDomain) return false;
    if (!activeAnchor) return true;
    return item.context_id === activeAnchor || item.entity_ids?.includes(activeAnchor);
  });
  const selectedMeasurementId =
    selected?.object_kind === "measurement" ? selected.id : selected?.measurement_id || measurements[0]?.id || "";
  const relatedConditions = (state?.conditions || []).filter((item) => item.measurement_id === selectedMeasurementId);
  const relatedDefinitions = (state?.propositions || []).filter((item) => item.leaf_ids?.some((id) => relatedConditions.some((condition) => condition.id === id)));

  return (
    <div className="graph-nav">
      <div className="graph-nav-head">
        <div>
          <div className="panel-label">Graph Navigator</div>
          <h3>Browse by thing, event, and measured value</h3>
        </div>
        <span>{measurements.length} measurements</span>
      </div>
      <div className="domain-tabs">
        {domains.map((item) => (
          <button
            key={item}
            className={item === activeDomain ? "active" : ""}
            onClick={() => {
              setDomain(item);
              setAnchorId("");
            }}
          >
            {item}
          </button>
        ))}
      </div>
      <div className="graph-columns">
        <div>
          <strong>Entities and events</strong>
          <div className="graph-list compact">
            {anchors.slice(0, 18).map((item) => (
              <button
                key={item.id}
                className={item.id === activeAnchor ? "active" : ""}
                onClick={() => {
                  setAnchorId(item.id);
                  select(item.id);
                }}
              >
                <span>{item.title}</span>
                <small>{item.kind}</small>
              </button>
            ))}
          </div>
        </div>
        <div>
          <strong>Measurements</strong>
          <div className="graph-list">
            {measurements.slice(0, 18).map((item) => (
              <button
                key={item.id}
                className={item.id === selectedMeasurementId ? "active" : ""}
                onClick={() => select(item.id)}
              >
                <span>{item.display_title || item.title}</span>
                <small>{item.measurement_kind} / {item.unit}</small>
              </button>
            ))}
          </div>
        </div>
        <div>
          <strong>Conditions and definitions</strong>
          <div className="graph-list">
            {[...relatedConditions, ...relatedDefinitions].slice(0, 18).map((item) => (
              <button key={item.id} className={item.id === selected?.id ? "active" : ""} onClick={() => select(item.id)}>
                <span>{item.short_name || item.title}</span>
                <small>{displayKind(item)} / {item.object_kind === "condition" ? predicateText(item) : pct(item.model_value)}</small>
              </button>
            ))}
          </div>
        </div>
      </div>
    </div>
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
            {selectLabel(item)}
          </option>
        ))}
      </select>
    </label>
  );
}

function selectLabel(value: string): string {
  if (!value) return "all";
  if (value === "proposition") return "definition";
  return value;
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
  if (item.object_kind === "entity") return "Entity";
  if (item.object_kind === "context") return "Context";
  if (item.object_kind === "measurement") return "Measurement";
  if (item.kind === "condition" || item.object_kind === "condition") return "Condition";
  if (item.kind === "proposition" || item.kind === "composition" || item.object_kind === "proposition") return "Market definition";
  return item.object_kind || item.kind || "Object";
}

function isTradable(item?: Instrument | null): boolean {
  return !!item && !["measurement", "entity", "context", "feed"].includes(item.object_kind || "");
}

function predicateText(item: Instrument): string {
  const predicate = (item.predicate || item.params?.predicate) as Record<string, unknown> | undefined;
  if (!predicate) return item.object_kind === "measurement" || item.object_kind === "entity" || item.object_kind === "context" ? "not a yes/no statement" : "formula";
  return predicateRecordText(predicate);
}

function predicateRecordText(predicate: Record<string, unknown>): string {
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

function pathText(item: Instrument): string {
  if (item.path?.length) return item.path.join(" / ");
  if (item.measurement?.path?.length) return item.measurement.path.join(" / ");
  if (item.object_kind === "entity") return `${item.domain || "domain"} / ${item.kind || "entity"} / ${item.title}`;
  if (item.object_kind === "context") return `${item.domain || "domain"} / ${item.kind || "context"} / ${item.title}`;
  return "-";
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
