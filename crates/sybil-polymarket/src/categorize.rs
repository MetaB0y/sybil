//! Polymarket tag → Sybil category-bucket derivation.
//!
//! `event.category` is always `null` in Gamma's responses; the real signal
//! lives in `event.tags[].label`. Tags are noisy and overlap (a Kraken IPO
//! event might carry `[exchange, Tech, Crypto, Finance, Business, IPOs]`).
//!
//! Each row in [`TABLE`] is a `(bucket, &[labels])` pair. For every market
//! we walk every row, and a row whose labels intersect the event's tags
//! contributes its bucket to the result. So a market tagged `[NBA, Trump]`
//! returns **both** `"Sports"` and `"Politics"`; the frontend has its own
//! priority list and picks which one to surface.
//!
//! Off-block: these category strings never enter `MarketMetadata` or the
//! block digest. They ride through `MarketRefData` → `MarketResponse.categories`
//! for display only.
//!
//! Changing display priority is a frontend-only edit. Adding a new bucket or
//! a new label needs a backend rebuild (because matching lives here).

use crate::polymarket::types::GammaTag;

/// One row of the bucket table.
struct CategoryRule {
    bucket: &'static str,
    /// Tag labels that map to this bucket. Case-insensitive comparison.
    labels: &'static [&'static str],
}

/// The bucket table. Row order is **not** display order — the frontend has
/// its own priority list. The order here only matters for the order entries
/// land in the result Vec (which the frontend can re-sort freely).
const TABLE: &[CategoryRule] = &[
    CategoryRule {
        bucket: "Elections",
        labels: &[
            "Elections",
            "World Elections",
            "Global Elections",
            "US Election",
            "Primaries",
            "Main Election",
            "President",
        ],
    },
    CategoryRule {
        bucket: "Politics",
        labels: &["Politics", "Trump", "Congress", "Senate"],
    },
    CategoryRule {
        bucket: "Geopolitics",
        labels: &["Geopolitics"],
    },
    CategoryRule {
        bucket: "AI",
        labels: &["AI"],
    },
    CategoryRule {
        bucket: "Tech",
        labels: &["Tech"],
    },
    CategoryRule {
        bucket: "Economy",
        labels: &["Economy"],
    },
    CategoryRule {
        bucket: "Culture",
        labels: &["Culture", "Movies", "Music", "Celebritards"],
    },
    CategoryRule {
        bucket: "Science",
        labels: &["Science"],
    },
    CategoryRule {
        bucket: "World",
        labels: &["World"],
    },
    CategoryRule {
        bucket: "Finance",
        labels: &["Finance", "Stocks", "Earnings", "IPOs", "IPO"],
    },
    CategoryRule {
        bucket: "Business",
        labels: &["Business"],
    },
    CategoryRule {
        bucket: "Weather",
        labels: &["Weather"],
    },
    CategoryRule {
        bucket: "Mentions",
        labels: &["Mentions"],
    },
    CategoryRule {
        bucket: "Sports",
        labels: &[
            "Sports",
            "Soccer",
            "NFL",
            "NBA",
            "NHL",
            "MLB",
            "UFC",
            "Tennis",
            "Boxing",
            "Cricket",
            "Chess",
            "Hockey",
            "Football",
            "Golf",
            "Formula 1",
            "Pickleball",
            "EPL",
            "MLS",
            "PGA",
            "Esports",
        ],
    },
    CategoryRule {
        bucket: "Crypto",
        labels: &["Crypto"],
    },
    CategoryRule {
        bucket: "Commodities",
        labels: &["Commodities"],
    },
];

/// Derive every category bucket whose label set intersects the event's
/// tags. Returns an empty Vec when nothing matches (caller turns that into
/// `None` for the off-block field).
///
/// Comparison is case-insensitive. A bucket appears at most once even if
/// multiple of its labels match. The returned order matches [`TABLE`] row
/// order so it's stable, but the frontend re-prioritizes anyway.
pub fn derive_categories(tags: &[GammaTag]) -> Vec<String> {
    if tags.is_empty() {
        return Vec::new();
    }
    let normalized: Vec<String> = tags.iter().map(|t| t.label.to_lowercase()).collect();
    let mut out = Vec::new();
    for rule in TABLE {
        let hit = rule
            .labels
            .iter()
            .any(|lbl| normalized.iter().any(|t| t == &lbl.to_lowercase()));
        if hit {
            out.push(rule.bucket.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(label: &str) -> GammaTag {
        GammaTag {
            label: label.to_string(),
            slug: String::new(),
        }
    }

    #[test]
    fn empty_tags_returns_empty() {
        assert!(derive_categories(&[]).is_empty());
    }

    #[test]
    fn single_match_returns_one_bucket() {
        assert_eq!(derive_categories(&[tag("NBA")]), vec!["Sports"]);
        assert_eq!(derive_categories(&[tag("Trump")]), vec!["Politics"]);
        assert_eq!(derive_categories(&[tag("Crypto")]), vec!["Crypto"]);
    }

    #[test]
    fn multi_match_returns_all_buckets() {
        // Tags `[NBA, Trump]` → Sports AND Politics (both buckets matched).
        // Order follows TABLE row order (Politics row 2, Sports row 14).
        let got = derive_categories(&[tag("NBA"), tag("Trump")]);
        assert_eq!(got, vec!["Politics", "Sports"]);
    }

    #[test]
    fn user_example_sports_politics_football() {
        // `[Sports, Politics, Football]` → Politics + Sports (Politics row
        // matches "Politics"; Sports row matches "Sports" + "Football",
        // but Sports bucket appears once).
        let got = derive_categories(&[tag("Sports"), tag("Politics"), tag("Football")]);
        assert_eq!(got, vec!["Politics", "Sports"]);
    }

    #[test]
    fn user_example_geopolitics_politics() {
        let got = derive_categories(&[tag("Geopolitics"), tag("Politics")]);
        assert_eq!(got, vec!["Politics", "Geopolitics"]);
    }

    #[test]
    fn user_example_sports_barcelona_world_cup() {
        // Only "Sports" matches; "Barcelona" and "World Cup" are not in our
        // table (we don't map team / tournament names directly).
        let got = derive_categories(&[tag("Sports"), tag("Barcelona"), tag("World Cup")]);
        assert_eq!(got, vec!["Sports"]);
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(derive_categories(&[tag("nba")]), vec!["Sports"]);
        assert_eq!(derive_categories(&[tag("TRUMP")]), vec!["Politics"]);
        assert_eq!(derive_categories(&[tag("CrYpTo")]), vec!["Crypto"]);
    }

    #[test]
    fn kraken_event_full() {
        // The full Kraken IPO tag set must produce Tech + Finance + Business
        // + Crypto (and not Economy, which the table doesn't map for these
        // tags). Order is row-order.
        let kraken = vec![
            tag("exchange"),
            tag("Tech"),
            tag("Crypto"),
            tag("Finance"),
            tag("Business"),
            tag("2025 Predictions"),
            tag("Featured"),
            tag("IPOs"),
        ];
        assert_eq!(
            derive_categories(&kraken),
            vec!["Tech", "Finance", "Business", "Crypto"]
        );
    }

    #[test]
    fn live_election_tags_from_prod_logs() {
        // Real tag set that fell through pre-Elections. After the Elections
        // row, these must categorize as Elections + Politics.
        let tags = vec![
            tag("World Elections"),
            tag("Global Elections"),
            tag("Elections"),
            tag("Politics"),
            tag("US Election"),
            tag("Earn 4%"),
            tag("Primaries"),
            tag("United States"),
        ];
        assert_eq!(derive_categories(&tags), vec!["Elections", "Politics"]);
    }

    #[test]
    fn unmatched_tags_returns_empty() {
        let tags = vec![tag("exchange"), tag("2025 Predictions"), tag("Featured")];
        assert!(derive_categories(&tags).is_empty());
    }
}
