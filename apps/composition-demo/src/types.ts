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
};

export type DemoState = {
  instruments: Instrument[];
  accounts: Record<string, number>;
  events: Array<{ event: string; timestamp: number }>;
  sybil_url: string;
  sybil_error?: string;
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

