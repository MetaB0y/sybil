export type Formula = { atom?: string; op?: string; args?: Formula[]; k?: number };

export type Market = {
  market_id: number;
  name: string;
  yes_price_nanos?: number;
  no_price_nanos?: number;
  status: string;
  volume_nanos?: number;
};

export type Instrument = {
  id: string;
  kind: "atom" | "composition";
  title: string;
  short_name: string;
  question: string;
  description: string;
  oracle_path: string;
  formula?: Formula | null;
  author: string;
  market_id?: number | null;
  fair_value: number;
  model_value?: number;
  leaf_ids: string[];
  market?: Market;
  last_price?: number;
  domain?: string;
  atom_type?: string;
  subject?: string;
  metric?: string;
  comparator?: string;
  threshold?: number | null;
  unit?: string;
  time_window?: string;
  resolver_primitive?: string;
  source?: string;
  source_url?: string;
  canonical_key?: string;
  compatible_ops?: string[];
  exclusivity_group?: string | null;
  search_score?: number;
  template_id?: string;
  params?: Record<string, unknown>;
  quality?: string;
  aliases?: Array<{
    source: string;
    source_id: string;
    question: string;
    event_title?: string;
    url?: string;
    fair_value?: number;
  }>;
};

export type DemoState = {
  instruments: Instrument[];
  accounts: Record<string, number>;
  events: Array<{ event: string; timestamp: number }>;
  sybil_url: string;
  sybil_error?: string;
  facets?: Facets;
  source_counts?: Record<string, number>;
  source_errors?: string[];
  instrument_counts?: {
    atoms: number;
    compositions: number;
    seeded: number;
    quoted: number;
  };
};

export type Facets = {
  domains: string[];
  atom_types: string[];
  sources: string[];
  template_ids?: string[];
  qualities?: string[];
  resolver_primitives?: string[];
};

export type SearchResult = {
  items: Instrument[];
  total: number;
  facets: Facets;
};

export type FormulaValidation = {
  valid: boolean;
  errors: string[];
  referenced_ids: string[];
  operator_count: number;
};

export type Discovery = {
  answer: string;
  recommendation_id: string | null;
  ranked_ids: string[];
  actions: string[];
};

export type TradeProposal = {
  instrument_id: string;
  market_id: number;
  side: "BUY_YES" | "BUY_NO" | "SELL_YES" | "SELL_NO";
  limit_price: number;
  quantity: number;
  notional: number;
  rationale: string;
};

export type Draft = Omit<Instrument, "market_id" | "leaf_ids" | "market" | "last_price">;
