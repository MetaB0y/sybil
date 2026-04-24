export type Formula = { condition?: string; atom?: string; op?: string; args?: Formula[]; k?: number };

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
  kind: "condition" | "proposition" | "atom" | "composition";
  object_kind?: "condition" | "proposition" | "measurement" | "feed" | "entity" | "context";
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
  measurement_id?: string;
  measurement_kind?: string;
  measurement?: Measurement;
  feed_ids?: string[];
  aggregation_semantics?: string;
  entity_ids?: string[];
  context_id?: string;
  path?: string[];
  display_title?: string;
  entities?: Entity[];
  context?: Context;
  predicate?: Record<string, unknown>;
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
  feeds?: DataFeed[];
  entities?: Entity[];
  contexts?: Context[];
  measurements?: Measurement[];
  conditions?: Instrument[];
  propositions?: Instrument[];
  markets?: Array<{ instrument_id: string; market_id: number; kind: string; question: string }>;
  implication_edges?: ImplicationEdge[];
  accounts: Record<string, number>;
  events: Array<{ event: string; timestamp: number }>;
  sybil_url: string;
  sybil_error?: string;
  facets?: Facets;
  source_counts?: Record<string, number>;
  source_errors?: string[];
  instrument_counts?: {
    atoms: number;
    conditions?: number;
    compositions: number;
    propositions?: number;
    measurements?: number;
    entities?: number;
    contexts?: number;
    feeds?: number;
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
  object_kinds?: string[];
  measurement_kinds?: string[];
  measurement_ids?: string[];
  entity_ids?: string[];
  context_ids?: string[];
  predicate_ops?: string[];
};

export type SearchResult = {
  items: Instrument[];
  total: number;
  facets: Facets;
};

export type FormulaValidation = {
  valid: boolean;
  errors: string[];
  warnings?: string[];
  referenced_ids: string[];
  referenced_conditions?: string[];
  operator_count: number;
  canonical_key?: string;
  duplicate?: boolean;
};

export type Discovery = {
  mode?: string;
  answer: string;
  recommendation_id: string | null;
  ranked_ids: string[];
  actions: string[];
  questions?: string[];
  thesis?: string;
  proxy_markets?: string[];
  hedge_markets?: string[];
  creation_prompt?: string;
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

export type DataFeed = {
  id: string;
  name: string;
  domain: string;
  trust_tier: string;
  resolver_primitive: string;
  description: string;
};

export type Measurement = {
  id: string;
  object_kind: "measurement";
  domain: string;
  measurement_kind: string;
  subject: string;
  unit: string;
  feed_ids: string[];
  aggregation_semantics: string;
  title: string;
  description: string;
  resolver_primitive: string;
  canonical_key: string;
  entity_ids?: string[];
  context_id?: string;
  path?: string[];
  display_title?: string;
};

export type Entity = {
  id: string;
  object_kind: "entity";
  kind: string;
  name: string;
  title: string;
  short_name: string;
  domain: string;
  aliases?: string[];
  external_refs?: Record<string, string>;
  description: string;
};

export type Context = {
  id: string;
  object_kind: "context";
  kind: string;
  title: string;
  short_name: string;
  domain: string;
  description: string;
  entity_ids?: string[];
  start?: string;
  end?: string;
};

export type ImplicationEdge = {
  from: string;
  to: string;
  type: string;
  label: string;
  no_arb: string;
};

export type WizardDraft = {
  draft_id?: string;
  title: string;
  short_name?: string;
  question?: string;
  description?: string;
  domain?: string;
  formula: Formula;
  validation?: FormulaValidation;
  referenced_conditions?: Instrument[];
  implication_edges?: ImplicationEdge[];
};
