use serde::{Deserialize, Deserializer};

use crate::error::Error;

/// Deserialize a value that might be a JSON string, number, or null as Option<f64>.
/// Polymarket returns some numeric fields as strings at market level but numbers at event level.
fn string_or_float<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    use serde::de;

    struct Visitor;
    impl<'de> de::Visitor<'de> for Visitor {
        type Value = Option<f64>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string, number, or null")
        }
        fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
            Ok(Some(v))
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Some(v as f64))
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(Some(v as f64))
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            if v.is_empty() {
                Ok(None)
            } else {
                v.parse::<f64>().map(Some).map_err(de::Error::custom)
            }
        }
        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
    }

    deserializer.deserialize_any(Visitor)
}

/// Event from Gamma API. Contains one or more markets.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GammaEvent {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub closed: bool,
    #[serde(default)]
    pub enable_neg_risk: bool,
    /// Alias: Gamma returns both `negRisk` and `enableNegRisk`.
    #[serde(default, alias = "negRisk")]
    pub neg_risk: bool,
    #[serde(default)]
    pub markets: Vec<GammaMarket>,
    #[serde(default, deserialize_with = "string_or_float")]
    pub volume: Option<f64>,
    #[serde(default, deserialize_with = "string_or_float")]
    pub liquidity: Option<f64>,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

impl GammaEvent {
    /// Whether this is a NegRisk multi-outcome event.
    /// The API returns both `negRisk` and `enableNegRisk`; either being true suffices.
    pub fn is_neg_risk(&self) -> bool {
        self.neg_risk || self.enable_neg_risk
    }
}

/// Market nested inside a GammaEvent.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GammaMarket {
    pub condition_id: String,
    pub question: String,
    /// JSON-encoded string: `["Yes", "No"]`
    #[serde(default)]
    pub outcomes: String,
    /// JSON-encoded string: `["0.55", "0.45"]`
    #[serde(default)]
    pub outcome_prices: String,
    /// JSON-encoded string of token IDs (large integers as strings).
    #[serde(default)]
    pub clob_token_ids: String,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub closed: bool,
    #[serde(default)]
    pub neg_risk: bool,
    /// Short outcome name for NegRisk multi-outcome events.
    #[serde(default)]
    pub group_item_title: Option<String>,
    #[serde(default, deserialize_with = "string_or_float")]
    pub best_bid: Option<f64>,
    #[serde(default, deserialize_with = "string_or_float")]
    pub best_ask: Option<f64>,
    #[serde(default, deserialize_with = "string_or_float")]
    pub last_trade_price: Option<f64>,
    #[serde(default, deserialize_with = "string_or_float")]
    pub volume: Option<f64>,
    #[serde(default, deserialize_with = "string_or_float")]
    pub liquidity: Option<f64>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default)]
    pub resolution_source: Option<String>,
}

impl GammaMarket {
    /// Parse the double-encoded token IDs.
    /// `clobTokenIds` is a JSON string like `["123...", "456..."]` inside the JSON response.
    pub fn parsed_token_ids(&self) -> Result<Vec<String>, Error> {
        if self.clob_token_ids.is_empty() {
            return Ok(vec![]);
        }
        serde_json::from_str(&self.clob_token_ids).map_err(Error::Json)
    }

    /// Parse the double-encoded outcome prices.
    /// Returns prices as f64 (0.0 to 1.0).
    pub fn parsed_outcome_prices(&self) -> Result<Vec<f64>, Error> {
        if self.outcome_prices.is_empty() {
            return Ok(vec![]);
        }
        let strings: Vec<String> = serde_json::from_str(&self.outcome_prices)?;
        strings
            .iter()
            .map(|s| {
                s.parse::<f64>()
                    .map_err(|e| Error::PolymarketApi(format!("bad price '{}': {}", s, e)))
            })
            .collect()
    }

    /// Parse the double-encoded outcomes list.
    pub fn parsed_outcomes(&self) -> Result<Vec<String>, Error> {
        if self.outcomes.is_empty() {
            return Ok(vec![]);
        }
        serde_json::from_str(&self.outcomes).map_err(Error::Json)
    }

    /// Best estimate of the current YES price.
    pub fn yes_price(&self) -> Option<f64> {
        // Try outcome_prices first, fall back to best_bid/ask midpoint, then last_trade
        if let Ok(prices) = self.parsed_outcome_prices() {
            if !prices.is_empty() {
                return Some(prices[0]);
            }
        }
        match (self.best_bid, self.best_ask) {
            (Some(bid), Some(ask)) => Some((bid + ask) / 2.0),
            _ => self.last_trade_price,
        }
    }
}

/// WebSocket message from the Polymarket CLOB.
/// We only care about a subset of message types.
#[derive(Debug, Clone, Deserialize)]
pub struct ClobWsMessage {
    #[serde(default)]
    pub event_type: Option<String>,
    #[serde(default)]
    pub asset_id: Option<String>,
    #[serde(default)]
    pub market: Option<String>,
    #[serde(default)]
    pub price: Option<String>,
    #[serde(default)]
    pub best_bid: Option<String>,
    #[serde(default)]
    pub best_ask: Option<String>,
    // For new_market / market_resolved events
    #[serde(default)]
    pub winning_asset_id: Option<String>,
}

impl ClobWsMessage {
    /// Extract midpoint price from this message, if available.
    /// Works for price_change, last_trade_price, and best_bid_ask messages.
    pub fn midpoint(&self) -> Option<(String, f64)> {
        let asset_id = self.asset_id.as_ref()?;

        // Direct price field (last_trade_price, price_change)
        if let Some(ref price_str) = self.price {
            if let Ok(p) = price_str.parse::<f64>() {
                return Some((asset_id.clone(), p));
            }
        }

        // Bid/ask midpoint (best_bid_ask)
        if let (Some(ref bid_str), Some(ref ask_str)) = (&self.best_bid, &self.best_ask) {
            if let (Ok(bid), Ok(ask)) = (bid_str.parse::<f64>(), ask_str.parse::<f64>()) {
                return Some((asset_id.clone(), (bid + ask) / 2.0));
            }
        }

        None
    }
}

/// Midpoint response from CLOB REST API.
#[derive(Debug, Deserialize)]
pub struct MidpointResponse {
    pub mid: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_clob_token_ids() {
        let market = GammaMarket {
            condition_id: "0xabc".into(),
            question: "Test?".into(),
            outcomes: r#"["Yes","No"]"#.into(),
            outcome_prices: r#"["0.55","0.45"]"#.into(),
            clob_token_ids: r#"["123456789","987654321"]"#.into(),
            active: true,
            closed: false,
            neg_risk: false,
            group_item_title: None,
            best_bid: None,
            best_ask: None,
            last_trade_price: None,
            volume: None,
            liquidity: None,
            slug: None,
            description: None,
            end_date: None,
            resolution_source: None,
        };

        let ids = market.parsed_token_ids().unwrap();
        assert_eq!(ids, vec!["123456789", "987654321"]);

        let prices = market.parsed_outcome_prices().unwrap();
        assert_eq!(prices, vec![0.55, 0.45]);

        let outcomes = market.parsed_outcomes().unwrap();
        assert_eq!(outcomes, vec!["Yes", "No"]);

        assert!((market.yes_price().unwrap() - 0.55).abs() < 1e-9);
    }

    #[test]
    fn parse_empty_fields() {
        let market = GammaMarket {
            condition_id: "0x".into(),
            question: "Empty".into(),
            outcomes: String::new(),
            outcome_prices: String::new(),
            clob_token_ids: String::new(),
            active: false,
            closed: false,
            neg_risk: false,
            group_item_title: None,
            best_bid: None,
            best_ask: None,
            last_trade_price: None,
            volume: None,
            liquidity: None,
            slug: None,
            description: None,
            end_date: None,
            resolution_source: None,
        };

        assert!(market.parsed_token_ids().unwrap().is_empty());
        assert!(market.parsed_outcome_prices().unwrap().is_empty());
        assert!(market.yes_price().is_none());
    }

    #[test]
    fn ws_message_midpoint() {
        let msg = ClobWsMessage {
            event_type: Some("last_trade_price".into()),
            asset_id: Some("12345".into()),
            market: None,
            price: Some("0.65".into()),
            best_bid: None,
            best_ask: None,
            winning_asset_id: None,
        };
        let (id, price) = msg.midpoint().unwrap();
        assert_eq!(id, "12345");
        assert!((price - 0.65).abs() < 1e-9);

        let msg2 = ClobWsMessage {
            event_type: Some("best_bid_ask".into()),
            asset_id: Some("99".into()),
            market: None,
            price: None,
            best_bid: Some("0.40".into()),
            best_ask: Some("0.50".into()),
            winning_asset_id: None,
        };
        let (_, price) = msg2.midpoint().unwrap();
        assert!((price - 0.45).abs() < 1e-9);
    }
}
