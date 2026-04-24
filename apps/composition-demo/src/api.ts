import type { DemoState, Discovery, Draft, Formula, FormulaValidation, SearchResult, TradeProposal, WizardDraft } from "./types";

export const DEMO_URL = import.meta.env.VITE_COMPOSITION_DEMO_URL || "http://localhost:8787";
export const SYBIL_URL = import.meta.env.VITE_SYBIL_API_URL || "http://localhost:3001";
const NANOS = 1_000_000_000;

async function json<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...(init?.headers || {}),
    },
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(text || `${res.status} ${res.statusText}`);
  }
  return (await res.json()) as T;
}

export function getState(): Promise<DemoState> {
  return json<DemoState>(`${DEMO_URL}/state?sybil_url=${encodeURIComponent(SYBIL_URL)}`);
}

export function seedDemo(): Promise<DemoState> {
  return json<DemoState>(`${DEMO_URL}/seed`, {
    method: "POST",
    body: JSON.stringify({ sybil_url: SYBIL_URL }),
  });
}

export function importSources(force = false, max_atoms = 110): Promise<DemoState> {
  return json<DemoState>(`${DEMO_URL}/sources/import`, {
    method: "POST",
    body: JSON.stringify({ sybil_url: SYBIL_URL, force, max_atoms }),
  });
}

export function searchExplorer(params: {
  query?: string;
  domain?: string;
  atom_type?: string;
  source?: string;
  kind?: string;
  template_id?: string;
  quality?: string;
  resolver_primitive?: string;
  object_kind?: string;
  measurement_kind?: string;
  measurement_id?: string;
  predicate_op?: string;
  limit?: number;
}): Promise<SearchResult> {
  return json<SearchResult>(`${DEMO_URL}/explorer/search`, {
    method: "POST",
    body: JSON.stringify({ sybil_url: SYBIL_URL, ...params }),
  });
}

export function createWizardDraft(prompt: string): Promise<WizardDraft> {
  return json<WizardDraft>(`${DEMO_URL}/wizard/draft`, {
    method: "POST",
    body: JSON.stringify({ sybil_url: SYBIL_URL, prompt }),
  });
}

export function editWizardDraft(params: {
  draft_id?: string;
  draft?: WizardDraft;
  operation: string;
  condition_id?: string;
  from_condition_id?: string;
  to_condition_id?: string;
  operator?: string;
  k?: number;
  formula?: Formula;
}): Promise<WizardDraft> {
  return json<WizardDraft>(`${DEMO_URL}/wizard/edit`, {
    method: "POST",
    body: JSON.stringify({ sybil_url: SYBIL_URL, ...params }),
  });
}

export function validateWizardDraft(draft: WizardDraft): Promise<WizardDraft> {
  return json<WizardDraft>(`${DEMO_URL}/wizard/validate`, {
    method: "POST",
    body: JSON.stringify({ sybil_url: SYBIL_URL, draft }),
  });
}

export function publishWizardDraft(draft: WizardDraft): Promise<DemoState> {
  return json<DemoState>(`${DEMO_URL}/wizard/publish`, {
    method: "POST",
    body: JSON.stringify({ sybil_url: SYBIL_URL, draft }),
  });
}

export function validateFormula(formula: Formula): Promise<FormulaValidation> {
  return json<FormulaValidation>(`${DEMO_URL}/formula/validate`, {
    method: "POST",
    body: JSON.stringify({ formula }),
  });
}

export function quoteOnce(): Promise<{ orders: number; mm_account_id?: number }> {
  return json(`${DEMO_URL}/quote`, {
    method: "POST",
    body: JSON.stringify({ sybil_url: SYBIL_URL }),
  });
}

export function triggerEvent(event: string): Promise<{ orders: number }> {
  return json(`${DEMO_URL}/event`, {
    method: "POST",
    body: JSON.stringify({ sybil_url: SYBIL_URL, event }),
  });
}

export function discover(query: string, mode?: string): Promise<Discovery> {
  return json<Discovery>(`${DEMO_URL}/agent/discover`, {
    method: "POST",
    body: JSON.stringify({ sybil_url: SYBIL_URL, query, mode }),
  });
}

export function draftComposition(prompt: string): Promise<Draft> {
  return json<Draft>(`${DEMO_URL}/agent/draft-composition`, {
    method: "POST",
    body: JSON.stringify({ sybil_url: SYBIL_URL, prompt }),
  });
}

export function createDraft(draft: Draft): Promise<DemoState> {
  return json<DemoState>(`${DEMO_URL}/markets/create-draft`, {
    method: "POST",
    body: JSON.stringify({ sybil_url: SYBIL_URL, draft }),
  });
}

export function proposeTrade(params: {
  instrument_id: string;
  intent: string;
  side?: string;
}): Promise<TradeProposal> {
  return json<TradeProposal>(`${DEMO_URL}/agent/propose-trade`, {
    method: "POST",
    body: JSON.stringify({ sybil_url: SYBIL_URL, ...params }),
  });
}

export async function createAccount(dollars = 500): Promise<number> {
  const res = await json<{ account_id: number }>(`${SYBIL_URL}/v1/accounts`, {
    method: "POST",
    body: JSON.stringify({ initial_balance_nanos: Math.round(dollars * NANOS) }),
  });
  return res.account_id;
}

export function submitTrade(account_id: number, proposal: TradeProposal): Promise<unknown> {
  return json(`${DEMO_URL}/orders/submit`, {
    method: "POST",
    body: JSON.stringify({
      sybil_url: SYBIL_URL,
      account_id,
      market_id: proposal.market_id,
      side: proposal.side,
      price: proposal.limit_price,
      quantity: proposal.quantity,
    }),
  });
}

export function pct(value?: number | null): string {
  if (value === undefined || value === null) return "-";
  return `${Math.round(value * 100)}%`;
}

export function nanosPct(nanos?: number): string {
  if (!nanos) return "-";
  return `${(nanos / 10_000_000).toFixed(1)}%`;
}
