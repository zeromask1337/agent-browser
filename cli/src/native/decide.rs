//! Typed decision primitive behind the `act` command.
//!
//! `act "<instruction>"` turns the current snapshot into a closed set of
//! candidate elements, asks one decider to pick an operation and a target, and
//! then runs that action through the ordinary handlers. The decider never
//! writes selectors, coordinates, or JavaScript: it can only return an option
//! this module offered.
//!
//! Two deciders implement the same contract:
//!
//! - [`JevDecider`] calls TypeSafe's System One API with a Choice question per
//!   head. Its probabilities are calibrated, so `--min-confidence` applies.
//! - [`LlmDecider`] is the evaluation baseline. It uses the AI Gateway with a
//!   structured-output request. Its probabilities are 0 or 1, so
//!   `--min-confidence` does not apply.
//!
//! Page data leaves the machine only as candidate role, accessible name, the
//! page URL and title, the instruction, and the caller's value key names.
//! Input values, passwords, cookies, storage, and auth vault entries are never
//! added to a request.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::cdp::client::CdpClient;

/// Roles offered to a decider. Only actionable, visible elements are offered.
pub const CANDIDATE_ROLES: &[&str] = &[
    "link",
    "button",
    "textbox",
    "searchbox",
    "combobox",
    "checkbox",
    "radio",
    "option",
    "menuitem",
    "tab",
];

/// Candidate cap. The list is truncated in DOM order and the response reports
/// truncation so a caller can snapshot a narrower region.
pub const MAX_CANDIDATES: usize = 80;

/// Closed operation set for the first slice.
pub const OPERATIONS: &[&str] = &[
    "click",
    "fill",
    "select",
    "scroll_down",
    "scroll_up",
    "back",
    "wait",
    "done",
];

pub const DEFAULT_TYPESAFE_BASE_URL: &str = "https://api.typesafe.ai";
pub const DEFAULT_TYPESAFE_MODEL: &str = "jev-latest";
/// Same default as the existing chat client.
pub const DEFAULT_DECIDER_MODEL: &str = "anthropic/claude-sonnet-4.6";
pub const DEFAULT_AI_GATEWAY_URL: &str = "https://ai-gateway.vercel.sh";

/// Probability sum tolerance accepted from a decider.
const PROBABILITY_SUM_TOLERANCE: f64 = 0.02;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Click,
    Fill,
    Select,
    ScrollDown,
    ScrollUp,
    Back,
    Wait,
    Done,
}

impl Operation {
    pub fn as_str(self) -> &'static str {
        match self {
            Operation::Click => "click",
            Operation::Fill => "fill",
            Operation::Select => "select",
            Operation::ScrollDown => "scroll_down",
            Operation::ScrollUp => "scroll_up",
            Operation::Back => "back",
            Operation::Wait => "wait",
            Operation::Done => "done",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        OPERATIONS
            .iter()
            .find(|op| **op == value)
            .map(|op| match *op {
                "click" => Operation::Click,
                "fill" => Operation::Fill,
                "select" => Operation::Select,
                "scroll_down" => Operation::ScrollDown,
                "scroll_up" => Operation::ScrollUp,
                "back" => Operation::Back,
                "wait" => Operation::Wait,
                "done" => Operation::Done,
                _ => unreachable!("OPERATIONS only contains known operations"),
            })
    }

    /// Operations that require a target element.
    pub fn needs_target(self) -> bool {
        matches!(self, Operation::Click | Operation::Fill | Operation::Select)
    }

    /// Operations that consume a caller-supplied value.
    pub fn needs_value(self) -> bool {
        matches!(self, Operation::Fill | Operation::Select)
    }
}

fn operation_criteria() -> Value {
    let mut criteria = Map::new();
    criteria.insert("click".to_string(), json!("Click the target element once."));
    criteria.insert(
        "fill".to_string(),
        json!("Enter one caller-supplied value into the target field."),
    );
    criteria.insert(
        "select".to_string(),
        json!("Choose one caller-supplied value in the target dropdown."),
    );
    criteria.insert(
        "scroll_down".to_string(),
        json!("Scroll the page down to reveal more content."),
    );
    criteria.insert(
        "scroll_up".to_string(),
        json!("Scroll the page up to reveal earlier content."),
    );
    criteria.insert("back".to_string(), json!("Go back in the browser history."));
    criteria.insert(
        "wait".to_string(),
        json!("Wait briefly for the page to settle, then look again."),
    );
    criteria.insert(
        "done".to_string(),
        json!("The instruction is already satisfied, so no action is needed."),
    );
    Value::Object(criteria)
}

/// One offered element.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    /// Option key, always the snapshot ref id such as `e12`.
    pub key: String,
    pub role: String,
    pub name: String,
    /// Index among candidates that share the same role and name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nth: Option<usize>,
}

impl Candidate {
    /// One-line description sent as the option description.
    pub fn description(&self) -> String {
        let name = if self.name.is_empty() {
            "(no accessible name)".to_string()
        } else {
            self.name.clone()
        };
        match self.nth {
            Some(nth) => format!("role {} named {:?}, occurrence {}", self.role, name, nth),
            None => format!("role {} named {:?}", self.role, name),
        }
    }
}

/// A ref that may become a candidate, decoupled from `RefEntry` so tests can
/// build candidates without a CDP session.
#[derive(Clone, Debug, PartialEq)]
pub struct CandidateInput {
    pub key: String,
    pub role: String,
    pub name: String,
    pub nth: Option<usize>,
    pub backend_node_id: Option<i64>,
    /// Refs from another frame are not measurable from the active page session.
    pub frame_id: Option<String>,
}

/// Viewport-relative rectangle in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Candidates plus whether the cap dropped any.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CandidateSet {
    pub candidates: Vec<Candidate>,
    pub truncated: bool,
}

/// Keep visible, actionable refs in DOM order and cap the list.
///
/// `visible` maps a candidate key to its rectangle when the element passed the
/// visibility probe. A key that is absent is not visible.
pub fn assemble_candidates(
    inputs: &[CandidateInput],
    visible: &HashMap<String, Rect>,
    cap: usize,
) -> CandidateSet {
    let actionable: Vec<&CandidateInput> = inputs
        .iter()
        .filter(|input| CANDIDATE_ROLES.contains(&input.role.as_str()))
        .filter(|input| input.frame_id.is_none())
        .filter(|input| visible.contains_key(&input.key))
        .collect();

    let truncated = actionable.len() > cap;
    let candidates = actionable
        .into_iter()
        .take(cap)
        .map(|input| Candidate {
            key: input.key.clone(),
            role: input.role.clone(),
            name: input.name.clone(),
            nth: input.nth,
        })
        .collect();

    CandidateSet {
        candidates,
        truncated,
    }
}

/// Hash of the URL plus candidate identity and geometry at decision time.
///
/// A change means the page moved under the decision, so the caller decides
/// once more and then reports `stale` instead of acting on a stale ref.
pub fn fingerprint(url: &str, candidates: &[Candidate], visible: &HashMap<String, Rect>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    for candidate in candidates {
        hasher.update(b"\n");
        hasher.update(candidate.key.as_bytes());
        hasher.update(b"\x1f");
        hasher.update(candidate.role.as_bytes());
        hasher.update(b"\x1f");
        hasher.update(candidate.name.as_bytes());
        if let Some(rect) = visible.get(&candidate.key) {
            hasher.update(
                format!(
                    "\x1f{:.0},{:.0},{:.0},{:.0}",
                    rect.x, rect.y, rect.width, rect.height
                )
                .as_bytes(),
            );
        }
    }
    let digest = hasher.finalize();
    hex::encode(&digest[..8])
}

/// Read every element's viewport rectangle in one page evaluation, then map the
/// tagged elements back to backend node ids in one DOM call.
///
/// The page is tagged with `data-ab-cand` the same way the snapshot tags
/// cursor-interactive elements. No CDP call is made per candidate.
pub async fn probe_visible_rects(
    client: &CdpClient,
    session_id: &str,
) -> Result<HashMap<i64, Rect>, String> {
    let js = r#"
(function() {
    var out = [];
    var els = document.querySelectorAll('*');
    for (var i = 0; i < els.length; i++) {
        var el = els[i];
        try { el.setAttribute('data-ab-cand', String(i)); } catch (e) { continue; }
        var r = el.getBoundingClientRect();
        var cs = getComputedStyle(el);
        var visible = r.width > 0 && r.height > 0 &&
            cs.display !== 'none' && cs.visibility !== 'hidden' &&
            r.bottom > 0 && r.right > 0 &&
            r.top < (window.innerHeight || 0) && r.left < (window.innerWidth || 0);
        out.push({ x: r.x, y: r.y, w: r.width, h: r.height, v: visible });
    }
    return out;
})()
"#;

    let result: Value = client
        .send_command(
            "Runtime.evaluate",
            Some(json!({
                "expression": js,
                "returnByValue": true,
                "awaitPromise": false,
            })),
            Some(session_id),
        )
        .await?;

    if let Some(exception) = result
        .get("exceptionDetails")
        .and_then(|details| details.get("text"))
        .and_then(|text| text.as_str())
    {
        return Err(format!("Could not measure the page: {}", exception));
    }

    let rects: Vec<Value> = result
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| serde_json::from_value::<Vec<Value>>(v.clone()).ok())
        .unwrap_or_default();

    let doc: Value = client
        .send_command(
            "DOM.getDocument",
            Some(json!({ "depth": -1 })),
            Some(session_id),
        )
        .await?;

    let mut by_backend: HashMap<i64, Rect> = HashMap::new();
    if let Some(root) = doc.get("root") {
        collect_tagged_rects(root, &rects, &mut by_backend);
    }
    Ok(by_backend)
}

fn collect_tagged_rects(node: &Value, rects: &[Value], out: &mut HashMap<i64, Rect>) {
    if node.get("nodeType").and_then(|v| v.as_i64()) == Some(1) {
        let index = node
            .get("attributes")
            .and_then(|v| v.as_array())
            .and_then(|attrs| {
                attrs
                    .chunks(2)
                    .find(|pair| pair.first().and_then(|v| v.as_str()) == Some("data-ab-cand"))
                    .and_then(|pair| pair.get(1))
                    .and_then(|v| v.as_str())
                    .and_then(|value| value.parse::<usize>().ok())
            });
        if let (Some(index), Some(backend)) =
            (index, node.get("backendNodeId").and_then(|v| v.as_i64()))
        {
            if let Some(rect) = rects.get(index) {
                let visible = rect.get("v").and_then(|v| v.as_bool()).unwrap_or(false);
                if visible {
                    out.insert(
                        backend,
                        Rect {
                            x: rect.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0),
                            y: rect.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0),
                            width: rect.get("w").and_then(|v| v.as_f64()).unwrap_or(0.0),
                            height: rect.get("h").and_then(|v| v.as_f64()).unwrap_or(0.0),
                        },
                    );
                }
            }
        }
    }
    if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
        for child in children {
            collect_tagged_rects(child, rects, out);
        }
    }
    if let Some(content) = node.get("contentDocument") {
        collect_tagged_rects(content, rects, out);
    }
}

/// Convert the current ref map into candidate inputs in ref order.
pub fn candidate_inputs(ref_map: &super::element::RefMap) -> Vec<CandidateInput> {
    ref_map
        .entries_sorted()
        .into_iter()
        .map(|(key, entry)| CandidateInput {
            key,
            role: entry.role,
            name: entry.name,
            nth: entry.nth,
            backend_node_id: entry.backend_node_id,
            frame_id: entry.frame_id,
        })
        .collect()
}

/// Everything a decider may see about the current request.
#[derive(Clone, Debug)]
pub struct DecisionInput {
    pub instruction: String,
    pub url: String,
    pub title: String,
    /// Caller-supplied value key names. Values themselves are never included.
    pub value_keys: Vec<String>,
    pub candidates: Vec<Candidate>,
    pub truncated: bool,
}

/// The state and questions sent to a decider.
#[derive(Clone, Debug)]
pub struct DecisionRequest {
    pub model: String,
    pub state: Value,
    pub questions: Value,
    /// Which question ids were offered, for response validation.
    pub offered: OfferedHeads,
}

impl DecisionRequest {
    pub fn build(model: &str, input: &DecisionInput, ask_value: bool) -> Self {
        let mut candidate_index = Map::new();
        for (position, candidate) in input.candidates.iter().enumerate() {
            candidate_index.insert(
                candidate.key.clone(),
                json!({
                    "role": candidate.role,
                    "name": candidate.name,
                    "nth": candidate.nth,
                    "position": position,
                }),
            );
        }

        let state = json!({
            "instruction": input.instruction,
            "page": { "url": input.url, "title": input.title },
            "value_keys": input.value_keys,
            "candidates": candidate_index,
            "candidates_truncated": input.truncated,
        });

        let mut questions = Map::new();
        questions.insert(
            "operation".to_string(),
            json!({
                "type": "choice",
                "instructions": "Which single operation should be performed next to satisfy the instruction? Use done only when the page already shows the result.",
                "criteria": operation_criteria(),
            }),
        );

        if !input.candidates.is_empty() {
            let mut criteria = Map::new();
            for candidate in &input.candidates {
                criteria.insert(candidate.key.clone(), json!(candidate.description()));
            }
            questions.insert(
                "target".to_string(),
                json!({
                    "type": "choice",
                    "instructions": "Which offered element should click, fill, or select act on? Ignore this question for other operations.",
                    "criteria": Value::Object(criteria),
                }),
            );
        }

        if ask_value {
            let mut criteria = Map::new();
            for key in &input.value_keys {
                criteria.insert(
                    key.clone(),
                    json!(format!("The caller-supplied value named {:?}", key)),
                );
            }
            questions.insert(
                "value_key".to_string(),
                json!({
                    "type": "choice",
                    "instructions": "Which caller-supplied value key should be entered or selected?",
                    "criteria": Value::Object(criteria),
                }),
            );
        }

        Self {
            model: model.to_string(),
            state,
            questions: Value::Object(questions),
            offered: OfferedHeads {
                target: !input.candidates.is_empty(),
                value_key: ask_value,
            },
        }
    }

    /// The documented System One request body.
    pub fn jev_body(&self) -> Value {
        json!({
            "model": self.model,
            "state": self.state,
            "questions": self.questions,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct OfferedHeads {
    pub target: bool,
    pub value_key: bool,
}

/// One parsed, validated decision.
#[derive(Clone, Debug, Serialize)]
pub struct Decision {
    pub operation: Operation,
    pub target: Option<String>,
    pub value_key: Option<String>,
    /// Distribution that gated the decision: the target head for operations
    /// that need a target, otherwise the operation head.
    pub probabilities: Map<String, Value>,
    /// Lowest confidence among the heads that mattered.
    pub confidence: f64,
}

/// Raw answers from a decider.
#[derive(Clone, Debug)]
pub struct DeciderResponse {
    pub answers: Value,
    /// Present when the provider reports token usage.
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// Errors that never contain the API key or the response body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecideError {
    /// The API key is missing or malformed.
    Config(String),
    /// The provider rejected the key.
    Auth,
    /// The request was too large.
    PayloadTooLarge,
    /// The provider rejected the request as malformed.
    InvalidRequest,
    /// The provider asked us to slow down.
    RateLimited { retry_after_secs: Option<u64> },
    /// The provider is overloaded.
    Overloaded,
    /// The network failed.
    Transport,
    /// The provider returned something this contract cannot parse.
    Protocol,
    /// The answer was outside the offered options or its probabilities did not
    /// form a distribution.
    InvalidAnswer(String),
}

impl DecideError {
    pub fn message(&self) -> String {
        match self {
            DecideError::Config(message) => message.clone(),
            DecideError::Auth => {
                "The decider rejected the API key. Check TYPESAFE_API_KEY.".to_string()
            }
            DecideError::PayloadTooLarge => {
                "The decider rejected the request as too large. Snapshot a smaller region."
                    .to_string()
            }
            DecideError::InvalidRequest => {
                "The decider rejected the request as malformed.".to_string()
            }
            DecideError::RateLimited {
                retry_after_secs: Some(secs),
            } => format!("The decider is rate limited. Retry after {} seconds.", secs),
            DecideError::RateLimited {
                retry_after_secs: None,
            } => "The decider is rate limited.".to_string(),
            DecideError::Overloaded => "The decider is overloaded. Retry in a moment.".to_string(),
            DecideError::Transport => {
                "Could not reach the decider. Check the network and TYPESAFE_BASE_URL.".to_string()
            }
            DecideError::Protocol => {
                "The decider returned a response this client could not parse.".to_string()
            }
            DecideError::InvalidAnswer(message) => message.clone(),
        }
    }
}

/// The single contract both deciders implement.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait Decider: Send + Sync {
    fn name(&self) -> &'static str;

    /// Model id used for the request.
    fn model(&self) -> &str;

    /// Whether probabilities are meaningful enough for `--min-confidence`.
    fn calibrated(&self) -> bool {
        true
    }

    async fn decide(&self, request: &DecisionRequest) -> Result<DeciderResponse, DecideError>;
}

fn validate_probabilities(
    label: &str,
    choice: &str,
    probabilities: &Map<String, Value>,
    offered: &Map<String, Value>,
) -> Result<(f64, Map<String, Value>), DecideError> {
    let invalid = |message: String| DecideError::InvalidAnswer(message);

    if probabilities.len() != offered.len() {
        return Err(invalid(format!(
            "The {} answer must include a probability for every offered option.",
            label
        )));
    }
    for key in offered.keys() {
        if !probabilities.contains_key(key) {
            return Err(invalid(format!(
                "The {} answer is missing an offered option.",
                label
            )));
        }
    }
    let mut sum = 0.0;
    let mut max = f64::NEG_INFINITY;
    let mut chosen = None;
    for (key, value) in probabilities {
        let Some(value) = value.as_f64() else {
            return Err(invalid(format!(
                "The {} answer contains a non-numeric probability.",
                label
            )));
        };
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(invalid(format!(
                "The {} answer contains a probability outside 0..1.",
                label
            )));
        }
        sum += value;
        if value > max {
            max = value;
        }
        if key == choice {
            chosen = Some(value);
        }
    }
    let Some(chosen) = chosen else {
        return Err(invalid(format!(
            "The {} answer did not choose an offered option.",
            label
        )));
    };
    if (sum - 1.0).abs() > PROBABILITY_SUM_TOLERANCE {
        return Err(invalid(format!(
            "The {} answer's probabilities must sum to about 1.",
            label
        )));
    }
    if chosen + 1e-9 < max {
        return Err(invalid(format!(
            "The {} answer must favor the choice it returned.",
            label
        )));
    }
    Ok((chosen, probabilities.clone()))
}

fn choice_answer(
    answers: &Value,
    head: &str,
    questions: &Value,
) -> Result<(String, Map<String, Value>, f64), DecideError> {
    let answer = answers.get(head).ok_or_else(|| {
        DecideError::InvalidAnswer(format!("The response did not include the {} answer.", head))
    })?;
    let offered = questions
        .get(head)
        .and_then(|q| q.get("criteria"))
        .and_then(|c| c.as_object())
        .ok_or_else(|| DecideError::InvalidAnswer(format!("Invalid {} question.", head)))?;

    let choice = answer
        .get("choice")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            DecideError::InvalidAnswer(format!("The {} answer did not name a choice.", head))
        })?
        .to_string();
    if !offered.contains_key(&choice) {
        return Err(DecideError::InvalidAnswer(format!(
            "The {} answer was not one of the offered options.",
            head
        )));
    }

    let probabilities = answer
        .get("probabilities")
        .and_then(|v| v.as_object())
        .ok_or_else(|| {
            DecideError::InvalidAnswer(format!("The {} answer had no probabilities.", head))
        })?;
    let (_, probabilities) = validate_probabilities(head, &choice, probabilities, offered)?;

    let confidence = match answer.get("confidence").and_then(|v| v.as_f64()) {
        Some(confidence) if confidence.is_finite() && (0.0..=1.0).contains(&confidence) => {
            confidence
        }
        Some(_) => {
            return Err(DecideError::InvalidAnswer(format!(
                "The {} confidence must be between 0 and 1.",
                head
            )))
        }
        None => probabilities
            .get(&choice)
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
    };

    Ok((choice, probabilities, confidence))
}

/// Validate a raw answer strictly against the offered options.
pub fn validate_decision(
    decision_request: &DecisionRequest,
    answers: &Value,
) -> Result<Decision, DecideError> {
    let questions = &decision_request.questions;

    let (operation_name, operation_probabilities, operation_confidence) =
        choice_answer(answers, "operation", questions)?;
    let operation = Operation::parse(&operation_name).ok_or_else(|| {
        DecideError::InvalidAnswer("The operation answer was not a known operation.".to_string())
    })?;

    let mut target = None;
    let mut target_head = None;
    if decision_request.offered.target {
        let (choice, probabilities, confidence) = choice_answer(answers, "target", questions)?;
        if operation.needs_target() {
            target = Some(choice);
            target_head = Some((probabilities, confidence));
        }
        // The target head is asked in the same request, so a non-target
        // operation may still return one. It is ignored, not an error.
    }

    if operation.needs_target() && target.is_none() {
        return Err(DecideError::InvalidAnswer(
            "The operation needs a target, but no target answer was offered.".to_string(),
        ));
    }

    let mut value_key = None;
    let mut value_confidence = None;
    if decision_request.offered.value_key {
        if operation.needs_value() {
            let (choice, _, confidence) = choice_answer(answers, "value_key", questions)?;
            value_key = Some(choice);
            value_confidence = Some(confidence);
        }
    } else if operation.needs_value() && decision_request.state_value_key_count() > 0 {
        // One supplied value needs no decision.
        value_key = decision_request.state_value_keys().into_iter().next();
    }

    if operation.needs_value() && value_key.is_none() {
        return Err(DecideError::InvalidAnswer(
            "The operation needs a caller-supplied value, but none was supplied.".to_string(),
        ));
    }

    let gating = match target_head {
        Some((probabilities, confidence)) => (probabilities, confidence),
        None => (operation_probabilities.clone(), operation_confidence),
    };
    let confidence = match value_confidence {
        Some(value) => gating.1.min(value),
        None => gating.1,
    };

    Ok(Decision {
        operation,
        target,
        value_key,
        probabilities: gating.0,
        confidence,
    })
}

impl DecisionRequest {
    fn state_value_keys(&self) -> Vec<String> {
        self.state
            .get("value_keys")
            .and_then(|v| v.as_array())
            .map(|keys| {
                keys.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn state_value_key_count(&self) -> usize {
        self.state_value_keys().len()
    }
}

/// Ask the decider, validate, and retry exactly once on an invalid answer.
pub async fn decide_with_retry(
    decider: &dyn Decider,
    request: &DecisionRequest,
) -> Result<Decision, DecideError> {
    let mut last = None;
    for attempt in 0..2 {
        let response = decider.decide(request).await?;
        match validate_decision(request, &response.answers) {
            Ok(decision) => return Ok(decision),
            Err(DecideError::InvalidAnswer(message)) if attempt == 0 => {
                last = Some(message);
            }
            Err(error) => return Err(error),
        }
    }
    Err(DecideError::InvalidAnswer(format!(
        "The decider returned an invalid answer twice and nothing ran. {}",
        last.unwrap_or_default()
    )))
}

/// Validate a TypeSafe API key the same way the vendor SDKs do.
///
/// Surrounding whitespace is stripped. Empty keys, internal whitespace,
/// control characters, and non-ASCII characters are rejected. The key itself
/// is never included in an error message.
pub fn validate_api_key(raw: &str) -> Result<String, String> {
    let key = raw.trim();
    if key.is_empty() {
        return Err("TYPESAFE_API_KEY is empty.".to_string());
    }
    if key.chars().any(char::is_whitespace) {
        return Err("TYPESAFE_API_KEY must not contain whitespace.".to_string());
    }
    if key.chars().any(char::is_control) {
        return Err("TYPESAFE_API_KEY must not contain control characters.".to_string());
    }
    if !key.is_ascii() {
        return Err("TYPESAFE_API_KEY must be ASCII.".to_string());
    }
    Ok(key.to_string())
}

fn build_http_client() -> Result<reqwest::Client, String> {
    let builder =
        crate::tls::apply_to_reqwest(reqwest::Client::builder(), &crate::tls::process_options())
            .map_err(|error| format!("Failed to configure TLS for the decider: {}", error))?;
    builder
        .build()
        .map_err(|error| format!("Failed to create the decider HTTP client: {}", error))
}

/// TypeSafe System One decider.
pub struct JevDecider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl JevDecider {
    pub fn new(api_key: &str, base_url: &str, model: &str) -> Result<Self, String> {
        let api_key = validate_api_key(api_key)?;
        Ok(Self {
            client: build_http_client()?,
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
        })
    }

    fn endpoint(&self) -> String {
        format!("{}/v1/systemone", self.base_url)
    }
}

fn retry_after_secs(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
}

#[async_trait]
impl Decider for JevDecider {
    fn name(&self) -> &'static str {
        "jev"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn decide(&self, request: &DecisionRequest) -> Result<DeciderResponse, DecideError> {
        let body = request.jev_body();
        let mut attempt = 0;
        loop {
            let response = self
                .client
                .post(self.endpoint())
                .header(
                    reqwest::header::AUTHORIZATION,
                    format!("Bearer {}", self.api_key),
                )
                .json(&body)
                .send()
                .await
                .map_err(|_| DecideError::Transport)?;

            let status = response.status();
            if status.is_success() {
                let payload: Value = response.json().await.map_err(|_| DecideError::Protocol)?;
                let answers = payload
                    .get("answers")
                    .cloned()
                    .ok_or(DecideError::Protocol)?;
                let usage = payload.get("usage");
                return Ok(DeciderResponse {
                    answers,
                    input_tokens: usage
                        .and_then(|u| u.get("input_tokens"))
                        .and_then(|v| v.as_u64()),
                    output_tokens: usage
                        .and_then(|u| u.get("output_tokens"))
                        .and_then(|v| v.as_u64()),
                });
            }

            let retry_after = retry_after_secs(&response);
            let error = match status.as_u16() {
                401 => DecideError::Auth,
                413 => DecideError::PayloadTooLarge,
                422 => DecideError::InvalidRequest,
                429 => DecideError::RateLimited {
                    retry_after_secs: retry_after,
                },
                529 => DecideError::Overloaded,
                500..=599 => DecideError::Transport,
                _ => DecideError::InvalidRequest,
            };

            let retryable = matches!(
                error,
                DecideError::RateLimited { .. } | DecideError::Overloaded | DecideError::Transport
            );
            if !retryable || attempt >= 2 {
                return Err(error);
            }
            attempt += 1;
            let delay = retry_after
                .map(std::time::Duration::from_secs)
                .unwrap_or_else(|| std::time::Duration::from_millis(500 * attempt));
            tokio::time::sleep(delay).await;
        }
    }
}

/// AI Gateway decider used as the evaluation baseline.
pub struct LlmDecider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl LlmDecider {
    pub fn new(api_key: &str, model: &str) -> Result<Self, String> {
        let api_key = api_key.trim();
        if api_key.is_empty() {
            return Err("AI_GATEWAY_API_KEY is empty.".to_string());
        }
        Ok(Self {
            client: build_http_client()?,
            api_key: api_key.to_string(),
            base_url: std::env::var("AI_GATEWAY_URL")
                .unwrap_or_else(|_| DEFAULT_AI_GATEWAY_URL.to_string())
                .trim_end_matches('/')
                .to_string(),
            model: model.to_string(),
        })
    }

    fn schema(request: &DecisionRequest) -> Value {
        let mut properties = Map::new();
        properties.insert(
            "operation".to_string(),
            json!({
                "type": "string",
                "enum": OPERATIONS,
            }),
        );
        let mut required = vec![json!("operation")];
        if let Some(targets) = request
            .questions
            .get("target")
            .and_then(|q| q.get("criteria"))
            .and_then(|c| c.as_object())
        {
            properties.insert(
                "target".to_string(),
                json!({
                    "type": "string",
                    "enum": targets.keys().collect::<Vec<_>>(),
                }),
            );
            required.push(json!("target"));
        }
        if let Some(values) = request
            .questions
            .get("value_key")
            .and_then(|q| q.get("criteria"))
            .and_then(|c| c.as_object())
        {
            properties.insert(
                "value_key".to_string(),
                json!({
                    "type": "string",
                    "enum": values.keys().collect::<Vec<_>>(),
                }),
            );
            required.push(json!("value_key"));
        }

        json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        })
    }
}

fn parse_llm_json(content: &str) -> Result<Value, DecideError> {
    let trimmed = content.trim();
    let candidate = if let Some(rest) = trimmed.strip_prefix("```json") {
        rest.trim().trim_end_matches("```").trim()
    } else if let Some(rest) = trimmed.strip_prefix("```") {
        rest.trim().trim_end_matches("```").trim()
    } else {
        trimmed
    };
    serde_json::from_str(candidate).map_err(|_| DecideError::Protocol)
}

#[async_trait]
impl Decider for LlmDecider {
    fn name(&self) -> &'static str {
        "llm"
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn calibrated(&self) -> bool {
        false
    }

    async fn decide(&self, request: &DecisionRequest) -> Result<DeciderResponse, DecideError> {
        let schema = Self::schema(request);
        let system = "You are a browser action decoder. Read the JSON state and questions, then return one JSON object that satisfies the schema. Return an offered option key exactly. Never invent a key.";
        let user = json!({ "state": request.state, "questions": request.questions }).to_string();
        let body = json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ],
            "temperature": 0,
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "browser_decision",
                    "strict": true,
                    "schema": schema,
                }
            }
        });

        let response = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.api_key),
            )
            .json(&body)
            .send()
            .await
            .map_err(|_| DecideError::Transport)?;

        let status = response.status();
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 => DecideError::Auth,
                413 => DecideError::PayloadTooLarge,
                422 => DecideError::InvalidRequest,
                429 => DecideError::RateLimited {
                    retry_after_secs: retry_after_secs(&response),
                },
                529 => DecideError::Overloaded,
                500..=599 => DecideError::Transport,
                _ => DecideError::InvalidRequest,
            });
        }

        let payload: Value = response.json().await.map_err(|_| DecideError::Protocol)?;
        let content = payload
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or(DecideError::Protocol)?;
        let answer = parse_llm_json(content)?;

        let mut answers = Map::new();
        let operation = answer
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or(DecideError::Protocol)?;
        answers.insert(
            "operation".to_string(),
            synthesized_choice(request, "operation", operation),
        );
        for head in ["target", "value_key"] {
            if let Some(choice) = answer.get(head).and_then(|v| v.as_str()) {
                answers.insert(head.to_string(), synthesized_choice(request, head, choice));
            }
        }

        let usage = payload.get("usage");
        Ok(DeciderResponse {
            answers: Value::Object(answers),
            input_tokens: usage
                .and_then(|u| u.get("prompt_tokens"))
                .and_then(|v| v.as_u64()),
            output_tokens: usage
                .and_then(|u| u.get("completion_tokens"))
                .and_then(|v| v.as_u64()),
        })
    }
}

/// LLM probabilities are 0 or 1, so build a distribution that satisfies the
/// same validator without pretending to be calibrated.
fn synthesized_choice(request: &DecisionRequest, head: &str, choice: &str) -> Value {
    let keys: Vec<String> = request
        .questions
        .get(head)
        .and_then(|q| q.get("criteria"))
        .and_then(|c| c.as_object())
        .map(|criteria| criteria.keys().cloned().collect())
        .unwrap_or_default();
    let mut probabilities = Map::new();
    for key in keys {
        probabilities.insert(key.clone(), json!(if key == choice { 1.0 } else { 0.0 }));
    }
    json!({
        "type": "choice",
        "choice": choice,
        "confidence": 1.0,
        "probabilities": Value::Object(probabilities),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeciderKind {
    Jev,
    Llm,
}

impl DeciderKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "jev" => Some(Self::Jev),
            "llm" => Some(Self::Llm),
            _ => None,
        }
    }
}

/// Build the decider selected by `AGENT_BROWSER_DECIDER`.
///
/// There is no auto-selection. An unset variable lists the two options, and a
/// missing key for the chosen decider names the variable to set.
pub fn decider_from_env() -> Result<Box<dyn Decider>, String> {
    let raw = crate::flags::env_var_string("AGENT_BROWSER_DECIDER")
        .ok_or_else(|| "AGENT_BROWSER_DECIDER is not set. Set it to jev or llm.".to_string())?;
    let kind = DeciderKind::parse(&raw)
        .ok_or_else(|| "AGENT_BROWSER_DECIDER must be jev or llm.".to_string())?;

    match kind {
        DeciderKind::Jev => {
            let key = std::env::var("TYPESAFE_API_KEY").map_err(|_| {
                "TYPESAFE_API_KEY is not set. It is required when AGENT_BROWSER_DECIDER=jev."
                    .to_string()
            })?;
            let key = validate_api_key(&key)?;
            let base_url = crate::flags::env_var_string("TYPESAFE_BASE_URL")
                .unwrap_or_else(|| DEFAULT_TYPESAFE_BASE_URL.to_string());
            let model = crate::flags::env_var_string("TYPESAFE_MODEL")
                .unwrap_or_else(|| DEFAULT_TYPESAFE_MODEL.to_string());
            Ok(Box::new(JevDecider::new(&key, &base_url, &model)?))
        }
        DeciderKind::Llm => {
            let key = std::env::var("AI_GATEWAY_API_KEY").map_err(|_| {
                "AI_GATEWAY_API_KEY is not set. It is required when AGENT_BROWSER_DECIDER=llm."
                    .to_string()
            })?;
            let model = crate::flags::env_var_string("AGENT_BROWSER_DECIDER_MODEL")
                .unwrap_or_else(|| DEFAULT_DECIDER_MODEL.to_string());
            Ok(Box::new(LlmDecider::new(&key, &model)?))
        }
    }
}

/// Human-readable decider summary for `doctor` and `act --json`.
pub fn decider_summary() -> String {
    match crate::flags::env_var_string("AGENT_BROWSER_DECIDER") {
        Some(value) => match DeciderKind::parse(&value) {
            Some(DeciderKind::Jev) => "jev".to_string(),
            Some(DeciderKind::Llm) => "llm".to_string(),
            None => "invalid".to_string(),
        },
        None => "unset".to_string(),
    }
}

/// Constant used by tests and callers that need a stable default base URL.
pub fn default_base_url() -> &'static str {
    DEFAULT_TYPESAFE_BASE_URL
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::EnvGuard;
    use std::sync::Arc;

    fn candidate(key: &str, role: &str, name: &str, nth: Option<usize>) -> CandidateInput {
        CandidateInput {
            key: key.to_string(),
            role: role.to_string(),
            name: name.to_string(),
            nth,
            backend_node_id: Some(1),
            frame_id: None,
        }
    }

    fn rects(keys: &[&str]) -> HashMap<String, Rect> {
        keys.iter()
            .map(|key| {
                (
                    key.to_string(),
                    Rect {
                        x: 10.0,
                        y: 20.0,
                        width: 100.0,
                        height: 30.0,
                    },
                )
            })
            .collect()
    }

    fn input() -> DecisionInput {
        DecisionInput {
            instruction: "Click Submit".to_string(),
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            value_keys: vec![],
            candidates: vec![
                Candidate {
                    key: "e1".to_string(),
                    role: "button".to_string(),
                    name: "Submit".to_string(),
                    nth: None,
                },
                Candidate {
                    key: "e2".to_string(),
                    role: "link".to_string(),
                    name: "Help".to_string(),
                    nth: None,
                },
            ],
            truncated: false,
        }
    }

    fn answer(head: &str, choice: &str, probabilities: Value) -> Value {
        json!({
            "type": "choice",
            "choice": choice,
            "confidence": 0.8,
            "probabilities": probabilities,
        })
    }

    fn valid_answers(request: &DecisionRequest) -> Value {
        json!({
            "operation": answer("operation", "click", json!({
                "click": 0.9, "fill": 0.0, "select": 0.0, "scroll_down": 0.0,
                "scroll_up": 0.0, "back": 0.0, "wait": 0.05, "done": 0.05
            })),
            "target": answer("target", "e1", json!({ "e1": 0.95, "e2": 0.05 })),
        })
    }

    #[test]
    fn candidates_keep_dom_order_and_filter_roles_and_visibility() {
        let inputs = vec![
            candidate("e1", "button", "One", None),
            candidate("e2", "heading", "Not actionable", None),
            candidate("e3", "link", "Three", None),
            candidate("e4", "textbox", "Not visible", None),
        ];
        let visible = rects(&["e1", "e3"]);
        let set = assemble_candidates(&inputs, &visible, MAX_CANDIDATES);
        assert_eq!(
            set.candidates
                .iter()
                .map(|candidate| candidate.key.as_str())
                .collect::<Vec<_>>(),
            vec!["e1", "e3"]
        );
        assert!(!set.truncated);
    }

    #[test]
    fn candidates_cap_and_report_truncation() {
        let inputs: Vec<CandidateInput> = (0..5)
            .map(|index| {
                candidate(
                    &format!("e{}", index + 1),
                    "button",
                    &format!("Button {}", index),
                    None,
                )
            })
            .collect();
        let visible = rects(&["e1", "e2", "e3", "e4", "e5"]);
        let set = assemble_candidates(&inputs, &visible, 3);
        assert_eq!(set.candidates.len(), 3);
        assert!(set.truncated);
    }

    #[test]
    fn candidates_skip_other_frames() {
        let mut input = candidate("e1", "button", "One", None);
        input.frame_id = Some("frame".to_string());
        let visible = rects(&["e1"]);
        assert!(assemble_candidates(&[input], &visible, MAX_CANDIDATES)
            .candidates
            .is_empty());
    }

    #[test]
    fn candidate_description_includes_nth_only_when_repeated() {
        let candidate = Candidate {
            key: "e9".to_string(),
            role: "link".to_string(),
            name: "Read more".to_string(),
            nth: Some(1),
        };
        assert!(candidate.description().contains("occurrence 1"));
        let unique = Candidate {
            nth: None,
            ..candidate
        };
        assert!(!unique.description().contains("occurrence"));
    }

    #[test]
    fn request_carries_no_values_cookies_or_passwords() {
        let mut input = input();
        input.value_keys = vec!["email".to_string()];
        let request = DecisionRequest::build("jev-latest", &input, true);
        let body = request.jev_body().to_string();
        assert!(body.contains("email"));
        assert!(!body.contains("secret@example.com"));
        assert!(!body.contains("password"));
        assert!(!body.contains("cookie"));
        assert!(!body.contains("localStorage"));
        assert!(body.contains("\"model\":\"jev-latest\""));
        assert_eq!(
            request.questions["operation"]["criteria"]
                .as_object()
                .unwrap()
                .len(),
            OPERATIONS.len()
        );
        assert_eq!(
            request.questions["target"]["criteria"]
                .as_object()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn request_omits_value_head_when_not_supplied() {
        let request = DecisionRequest::build("jev-latest", &input(), false);
        assert!(request.questions.get("value_key").is_none());
    }

    #[test]
    fn validates_a_correct_answer() {
        let request = DecisionRequest::build("jev-latest", &input(), false);
        let decision = validate_decision(&request, &valid_answers(&request)).unwrap();
        assert_eq!(decision.operation, Operation::Click);
        assert_eq!(decision.target.as_deref(), Some("e1"));
        assert_eq!(
            decision.probabilities.get("e1").and_then(|v| v.as_f64()),
            Some(0.95)
        );
    }

    #[test]
    fn rejects_unknown_choice() {
        let request = DecisionRequest::build("jev-latest", &input(), false);
        let answers = json!({
            "operation": answer("operation", "click", json!({
                "click": 0.9, "fill": 0.0, "select": 0.0, "scroll_down": 0.0,
                "scroll_up": 0.0, "back": 0.0, "wait": 0.05, "done": 0.05
            })),
            "target": answer("target", "e99", json!({ "e1": 0.5, "e2": 0.5 })),
        });
        assert!(matches!(
            validate_decision(&request, &answers),
            Err(DecideError::InvalidAnswer(_))
        ));
    }

    #[test]
    fn rejects_missing_option() {
        let request = DecisionRequest::build("jev-latest", &input(), false);
        let answers = json!({
            "operation": answer("operation", "click", json!({
                "click": 0.95, "fill": 0.0, "select": 0.0, "scroll_down": 0.0,
                "scroll_up": 0.0, "back": 0.0, "wait": 0.05
            })),
            "target": answer("target", "e1", json!({ "e1": 0.95, "e2": 0.05 })),
        });
        assert!(matches!(
            validate_decision(&request, &answers),
            Err(DecideError::InvalidAnswer(_))
        ));
    }

    #[test]
    fn rejects_bad_probability_sum() {
        let request = DecisionRequest::build("jev-latest", &input(), false);
        let answers = json!({
            "operation": answer("operation", "click", json!({
                "click": 0.5, "fill": 0.0, "select": 0.0, "scroll_down": 0.0,
                "scroll_up": 0.0, "back": 0.0, "wait": 0.0, "done": 0.0
            })),
            "target": answer("target", "e1", json!({ "e1": 0.95, "e2": 0.05 })),
        });
        assert!(matches!(
            validate_decision(&request, &answers),
            Err(DecideError::InvalidAnswer(_))
        ));
    }

    #[test]
    fn rejects_choice_that_is_not_the_max() {
        let request = DecisionRequest::build("jev-latest", &input(), false);
        let answers = json!({
            "operation": answer("operation", "click", json!({
                "click": 0.9, "fill": 0.0, "select": 0.0, "scroll_down": 0.0,
                "scroll_up": 0.0, "back": 0.0, "wait": 0.05, "done": 0.05
            })),
            "target": answer("target", "e1", json!({ "e1": 0.1, "e2": 0.9 })),
        });
        assert!(matches!(
            validate_decision(&request, &answers),
            Err(DecideError::InvalidAnswer(_))
        ));
    }

    struct ScriptedDecider {
        answers: std::sync::Mutex<Vec<Result<Value, DecideError>>>,
        calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl Decider for ScriptedDecider {
        fn name(&self) -> &'static str {
            "scripted"
        }

        fn model(&self) -> &str {
            "scripted"
        }

        async fn decide(&self, _request: &DecisionRequest) -> Result<DeciderResponse, DecideError> {
            let index = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut queue = self.answers.lock().unwrap();
            let next = if queue.is_empty() {
                return Err(DecideError::Protocol);
            } else {
                queue.remove(0)
            };
            next.map(|answers| DeciderResponse {
                answers,
                input_tokens: None,
                output_tokens: None,
            })
        }
    }

    #[tokio::test]
    async fn retries_once_after_invalid_then_valid() {
        let request = DecisionRequest::build("jev-latest", &input(), false);
        let bad = json!({ "operation": answer("operation", "nope", json!({ "nope": 1.0 })) });
        let decider = Arc::new(ScriptedDecider {
            answers: std::sync::Mutex::new(vec![Ok(bad), Ok(valid_answers(&request))]),
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let decision = decide_with_retry(decider.as_ref(), &request).await.unwrap();
        assert_eq!(decision.operation, Operation::Click);
        assert_eq!(decider.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn invalid_twice_is_an_error() {
        let request = DecisionRequest::build("jev-latest", &input(), false);
        let bad = json!({ "operation": answer("operation", "nope", json!({ "nope": 1.0 })) });
        let decider = Arc::new(ScriptedDecider {
            answers: std::sync::Mutex::new(vec![Ok(bad.clone()), Ok(bad)]),
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let error = decide_with_retry(decider.as_ref(), &request)
            .await
            .unwrap_err();
        assert!(matches!(error, DecideError::InvalidAnswer(_)));
        assert_eq!(decider.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn fingerprint_changes_when_geometry_changes() {
        let candidates = input().candidates;
        let mut first = rects(&["e1", "e2"]);
        let before = fingerprint("https://example.com", &candidates, &first);
        assert_eq!(
            before,
            fingerprint("https://example.com", &candidates, &first),
            "fingerprint must be stable"
        );
        first.get_mut("e1").unwrap().y += 40.0;
        assert_ne!(
            before,
            fingerprint("https://example.com", &candidates, &first)
        );
        assert_ne!(
            before,
            fingerprint(
                "https://example.com/other",
                &candidates,
                &rects(&["e1", "e2"])
            )
        );
    }

    #[test]
    fn key_validation_trims_and_rejects_bad_keys() {
        assert_eq!(validate_api_key("  abc123  ").unwrap(), "abc123");
        assert!(validate_api_key("   ").is_err());
        assert!(validate_api_key("ab cd").is_err());
        assert!(validate_api_key("ab\ncd").is_err());
        assert!(validate_api_key("ab\u{00e9}").is_err());
        let error = validate_api_key("ab cd").unwrap_err();
        assert!(!error.contains("ab cd"));
    }

    #[test]
    fn error_messages_never_include_the_key_or_body() {
        for error in [
            DecideError::Auth,
            DecideError::PayloadTooLarge,
            DecideError::InvalidRequest,
            DecideError::RateLimited {
                retry_after_secs: Some(3),
            },
            DecideError::Overloaded,
            DecideError::Transport,
            DecideError::Protocol,
        ] {
            let message = error.message();
            assert!(!message.contains("sk-"));
            assert!(!message.contains("Bearer"));
        }
    }

    #[test]
    fn decider_selection_requires_an_explicit_choice() {
        let guard = EnvGuard::new(&[
            "AGENT_BROWSER_DECIDER",
            "TYPESAFE_API_KEY",
            "TYPESAFE_BASE_URL",
            "TYPESAFE_MODEL",
        ]);
        guard.remove("AGENT_BROWSER_DECIDER");
        let error = decider_from_env().err().unwrap();
        assert!(error.contains("jev"));
        assert!(error.contains("llm"));

        guard.set("AGENT_BROWSER_DECIDER", "nope");
        assert!(decider_from_env().err().unwrap().contains("jev or llm"));

        guard.set("AGENT_BROWSER_DECIDER", "jev");
        guard.remove("TYPESAFE_API_KEY");
        assert!(decider_from_env()
            .err()
            .unwrap()
            .contains("TYPESAFE_API_KEY"));

        guard.set("TYPESAFE_API_KEY", "   ");
        assert!(decider_from_env()
            .err()
            .unwrap()
            .contains("TYPESAFE_API_KEY"));

        guard.set("TYPESAFE_API_KEY", " bad key ");
        assert!(decider_from_env().err().unwrap().contains("whitespace"));
    }

    #[tokio::test]
    async fn jev_decider_sends_authorization_and_json_shape() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0_u8; 16384];
            let read = stream.read(&mut buffer).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            let body_start = request.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
            let body = request[body_start..].to_string();
            let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);

            let answers = json!({
                "operation": {
                    "type": "choice",
                    "choice": "click",
                    "confidence": 1.0,
                    "probabilities": {
                        "click": 1.0, "fill": 0.0, "select": 0.0, "scroll_down": 0.0,
                        "scroll_up": 0.0, "back": 0.0, "wait": 0.0, "done": 0.0
                    }
                },
                "target": {
                    "type": "choice",
                    "choice": "e1",
                    "confidence": 1.0,
                    "probabilities": { "e1": 1.0, "e2": 0.0 }
                }
            });
            let response_body = json!({
                "model": "jev-test",
                "answers": answers,
                "usage": { "input_tokens": 7, "output_tokens": 2 }
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            let _ = stream.write_all(response.as_bytes()).await;
            (request, parsed)
        });

        let decider =
            JevDecider::new("test-key", &format!("http://{}", addr), "jev-latest").unwrap();
        let request = DecisionRequest::build("jev-latest", &input(), false);
        let response = decider.decide(&request).await.unwrap();
        assert!(response.answers.get("operation").is_some());

        let (raw, parsed) = server.await.unwrap();
        assert!(raw
            .to_lowercase()
            .contains("authorization: bearer test-key"));
        assert_eq!(parsed["model"], "jev-latest");
        assert_eq!(parsed["questions"]["operation"]["type"], "choice");
        assert_eq!(
            parsed["questions"]["target"]["criteria"]["e1"],
            json!("role button named \"Submit\"")
        );
        assert!(parsed["state"].get("value_keys").is_some());
        assert!(!parsed.to_string().contains("cookie"));
    }

    #[tokio::test]
    async fn jev_decider_does_not_retry_4xx() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_for_server = hits.clone();

        let server = tokio::spawn(async move {
            loop {
                let (mut stream, _) = match listener.accept().await {
                    Ok(value) => value,
                    Err(_) => break,
                };
                let mut buffer = vec![0_u8; 4096];
                let _ = stream.read(&mut buffer).await;
                hits_for_server.fetch_add(1, Ordering::SeqCst);
                let body = "{\"detail\":\"invalid api key\"}";
                let response = format!(
                    "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });

        let decider =
            JevDecider::new("test-key", &format!("http://{}", addr), "jev-latest").unwrap();
        let request = DecisionRequest::build("jev-latest", &input(), false);
        let error = decider.decide(&request).await.unwrap_err();
        assert_eq!(error, DecideError::Auth);
        assert!(!error.message().contains("invalid api key"));
        server.abort();
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }
}
