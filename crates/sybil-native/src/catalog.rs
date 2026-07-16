//! Native Sybil market template catalog.
//!
//! This is the non-Polymarket creation path: checked-in JSON defines native
//! markets, the sync actor expands enabled templates into Sybil market-create
//! requests, and native provenance deliberately never sets
//! `polymarket_condition_id`. The field remaining null is the mirror/native
//! discriminator established by SYB-150.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sybil_api_types::{CreateMarketRequest, SetMarketMetadataRequest};
use url::Url;

use crate::error::Error;
use crate::parse_iso8601_to_ms;

const CATEGORICAL_SUM_TOLERANCE: f64 = 1e-9;

/// Parsed native market catalog.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct NativeMarketCatalog {
    /// Free-form provenance note for humans editing the file. Ignored by code.
    #[serde(default)]
    pub description: String,
    /// Native market templates, in listing order.
    #[serde(default)]
    pub markets: Vec<NativeMarketTemplate>,
}

/// One native market template.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NativeMarketTemplate {
    /// Stable catalog id. This is the native idempotency key.
    pub id: String,
    /// Per-template enablement. Disabled placeholders validate but do not
    /// create markets or seed the MM.
    pub enabled: bool,
    /// Event/market title.
    pub title: String,
    /// Binary or categorical/multi-outcome shape.
    pub outcome_set: NativeOutcomeSet,
    /// Display units, e.g. "probability" or "percent".
    pub units: String,
    /// RFC-3339 UTC display end time.
    pub end_time: String,
    /// Frontend-displayable resolution criteria text.
    pub resolution_criteria: String,
    /// Primary source URL used in `external_url`.
    pub source_url: String,
    /// Single display category for native markets.
    pub category: String,
    /// Candidate resolution adapter, scoped now but not implemented yet.
    pub resolution_source: ResolutionSourceConfig,
    /// Binary-market quote range. Categorical templates use per-outcome ranges.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_range: Option<NativeQuoteRange>,
    /// Optional event/group card image (logo or hero). Becomes each child
    /// market's `event_image_url`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_image_url: Option<String>,
    /// Optional event/group icon (frontend `onError` fallback for the image).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_icon_url: Option<String>,
}

/// Template outcome shape.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NativeOutcomeSet {
    /// Single binary market. Sybil binary outcomes are still YES/NO; these
    /// labels document how the frontend should describe them.
    Binary { yes: String, no: String },
    /// Categorical/multi-outcome template. Each enabled outcome becomes one
    /// binary market, and all enabled outcomes are placed in one MarketGroup.
    Categorical { outcomes: Vec<NativeOutcome> },
    /// Nested threshold ladder. Each enabled rung becomes one independent
    /// binary child market ("value {direction} {threshold} {unit}"). Rungs
    /// share display-event metadata but never enter a core `MarketGroup`,
    /// because several nested thresholds can resolve YES simultaneously.
    Threshold {
        /// Bigger-is-the-question or smaller-is-the-question semantics.
        direction: NativeThresholdDirection,
        /// Display unit, e.g. `"tokens"`, `"hours"`, `"USD/MTok"`.
        unit: String,
        outcomes: Vec<NativeThresholdRung>,
    },
}

/// Comparison direction for one nested threshold ladder.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeThresholdDirection {
    Above,
    Below,
}

/// One categorical outcome.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NativeOutcome {
    /// Stable outcome id within the parent template.
    pub id: String,
    /// Compact display label for this outcome inside the parent event card.
    pub title: String,
    /// Natural-language binary question used as the child market's title.
    /// This deliberately mirrors Polymarket's event-title + market-question
    /// model instead of synthesizing `"{event}: {outcome}"`.
    pub market_title: String,
    /// Per-outcome enablement.
    pub enabled: bool,
    /// YES-price quote range used to seed the MM for this child market.
    pub quote_range: NativeQuoteRange,
    /// Optional per-child market image (e.g. the company logo). Becomes this
    /// outcome's `market_image_url`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
}

/// One rung of a threshold ladder. Each enabled rung expands to an independent
/// binary child market displayed under the parent event.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NativeThresholdRung {
    /// Stable rung id within the parent template.
    pub id: String,
    /// Compact display label, e.g. "≥ 8 hours" or "≤ $5 / MTok".
    pub title: String,
    /// Natural-language binary question used as the child market's title.
    pub market_title: String,
    /// Per-rung enablement.
    pub enabled: bool,
    /// Numeric rung threshold, expressed in the ladder's `unit`.
    pub threshold: f64,
    /// YES-price quote range used to seed the MM for this rung's child market.
    pub quote_range: NativeQuoteRange,
    /// Optional per-child market image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
}

/// YES-price quote range used by the native MM bootstrap.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct NativeQuoteRange {
    pub min: f64,
    pub max: f64,
    pub initial: f64,
}

/// Native resolution-source config.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResolutionSourceConfig {
    /// Manual resolution is an operator workflow: humans inspect the listed
    /// source and submit an admin/signed resolution through Sybil. This ticket
    /// only records the intended source and display instructions.
    Manual { instructions: String },
    /// API polling is the future automated adapter shape: a source-specific
    /// poller would fetch a deterministic endpoint, map its response into a
    /// payout, and submit a resolution. No HTTP adapter is implemented here.
    ApiPoll {
        endpoint: String,
        #[serde(default)]
        method: Option<String>,
        notes: String,
    },
}

/// Skeleton for future native resolution adapters.
///
/// Implementations are intentionally out of scope for SYB-151 mechanism work:
/// an API-poll adapter would read a configured endpoint and produce a payout,
/// while a manual adapter would expose operator instructions and wait for an
/// explicit resolution submission.
pub trait ResolutionSource {
    fn config(&self) -> &ResolutionSourceConfig;
    fn source_url(&self) -> &str;
}

/// Expanded child market ready for Sybil creation and MM bootstrap.
#[derive(Debug, Clone)]
pub struct NativeMarketSpec {
    pub template_id: String,
    pub market_key: String,
    pub name: String,
    pub outcome_title: Option<String>,
    pub quote_range: NativeQuoteRange,
    pub group_key: Option<String>,
    pub group_size: usize,
    pub end_time_ms: u64,
    description: Option<String>,
    category: String,
    resolution_criteria: String,
    source_url: String,
    event_title: String,
    /// Resolution adapter config, copied from the parent template. Drives the
    /// SYB-48 auto-resolution poller (`api_poll` is fetched + LLM-evaluated;
    /// `manual` is left entirely to operators).
    resolution_source: ResolutionSourceConfig,
    /// Event/group image, copied from the parent template (shared by siblings).
    event_image_url: Option<String>,
    /// Event/group icon fallback, copied from the parent template.
    event_icon_url: Option<String>,
    /// Per-child market image (categorical outcome / threshold rung logo).
    market_image_url: Option<String>,
    /// Structured threshold semantics. Display titles are not trusted as the
    /// resolution predicate.
    threshold_question: Option<NativeThresholdQuestion>,
}

#[derive(Debug, Clone)]
struct NativeThresholdQuestion {
    direction: NativeThresholdDirection,
    unit: String,
    threshold: f64,
}

impl NativeMarketCatalog {
    /// Load and validate a native catalog from a JSON file on disk.
    pub fn load(path: &Path) -> Result<Self, Error> {
        let data = std::fs::read_to_string(path)?;
        Self::parse_json(&data)
    }

    /// Parse and validate a native catalog from a JSON string.
    pub fn parse_json(data: &str) -> Result<Self, Error> {
        let parsed: Self = serde_json::from_str(data)?;
        parsed.validate()?;
        Ok(parsed)
    }

    /// Enabled child markets, in catalog order.
    pub fn enabled_market_specs(&self) -> Vec<NativeMarketSpec> {
        self.markets
            .iter()
            .filter(|template| template.enabled)
            .flat_map(NativeMarketTemplate::enabled_market_specs)
            .collect()
    }

    /// Number of templates in the catalog, including disabled placeholders.
    pub fn len(&self) -> usize {
        self.markets.len()
    }

    /// Whether the catalog contains no templates.
    pub fn is_empty(&self) -> bool {
        self.markets.is_empty()
    }

    fn validate(&self) -> Result<(), Error> {
        let mut ids = HashSet::new();
        let mut event_titles = HashSet::new();
        for (i, template) in self.markets.iter().enumerate() {
            let context = format!("native market #{i} ({:?})", template.id);
            validate_id(&template.id, &context)?;
            if !ids.insert(template.id.clone()) {
                return Err(Error::NativeCatalog(format!(
                    "{context} duplicates template id {:?}",
                    template.id
                )));
            }
            template.validate(&context)?;
            if template.enabled && !event_titles.insert(template.title.trim().to_ascii_lowercase())
            {
                return Err(Error::NativeCatalog(format!(
                    "{context} duplicates enabled event title {:?}",
                    template.title
                )));
            }
        }

        // Market questions are public discovery keys as well as display copy.
        // Reject case-insensitive duplicates across templates so two native
        // contracts cannot become visually indistinguishable.
        let mut market_titles = HashSet::new();
        for spec in self.enabled_market_specs() {
            if !market_titles.insert(spec.name.trim().to_lowercase()) {
                return Err(Error::NativeCatalog(format!(
                    "enabled native market title {:?} is duplicated",
                    spec.name
                )));
            }
        }
        Ok(())
    }
}

impl NativeMarketTemplate {
    fn validate(&self, context: &str) -> Result<(), Error> {
        validate_nonempty("title", &self.title, context)?;
        if self.enabled {
            validate_question_title("title", &self.title, context)?;
        }
        validate_nonempty("units", &self.units, context)?;
        validate_nonempty("resolution_criteria", &self.resolution_criteria, context)?;
        validate_nonempty("category", &self.category, context)?;
        validate_url("source_url", &self.source_url, context)?;
        if let Some(u) = &self.event_image_url {
            validate_image_url("event_image_url", u, context)?;
        }
        if let Some(u) = &self.event_icon_url {
            validate_image_url("event_icon_url", u, context)?;
        }
        let end_time_ms = parse_iso8601_to_ms(&self.end_time)
            .and_then(|ms| u64::try_from(ms).ok())
            .ok_or_else(|| {
                Error::NativeCatalog(format!(
                    "{context} has invalid UTC end_time {:?}",
                    self.end_time
                ))
            })?;
        if end_time_ms == 0 {
            return Err(Error::NativeCatalog(format!(
                "{context} end_time must be after the Unix epoch"
            )));
        }

        match &self.resolution_source {
            ResolutionSourceConfig::Manual { instructions } => {
                validate_nonempty("resolution_source.instructions", instructions, context)?;
            }
            ResolutionSourceConfig::ApiPoll {
                endpoint,
                method,
                notes,
            } => {
                validate_url("resolution_source.endpoint", endpoint, context)?;
                if let Some(method) = method {
                    let method = method.trim();
                    if method != "GET" && method != "POST" {
                        return Err(Error::NativeCatalog(format!(
                            "{context} resolution_source.method must be GET or POST"
                        )));
                    }
                }
                validate_nonempty("resolution_source.notes", notes, context)?;
            }
        }

        match &self.outcome_set {
            NativeOutcomeSet::Binary { yes, no } => {
                validate_nonempty("outcome_set.yes", yes, context)?;
                validate_nonempty("outcome_set.no", no, context)?;
                let quote_range = self.quote_range.ok_or_else(|| {
                    Error::NativeCatalog(format!("{context} binary market is missing quote_range"))
                })?;
                quote_range.validate(&format!("{context} quote_range"))?;
            }
            NativeOutcomeSet::Categorical { outcomes } => {
                if self.quote_range.is_some() {
                    return Err(Error::NativeCatalog(format!(
                        "{context} categorical template must use per-outcome quote_range values"
                    )));
                }
                if outcomes.len() < 2 {
                    return Err(Error::NativeCatalog(format!(
                        "{context} categorical template needs at least two outcomes"
                    )));
                }
                let mut outcome_ids = HashSet::new();
                let mut market_titles = HashSet::new();
                let mut enabled_count = 0usize;
                let mut initial_sum = 0.0;
                let mut minimum_sum = 0.0;
                let mut maximum_sum = 0.0;
                for (i, outcome) in outcomes.iter().enumerate() {
                    let outcome_context = format!("{context} outcome #{i} ({:?})", outcome.id);
                    validate_id(&outcome.id, &outcome_context)?;
                    if !outcome_ids.insert(outcome.id.clone()) {
                        return Err(Error::NativeCatalog(format!(
                            "{outcome_context} duplicates outcome id {:?}",
                            outcome.id
                        )));
                    }
                    validate_nonempty("title", &outcome.title, &outcome_context)?;
                    validate_child_market_title(
                        &outcome.market_title,
                        &self.title,
                        &outcome.title,
                        &outcome_context,
                    )?;
                    if !market_titles.insert(outcome.market_title.trim().to_lowercase()) {
                        return Err(Error::NativeCatalog(format!(
                            "{outcome_context} duplicates child market_title {:?}",
                            outcome.market_title
                        )));
                    }
                    outcome
                        .quote_range
                        .validate(&format!("{outcome_context} quote_range"))?;
                    if let Some(u) = &outcome.image_url {
                        validate_image_url("image_url", u, &outcome_context)?;
                    }
                    if outcome.enabled {
                        enabled_count += 1;
                        initial_sum += outcome.quote_range.initial;
                        minimum_sum += outcome.quote_range.min;
                        maximum_sum += outcome.quote_range.max;
                    }
                }
                if self.enabled && enabled_count < 2 {
                    return Err(Error::NativeCatalog(format!(
                        "{context} enabled categorical template needs at least two enabled outcomes"
                    )));
                }
                if self.enabled && (1.0 - initial_sum).abs() > CATEGORICAL_SUM_TOLERANCE {
                    return Err(Error::NativeCatalog(format!(
                        "{context} enabled categorical initial prices must sum to 1.0, got {initial_sum:.4}"
                    )));
                }
                if self.enabled
                    && (minimum_sum > 1.0 + CATEGORICAL_SUM_TOLERANCE
                        || maximum_sum < 1.0 - CATEGORICAL_SUM_TOLERANCE)
                {
                    return Err(Error::NativeCatalog(format!(
                        "{context} enabled categorical quote ranges cannot contain a probability vector summing to 1.0 (min sum {minimum_sum:.4}, max sum {maximum_sum:.4})"
                    )));
                }
            }
            NativeOutcomeSet::Threshold {
                direction,
                unit,
                outcomes,
            } => {
                if self.quote_range.is_some() {
                    return Err(Error::NativeCatalog(format!(
                        "{context} threshold template must use per-rung quote_range values"
                    )));
                }
                validate_nonempty("outcome_set.unit", unit, context)?;
                if outcomes.len() < 2 {
                    return Err(Error::NativeCatalog(format!(
                        "{context} threshold template needs at least two rungs"
                    )));
                }
                let mut rung_ids = HashSet::new();
                let mut market_titles = HashSet::new();
                let mut enabled_count = 0usize;
                let mut previous: Option<&NativeThresholdRung> = None;
                for (i, rung) in outcomes.iter().enumerate() {
                    let rung_context = format!("{context} rung #{i} ({:?})", rung.id);
                    validate_id(&rung.id, &rung_context)?;
                    if !rung_ids.insert(rung.id.clone()) {
                        return Err(Error::NativeCatalog(format!(
                            "{rung_context} duplicates rung id {:?}",
                            rung.id
                        )));
                    }
                    validate_nonempty("title", &rung.title, &rung_context)?;
                    validate_child_market_title(
                        &rung.market_title,
                        &self.title,
                        &rung.title,
                        &rung_context,
                    )?;
                    if !market_titles.insert(rung.market_title.trim().to_lowercase()) {
                        return Err(Error::NativeCatalog(format!(
                            "{rung_context} duplicates child market_title {:?}",
                            rung.market_title
                        )));
                    }
                    if !rung.threshold.is_finite() {
                        return Err(Error::NativeCatalog(format!(
                            "{rung_context} threshold must be finite"
                        )));
                    }
                    rung.quote_range
                        .validate(&format!("{rung_context} quote_range"))?;
                    if let Some(u) = &rung.image_url {
                        validate_image_url("image_url", u, &rung_context)?;
                    }
                    if rung.enabled {
                        enabled_count += 1;
                    }
                    if let Some(previous) = previous {
                        let ordered = match direction {
                            NativeThresholdDirection::Above => rung.threshold > previous.threshold,
                            NativeThresholdDirection::Below => rung.threshold < previous.threshold,
                        };
                        if !ordered {
                            return Err(Error::NativeCatalog(format!(
                                "{rung_context} thresholds must be strictly ordered from the easiest predicate to the hardest"
                            )));
                        }
                        if rung.quote_range.min > previous.quote_range.min + f64::EPSILON
                            || rung.quote_range.initial
                                > previous.quote_range.initial + f64::EPSILON
                            || rung.quote_range.max > previous.quote_range.max + f64::EPSILON
                        {
                            return Err(Error::NativeCatalog(format!(
                                "{rung_context} nested threshold quote ranges must be non-increasing"
                            )));
                        }
                    }
                    previous = Some(rung);
                }
                if self.enabled && enabled_count < 2 {
                    return Err(Error::NativeCatalog(format!(
                        "{context} enabled threshold template needs at least two enabled rungs"
                    )));
                }
                // Rungs are nested/independent: initial prices are intentionally
                // NOT summed (unlike categorical outcomes, which must sum to 1).
            }
        }

        Ok(())
    }

    fn enabled_market_specs(&self) -> Vec<NativeMarketSpec> {
        let end_time_ms = parse_iso8601_to_ms(&self.end_time)
            .and_then(|ms| u64::try_from(ms).ok())
            .expect("native catalog validation guarantees parseable end_time");
        match &self.outcome_set {
            NativeOutcomeSet::Binary { .. } => {
                let quote_range = self
                    .quote_range
                    .expect("native catalog validation guarantees binary quote_range");
                vec![NativeMarketSpec {
                    template_id: self.id.clone(),
                    market_key: binary_market_key(&self.id),
                    name: self.title.clone(),
                    outcome_title: None,
                    quote_range,
                    group_key: None,
                    group_size: 0,
                    end_time_ms,
                    description: Some(format!("Native market. Units: {}.", self.units)),
                    category: self.category.clone(),
                    resolution_criteria: self.resolution_criteria.clone(),
                    source_url: self.source_url.clone(),
                    event_title: self.title.clone(),
                    resolution_source: self.resolution_source.clone(),
                    event_image_url: self.event_image_url.clone(),
                    event_icon_url: self.event_icon_url.clone(),
                    market_image_url: self.event_image_url.clone(),
                    threshold_question: None,
                }]
            }
            NativeOutcomeSet::Categorical { outcomes } => {
                let enabled: Vec<_> = outcomes.iter().filter(|outcome| outcome.enabled).collect();
                let group_size = enabled.len();
                let group_key = Some(native_group_key(&self.id));
                enabled
                    .into_iter()
                    .map(|outcome| NativeMarketSpec {
                        template_id: self.id.clone(),
                        market_key: outcome_market_key(&self.id, &outcome.id),
                        name: outcome.market_title.clone(),
                        outcome_title: Some(outcome.title.clone()),
                        quote_range: outcome.quote_range,
                        group_key: group_key.clone(),
                        group_size,
                        end_time_ms,
                        description: Some(format!(
                            "Native categorical market. Units: {}.",
                            self.units
                        )),
                        category: self.category.clone(),
                        resolution_criteria: self.resolution_criteria.clone(),
                        source_url: self.source_url.clone(),
                        event_title: self.title.clone(),
                        resolution_source: self.resolution_source.clone(),
                        event_image_url: self.event_image_url.clone(),
                        event_icon_url: self.event_icon_url.clone(),
                        market_image_url: outcome.image_url.clone(),
                        threshold_question: None,
                    })
                    .collect()
            }
            NativeOutcomeSet::Threshold {
                direction,
                unit,
                outcomes,
            } => {
                let enabled: Vec<_> = outcomes.iter().filter(|rung| rung.enabled).collect();
                enabled
                    .into_iter()
                    .map(|rung| NativeMarketSpec {
                        template_id: self.id.clone(),
                        market_key: outcome_market_key(&self.id, &rung.id),
                        name: rung.market_title.clone(),
                        outcome_title: Some(rung.title.clone()),
                        quote_range: rung.quote_range,
                        group_key: None,
                        group_size: 0,
                        end_time_ms,
                        description: Some(format!(
                            "Native threshold market. Units: {}.",
                            self.units
                        )),
                        category: self.category.clone(),
                        resolution_criteria: self.resolution_criteria.clone(),
                        source_url: self.source_url.clone(),
                        event_title: self.title.clone(),
                        resolution_source: self.resolution_source.clone(),
                        event_image_url: self.event_image_url.clone(),
                        event_icon_url: self.event_icon_url.clone(),
                        market_image_url: rung.image_url.clone(),
                        threshold_question: Some(NativeThresholdQuestion {
                            direction: *direction,
                            unit: unit.clone(),
                            threshold: rung.threshold,
                        }),
                    })
                    .collect()
            }
        }
    }
}

impl NativeQuoteRange {
    fn validate(&self, context: &str) -> Result<(), Error> {
        if !(self.min.is_finite() && self.max.is_finite() && self.initial.is_finite()) {
            return Err(Error::NativeCatalog(format!(
                "{context} values must be finite"
            )));
        }
        if !(self.min > 0.01 && self.max < 0.99 && self.min < self.max) {
            return Err(Error::NativeCatalog(format!(
                "{context} must satisfy 0.01 < min < max < 0.99"
            )));
        }
        if !(self.initial >= self.min && self.initial <= self.max) {
            return Err(Error::NativeCatalog(format!(
                "{context} initial must lie inside [min, max]"
            )));
        }
        Ok(())
    }
}

impl NativeMarketSpec {
    pub fn group_name(&self) -> &str {
        &self.event_title
    }

    /// Resolution adapter config inherited from the parent template.
    pub fn resolution_source(&self) -> &ResolutionSourceConfig {
        &self.resolution_source
    }

    /// Full, verbatim resolution criteria shown to operators and the LLM.
    pub fn resolution_criteria(&self) -> &str {
        &self.resolution_criteria
    }

    /// Primary source URL for this market's resolution.
    pub fn source_url(&self) -> &str {
        &self.source_url
    }

    /// The specific YES/NO question this child market settles. For a binary
    /// template this is just the market name; for a categorical child it is
    /// phrased as "did this outcome win?" so a single scalar payout in [0,1]
    /// (YES probability) is always well defined.
    pub fn resolution_question(&self) -> String {
        if let Some(question) = &self.threshold_question {
            let comparison = match question.direction {
                NativeThresholdDirection::Above => "greater than or equal to",
                NativeThresholdDirection::Below => "less than or equal to",
            };
            return format!(
                "For the event \"{}\": resolve YES if and only if the observed value is {comparison} {} {}, otherwise NO.",
                self.event_title, question.threshold, question.unit
            );
        }
        match &self.outcome_title {
            Some(outcome) => format!(
                "For the event \"{}\": resolve YES if the outcome \"{}\" is the winning \
                 outcome, otherwise NO.",
                self.event_title, outcome
            ),
            None => self.name.clone(),
        }
    }

    pub fn create_request(&self) -> CreateMarketRequest {
        CreateMarketRequest {
            name: self.name.clone(),
            description: self.description.clone(),
            category: Some(self.category.clone()),
            tags: Some(vec!["native".to_string(), self.category.clone()]),
            resolution_criteria: Some(self.resolution_criteria.clone()),
            expiry_timestamp_ms: Some(self.end_time_ms),
            resolution_template: None,
        }
    }

    pub fn metadata_request(&self) -> SetMarketMetadataRequest {
        SetMarketMetadataRequest {
            external_url: Some(self.source_url.clone()),
            event_id: Some(native_group_key(&self.template_id)),
            event_title: Some(self.event_title.clone()),
            event_image_url: self.event_image_url.clone(),
            event_icon_url: self.event_icon_url.clone(),
            event_end_date_ms: Some(self.end_time_ms),
            market_image_url: self.market_image_url.clone(),
            market_icon_url: None,
            market_end_date_ms: Some(self.end_time_ms),
            category: Some(self.category.clone()),
            categories: None,
            polymarket_condition_id: None,
            event_start_date_ms: None,
            market_start_date_ms: None,
            group_item_title: self.outcome_title.clone(),
            closed: Some(false),
        }
    }
}

pub fn binary_market_key(template_id: &str) -> String {
    template_id.to_string()
}

pub fn outcome_market_key(template_id: &str, outcome_id: &str) -> String {
    format!("{template_id}:{outcome_id}")
}

pub fn native_group_key(template_id: &str) -> String {
    format!("native:{template_id}")
}

fn validate_id(id: &str, context: &str) -> Result<(), Error> {
    let id = id.trim();
    if id.is_empty() {
        return Err(Error::NativeCatalog(format!("{context} has an empty id")));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(Error::NativeCatalog(format!(
            "{context} id must use lowercase ascii letters, digits, '-' or '_'"
        )));
    }
    Ok(())
}

fn validate_nonempty(field: &str, value: &str, context: &str) -> Result<(), Error> {
    if value.trim().is_empty() {
        return Err(Error::NativeCatalog(format!("{context} has empty {field}")));
    }
    Ok(())
}

fn validate_url(field: &str, value: &str, context: &str) -> Result<(), Error> {
    let parsed = Url::parse(value.trim()).map_err(|e| {
        Error::NativeCatalog(format!("{context} has invalid {field} {:?}: {e}", value))
    })?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        scheme => Err(Error::NativeCatalog(format!(
            "{context} {field} must be http(s), got {scheme:?}"
        ))),
    }
}

fn validate_image_url(field: &str, value: &str, context: &str) -> Result<(), Error> {
    let value = value.trim();
    if let Some(path) = value.strip_prefix('/') {
        let safe = !path.is_empty()
            && !path.starts_with('/')
            && !path.contains('\\')
            && !path.contains('?')
            && !path.contains('#')
            && path
                .split('/')
                .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
        if safe {
            return Ok(());
        }
        return Err(Error::NativeCatalog(format!(
            "{context} has unsafe same-origin {field} {:?}",
            value
        )));
    }
    validate_url(field, value, context)
}

fn validate_question_title(field: &str, value: &str, context: &str) -> Result<(), Error> {
    validate_nonempty(field, value, context)?;
    if value.trim() != value || !value.ends_with('?') {
        return Err(Error::NativeCatalog(format!(
            "{context} {field} must be a trimmed question ending in '?'"
        )));
    }
    Ok(())
}

fn validate_child_market_title(
    market_title: &str,
    event_title: &str,
    outcome_title: &str,
    context: &str,
) -> Result<(), Error> {
    validate_question_title("market_title", market_title, context)?;
    let legacy_title = format!("{}: {}", event_title.trim(), outcome_title.trim());
    if market_title == legacy_title || market_title.starts_with(&format!("{}:", event_title.trim()))
    {
        return Err(Error::NativeCatalog(format!(
            "{context} market_title must be a standalone question, not an event-title/outcome concatenation"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_in_catalog_is_research_backed() {
        let data = include_str!("../native_markets.json");
        let catalog = NativeMarketCatalog::parse_json(data).unwrap();
        assert_eq!(catalog.len(), 26);

        // Every shipped template is enabled.
        let disabled: Vec<&str> = catalog
            .markets
            .iter()
            .filter(|m| !m.enabled)
            .map(|m| m.id.as_str())
            .collect();
        assert!(
            disabled.is_empty(),
            "unexpected disabled templates: {disabled:?}"
        );

        // Enabled specs: 18 categorical groups + 8 threshold ladders expand to
        // 134 child markets.
        let specs = catalog.enabled_market_specs();
        assert_eq!(specs.len(), 134);
        assert!(specs.iter().all(|s| s.end_time_ms > 0));
        for spec in &specs {
            assert!(
                spec.name.ends_with('?'),
                "native market title is not a question: {:?}",
                spec.name
            );
            // Native provenance: a native child market never carries a
            // Polymarket condition id (the mirror/native discriminator).
            assert_eq!(spec.metadata_request().polymarket_condition_id, None);
        }
        for template in &catalog.markets {
            if matches!(template.outcome_set, NativeOutcomeSet::Threshold { .. }) {
                assert!(
                    specs
                        .iter()
                        .filter(|spec| spec.template_id == template.id)
                        .all(|spec| spec.group_key.is_none() && spec.group_size == 0),
                    "threshold template {} must not create a mutually-exclusive core group",
                    template.id
                );
            }
        }

        // The retired app id had a different categorical membership. Reusing
        // it would append children to an old persisted MarketGroup. The final-
        // week model event is intentionally restored with its original exact
        // membership and stable id.
        assert!(
            catalog
                .markets
                .iter()
                .all(|template| template.id != "openrouter_top_app_dec_2026")
        );

        let title_for = |market_key: &str| {
            specs
                .iter()
                .find(|spec| spec.market_key == market_key)
                .map(|spec| spec.name.as_str())
        };
        assert_eq!(
            catalog
                .markets
                .iter()
                .find(|template| template.id == "aa_intelligence_leader_eoy2026")
                .map(|template| template.title.as_str()),
            Some(
                "Which company will lead the Artificial Analysis Intelligence Index at end of 2026?"
            )
        );
        assert_eq!(
            title_for("openrouter_top_image_company_dec_2026:or_img_bytedance"),
            Some("Will ByteDance lead OpenRouter image-model usage in December 2026?")
        );
        assert_eq!(
            title_for("anthropic_flagship_input_price_below_eoy2026:anthropic_input_le_5"),
            Some(
                "Will Anthropic's flagship Opus base input API price be at or below $5/MTok at end of 2026?"
            )
        );
        assert_eq!(
            title_for("openai_flagship_input_price_below_eoy2026:openai_input_le_5"),
            Some(
                "Will OpenAI's flagship model standard input API price be at or below $5/MTok at end of 2026?"
            )
        );
        assert!(
            specs.iter().all(|spec| spec.market_key
                != "context_window_max_advertised_eoy2026:context_window_above_10m_eoy2026"),
            "the retired 10M context-window market must not return"
        );

        for market in &catalog.markets {
            let expected_logo = if market.id == "aa_intelligence_leader_eoy2026" {
                Some("/native-market-logos/artificial-analysis.svg")
            } else if market.id.starts_with("openrouter_") {
                Some("/native-market-logos/openrouter.svg")
            } else if market.id.starts_with("metr_") {
                Some("/native-market-logos/metr.svg")
            } else if market.id == "anthropic_flagship_input_price_below_eoy2026" {
                Some("/native-market-logos/anthropic.svg")
            } else if market.id == "openai_flagship_input_price_below_eoy2026" {
                Some("/native-market-logos/openai.svg")
            } else {
                None
            };
            if let Some(expected_logo) = expected_logo {
                assert_eq!(
                    market.event_image_url.as_deref(),
                    Some(expected_logo),
                    "unexpected event logo for {}",
                    market.id
                );
            }
        }

        // Same-origin logo paths are deployment artifacts, so fail the catalog
        // test if a referenced public asset is renamed or omitted.
        let public_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../frontend/web/public");
        for market in &catalog.markets {
            for image_url in [&market.event_image_url, &market.event_icon_url]
                .into_iter()
                .flatten()
                .filter(|url| url.starts_with('/'))
            {
                let path = public_dir.join(image_url.trim_start_matches('/'));
                assert!(
                    path.is_file(),
                    "{} references missing same-origin image {}",
                    market.id,
                    path.display()
                );
            }
        }

        // No placeholder text may survive in shipped entries.
        for market in &catalog.markets {
            let text = format!("{} {}", market.title, market.resolution_criteria);
            assert!(
                !text.to_ascii_lowercase().contains("placeholder"),
                "placeholder text leaked into {}",
                market.id
            );
        }
    }

    #[test]
    fn expands_enabled_binary_market() {
        let json = r#"{
            "markets": [{
                "id": "native_binary",
                "enabled": true,
                "title": "Will the test pass?",
                "outcome_set": { "type": "binary", "yes": "Yes", "no": "No" },
                "units": "probability",
                "end_time": "2026-12-31T23:59:00Z",
                "resolution_criteria": "Resolve YES if the test passes.",
                "source_url": "https://example.com/test",
                "category": "Testing",
                "resolution_source": { "type": "manual", "instructions": "Read the test log." },
                "quote_range": { "min": 0.40, "max": 0.60, "initial": 0.50 }
            }]
        }"#;
        let catalog = NativeMarketCatalog::parse_json(json).unwrap();
        let specs = catalog.enabled_market_specs();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].market_key, "native_binary");
        assert_eq!(specs[0].group_key, None);
        assert_eq!(specs[0].quote_range.initial, 0.50);
        let req = specs[0].create_request();
        assert_eq!(req.name, "Will the test pass?");
        assert_eq!(req.resolution_template, None);
        let metadata = specs[0].metadata_request();
        assert_eq!(
            metadata.external_url.as_deref(),
            Some("https://example.com/test")
        );
        assert_eq!(metadata.polymarket_condition_id, None);
    }

    #[test]
    fn expands_enabled_categorical_market_group() {
        let json = r#"{
            "markets": [{
                "id": "native_multi",
                "enabled": true,
                "title": "Which option wins?",
                "outcome_set": {
                    "type": "categorical",
                    "outcomes": [
                        { "id": "a", "title": "A", "market_title": "Will A win?", "enabled": true, "quote_range": { "min": 0.20, "max": 0.50, "initial": 0.30 } },
                        { "id": "b", "title": "B", "market_title": "Will B win?", "enabled": true, "quote_range": { "min": 0.20, "max": 0.50, "initial": 0.30 } },
                        { "id": "c", "title": "C", "market_title": "Will C win?", "enabled": true, "quote_range": { "min": 0.10, "max": 0.50, "initial": 0.40 } }
                    ]
                },
                "units": "probability",
                "end_time": "2026-12-31T23:59:00Z",
                "resolution_criteria": "Resolve to the winning option.",
                "source_url": "https://example.com/test",
                "category": "Testing",
                "resolution_source": { "type": "manual", "instructions": "Read the result." }
            }]
        }"#;
        let specs = NativeMarketCatalog::parse_json(json)
            .unwrap()
            .enabled_market_specs();
        assert_eq!(specs.len(), 3);
        assert!(
            specs
                .iter()
                .all(|spec| spec.group_key.as_deref() == Some("native:native_multi"))
        );
        assert!(specs.iter().all(|spec| spec.group_size == 3));
        assert_eq!(specs[0].market_key, "native_multi:a");
        assert_eq!(specs[0].name, "Will A win?");
        assert_eq!(
            specs[0].metadata_request().group_item_title.as_deref(),
            Some("A")
        );
    }

    #[test]
    fn categorical_market_titles_are_standalone_unique_questions() {
        let json = r#"{
            "markets": [{
                "id": "native_multi",
                "enabled": true,
                "title": "Which option wins?",
                "outcome_set": {
                    "type": "categorical",
                    "outcomes": [
                        { "id": "a", "title": "A", "market_title": "Will A win?", "enabled": true, "quote_range": { "min": 0.20, "max": 0.70, "initial": 0.50 } },
                        { "id": "b", "title": "B", "market_title": "Will B win?", "enabled": true, "quote_range": { "min": 0.20, "max": 0.70, "initial": 0.50 } }
                    ]
                },
                "units": "probability",
                "end_time": "2026-12-31T23:59:00Z",
                "resolution_criteria": "Resolve to the winning option.",
                "source_url": "https://example.com/test",
                "category": "Testing",
                "resolution_source": { "type": "manual", "instructions": "Read the result." }
            }]
        }"#;
        NativeMarketCatalog::parse_json(json).unwrap();

        let under = json.replacen("\"initial\": 0.50", "\"initial\": 0.40", 1);
        let err = NativeMarketCatalog::parse_json(&under).unwrap_err();
        assert!(err.to_string().contains("must sum to 1.0"), "{err}");

        let over = json.replacen("\"initial\": 0.50", "\"initial\": 0.60", 1);
        let err = NativeMarketCatalog::parse_json(&over).unwrap_err();
        assert!(err.to_string().contains("must sum to 1.0"), "{err}");

        let legacy = json.replace("Will A win?", "Which option wins?: A?");
        let err = NativeMarketCatalog::parse_json(&legacy).unwrap_err();
        assert!(err.to_string().contains("standalone question"), "{err}");

        let duplicate = json.replace("Will B win?", "WILL A WIN?");
        let err = NativeMarketCatalog::parse_json(&duplicate).unwrap_err();
        assert!(
            err.to_string().contains("duplicates child market_title"),
            "{err}"
        );

        let not_a_question = json.replace("Will A win?", "A wins");
        let err = NativeMarketCatalog::parse_json(&not_a_question).unwrap_err();
        assert!(err.to_string().contains("ending in '?'"), "{err}");

        let missing = json.replace("\"market_title\": \"Will A win?\", ", "");
        let err = NativeMarketCatalog::parse_json(&missing).unwrap_err();
        assert!(
            err.to_string().contains("missing field `market_title`"),
            "{err}"
        );
    }

    #[test]
    fn same_origin_images_must_use_safe_public_paths() {
        assert!(
            validate_image_url(
                "event_image_url",
                "/native-market-logos/openrouter.svg",
                "test"
            )
            .is_ok()
        );
        for bad in [
            "//cdn.example/logo.svg",
            "/../logo.svg",
            "/logos//logo.svg",
            "/logo.svg?version=1",
            "/logo.svg#mark",
        ] {
            let err = validate_image_url("event_image_url", bad, "test").unwrap_err();
            assert!(err.to_string().contains("unsafe same-origin"), "{err}");
        }
    }

    #[test]
    fn expands_enabled_threshold_ladder() {
        let json = r#"{
            "markets": [{
                "id": "native_ladder",
                "enabled": true,
                "title": "How large will X be?",
                "outcome_set": {
                    "type": "threshold",
                    "direction": "above",
                    "unit": "tokens",
                    "outcomes": [
                        { "id": "ge_2m", "title": "≥ 2M", "market_title": "Will X be at least 2M tokens?", "enabled": true, "threshold": 2000000, "quote_range": { "min": 0.30, "max": 0.70, "initial": 0.50 } },
                        { "id": "ge_5m", "title": "≥ 5M", "market_title": "Will X be at least 5M tokens?", "enabled": true, "threshold": 5000000, "quote_range": { "min": 0.06, "max": 0.30, "initial": 0.15 } },
                        { "id": "ge_10m", "title": "≥ 10M", "market_title": "Will X be at least 10M tokens?", "enabled": false, "threshold": 10000000, "quote_range": { "min": 0.02, "max": 0.16, "initial": 0.06 } }
                    ]
                },
                "units": "tokens",
                "end_time": "2026-12-31T23:59:00Z",
                "resolution_criteria": "Each rung resolves YES if X is at least the threshold.",
                "source_url": "https://example.com/x",
                "category": "AI",
                "event_image_url": "https://example.com/logo.png",
                "resolution_source": { "type": "manual", "instructions": "Read the tracker." }
            }]
        }"#;
        let catalog = NativeMarketCatalog::parse_json(json).unwrap();
        let specs = catalog.enabled_market_specs();
        // Only the two enabled rungs expand; the disabled rung is skipped.
        assert_eq!(specs.len(), 2);
        assert!(specs.iter().all(|spec| spec.group_key.is_none()));
        assert!(specs.iter().all(|spec| spec.group_size == 0));
        assert_eq!(specs[0].market_key, "native_ladder:ge_2m");
        assert_eq!(specs[0].name, "Will X be at least 2M tokens?");
        assert_eq!(
            specs[0].resolution_question(),
            "For the event \"How large will X be?\": resolve YES if and only if the observed value is greater than or equal to 2000000 tokens, otherwise NO."
        );
        let metadata = specs[0].metadata_request();
        assert_eq!(metadata.group_item_title.as_deref(), Some("\u{2265} 2M"));
        // Template image flows to every child's event image.
        assert_eq!(
            metadata.event_image_url.as_deref(),
            Some("https://example.com/logo.png")
        );
        assert_eq!(metadata.polymarket_condition_id, None);
    }

    #[test]
    fn threshold_below_direction_and_bad_direction() {
        let ok = r#"{
            "markets": [{
                "id": "price_ladder", "enabled": true, "title": "How low?",
                "outcome_set": { "type": "threshold", "direction": "below", "unit": "USD/MTok",
                    "outcomes": [
                        { "id": "le_10", "title": "≤ $10", "market_title": "Will the price be at most $10?", "enabled": true, "threshold": 10, "quote_range": { "min": 0.5, "max": 0.9, "initial": 0.7 } },
                        { "id": "le_5", "title": "≤ $5", "market_title": "Will the price be at most $5?", "enabled": true, "threshold": 5, "quote_range": { "min": 0.06, "max": 0.34, "initial": 0.16 } }
                    ] },
                "units": "USD/MTok", "end_time": "2026-12-31T23:59:00Z",
                "resolution_criteria": "Rungs on price.", "source_url": "https://example.com/p",
                "category": "AI", "resolution_source": { "type": "manual", "instructions": "Read price." }
            }]
        }"#;
        assert_eq!(
            NativeMarketCatalog::parse_json(ok)
                .unwrap()
                .enabled_market_specs()
                .len(),
            2
        );
        let bad = ok.replace("\"below\"", "\"sideways\"");
        let err = NativeMarketCatalog::parse_json(&bad).unwrap_err();
        assert!(err.to_string().contains("unknown variant"), "{err}");
    }

    #[test]
    fn threshold_ladders_require_ordered_thresholds_and_monotone_quotes() {
        let json = r#"{
            "markets": [{
                "id": "native_ladder", "enabled": true, "title": "How large?",
                "outcome_set": { "type": "threshold", "direction": "above", "unit": "x",
                    "outcomes": [
                        { "id": "ge_2", "title": "≥ 2", "market_title": "Will X be at least 2?", "enabled": true, "threshold": 2, "quote_range": { "min": 0.4, "max": 0.8, "initial": 0.6 } },
                        { "id": "ge_5", "title": "≥ 5", "market_title": "Will X be at least 5?", "enabled": true, "threshold": 5, "quote_range": { "min": 0.2, "max": 0.6, "initial": 0.3 } }
                    ] },
                "units": "x", "end_time": "2026-12-31T23:59:00Z",
                "resolution_criteria": "Compare the observed value.",
                "source_url": "https://example.com/x", "category": "AI",
                "resolution_source": { "type": "manual", "instructions": "Read x." }
            }]
        }"#;
        NativeMarketCatalog::parse_json(json).unwrap();

        let duplicate = json.replace("\"threshold\": 5", "\"threshold\": 2");
        let err = NativeMarketCatalog::parse_json(&duplicate).unwrap_err();
        assert!(err.to_string().contains("strictly ordered"), "{err}");

        let non_monotone = json.replace(
            "\"min\": 0.2, \"max\": 0.6, \"initial\": 0.3",
            "\"min\": 0.5, \"max\": 0.9, \"initial\": 0.7",
        );
        let err = NativeMarketCatalog::parse_json(&non_monotone).unwrap_err();
        assert!(err.to_string().contains("non-increasing"), "{err}");
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let json = r#"{
            "markets": [
                {
                    "id": "dup",
                    "enabled": false,
                    "title": "A",
                    "outcome_set": { "type": "binary", "yes": "Yes", "no": "No" },
                    "units": "probability",
                    "end_time": "2026-12-31T23:59:00Z",
                    "resolution_criteria": "Criteria",
                    "source_url": "https://example.com/a",
                    "category": "Testing",
                    "resolution_source": { "type": "manual", "instructions": "Manual" },
                    "quote_range": { "min": 0.40, "max": 0.60, "initial": 0.50 }
                },
                {
                    "id": "dup",
                    "enabled": false,
                    "title": "B",
                    "outcome_set": { "type": "binary", "yes": "Yes", "no": "No" },
                    "units": "probability",
                    "end_time": "2026-12-31T23:59:00Z",
                    "resolution_criteria": "Criteria",
                    "source_url": "https://example.com/b",
                    "category": "Testing",
                    "resolution_source": { "type": "manual", "instructions": "Manual" },
                    "quote_range": { "min": 0.40, "max": 0.60, "initial": 0.50 }
                }
            ]
        }"#;
        let err = NativeMarketCatalog::parse_json(json).unwrap_err();
        assert!(err.to_string().contains("duplicates template id"), "{err}");
    }

    #[test]
    fn bad_quote_range_is_rejected() {
        let json = r#"{
            "markets": [{
                "id": "bad_range",
                "enabled": true,
                "title": "Bad range?",
                "outcome_set": { "type": "binary", "yes": "Yes", "no": "No" },
                "units": "probability",
                "end_time": "2026-12-31T23:59:00Z",
                "resolution_criteria": "Criteria",
                "source_url": "https://example.com/bad",
                "category": "Testing",
                "resolution_source": { "type": "manual", "instructions": "Manual" },
                "quote_range": { "min": 0.60, "max": 0.40, "initial": 0.50 }
            }]
        }"#;
        let err = NativeMarketCatalog::parse_json(json).unwrap_err();
        assert!(err.to_string().contains("0.01 < min < max < 0.99"), "{err}");
    }
}
