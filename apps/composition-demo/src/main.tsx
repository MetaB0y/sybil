import React, { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  createAccount,
  createDraft,
  discover,
  draftComposition,
  getState,
  nanosPct,
  pct,
  proposeTrade,
  quoteOnce,
  seedDemo,
  submitTrade,
  triggerEvent,
} from "./api";
import type { DemoState, Draft, Formula, Instrument, TradeProposal } from "./types";
import "./styles.css";

function App() {
  const [state, setState] = useState<DemoState | null>(null);
  const [selectedId, setSelectedId] = useState("iran_mainstream");
  const [query, setQuery] = useState("I want to bet no on US invading Iran, but only if it means a real invasion.");
  const [agentAnswer, setAgentAnswer] = useState("");
  const [rankedIds, setRankedIds] = useState<string[]>([]);
  const [proposal, setProposal] = useState<TradeProposal | null>(null);
  const [draftPrompt, setDraftPrompt] = useState("Create a stricter definition where only sustained ground occupation counts.");
  const [draft, setDraft] = useState<Draft | null>(null);
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
  const selected = instruments.find((item) => item.id === selectedId) || instruments.find((item) => item.kind === "composition");
  const compositions = instruments.filter((item) => item.kind === "composition");
  const atoms = instruments.filter((item) => item.kind === "atom");

  const ranked = useMemo(() => {
    if (!rankedIds.length) return compositions;
    const byId = new Map(compositions.map((item) => [item.id, item]));
    return rankedIds.map((id) => byId.get(id)).filter(Boolean) as Instrument[];
  }, [rankedIds, compositions]);

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
      setToast("Seeded Iran composition markets");
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
            An agentic UI for discovering, creating, and trading precise compositions over atomic Iran escalation clauses.
          </p>
        </div>
        <div className="hero-actions">
          <button onClick={runSeed} disabled={!!busy}>{busy === "seed" ? "Seeding..." : "Seed Markets"}</button>
          <button onClick={runQuote} disabled={!!busy}>Quote Once</button>
          <button className="danger" onClick={runEvent} disabled={!!busy}>
            Simulate helicopter incident
          </button>
        </div>
      </header>

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
                <Metric label="Status" value={selected.market?.status || "unseeded"} />
              </div>

              <div className="formula-card">
                <div className="panel-label">Resolution Formula</div>
                <FormulaView formula={selected.formula || null} instruments={instruments} />
              </div>

              <div className="atom-grid">
                {atoms.map((atom) => (
                  <button
                    key={atom.id}
                    className={selected.leaf_ids?.includes(atom.id) ? "atom used" : "atom"}
                    onClick={() => setSelectedId(atom.id)}
                  >
                    <span>{atom.short_name}</span>
                    <strong>{nanosPct(atom.market?.yes_price_nanos) || pct(atom.fair_value)}</strong>
                    <small>{atom.oracle_path}</small>
                  </button>
                ))}
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
            {compositions.map((item) => (
              <button key={item.id} onClick={() => setSelectedId(item.id)} className={item.id === selected?.id ? "def active" : "def"}>
                <span>{item.short_name}</span>
                <small>{item.author}</small>
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

function FormulaView({ formula, instruments }: { formula: Formula | null; instruments: Instrument[] }) {
  if (!formula) return <div className="formula-node atom-node">Atomic market</div>;
  if (formula.atom) {
    const atom = instruments.find((item) => item.id === formula.atom);
    return <div className="formula-node atom-node">{atom?.short_name || formula.atom}</div>;
  }
  return (
    <div className="formula-node">
      <div className="op">{formula.op}</div>
      <div className="children">
        {(formula.args || []).map((arg, idx) => (
          <FormulaView key={idx} formula={arg} instruments={instruments} />
        ))}
      </div>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<App />);
