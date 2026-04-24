import React, { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  createAccount,
  createDraft,
  discover,
  draftComposition,
  getState,
  importSources,
  nanosPct,
  pct,
  proposeTrade,
  quoteOnce,
  searchExplorer,
  seedDemo,
  submitTrade,
  triggerEvent,
} from "./api";
import type { DemoState, Draft, Formula, Instrument, SearchResult, TradeProposal } from "./types";
import "./styles.css";

function App() {
  const [state, setState] = useState<DemoState | null>(null);
  const [selectedId, setSelectedId] = useState("");
  const [query, setQuery] = useState("I want a basket around 2028 election outcomes and macro conditions.");
  const [agentAnswer, setAgentAnswer] = useState("");
  const [rankedIds, setRankedIds] = useState<string[]>([]);
  const [proposal, setProposal] = useState<TradeProposal | null>(null);
  const [draftPrompt, setDraftPrompt] = useState("Build an AND composition from the most relevant election and macro atoms.");
  const [draft, setDraft] = useState<Draft | null>(null);
  const [explorerQuery, setExplorerQuery] = useState("election");
  const [domain, setDomain] = useState("");
  const [atomType, setAtomType] = useState("");
  const [source, setSource] = useState("");
  const [templateId, setTemplateId] = useState("");
  const [quality, setQuality] = useState("");
  const [resolverPrimitive, setResolverPrimitive] = useState("");
  const [kind, setKind] = useState("");
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
  const selected =
    instruments.find((item) => item.id === selectedId) ||
    searchResult?.items[0] ||
    instruments.find((item) => item.kind === "composition") ||
    instruments[0];
  const compositions = instruments.filter((item) => item.kind === "composition");
  const atoms = instruments.filter((item) => item.kind === "atom");

  const ranked = useMemo(() => {
    if (!rankedIds.length) return searchResult?.items.slice(0, 8) || compositions.slice(0, 8);
    const byId = new Map(instruments.map((item) => [item.id, item]));
    return rankedIds.map((id) => byId.get(id)).filter(Boolean) as Instrument[];
  }, [rankedIds, instruments, searchResult, compositions]);

  useEffect(() => {
    const timer = setTimeout(() => {
      searchExplorer({
        query: explorerQuery,
        domain,
        atom_type: atomType,
        source,
        template_id: templateId,
        quality,
        resolver_primitive: resolverPrimitive,
        kind,
        limit: 80,
      })
        .then((result) => {
          setSearchResult(result);
          if (!selectedId && result.items[0]) setSelectedId(result.items[0].id);
        })
        .catch(() => undefined);
    }, 220);
    return () => clearTimeout(timer);
  }, [explorerQuery, domain, atomType, source, templateId, quality, resolverPrimitive, kind, selectedId]);

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
      setToast("Seeded imported atom universe");
    } catch (e) {
      setToast(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function runImport() {
    setBusy("import");
    try {
      setState(await importSources(false, 300));
      setToast("Built template atom universe + source aliases");
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

  async function runEvent() {
    setBusy("event");
    try {
      await triggerEvent("helicopter");
      setToast("Simulated helicopter incident");
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
      setDraft(await draftComposition(draftPrompt));
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
      const next = await createDraft(draft);
      setState(next);
      setSelectedId(next.instruments[next.instruments.length - 1].id);
      setDraft(null);
      setToast("Created composition market");
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
            An agentic UI for exploring hundreds of typed atoms, composing precise conditions, and checking whether liquidity can follow.
          </p>
        </div>
        <div className="hero-actions">
          <button onClick={runImport} disabled={!!busy}>{busy === "import" ? "Building..." : "Build Atom Rules"}</button>
          <button onClick={runSeed} disabled={!!busy}>{busy === "seed" ? "Seeding..." : "Seed 300 Markets"}</button>
          <button onClick={runQuote} disabled={!!busy}>Quote Once</button>
          <button className="danger" onClick={runEvent} disabled={!!busy}>
            Simulate helicopter incident
          </button>
        </div>
      </header>

      <section className="stats-strip">
        <Metric label="Atoms" value={state?.instrument_counts?.atoms ?? atoms.length} />
        <Metric label="Compositions" value={state?.instrument_counts?.compositions ?? compositions.length} />
        <Metric label="Seeded" value={state?.instrument_counts?.seeded ?? 0} />
        <Metric label="Quoted" value={state?.instrument_counts?.quoted ?? 0} />
      </section>

      <section className="shell">
        <aside className="panel agent-panel">
          <div className="panel-label">Discovery Agent</div>
          <textarea value={query} onChange={(e) => setQuery(e.target.value)} />
          <button className="primary" onClick={runAgent} disabled={!!busy}>
            {busy === "agent" ? "Thinking..." : "Find the right composition"}
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
            <div className="panel-label">Agent Market Creation</div>
            <textarea value={draftPrompt} onChange={(e) => setDraftPrompt(e.target.value)} />
            <button onClick={makeDraft} disabled={!!busy}>{busy === "draft" ? "Drafting..." : "Draft composition"}</button>
            {draft && (
              <div className="draft">
                <h3>{draft.short_name}</h3>
                <p>{draft.description}</p>
                <FormulaView formula={draft.formula || null} instruments={instruments} />
                <button className="primary" onClick={approveDraft} disabled={!!busy}>
                  Approve and create market
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
                  <div className="panel-label">{selected.kind}</div>
                  <h2>{selected.title}</h2>
                  <p>{selected.description}</p>
                </div>
                <div className="price-orb">
                  <span>YES</span>
                  <strong>{nanosPct(selected.market?.yes_price_nanos) || pct(selected.model_value)}</strong>
                </div>
              </div>

              <div className="metrics">
                <Metric label="Market ID" value={selected.market_id ?? "-"} />
                <Metric label="Model fair" value={pct(selected.model_value ?? selected.fair_value)} />
                <Metric label="Volume" value={`$${Math.round((selected.market?.volume_nanos || 0) / 1_000_000_000)}`} />
                <Metric label="Template" value={selected.template_id || selected.atom_type || selected.kind} />
              </div>

              <div className="metadata-grid">
                <Meta label="Domain" value={selected.domain || "-"} />
                <Meta label="Quality" value={selected.quality || "-"} />
                <Meta label="Resolver" value={selected.resolver_primitive || selected.oracle_path} />
                <Meta label="Time window" value={selected.time_window || "-"} />
                <Meta label="Canonical key" value={selected.canonical_key || selected.id} />
                <Meta label="Status" value={selected.market?.status || "unseeded"} />
                <Meta label="Params" value={selected.params ? JSON.stringify(selected.params) : "-"} />
                <Meta label="Aliases" value={selected.aliases?.length || 0} />
                <Meta label="Source" value={selected.source || "-"} />
              </div>

              <div className="formula-card">
                <div className="panel-label">Resolution Formula</div>
                <FormulaView formula={selected.formula || null} instruments={instruments} />
              </div>

              <div className="explorer">
                <div className="explorer-head">
                  <div>
                    <div className="panel-label">Atom Explorer</div>
                    <h3>{searchResult?.total ?? atoms.length} matches</h3>
                  </div>
                  <input value={explorerQuery} onChange={(e) => setExplorerQuery(e.target.value)} placeholder="Search atoms, sources, metrics." />
                </div>
                <div className="filters">
                  <Select label="Kind" value={kind} setValue={setKind} values={["", "atom", "composition"]} />
                  <Select label="Domain" value={domain} setValue={setDomain} values={["", ...(state?.facets?.domains || searchResult?.facets.domains || [])]} />
                  <Select label="Template" value={templateId} setValue={setTemplateId} values={["", ...(state?.facets?.template_ids || searchResult?.facets.template_ids || [])]} />
                  <Select label="Quality" value={quality} setValue={setQuality} values={["", ...(state?.facets?.qualities || searchResult?.facets.qualities || [])]} />
                  <Select label="Resolver" value={resolverPrimitive} setValue={setResolverPrimitive} values={["", ...(state?.facets?.resolver_primitives || searchResult?.facets.resolver_primitives || [])]} />
                  <Select label="Source" value={source} setValue={setSource} values={["", ...(state?.facets?.sources || searchResult?.facets.sources || [])]} />
                </div>
                <div className="atom-list">
                  {(searchResult?.items || atoms.slice(0, 80)).map((atom) => (
                  <button
                    key={atom.id}
                    className={selected.leaf_ids?.includes(atom.id) || atom.id === selected.id ? "atom used" : "atom"}
                    onClick={() => setSelectedId(atom.id)}
                  >
                    <span>{atom.short_name}</span>
                    <strong>{nanosPct(atom.market?.yes_price_nanos) || pct(atom.fair_value)}</strong>
                    <small>{atom.domain || "unknown"} / {atom.template_id || atom.atom_type || atom.kind} / {atom.quality || "seed"}</small>
                    <small>{atom.question}</small>
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
            <button onClick={() => askTrade("BUY_YES")} disabled={!selected || !!busy}>Propose YES</button>
            <button onClick={() => askTrade("BUY_NO")} disabled={!selected || !!busy}>Propose NO</button>
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
            <p className="muted">The agent can propose a trade after you pick a composition. Nothing submits without confirmation.</p>
          )}

          <div className="definitions">
            <div className="panel-label">Known Compositions</div>
            {compositions.slice(0, 18).map((item) => (
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

function Metric({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function Meta({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="meta">
      <span>{label}</span>
      <strong title={String(value)}>{value}</strong>
    </div>
  );
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
  if (!formula) return <div className="formula-node atom-node">Atomic market</div>;
  if (formula.atom) {
    const atom = instruments.find((item) => item.id === formula.atom);
    return <div className="formula-node atom-node">{atom?.short_name || formula.atom}</div>;
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

createRoot(document.getElementById("root")!).render(<App />);
