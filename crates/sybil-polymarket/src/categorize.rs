//! Polymarket tag → Sybil category derivation.
//!
//! `event.category` is always `null` in Gamma's responses; the real signal
//! lives in `event.tags[].label`. Tags are noisy and overlap (a Kraken IPO
//! event might carry `[exchange, Tech, Crypto, Finance, Business, IPOs]`), so
//! we collapse them onto a fixed 15-bucket taxonomy via a priority-ordered
//! lookup. The first row whose label set has a case-insensitive intersection
//! with `event.tags[].label` wins.
//!
//! The table is hardcoded by design: ops shouldn't need a code change to
//! grow it, but the long tail is small enough today that a config file
//! would be overkill. New unmatched labels are logged at `info!` so we can
//! see what to add later.
//!
//! Off-block: this category never enters `MarketMetadata` or the block
//! digest. It rides through `MarketRefData` → `MarketResponse.category` for
//! display only.

use tracing::info;

use crate::polymarket::types::GammaTag;

/// One row of the priority table.
struct CategoryRule {
    bucket: &'static str,
    /// Tag labels that map to this bucket. Case-insensitive comparison.
    labels: &'static [&'static str],
}

/// Walks top-to-bottom; the first bucket whose `labels` intersects `tags`
/// wins. See module doc for rationale.
const TABLE: &[CategoryRule] = &[
    // Elections wins over Politics: a market tagged with both `Elections`
    // and `Trump` is conceptually an election market.
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
        // `Politics` (the literal Polymarket tag) was the gap that put 161
        // election-adjacent events in the "no category" bucket on the first
        // deploy. The specific Trump/Congress/Senate labels stay so future
        // single-topic Politics events still bucket correctly.
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

/// Derive a category bucket from a Polymarket event's tags. Returns `None`
/// when no tag matches any row (the frontend renders no chip in that case).
/// Logs each unmatched label at `info!` so the table can be grown from
/// observed tags.
pub fn derive_category(tags: &[GammaTag]) -> Option<String> {
    if tags.is_empty() {
        return None;
    }

    let normalized: Vec<String> = tags.iter().map(|t| t.label.to_lowercase()).collect();

    for rule in TABLE {
        for label in rule.labels {
            if normalized.iter().any(|t| t == &label.to_lowercase()) {
                return Some(rule.bucket.to_string());
            }
        }
    }

    // Log the unmatched labels so we can grow the table later.
    let unmatched: Vec<&str> = tags
        .iter()
        .map(|t| t.label.as_str())
        .filter(|l| !l.is_empty())
        .collect();
    if !unmatched.is_empty() {
        info!(tags = ?unmatched, "no category match for tags");
    }
    None
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
    fn empty_tags_no_category() {
        assert_eq!(derive_category(&[]), None);
    }

    #[test]
    fn each_row_matches() {
        let cases = [
            ("Elections", "Elections"),
            ("World Elections", "Elections"),
            ("US Election", "Elections"),
            ("Primaries", "Elections"),
            ("President", "Elections"),
            ("Politics", "Politics"),
            ("Trump", "Politics"),
            ("Senate", "Politics"),
            ("Geopolitics", "Geopolitics"),
            ("AI", "AI"),
            ("Tech", "Tech"),
            ("Economy", "Economy"),
            ("Culture", "Culture"),
            ("Movies", "Culture"),
            ("Music", "Culture"),
            ("Science", "Science"),
            ("World", "World"),
            ("Finance", "Finance"),
            ("IPOs", "Finance"),
            ("IPO", "Finance"),
            ("Earnings", "Finance"),
            ("Stocks", "Finance"),
            ("Business", "Business"),
            ("Weather", "Weather"),
            ("Mentions", "Mentions"),
            ("NBA", "Sports"),
            ("Soccer", "Sports"),
            ("Formula 1", "Sports"),
            ("Esports", "Sports"),
            ("Crypto", "Crypto"),
            ("Commodities", "Commodities"),
        ];
        for (label, expected) in cases {
            let got = derive_category(&[tag(label)]);
            assert_eq!(
                got.as_deref(),
                Some(expected),
                "tag {:?} should map to {}",
                label,
                expected
            );
        }
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(derive_category(&[tag("nba")]).as_deref(), Some("Sports"));
        assert_eq!(derive_category(&[tag("TRUMP")]).as_deref(), Some("Politics"));
        assert_eq!(derive_category(&[tag("CrYpTo")]).as_deref(), Some("Crypto"));
    }

    #[test]
    fn priority_resolves_ambiguity() {
        // Politics (row 2) beats Sports (row 14).
        let tags = vec![tag("NBA"), tag("Trump")];
        assert_eq!(derive_category(&tags).as_deref(), Some("Politics"));
    }

    #[test]
    fn elections_beats_politics() {
        // A market tagged with both should land in Elections, not Politics.
        let tags = vec![tag("Trump"), tag("Elections"), tag("US Election")];
        assert_eq!(derive_category(&tags).as_deref(), Some("Elections"));
    }

    #[test]
    fn live_election_tags_from_prod_logs() {
        // Real tag set from sybil-polymarket logs that fell through on the
        // first deploy. After the Elections row, these must categorize.
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
        assert_eq!(derive_category(&tags).as_deref(), Some("Elections"));
    }

    #[test]
    fn worked_examples_from_live_api() {
        // Kraken IPO event tags: Tech wins over Crypto/Finance/Business.
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
        assert_eq!(derive_category(&kraken).as_deref(), Some("Tech"));

        // MicroStrategy event tags: Economy wins over Finance/Business/Crypto.
        let mstr = vec![
            tag("Finance"),
            tag("Economy"),
            tag("Business"),
            tag("2025 Predictions"),
            tag("Crypto"),
            tag("MicroStrategy"),
            tag("Stocks"),
        ];
        assert_eq!(derive_category(&mstr).as_deref(), Some("Economy"));
    }

    #[test]
    fn unmatched_tags_return_none() {
        let tags = vec![tag("exchange"), tag("2025 Predictions"), tag("Featured")];
        assert_eq!(derive_category(&tags), None);
    }
}
