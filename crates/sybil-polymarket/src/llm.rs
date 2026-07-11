//! LLM client abstraction for automated resolution (SYB-48).
//!
//! A tiny trait so the resolver can be driven by a real OpenRouter model in
//! production and a deterministic mock in every test (no network in tests). The
//! model is asked to return STRICT JSON; parsing is fail-closed — anything we
//! cannot confidently interpret becomes an error the caller escalates rather
//! than a resolution.

use std::time::Duration;

use serde::Deserialize;
use sybil_api_types::NANOS_PER_DOLLAR;

/// Error surface for LLM calls + parsing.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("LLM transport error: {0}")]
    Transport(String),
    #[error("LLM returned no choices")]
    Empty,
    /// The model's text could not be parsed into a strict verdict. This is the
    /// fail-closed path: the caller MUST escalate, never resolve.
    #[error("could not parse strict verdict from model output: {0}")]
    Parse(String),
}

/// A parsed, validated resolution verdict.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmVerdict {
    /// YES payout per share, in nanodollars (0..=NANOS_PER_DOLLAR). For a binary
    /// market this is 0 (NO) or 1e9 (YES); a scalar market may land in between.
    pub payout_nanos: u64,
    /// Model confidence in [0, 1].
    pub confidence: f64,
    /// Free-text justification.
    pub reasoning: String,
    /// Short verbatim excerpts from the fetched source.
    pub evidence_excerpts: Vec<String>,
}

/// A single evaluation request handed to a model.
#[derive(Debug, Clone)]
pub struct LlmRequest {
    /// The specific YES/NO question this market settles.
    pub question: String,
    /// The market's FULL resolution criteria, verbatim.
    pub resolution_criteria: String,
    /// The content fetched from the resolution source.
    pub source_content: String,
}

impl LlmRequest {
    /// Build the user prompt. Kept deterministic so the mock and the real client
    /// see identical text, and so prompt changes are reviewable in diffs.
    pub fn user_prompt(&self) -> String {
        format!(
            "You are an impartial resolver for a prediction market. Decide the \
             outcome STRICTLY from the resolution criteria and the fetched source \
             content. If the source is insufficient, ambiguous, or contradictory, \
             report LOW confidence rather than guessing.\n\n\
             QUESTION:\n{}\n\n\
             RESOLUTION CRITERIA (authoritative):\n{}\n\n\
             FETCHED SOURCE CONTENT:\n{}\n\n\
             Respond with STRICT JSON and nothing else, matching:\n\
             {{\n  \
             \"payout_fraction\": <number in [0,1]; probability that YES is correct, \
             1 = YES wins, 0 = NO wins>,\n  \
             \"confidence\": <number in [0,1]>,\n  \
             \"reasoning\": <string>,\n  \
             \"evidence_excerpts\": [<short verbatim quotes from the source>]\n}}",
            self.question, self.resolution_criteria, self.source_content
        )
    }
}

/// Abstraction over an LLM chat completion.
#[async_trait::async_trait]
pub trait LlmClient: Send + Sync {
    /// Return the model's raw text completion for `req`.
    async fn complete(&self, req: &LlmRequest) -> Result<String, LlmError>;

    /// Evaluate a request and parse a strict verdict. Default implementation
    /// wires `complete` + [`parse_verdict`]; both failure modes surface as
    /// `LlmError` so the resolver can fail closed.
    async fn evaluate(&self, req: &LlmRequest) -> Result<LlmVerdict, LlmError> {
        let raw = self.complete(req).await?;
        parse_verdict(&raw)
    }
}

/// Raw JSON shape the model is asked to emit.
#[derive(Debug, Deserialize)]
struct RawVerdict {
    #[serde(default)]
    payout_fraction: Option<f64>,
    /// Accepted as an alternative to `payout_fraction` for pure binary answers.
    #[serde(default)]
    proposed_outcome: Option<String>,
    confidence: f64,
    #[serde(default)]
    reasoning: String,
    #[serde(default)]
    evidence_excerpts: Vec<String>,
}

/// Parse the model's text into a validated [`LlmVerdict`], fail-closed.
///
/// Tolerates a JSON object embedded in surrounding prose or ```json fences (we
/// slice the outermost `{...}`), but rejects anything whose numbers are out of
/// range, non-finite, or that specifies neither a payout fraction nor a
/// recognizable YES/NO outcome.
pub fn parse_verdict(raw: &str) -> Result<LlmVerdict, LlmError> {
    let json = extract_json_object(raw)
        .ok_or_else(|| LlmError::Parse("no JSON object found in output".into()))?;
    let parsed: RawVerdict =
        serde_json::from_str(json).map_err(|e| LlmError::Parse(format!("invalid JSON: {e}")))?;

    if !parsed.confidence.is_finite() || !(0.0..=1.0).contains(&parsed.confidence) {
        return Err(LlmError::Parse(format!(
            "confidence out of range: {}",
            parsed.confidence
        )));
    }

    let fraction = match (parsed.payout_fraction, parsed.proposed_outcome.as_deref()) {
        (Some(f), _) => {
            if !f.is_finite() || !(0.0..=1.0).contains(&f) {
                return Err(LlmError::Parse(format!(
                    "payout_fraction out of range: {f}"
                )));
            }
            f
        }
        (None, Some(outcome)) => match outcome.trim().to_ascii_uppercase().as_str() {
            "YES" | "TRUE" | "1" => 1.0,
            "NO" | "FALSE" | "0" => 0.0,
            other => {
                return Err(LlmError::Parse(format!(
                    "unrecognized proposed_outcome: {other:?}"
                )));
            }
        },
        (None, None) => {
            return Err(LlmError::Parse(
                "neither payout_fraction nor proposed_outcome present".into(),
            ));
        }
    };

    // Round to nearest nano; clamp is a no-op given the range check above but
    // kept as a defensive belt against float edge cases.
    let payout_nanos = ((fraction * NANOS_PER_DOLLAR as f64).round() as i128)
        .clamp(0, NANOS_PER_DOLLAR as i128) as u64;

    Ok(LlmVerdict {
        payout_nanos,
        confidence: parsed.confidence,
        reasoning: parsed.reasoning,
        evidence_excerpts: parsed.evidence_excerpts,
    })
}

/// Slice the outermost balanced `{...}` from arbitrary model text.
fn extract_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, ch) in raw[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&raw[start..start + offset + ch.len_utf8()]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Deterministic mock used by every test — never touches the network.
///
/// Constructed with a canned raw completion (so parse-failure paths can be
/// exercised) or with a ready-made verdict rendered back to JSON.
pub struct MockLlm {
    raw: String,
}

impl MockLlm {
    /// Return this exact raw text for every call.
    pub fn raw(raw: impl Into<String>) -> Self {
        Self { raw: raw.into() }
    }

    /// Return a well-formed strict-JSON completion for the given fraction +
    /// confidence.
    pub fn verdict(payout_fraction: f64, confidence: f64) -> Self {
        Self {
            raw: format!(
                "{{\"payout_fraction\": {payout_fraction}, \"confidence\": {confidence}, \
                 \"reasoning\": \"mock\", \"evidence_excerpts\": [\"mock evidence\"]}}"
            ),
        }
    }
}

#[async_trait::async_trait]
impl LlmClient for MockLlm {
    async fn complete(&self, _req: &LlmRequest) -> Result<String, LlmError> {
        Ok(self.raw.clone())
    }
}

/// OpenRouter-backed client (OpenAI-compatible chat completions). Uses the same
/// `OPENROUTER_API_KEY` the arena bots use. Plain `reqwest`, bounded timeout,
/// zero retries — the resolver's own poll loop provides retry cadence.
pub struct OpenRouterClient {
    http: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenRouterClient {
    pub fn new(http: reqwest::Client, api_key: String, model: String) -> Self {
        Self {
            http,
            api_key,
            model,
            base_url: "https://openrouter.ai/api/v1".to_string(),
        }
    }

    /// Timeout applied to each completion request.
    pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
}

#[async_trait::async_trait]
impl LlmClient for OpenRouterClient {
    async fn complete(&self, req: &LlmRequest) -> Result<String, LlmError> {
        let body = serde_json::json!({
            "model": self.model,
            "messages": [{ "role": "user", "content": req.user_prompt() }],
            // Nudge the model toward emitting a bare JSON object.
            "response_format": { "type": "json_object" },
            "temperature": 0.0,
        });

        let resp = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .timeout(Self::REQUEST_TIMEOUT)
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::Transport(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(LlmError::Transport(format!("HTTP {status}: {text}")));
        }

        let value: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| LlmError::Transport(e.to_string()))?;

        value
            .pointer("/choices/0/message/content")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or(LlmError::Empty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_payout_fraction() {
        let v = parse_verdict(
            "{\"payout_fraction\": 1.0, \"confidence\": 0.95, \"reasoning\": \"yes\", \"evidence_excerpts\": [\"e\"]}",
        )
        .unwrap();
        assert_eq!(v.payout_nanos, NANOS_PER_DOLLAR);
        assert_eq!(v.confidence, 0.95);
        assert_eq!(v.evidence_excerpts, vec!["e".to_string()]);
    }

    #[test]
    fn parses_yes_no_outcome() {
        let yes = parse_verdict("{\"proposed_outcome\": \"YES\", \"confidence\": 0.9}").unwrap();
        assert_eq!(yes.payout_nanos, NANOS_PER_DOLLAR);
        let no = parse_verdict("{\"proposed_outcome\": \"no\", \"confidence\": 0.9}").unwrap();
        assert_eq!(no.payout_nanos, 0);
    }

    #[test]
    fn extracts_json_from_fenced_prose() {
        let raw = "Sure! Here is my answer:\n```json\n{\"payout_fraction\": 0.5, \"confidence\": 0.8}\n```\nThanks!";
        let v = parse_verdict(raw).unwrap();
        assert_eq!(v.payout_nanos, NANOS_PER_DOLLAR / 2);
    }

    #[test]
    fn garbled_output_is_parse_error() {
        assert!(matches!(
            parse_verdict("the market resolves YES i think"),
            Err(LlmError::Parse(_))
        ));
        assert!(matches!(
            parse_verdict("{not json"),
            Err(LlmError::Parse(_))
        ));
    }

    #[test]
    fn out_of_range_numbers_rejected() {
        assert!(matches!(
            parse_verdict("{\"payout_fraction\": 1.5, \"confidence\": 0.9}"),
            Err(LlmError::Parse(_))
        ));
        assert!(matches!(
            parse_verdict("{\"payout_fraction\": 0.5, \"confidence\": 2.0}"),
            Err(LlmError::Parse(_))
        ));
    }

    #[test]
    fn missing_outcome_is_parse_error() {
        assert!(matches!(
            parse_verdict("{\"confidence\": 0.9, \"reasoning\": \"hmm\"}"),
            Err(LlmError::Parse(_))
        ));
    }

    #[tokio::test]
    async fn mock_evaluate_roundtrips() {
        let mock = MockLlm::verdict(1.0, 0.97);
        let req = LlmRequest {
            question: "q".into(),
            resolution_criteria: "c".into(),
            source_content: "s".into(),
        };
        let v = mock.evaluate(&req).await.unwrap();
        assert_eq!(v.payout_nanos, NANOS_PER_DOLLAR);
        assert_eq!(v.confidence, 0.97);
    }
}
