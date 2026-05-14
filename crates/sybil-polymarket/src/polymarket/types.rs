use serde::{Deserialize, Deserializer};

use crate::error::Error;

/// Parse an ISO-8601 / RFC-3339 UTC timestamp (e.g. `"2025-12-31T12:00:00Z"`)
/// to epoch milliseconds. Polymarket's `endDate` fields always use `Z`-suffixed
/// UTC, so we don't bother with offsets. Returns `None` if the string doesn't
/// match the expected shape.
///
/// The civil-to-days formula is Howard Hinnant's well-known algorithm; correct
/// for any Gregorian date and avoids pulling in a date crate for one helper.
pub fn parse_iso8601_to_ms(s: &str) -> Option<i64> {
    // Accept `YYYY-MM-DDTHH:MM:SS` followed by `Z` or `.fff…Z` (fractional secs
    // tolerated, ignored). Anything else → None.
    let bytes = s.as_bytes();
    if bytes.len() < 20 {
        return None;
    }
    let year: i32 = s.get(0..4)?.parse().ok()?;
    let month: u32 = s.get(5..7)?.parse().ok()?;
    let day: u32 = s.get(8..10)?.parse().ok()?;
    let hour: u32 = s.get(11..13)?.parse().ok()?;
    let minute: u32 = s.get(14..16)?.parse().ok()?;
    let second: u32 = s.get(17..19)?.parse().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let secs_of_day = (hour as i64) * 3600 + (minute as i64) * 60 + (second as i64);
    Some((days * 86_400 + secs_of_day) * 1_000)
}

/// Howard Hinnant's `days_from_civil` — number of days since 1970-01-01 for a
/// Gregorian Y/M/D. Works for any valid date.
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * if m > 2 { m - 3 } else { m + 9 } + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era as i64 * 146_097 + doe as i64 - 719_468
}

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

/// Tag attached to a Gamma event. We only care about `label` for category
/// derivation, but we accept `slug` too for symmetry. Gamma sometimes omits
/// the slug, so both default to empty.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GammaTag {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub slug: String,
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
    #[serde(default)]
    pub tags: Vec<GammaTag>,
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
    /// Event-level image URL (primary).
    #[serde(default)]
    pub image: Option<String>,
    /// Event-level icon URL (used as a secondary URL by the frontend).
    #[serde(default)]
    pub icon: Option<String>,
}

impl GammaEvent {
    /// Whether this is a NegRisk multi-outcome event.
    /// The API returns both `negRisk` and `enableNegRisk`; either being true suffices.
    pub fn is_neg_risk(&self) -> bool {
        self.neg_risk || self.enable_neg_risk
    }

    /// Category filters are matched against Polymarket event tag labels and
    /// slugs. Gamma's top-level `category` field is usually absent for events.
    pub fn matches_category_filters(&self, include: &[String], exclude: &[String]) -> bool {
        if tags_match_any(&self.tags, exclude) {
            return false;
        }
        include.is_empty() || tags_match_any(&self.tags, include)
    }

    pub fn tag_labels(&self) -> Vec<String> {
        self.tags
            .iter()
            .filter_map(|tag| {
                let label = tag.label.trim();
                (!label.is_empty()).then(|| label.to_string())
            })
            .collect()
    }

    pub fn primary_category(&self) -> Option<String> {
        self.tags.iter().find_map(|tag| {
            let label = tag.label.trim();
            let force_hidden = tag.slug.starts_with("hide-")
                || tag.slug.starts_with("rewards-")
                || tag.slug.starts_with("earn-");
            (!label.is_empty() && !force_hidden).then(|| label.to_string())
        })
    }
}

fn tags_match_any(tags: &[GammaTag], filters: &[String]) -> bool {
    if filters.is_empty() {
        return false;
    }
    let filters: Vec<String> = filters.iter().map(|f| normalize_filter(f)).collect();
    tags.iter().any(|tag| {
        let label = normalize_filter(&tag.label);
        let slug = normalize_filter(&tag.slug);
        filters
            .iter()
            .any(|filter| filter == &label || filter == &slug)
    })
}

fn normalize_filter(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
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
    /// Per-market image URL (primary). May differ from the event-level image
    /// on NegRisk markets where each outcome has its own picture.
    #[serde(default)]
    pub image: Option<String>,
    /// Per-market icon URL (secondary).
    #[serde(default)]
    pub icon: Option<String>,
    /// True once Polymarket has settled the market. Paired with `outcome_prices`
    /// pinned to 0/1 to derive the YES payout.
    #[serde(default)]
    pub umared: Option<bool>,
    #[serde(default)]
    pub resolved_by: Option<String>,
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

    /// Returns the resolved YES payout in nanos if Polymarket has settled the
    /// market unambiguously (binary; outcome_prices pinned to {0.0, 1.0}).
    /// Returns `None` for anything ambiguous — non-binary, UMA-challenged,
    /// voided, or not yet resolved. SYB-23 intentionally only mirrors these
    /// clean cases.
    pub fn resolved_payout(&self) -> Option<u64> {
        if !self.closed {
            return None;
        }
        let prices = self.parsed_outcome_prices().ok()?;
        if prices.len() != 2 {
            return None;
        }
        let yes = prices[0];
        let no = prices[1];
        // Require crisp binary outcome: one side = 1.0, other = 0.0, tolerating
        // rounding within 1e-6.
        let clean_yes = (yes - 1.0).abs() < 1e-6 && no.abs() < 1e-6;
        let clean_no = yes.abs() < 1e-6 && (no - 1.0).abs() < 1e-6;
        if clean_yes {
            Some(sybil_api_types::NANOS_PER_DOLLAR)
        } else if clean_no {
            Some(0)
        } else {
            None
        }
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
            image: None,
            icon: None,
            umared: None,
            resolved_by: None,
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
    fn category_filters_match_event_tags() {
        let event = GammaEvent {
            id: "e1".into(),
            title: "Election".into(),
            description: String::new(),
            slug: String::new(),
            active: true,
            closed: false,
            enable_neg_risk: false,
            neg_risk: false,
            markets: Vec::new(),
            tags: vec![
                GammaTag {
                    label: "Global Elections".into(),
                    slug: "global-elections".into(),
                },
                GammaTag {
                    label: "Politics".into(),
                    slug: "politics".into(),
                },
            ],
            volume: None,
            liquidity: None,
            start_date: None,
            end_date: None,
            created_at: None,
            image: None,
            icon: None,
        };

        assert!(event.matches_category_filters(&["global elections".into()], &[]));
        assert!(event.matches_category_filters(&["global-elections".into()], &[]));
        assert!(!event.matches_category_filters(&["sports".into()], &[]));
        assert!(!event.matches_category_filters(&[], &["politics".into()]));
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
            image: None,
            icon: None,
            umared: None,
            resolved_by: None,
        };

        assert!(market.parsed_token_ids().unwrap().is_empty());
        assert!(market.parsed_outcome_prices().unwrap().is_empty());
        assert!(market.yes_price().is_none());
    }

    #[test]
    fn iso8601_to_ms() {
        // Polymarket's canonical shape.
        assert_eq!(
            parse_iso8601_to_ms("2025-12-31T12:00:00Z"),
            Some(1_767_182_400_000)
        );
        // Epoch zero.
        assert_eq!(parse_iso8601_to_ms("1970-01-01T00:00:00Z"), Some(0));
        // Past date (negative epoch).
        assert_eq!(parse_iso8601_to_ms("1969-12-31T23:59:59Z"), Some(-1_000));
        // Fractional seconds tolerated (we read only the first 19 chars).
        assert_eq!(
            parse_iso8601_to_ms("2025-12-31T12:00:00.123Z"),
            Some(1_767_182_400_000)
        );
        // Garbage → None.
        assert_eq!(parse_iso8601_to_ms(""), None);
        assert_eq!(parse_iso8601_to_ms("not a date"), None);
        assert_eq!(parse_iso8601_to_ms("2025-13-31T12:00:00Z"), None);
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
