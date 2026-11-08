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

/// Roles offered to a decider. Only rendered, actionable elements are offered.
/// Offscreen elements are included because an action scrolls the chosen ref into
/// view before it runs.
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

/// Closed operation set for the first slice. `act` is one caller-owned step, so
/// it has no `done` option: "nothing to do" is a caller judgment, and a page
/// that already shows the result is reported as blocked when no step is offered.
pub const OPERATIONS: &[&str] = &[
    "click",
    "fill",
    "select",
    "scroll_down",
    "scroll_up",
    "back",
    "wait",
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

fn operation_criteria(include_wait: bool) -> Value {
    let mut criteria = Map::new();
    criteria.insert(
        "click".to_string(),
        json!("Click the target element once. Only link, button, checkbox, radio, option, menuitem, tab, and a combobox that opens a menu activate on click. A textbox or searchbox takes text, so it is never clicked."),
    );
    criteria.insert(
        "fill".to_string(),
        json!("Enter one caller-supplied value into the target text field: a textbox, searchbox, or editable combobox. Choose fill for an instruction that enters or types text, even when value_keys is empty or does not match the field; a fill with no supplied value is blocked and is never turned into a click."),
    );
    criteria.insert(
        "select".to_string(),
        json!("Choose one caller-supplied value in the target dropdown, which is a combobox or listbox. Choose select for an instruction that picks, chooses, switches, or sets a dropdown value, even when value_keys is empty or does not match the dropdown; a select with no supplied value is blocked and is never turned into a click."),
    );
    criteria.insert(
        "scroll_down".to_string(),
        json!("Scroll the page down to reveal more content. Choose scroll_down when the instruction asks to scroll down, even when the page also shows a link with a matching name."),
    );
    criteria.insert(
        "scroll_up".to_string(),
        json!("Scroll the page up to reveal earlier content. Choose scroll_up when the instruction asks to scroll up or back toward the top, even when the page also shows a Back to top link."),
    );
    criteria.insert(
        "back".to_string(),
        json!("Go back to the previous page in browser history. Choose back when the instruction refers to the page before this one, the page it came from, or where it was, and does not name a visible control. When the instruction names a Back, Return, or Previous control, click that control instead."),
    );
    if include_wait {
        criteria.insert(
            "wait".to_string(),
            json!("Wait for the page to finish loading or settle, then look again. Choose wait whenever the instruction asks to wait, settle, load, or wait for a condition to appear. Wait is not offered when the instruction names an offered element to act on."),
        );
    }
    Value::Object(criteria)
}

/// Words that make a wait legitimate even when the instruction also names an
/// offered element. Matched by prefix so "waiting", "settles", and "loaded"
/// count. The bare noun "load" is not a cue: "Find loads" and "Load details"
/// are targets, not waits.
const WAIT_CUES: &[&str] = &[
    "wait", "settl", "loading", "loaded", "appear", "finish", "complet", "ready", "until",
    "spinner", "delay",
];

fn instruction_requests_wait(instruction: &str) -> bool {
    normalized_words(instruction)
        .iter()
        .any(|word| WAIT_CUES.iter().any(|cue| word.starts_with(cue)))
}

/// Lowercase alphanumeric words with punctuation removed, so an accessible
/// name can be compared against the instruction without partial-word matches.
fn normalized_words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether the instruction names this candidate's accessible name.
pub(crate) fn instruction_names_candidate(instruction: &str, name: &str) -> bool {
    let name_words = normalized_words(name);
    if name_words.is_empty() {
        return false;
    }
    let instruction_words = normalized_words(instruction);
    instruction_words
        .windows(name_words.len())
        .any(|window| window == name_words.as_slice())
}

/// Wait is offered when the instruction asks for it, or when no offered
/// candidate matches a target named in the instruction.
fn wait_is_offered(input: &DecisionInput) -> bool {
    instruction_requests_wait(&input.instruction)
        || !input
            .candidates
            .iter()
            .any(|candidate| instruction_names_candidate(&input.instruction, &candidate.name))
}

/// Phrase shapes that describe a move to an earlier page rather than a click on
/// a visible control. They are page-transition semantics, not fixture names.
const HISTORY_BACK_CUES: &[&[&str]] = &[
    &["previous", "page"],
    &["previous", "screen"],
    &["page", "before"],
    &["screen", "before"],
    &["step", "before"],
    &["before", "this"],
    &["came", "from"],
    &["opened", "before"],
    &["from", "before"],
    &["where", "i", "was"],
    &["page", "under"],
    &["under", "this"],
];

/// Words that explicitly name a visible control to act on. Such an instruction
/// is a click, so it must not be treated as a history request.
fn instruction_explicitly_names_a_control(instruction: &str) -> bool {
    normalized_words(instruction)
        .iter()
        .any(|word| matches!(word.as_str(), "click" | "press" | "tap"))
}

fn contains_word_sequence(words: &[String], sequence: &[&str]) -> bool {
    sequence.len() <= words.len()
        && words
            .windows(sequence.len())
            .any(|window| window.iter().zip(sequence).all(|(word, cue)| word == cue))
}

/// Whether the instruction asks to return to an earlier page in browser
/// history, as opposed to clicking a visible back, return, or previous link.
///
/// This gates the operation set, not the label of any fixture: when a history
/// transition is requested and no control is named, target-taking operations
/// are not offered, so a nearby "Return to ..." link cannot win.
fn instruction_requests_history_back(instruction: &str) -> bool {
    if instruction_explicitly_names_a_control(instruction) {
        return false;
    }
    let words = normalized_words(instruction);
    HISTORY_BACK_CUES
        .iter()
        .any(|cue| contains_word_sequence(&words, cue))
}

/// Whether this input is a history request. A back-like control that the
/// instruction names by accessible name is an explicit click target, as in
/// pagination ("previous page of results"), so it keeps the target head.
fn history_back_is_requested(input: &DecisionInput) -> bool {
    instruction_requests_history_back(&input.instruction)
        && !input.candidates.iter().any(|candidate| {
            candidate.is_back_like_control()
                && instruction_names_candidate(&input.instruction, &candidate.name)
        })
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
    /// True when a click activates the element. Links, buttons, checkboxes,
    /// radios, options, menu items, and tabs activate; text fields do not.
    #[serde(default)]
    pub activates: bool,
}

/// Whether a click activates an element with this role.
///
/// A textbox or searchbox takes text, so it is never a click target. A
/// combobox counts only when it opens a menu, which means it is being used as a
/// button rather than as a value field.
pub fn role_activates_on_click(role: &str, opens_menu: bool) -> bool {
    matches!(
        role,
        "link" | "button" | "checkbox" | "radio" | "option" | "menuitem" | "tab"
    ) || (role == "combobox" && opens_menu)
}

/// Whether an operation may act on a candidate with this role.
pub fn operation_supports_role(operation: Operation, role: &str, activates: bool) -> bool {
    match operation {
        Operation::Click => activates,
        Operation::Fill => matches!(role, "textbox" | "searchbox" | "combobox"),
        Operation::Select => matches!(role, "combobox" | "listbox"),
        _ => false,
    }
}

impl Candidate {
    /// One-line description sent as the option description.
    pub fn description(&self) -> String {
        let name = if self.name.is_empty() {
            "(no accessible name)".to_string()
        } else {
            self.name.clone()
        };
        let description = match self.nth {
            Some(nth) => format!("role {} named {:?}, occurrence {}", self.role, name, nth),
            None => format!("role {} named {:?}", self.role, name),
        };
        if self.is_back_like_control() {
            format!(
                "{} (in-page control; click it only when the instruction names it, otherwise a previous-page request is history back)",
                description
            )
        } else {
            description
        }
    }

    /// Whether this element can carry out the operation.
    pub fn supports(&self, operation: Operation) -> bool {
        operation_supports_role(operation, &self.role, self.activates)
    }

    /// Operations that could run on this element, for the state sent to a
    /// decider. This lets the decider see that a pairing is impossible before it
    /// answers instead of learning it from a blocked result.
    pub fn supported_operations(&self) -> Vec<&'static str> {
        [Operation::Click, Operation::Fill, Operation::Select]
            .into_iter()
            .filter(|operation| self.supports(*operation))
            .map(Operation::as_str)
            .collect()
    }

    /// Whether the accessible name reads like an in-page navigation control
    /// such as Back, Return, or Previous. Such a control is still a click
    /// target; it is not browser history.
    pub fn is_back_like_control(&self) -> bool {
        let words = normalized_words(&self.name);
        let Some(first) = words.first() else {
            return false;
        };
        matches!(first.as_str(), "back" | "return" | "previous")
            || words
                .windows(2)
                .any(|window| window[0] == "previous" && window[1] == "page")
    }
}

impl CandidateInput {
    fn activates_on_click(&self) -> bool {
        role_activates_on_click(&self.role, self.opens_menu)
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
    /// Set by the rendering probe for the `disabled` attribute,
    /// `aria-disabled="true"`, or a disabled fieldset ancestor. A disabled
    /// control cannot be activated or edited, so it is never offered.
    pub disabled: bool,
    /// True when the element opens a popup menu rather than an inline listbox.
    /// A combobox is click-activatable only in this case: it is used as a
    /// button, not as a value field.
    pub opens_menu: bool,
    /// True when the element intersects the viewport at probe time. Used only
    /// to rank candidates before the cap: the top of a long page is what the
    /// caller is looking at, so a visible control is offered before a rendered
    /// one that needs a scroll. Offscreen refs stay eligible.
    pub in_viewport: bool,
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

/// Keep rendered, actionable refs in DOM order and cap the list.
///
/// `rendered` maps a candidate key to its rectangle when the element passed the
/// rendering probe. A key that is absent is not rendered. Disabled controls are
/// dropped because they cannot be activated or edited. Offscreen elements are
/// included; an interaction scrolls the chosen ref into view before it runs.
pub fn assemble_candidates(
    inputs: &[CandidateInput],
    rendered: &HashMap<String, Rect>,
    cap: usize,
) -> CandidateSet {
    let actionable: Vec<&CandidateInput> = inputs
        .iter()
        .filter(|input| CANDIDATE_ROLES.contains(&input.role.as_str()))
        .filter(|input| input.frame_id.is_none())
        .filter(|input| !input.disabled)
        .filter(|input| rendered.contains_key(&input.key))
        .collect();

    // Offer what the caller can see first. A long page often has its target in
    // the viewport while earlier DOM controls are only announced to screen
    // readers or scrolled away, and the cap would otherwise drop the target.
    // Offscreen rendered refs stay eligible after the visible ones because an
    // action scrolls the chosen ref into view.
    let (mut in_viewport, mut offscreen): (Vec<&CandidateInput>, Vec<&CandidateInput>) =
        actionable.into_iter().partition(|input| input.in_viewport);
    in_viewport.append(&mut offscreen);
    let ordered = in_viewport;

    let truncated = ordered.len() > cap;
    let candidates = ordered
        .into_iter()
        .take(cap)
        .map(|input| Candidate {
            key: input.key.clone(),
            role: input.role.clone(),
            name: input.name.clone(),
            nth: input.nth,
            activates: input.activates_on_click(),
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

/// One rendered element measured by [`probe_rendered_elements`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderedElement {
    pub rect: Rect,
    /// True for the `disabled` attribute, `aria-disabled="true"`, or an
    /// element inside a disabled fieldset.
    pub disabled: bool,
    /// True when the element opens a popup menu rather than a listbox. This
    /// tells a combobox used as a button from a combobox used as a value field.
    pub opens_menu: bool,
    /// True when the element intersects the viewport at probe time.
    pub in_viewport: bool,
}

/// Read every element's viewport rectangle and disabled state in one page
/// evaluation, then map the tagged elements back to backend node ids in one DOM
/// call.
///
/// Elements are included when they are rendered, not only when they intersect
/// the current viewport. Ref interactions scroll the element into view before
/// they run, so an offscreen ref is still actionable, and long pages commonly
/// put the requested target below the fold.
///
/// The page is tagged with `data-ab-cand` the same way the snapshot tags
/// cursor-interactive elements. No CDP call is made per candidate.
pub async fn probe_rendered_elements(
    client: &CdpClient,
    session_id: &str,
) -> Result<HashMap<i64, RenderedElement>, String> {
    let js = r#"
(function() {
    var out = [];
    var els = document.querySelectorAll('*');
    for (var i = 0; i < els.length; i++) {
        var el = els[i];
        try { el.setAttribute('data-ab-cand', String(i)); } catch (e) { continue; }
        var r = el.getBoundingClientRect();
        var cs = getComputedStyle(el);
        var rendered = r.width > 0 && r.height > 0 &&
            cs.display !== 'none' && cs.visibility !== 'hidden' &&
            cs.visibility !== 'collapse';
        // :disabled covers the attribute and a disabled fieldset ancestor.
        // ARIA allows a custom control to be disabled without the attribute.
        var disabled = false;
        try {
            disabled = el.matches(':disabled') ||
                (el.hasAttribute && el.hasAttribute('disabled'));
        } catch (e) {
            disabled = !!(el.hasAttribute && el.hasAttribute('disabled'));
        }
        if (!disabled && el.getAttribute) {
            var aria = el.getAttribute('aria-disabled');
            disabled = aria !== null && aria.toLowerCase() === 'true';
        }
        // A combobox that opens a menu is being used as a button. A native
        // select or a listbox popup stays a value field.
        var opensMenu = false;
        if (el.getAttribute) {
            var popup = (el.getAttribute('aria-haspopup') || '').toLowerCase();
            opensMenu = (el.tagName || '').toUpperCase() === 'BUTTON' ||
                popup === 'menu' || popup === 'dialog' || popup === 'tree' || popup === 'grid';
        }
        // Intersects the visible viewport. Rendered but offscreen elements stay
        // eligible; this only ranks them behind what the caller can see.
        var vw = window.innerWidth || 0;
        var vh = window.innerHeight || 0;
        var inViewport = rendered && r.width > 0 && r.height > 0 &&
            r.right > 0 && r.bottom > 0 && r.x < vw && r.y < vh;
        out.push({ x: r.x, y: r.y, w: r.width, h: r.height, rendered: rendered, disabled: disabled, opensMenu: opensMenu, inViewport: inViewport });
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

    let mut by_backend: HashMap<i64, RenderedElement> = HashMap::new();
    if let Some(root) = doc.get("root") {
        collect_tagged_elements(root, &rects, &mut by_backend);
    }
    Ok(by_backend)
}

fn collect_tagged_elements(node: &Value, rects: &[Value], out: &mut HashMap<i64, RenderedElement>) {
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
                let rendered = rect
                    .get("rendered")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if rendered {
                    out.insert(
                        backend,
                        RenderedElement {
                            rect: Rect {
                                x: rect.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0),
                                y: rect.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0),
                                width: rect.get("w").and_then(|v| v.as_f64()).unwrap_or(0.0),
                                height: rect.get("h").and_then(|v| v.as_f64()).unwrap_or(0.0),
                            },
                            disabled: rect
                                .get("disabled")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false),
                            opens_menu: rect
                                .get("opensMenu")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false),
                            in_viewport: rect
                                .get("inViewport")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false),
                        },
                    );
                }
            }
        }
    }
    if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
        for child in children {
            collect_tagged_elements(child, rects, out);
        }
    }
    if let Some(content) = node.get("contentDocument") {
        collect_tagged_elements(content, rects, out);
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
            disabled: false,
            opens_menu: false,
            in_viewport: false,
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
                    "activates": candidate.activates,
                    "supports": candidate.supported_operations(),
                }),
            );
        }

        let history_back = history_back_is_requested(input);
        let state = json!({
            "instruction": input.instruction,
            "page": { "url": input.url, "title": input.title },
            "value_keys": input.value_keys,
            "candidates": candidate_index,
            "candidates_truncated": input.truncated,
            "history_back_requested": history_back,
        });

        let mut questions = Map::new();
        let wait_offered = wait_is_offered(input);
        let mut operation_criteria = operation_criteria(wait_offered);
        if history_back {
            // A previous-page request with no named control can only be
            // browser history. Hiding target-taking operations also removes the
            // independent target head's pull toward "Return to ..." links.
            if let Some(criteria) = operation_criteria.as_object_mut() {
                for operation in ["click", "fill", "select"] {
                    criteria.remove(operation);
                }
            }
        }
        questions.insert(
            "operation".to_string(),
            json!({
                "type": "choice",
                "instructions": "Which single operation should be performed next to satisfy the instruction? Choose click, fill, or select only when the instruction acts on an offered element and the operation fits that element's role: click activates a link, button, checkbox, radio, option, menuitem, tab, or a combobox that opens a menu; fill enters text into a textbox, searchbox, or editable combobox; select chooses a value in a combobox or listbox. Choose fill or select when the instruction asks to enter text or choose a dropdown value, even when value_keys is empty; the result is blocked rather than turned into a click. Choose scroll_down or scroll_up when the instruction asks to scroll, even if a link with a matching name is offered. Choose back when the instruction refers to a previous page, the page before this one, or the page it came from; back-like links and buttons on the page are in-page controls, not history. Choose wait only when the instruction asks to wait, settle, load, or wait for a condition to appear, or when no offered element matches a target named in the instruction.",
                "criteria": operation_criteria,
            }),
        );

        let target_offered = !history_back && !input.candidates.is_empty();
        if target_offered {
            let mut criteria = Map::new();
            for candidate in &input.candidates {
                criteria.insert(candidate.key.clone(), json!(candidate.description()));
            }
            questions.insert(
                "target".to_string(),
                json!({
                    "type": "choice",
                    "instructions": "Which offered element should click, fill, or select act on? Ignore this question for other operations. Pick an element whose role and name match the operation and the instruction. The state lists each candidate's supported operations; an unsupported pairing is blocked, not run. Prefer the element whose role and name match the requested action over a nearby input field.",
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
                target: target_offered,
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
    /// Set when the operation needs a target or value that was not offered.
    /// Nothing runs; the caller reports `blocked` with this reason.
    pub blocked_reason: Option<String>,
    /// True when the block is an operation/target role mismatch. The decision
    /// loop retries such a pairing once before reporting it, because another
    /// offered target can still satisfy the instruction.
    #[serde(skip)]
    pub(crate) pairing_blocked: bool,
    /// Distribution that gated the decision: the target head for operations
    /// that need a target, otherwise the operation head.
    pub probabilities: Map<String, Value>,
    /// Lowest confidence among the heads that mattered.
    pub confidence: f64,
}

/// The outcome of the auto-act gate that runs after a decision and before any
/// action runs.
///
/// This is the single classification used by `act` and by the decision eval, so
/// an eval case reports exactly what the daemon would do with that decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProposalOutcome {
    /// `act` would run the chosen operation with no confirmation step.
    Execute,
    /// The decision needs a value or an operation/target pairing that is not
    /// offered. The daemon reports `blocked`.
    Blocked,
    /// A calibrated decider scored the decision below the caller's
    /// `--min-confidence`. The daemon reports `blocked`.
    BelowThreshold,
    /// The caller asked for `--dry-run`, so nothing runs.
    DryRun,
    /// The page moved while the decision was being made.
    Stale,
    /// The action is covered by the caller's confirmation policy. The daemon
    /// parks it and reports `blocked` with `confirmation_required`.
    ConfirmationRequired,
}

impl ProposalOutcome {
    /// True only when `act` would touch the page with no confirmation step.
    pub fn would_execute(self) -> bool {
        matches!(self, ProposalOutcome::Execute)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ProposalOutcome::Execute => "execute",
            ProposalOutcome::Blocked => "blocked",
            ProposalOutcome::BelowThreshold => "below_threshold",
            ProposalOutcome::DryRun => "dry_run",
            ProposalOutcome::Stale => "stale",
            ProposalOutcome::ConfirmationRequired => "confirmation_required",
        }
    }
}

/// Classify a parsed decision against the auto-act gate.
///
/// The order matches `handle_act_with`: a stale page wins over everything,
/// then a blocked reason, then the confidence floor for calibrated deciders,
/// then dry run, then a caller confirmation requirement. Every branch except
/// [`ProposalOutcome::Execute`] leaves the page untouched.
pub fn proposal_outcome(
    decision: &Decision,
    calibrated: bool,
    min_confidence: Option<f64>,
    dry_run: bool,
    stale: bool,
    confirmation_required: bool,
) -> ProposalOutcome {
    if stale {
        return ProposalOutcome::Stale;
    }
    if decision.blocked_reason.is_some() {
        return ProposalOutcome::Blocked;
    }
    if calibrated {
        if let Some(min) = min_confidence {
            if decision.confidence < min {
                return ProposalOutcome::BelowThreshold;
            }
        }
    }
    if dry_run {
        return ProposalOutcome::DryRun;
    }
    if confirmation_required {
        return ProposalOutcome::ConfirmationRequired;
    }
    ProposalOutcome::Execute
}

/// Counters for one decision eval run.
///
/// `executed_total` counts decisions the auto-act gate would run. A blocked,
/// stale, dry-run, below-threshold, or confirmation-required decision never
/// contributes to it, even when the decision is otherwise wrong. That keeps
/// utility failures (an over-cautious block) out of the safety numerator.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProposalMetrics {
    pub total: usize,
    pub correct: usize,
    pub decision_wrong: usize,
    pub executed_total: usize,
    pub executed_wrong: usize,
    pub expected_blocked: usize,
    pub blocked: usize,
    pub blocked_correct: usize,
    pub errors: usize,
}

impl ProposalMetrics {
    pub fn record(
        &mut self,
        outcome: Option<ProposalOutcome>,
        expected_blocked: bool,
        correct: bool,
    ) {
        self.total += 1;
        if correct {
            self.correct += 1;
        } else {
            self.decision_wrong += 1;
        }
        if expected_blocked {
            self.expected_blocked += 1;
        }
        match outcome {
            Some(outcome) if outcome.would_execute() => {
                self.executed_total += 1;
                if !correct {
                    self.executed_wrong += 1;
                }
            }
            Some(ProposalOutcome::Blocked) => {
                self.blocked += 1;
                if correct && expected_blocked {
                    self.blocked_correct += 1;
                }
            }
            _ => {}
        }
    }

    /// Record a decider error. No decision existed, so nothing would execute.
    pub fn record_error(&mut self, expected_blocked: bool) {
        self.errors += 1;
        self.record(None, expected_blocked, false);
    }

    pub fn coverage(&self) -> f64 {
        ratio(self.executed_total, self.total)
    }

    pub fn executed_wrong_rate_of_total(&self) -> f64 {
        ratio(self.executed_wrong, self.total)
    }

    pub fn executed_wrong_rate_of_executed(&self) -> f64 {
        ratio(self.executed_wrong, self.executed_total)
    }

    pub fn blocked_rate(&self) -> f64 {
        ratio(self.blocked, self.total)
    }

    /// Rate of correct outcomes among cases labeled blocked.
    pub fn blocked_accuracy(&self) -> f64 {
        ratio(self.blocked_correct, self.expected_blocked)
    }
}

fn ratio(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

/// One-sided Clopper-Pearson upper bound for a binomial rate.
///
/// Solves `P(X <= wrong; total, p) = 1 - confidence` for `p` using the exact
/// binomial CDF. The bare point estimate is not treated as proof when this
/// bound is above the gate.
pub fn clopper_pearson_upper(wrong: usize, total: usize, confidence: f64) -> f64 {
    if total == 0 {
        return 1.0;
    }
    if wrong >= total {
        return 1.0;
    }
    let alpha = (1.0 - confidence).clamp(f64::MIN_POSITIVE, 1.0);
    if wrong == 0 {
        return 1.0 - alpha.powf(1.0 / total as f64);
    }
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..200 {
        let middle = (low + high) / 2.0;
        if binomial_cdf(wrong, total, middle) > alpha {
            low = middle;
        } else {
            high = middle;
        }
    }
    high
}

/// Exact binomial CDF `P(X <= k)` for `n` trials at probability `p`.
fn binomial_cdf(k: usize, n: usize, p: f64) -> f64 {
    if p <= 0.0 {
        return 1.0;
    }
    if p >= 1.0 {
        return if k >= n { 1.0 } else { 0.0 };
    }
    let ln_p = p.ln();
    let ln_q = (1.0 - p).ln();
    let ln_total = log_gamma(n as f64 + 1.0);
    let logs: Vec<f64> = (0..=k)
        .map(|i| {
            ln_total - log_gamma(i as f64 + 1.0) - log_gamma((n - i) as f64 + 1.0)
                + i as f64 * ln_p
                + (n - i) as f64 * ln_q
        })
        .collect();
    let max = logs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !max.is_finite() {
        return 0.0;
    }
    logs.iter().map(|log| (log - max).exp()).sum::<f64>() * max.exp()
}

/// Natural log of the gamma function (Lanczos, g = 7).
#[allow(clippy::excessive_precision)] // published Lanczos coefficients, kept verbatim
fn log_gamma(x: f64) -> f64 {
    const G: f64 = 7.0;
    const C: [f64; 9] = [
        0.999_999_999_999_809_93,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_13,
        -176.615_029_162_140_59,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_571_6e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        std::f64::consts::PI.ln() - (std::f64::consts::PI * x).sin().ln() - log_gamma(1.0 - x)
    } else {
        let x = x - 1.0;
        let mut a = C[0];
        let t = x + G + 0.5;
        for (index, coefficient) in C.iter().enumerate().skip(1) {
            a += coefficient / (x + index as f64);
        }
        0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
    }
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

fn state_candidate_supports(state: &Value, key: &str, operation: Operation) -> bool {
    let candidate = state
        .get("candidates")
        .and_then(|candidates| candidates.get(key));
    let role = candidate
        .and_then(|candidate| candidate.get("role"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let activates = candidate
        .and_then(|candidate| candidate.get("activates"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    operation_supports_role(operation, role, activates)
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

    let mut pairing_blocked = false;
    let mut reasons: Vec<String> = Vec::new();
    let mut target = None;
    let mut target_head = None;
    if decision_request.offered.target {
        let (choice, probabilities, confidence) = choice_answer(answers, "target", questions)?;
        if operation.needs_target() {
            // Keep the target so the blocked result can still name what was
            // chosen. A pairing that cannot run is a blocked decision, not an
            // invalid answer: the decider picked two offered options.
            target = Some(choice.clone());
            target_head = Some((probabilities, confidence));
            if !state_candidate_supports(&decision_request.state, &choice, operation) {
                pairing_blocked = true;
                reasons.push(format!(
                    "The chosen target does not support {}.",
                    operation.as_str()
                ));
            }
        }
        // The target head is asked in the same request, so a non-target
        // operation may still return one. It is ignored, not an error.
    }

    if operation.needs_target() && target.is_none() {
        reasons.push("The operation needs a target, but no target was offered.".to_string());
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
        reasons.push(
            "The operation needs a caller-supplied value, but none was supplied.".to_string(),
        );
    }

    let blocked_reason = if reasons.is_empty() {
        None
    } else {
        Some(reasons.join(" "))
    };

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
        blocked_reason,
        pairing_blocked,
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
    let mut blocked_pairing: Option<Decision> = None;
    for attempt in 0..2 {
        let response = decider.decide(request).await?;
        match validate_decision(request, &response.answers) {
            Ok(decision) => {
                if decision.pairing_blocked && attempt == 0 {
                    // One more look can find another offered target that fits
                    // the operation. If it does not, the pairing is returned as
                    // blocked rather than as an error.
                    last = decision.blocked_reason.clone();
                    blocked_pairing = Some(decision);
                    continue;
                }
                return Ok(decision);
            }
            Err(DecideError::InvalidAnswer(message)) if attempt == 0 => {
                last = Some(message);
            }
            Err(DecideError::InvalidAnswer(message)) => {
                if let Some(decision) = blocked_pairing.take() {
                    return Ok(decision);
                }
                return Err(DecideError::InvalidAnswer(message));
            }
            Err(error) => return Err(error),
        }
    }
    if let Some(decision) = blocked_pairing {
        return Ok(decision);
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

/// One shared client per process. A decider is built per command, so a fresh
/// client would pay a TCP and TLS handshake on every `act` step. Measured
/// against the TypeSafe endpoint: 80 ms on a new connection, 28 ms reused.
fn build_http_client() -> Result<reqwest::Client, String> {
    static SHARED: std::sync::OnceLock<Result<reqwest::Client, String>> =
        std::sync::OnceLock::new();
    SHARED
        .get_or_init(|| {
            let builder = crate::tls::apply_to_reqwest(
                reqwest::Client::builder(),
                &crate::tls::process_options(),
            )
            .map_err(|error| format!("Failed to configure TLS for the decider: {}", error))?;
            builder
                .build()
                .map_err(|error| format!("Failed to create the decider HTTP client: {}", error))
        })
        .clone()
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
        let operations: Vec<&String> = request
            .questions
            .get("operation")
            .and_then(|q| q.get("criteria"))
            .and_then(|c| c.as_object())
            .map(|criteria| criteria.keys().collect())
            .unwrap_or_default();
        properties.insert(
            "operation".to_string(),
            json!({
                "type": "string",
                "enum": operations,
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
            disabled: false,
            opens_menu: false,
            in_viewport: false,
        }
    }

    fn disabled_candidate(key: &str, role: &str, name: &str) -> CandidateInput {
        CandidateInput {
            disabled: true,
            ..candidate(key, role, name, None)
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
                    activates: true,
                },
                Candidate {
                    key: "e2".to_string(),
                    role: "link".to_string(),
                    name: "Help".to_string(),
                    nth: None,
                    activates: true,
                },
            ],
            truncated: false,
        }
    }

    fn answer(choice: &str, probabilities: Value) -> Value {
        json!({
            "type": "choice",
            "choice": choice,
            "confidence": 0.8,
            "probabilities": probabilities,
        })
    }

    fn offered_keys(request: &DecisionRequest, head: &str) -> Vec<String> {
        request.questions[head]["criteria"]
            .as_object()
            .map(|criteria| criteria.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// A distribution over the options actually offered for `head`.
    fn probabilities(request: &DecisionRequest, head: &str, chosen: &str, weight: f64) -> Value {
        let keys = offered_keys(request, head);
        let single = keys.len() <= 1;
        let others = keys.len().saturating_sub(1).max(1);
        let mut map = Map::new();
        for key in keys {
            let value = if key == chosen {
                if single {
                    1.0
                } else {
                    weight
                }
            } else {
                (1.0 - weight) / others as f64
            };
            map.insert(key, json!(value));
        }
        Value::Object(map)
    }

    fn textbox_input() -> DecisionInput {
        DecisionInput {
            instruction: "Fill the email address".to_string(),
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            value_keys: vec![],
            candidates: vec![Candidate {
                key: "e1".to_string(),
                role: "textbox".to_string(),
                name: "Email address".to_string(),
                nth: None,
                activates: false,
            }],
            truncated: false,
        }
    }

    fn valid_answers(request: &DecisionRequest) -> Value {
        let mut answers = json!({
            "operation": answer("click", probabilities(request, "operation", "click", 0.9)),
        });
        if request.offered.target {
            answers["target"] = answer("e1", probabilities(request, "target", "e1", 0.95));
        }
        answers
    }

    #[test]
    fn candidates_keep_dom_order_and_filter_roles_and_rendering() {
        let inputs = vec![
            candidate("e1", "button", "One", None),
            candidate("e2", "heading", "Not actionable", None),
            candidate("e3", "link", "Three", None),
            candidate("e4", "textbox", "Not rendered", None),
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
    fn candidates_include_offscreen_rendered_elements() {
        let inputs = vec![
            candidate("e1", "link", "Read the reviews", None),
            candidate("e2", "link", "Back to top", None),
        ];
        let rendered = HashMap::from([
            (
                "e1".to_string(),
                Rect {
                    x: 10.0,
                    y: 2400.0,
                    width: 120.0,
                    height: 24.0,
                },
            ),
            (
                "e2".to_string(),
                Rect {
                    x: 10.0,
                    y: 4800.0,
                    width: 90.0,
                    height: 24.0,
                },
            ),
        ]);
        let set = assemble_candidates(&inputs, &rendered, MAX_CANDIDATES);
        assert_eq!(
            set.candidates
                .iter()
                .map(|candidate| candidate.key.as_str())
                .collect::<Vec<_>>(),
            vec!["e1", "e2"]
        );
    }

    #[test]
    fn candidates_offer_in_viewport_controls_before_earlier_offscreen_ones() {
        // A long page can put screen-reader-only or scrolled-away controls at
        // the top of the DOM while the caller's target sits in the viewport.
        // The cap must not drop the visible control.
        let mut onscreen = candidate("e3", "searchbox", "Search Amazon", None);
        onscreen.in_viewport = true;
        let inputs = vec![
            candidate("e1", "link", "Search, option, forward slash", None),
            candidate("e2", "link", "Skip to main content", None),
            onscreen,
        ];
        let visible = rects(&["e1", "e2", "e3"]);
        let set = assemble_candidates(&inputs, &visible, 1);
        assert_eq!(
            set.candidates
                .iter()
                .map(|candidate| candidate.key.as_str())
                .collect::<Vec<_>>(),
            vec!["e3"]
        );
        assert!(set.truncated);
    }

    #[test]
    fn candidates_skip_disabled_elements() {
        // One input per condition the rendering probe normalizes into the flag:
        // the disabled attribute, aria-disabled="true", and a control inside a
        // disabled fieldset. None of them can be activated or edited.
        let inputs = vec![
            candidate("e1", "button", "Publish report", None),
            disabled_candidate("e2", "button", "Delete report"),
            disabled_candidate("e3", "checkbox", "Remember me"),
            disabled_candidate("e4", "textbox", "Email"),
            candidate("e5", "link", "Help", None),
        ];
        let visible = rects(&["e1", "e2", "e3", "e4", "e5"]);
        let set = assemble_candidates(&inputs, &visible, MAX_CANDIDATES);
        assert_eq!(
            set.candidates
                .iter()
                .map(|candidate| candidate.key.as_str())
                .collect::<Vec<_>>(),
            vec!["e1", "e5"]
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
            activates: true,
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
            // The instruction names the offered Submit button, so wait is not
            // an offered operation.
            OPERATIONS.len() - 1
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
    fn operation_criteria_distinguish_wait_done_and_history_back() {
        let mut input = input();
        input.instruction = "Wait for the page to finish loading".to_string();
        let request = DecisionRequest::build("jev-latest", &input, false);
        let criteria = request.questions["operation"]["criteria"]
            .as_object()
            .expect("operation criteria");
        let text = |key: &str| {
            criteria
                .get(key)
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_lowercase()
        };
        assert!(text("wait").contains("instruction asks to wait"));
        assert!(text("wait").contains("finish loading"));
        assert!(!criteria.contains_key("done"));
        assert!(text("back").contains("browser history"));
        assert!(text("back").contains("previous page"));
        assert!(text("scroll_down").contains("instruction asks to scroll down"));
        assert!(text("scroll_up").contains("instruction asks to scroll up"));
        assert!(text("scroll_up").contains("back to top"));
        assert!(text("fill").contains("value_keys"));
        assert!(text("select").contains("value_keys"));
        assert!(text("fill").contains("blocked"));
        assert!(text("select").contains("blocked"));
        let instructions = request.questions["operation"]["instructions"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase();
        assert!(instructions.contains("value_keys"));
        assert!(instructions.contains("scroll_down or scroll_up"));
        assert!(instructions.contains("wait"));
    }

    #[test]
    fn history_back_instruction_hides_target_taking_operations() {
        // Novel wording, not a fixture name: a return to an earlier location.
        let mut input = input();
        input.instruction = "Take me back to the roster I came from".to_string();
        input.candidates.push(Candidate {
            key: "e3".to_string(),
            role: "link".to_string(),
            name: "Return to roster".to_string(),
            nth: None,
            activates: true,
        });
        let request = DecisionRequest::build("jev-latest", &input, false);
        let offered = offered_keys(&request, "operation");
        assert!(offered.contains(&"back".to_string()));
        assert!(!offered.contains(&"click".to_string()));
        assert!(!request.offered.target);
        assert!(request.questions.get("target").is_none());
        let answers = json!({
            "operation": answer("back", probabilities(&request, "operation", "back", 0.9)),
        });
        let decision = validate_decision(&request, &answers).unwrap();
        assert_eq!(decision.operation, Operation::Back);
        assert!(decision.blocked_reason.is_none());
    }

    #[test]
    fn explicit_click_on_a_back_control_keeps_the_target_head() {
        // The same kind of visible control is clickable when the instruction
        // names the control instead of asking for an earlier page.
        let mut input = input();
        input.instruction = "Click the Return to roster link".to_string();
        input.candidates.push(Candidate {
            key: "e3".to_string(),
            role: "link".to_string(),
            name: "Return to roster".to_string(),
            nth: None,
            activates: true,
        });
        let request = DecisionRequest::build("jev-latest", &input, false);
        assert!(request.offered.target);
        assert!(offered_keys(&request, "operation").contains(&"click".to_string()));
        let answers = json!({
            "operation": answer("click", probabilities(&request, "operation", "click", 0.9)),
            "target": answer("e3", probabilities(&request, "target", "e3", 0.95)),
        });
        let decision = validate_decision(&request, &answers).unwrap();
        assert_eq!(decision.operation, Operation::Click);
        assert_eq!(decision.target.as_deref(), Some("e3"));
    }

    #[test]
    fn history_back_cues_are_generic_page_transitions() {
        for instruction in [
            "Go back to the previous page",
            "Return to the folder I opened before this one",
            "Take me back to the screen before this step",
            "Go to the page under this panel",
            "Return to where I was",
            "Go back to the account page from before",
        ] {
            assert!(
                instruction_requests_history_back(instruction),
                "{instruction}"
            );
        }
        for instruction in [
            "Return to the cart and change the quantity",
            "Scroll back up toward the top",
            "Click the Back to invoices link",
            "Press the Previous page button",
            "Wait for the page to finish loading",
        ] {
            assert!(
                !instruction_requests_history_back(instruction),
                "{instruction}"
            );
        }
    }

    #[test]
    fn a_named_previous_page_control_is_a_click_not_history() {
        let mut input = input();
        input.instruction = "Go back to the previous page of results".to_string();
        input.candidates.push(Candidate {
            key: "e4".to_string(),
            role: "button".to_string(),
            name: "Previous page".to_string(),
            nth: None,
            activates: true,
        });
        let request = DecisionRequest::build("jev-latest", &input, false);
        assert!(request.offered.target);
        assert!(request.questions.get("target").is_some());
        let answers = json!({
            "operation": answer("click", probabilities(&request, "operation", "click", 0.9)),
            "target": answer("e4", probabilities(&request, "target", "e4", 0.95)),
        });
        let decision = validate_decision(&request, &answers).unwrap();
        assert_eq!(decision.operation, Operation::Click);
        assert_eq!(decision.target.as_deref(), Some("e4"));
        assert!(decision.blocked_reason.is_none());
    }

    #[test]
    fn history_wording_wins_over_a_nav_link_named_in_the_instruction() {
        let mut input = input();
        input.instruction = "Go back to the cart from before this checkout".to_string();
        input.candidates.push(Candidate {
            key: "e5".to_string(),
            role: "link".to_string(),
            name: "Cart".to_string(),
            nth: None,
            activates: true,
        });
        let request = DecisionRequest::build("jev-latest", &input, false);
        assert!(!request.offered.target);
        assert!(!offered_keys(&request, "operation").contains(&"click".to_string()));
    }

    #[test]
    fn back_like_candidate_is_described_as_an_in_page_control() {
        let control = Candidate {
            key: "e5".to_string(),
            role: "link".to_string(),
            name: "Return to invoices".to_string(),
            nth: None,
            activates: true,
        };
        assert!(control.is_back_like_control());
        assert!(control.description().contains("in-page control"));
        let ordinary = Candidate {
            name: "Invoices".to_string(),
            ..control
        };
        assert!(!ordinary.is_back_like_control());
        assert!(!ordinary.description().contains("in-page control"));
    }

    #[test]
    fn state_lists_the_operations_each_candidate_supports() {
        let input = DecisionInput {
            instruction: "Enter the tracking code".to_string(),
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            value_keys: vec!["code".to_string()],
            candidates: vec![
                Candidate {
                    key: "e1".to_string(),
                    role: "textbox".to_string(),
                    name: "Tracking code".to_string(),
                    nth: None,
                    activates: false,
                },
                Candidate {
                    key: "e2".to_string(),
                    role: "combobox".to_string(),
                    name: "Carrier".to_string(),
                    nth: None,
                    activates: false,
                },
            ],
            truncated: false,
        };
        let request = DecisionRequest::build("jev-latest", &input, true);
        assert_eq!(
            request.state["candidates"]["e1"]["supports"],
            json!(["fill"])
        );
        assert_eq!(
            request.state["candidates"]["e2"]["supports"],
            json!(["fill", "select"])
        );
    }

    #[test]
    fn wait_is_not_offered_when_the_instruction_names_an_offered_candidate() {
        // input() names the offered Submit button and does not ask to wait.
        let request = DecisionRequest::build("jev-latest", &input(), false);
        assert!(!offered_keys(&request, "operation").contains(&"wait".to_string()));
        let answers = json!({
            "operation": answer("wait", probabilities(&request, "operation", "click", 0.9)),
            "target": answer("e1", probabilities(&request, "target", "e1", 0.95)),
        });
        assert!(matches!(
            validate_decision(&request, &answers),
            Err(DecideError::InvalidAnswer(_))
        ));
    }

    #[test]
    fn wait_is_offered_when_the_instruction_asks_to_wait() {
        let mut input = input();
        input.instruction = "Wait for the page to finish loading".to_string();
        let request = DecisionRequest::build("jev-latest", &input, false);
        assert!(offered_keys(&request, "operation").contains(&"wait".to_string()));
        let answers = json!({
            "operation": answer("wait", probabilities(&request, "operation", "wait", 0.9)),
            "target": answer("e1", probabilities(&request, "target", "e1", 0.95)),
        });
        let decision = validate_decision(&request, &answers).unwrap();
        assert_eq!(decision.operation, Operation::Wait);
    }

    #[test]
    fn wait_is_offered_when_no_candidate_matches_the_instruction() {
        let mut input = input();
        input.instruction = "Refresh the dashboard".to_string();
        let request = DecisionRequest::build("jev-latest", &input, false);
        assert!(offered_keys(&request, "operation").contains(&"wait".to_string()));
        let answers = json!({
            "operation": answer("wait", probabilities(&request, "operation", "wait", 0.9)),
            "target": answer("e1", probabilities(&request, "target", "e1", 0.95)),
        });
        let decision = validate_decision(&request, &answers).unwrap();
        assert_eq!(decision.operation, Operation::Wait);
    }

    #[test]
    fn done_is_not_an_offered_operation() {
        let mut input = input();
        input.instruction =
            "Everything on this report already matches what I asked for".to_string();
        let request = DecisionRequest::build("jev-latest", &input, false);
        let offered = offered_keys(&request, "operation");
        assert!(!offered.contains(&"done".to_string()));
        assert_eq!(Operation::parse("done"), None);
        let answers = json!({
            "operation": answer("done", probabilities(&request, "operation", "click", 0.9)),
            "target": answer("e1", probabilities(&request, "target", "e1", 0.95)),
        });
        assert!(matches!(
            validate_decision(&request, &answers),
            Err(DecideError::InvalidAnswer(_))
        ));
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
            "operation": answer("click", probabilities(&request, "operation", "click", 0.9)),
            "target": answer("e99", probabilities(&request, "target", "e1", 0.5)),
        });
        assert!(matches!(
            validate_decision(&request, &answers),
            Err(DecideError::InvalidAnswer(_))
        ));
    }

    #[test]
    fn rejects_missing_option() {
        let request = DecisionRequest::build("jev-latest", &input(), false);
        let mut operation = probabilities(&request, "operation", "click", 0.9)
            .as_object()
            .cloned()
            .expect("probabilities");
        operation.remove("back");
        let answers = json!({
            "operation": answer("click", Value::Object(operation)),
            "target": answer("e1", probabilities(&request, "target", "e1", 0.95)),
        });
        assert!(matches!(
            validate_decision(&request, &answers),
            Err(DecideError::InvalidAnswer(_))
        ));
    }

    #[test]
    fn rejects_bad_probability_sum() {
        let request = DecisionRequest::build("jev-latest", &input(), false);
        let mut operation = Map::new();
        for key in offered_keys(&request, "operation") {
            operation.insert(key, json!(0.0));
        }
        operation.insert("click".to_string(), json!(0.5));
        let answers = json!({
            "operation": answer("click", Value::Object(operation)),
            "target": answer("e1", probabilities(&request, "target", "e1", 0.95)),
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
            "operation": answer("click", probabilities(&request, "operation", "click", 0.1)),
            "target": answer("e1", probabilities(&request, "target", "e1", 0.1)),
        });
        assert!(matches!(
            validate_decision(&request, &answers),
            Err(DecideError::InvalidAnswer(_))
        ));
    }

    #[tokio::test]
    async fn blocked_decision_is_not_retried() {
        let mut input = input();
        input.candidates.clear();
        let request = DecisionRequest::build("jev-latest", &input, false);
        let answers = json!({
            "operation": answer("click", probabilities(&request, "operation", "click", 0.9)),
        });
        let decider = Arc::new(ScriptedDecider {
            answers: std::sync::Mutex::new(vec![Ok(answers)]),
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let decision = decide_with_retry(decider.as_ref(), &request).await.unwrap();
        assert_eq!(decision.operation, Operation::Click);
        assert!(decision
            .blocked_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("needs a target")));
        assert_eq!(decider.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn click_without_an_offered_target_is_blocked_not_an_error() {
        let mut input = input();
        input.candidates.clear();
        let request = DecisionRequest::build("jev-latest", &input, false);
        assert!(!request.offered.target);
        let answers = json!({
            "operation": answer("click", probabilities(&request, "operation", "click", 0.9)),
        });
        let decision = validate_decision(&request, &answers).unwrap();
        assert_eq!(decision.operation, Operation::Click);
        assert!(decision.target.is_none());
        assert!(decision
            .blocked_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("needs a target")));
    }

    #[test]
    fn fill_without_a_supplied_value_on_a_textbox_is_blocked_not_a_click() {
        let request = DecisionRequest::build("jev-latest", &textbox_input(), false);
        let answers = json!({
            "operation": answer("fill", probabilities(&request, "operation", "fill", 0.9)),
            "target": answer("e1", probabilities(&request, "target", "e1", 0.95)),
        });
        let decision = validate_decision(&request, &answers).unwrap();
        assert_eq!(decision.operation, Operation::Fill);
        assert_eq!(decision.target.as_deref(), Some("e1"));
        assert!(decision.value_key.is_none());
        assert!(decision
            .blocked_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("caller-supplied value")));
    }

    #[test]
    fn textbox_is_not_click_activatable() {
        assert!(!role_activates_on_click("textbox", false));
        assert!(!role_activates_on_click("searchbox", false));
        assert!(role_activates_on_click("button", false));
        assert!(role_activates_on_click("link", false));
        assert!(!role_activates_on_click("combobox", false));
        assert!(role_activates_on_click("combobox", true));
    }

    #[test]
    fn click_on_a_textbox_is_blocked_not_an_error() {
        let request = DecisionRequest::build("jev-latest", &textbox_input(), false);
        let answers = json!({
            "operation": answer("click", probabilities(&request, "operation", "click", 0.9)),
            "target": answer("e1", probabilities(&request, "target", "e1", 0.95)),
        });
        let decision = validate_decision(&request, &answers).unwrap();
        assert_eq!(decision.operation, Operation::Click);
        assert_eq!(decision.target.as_deref(), Some("e1"));
        assert!(decision
            .blocked_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("does not support click")));
    }

    #[test]
    fn select_on_a_non_select_target_is_blocked_not_an_error() {
        let input = DecisionInput {
            instruction: "Choose the settlement currency".to_string(),
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            value_keys: vec![],
            candidates: vec![Candidate {
                key: "e7".to_string(),
                role: "button".to_string(),
                name: "Settlement currency".to_string(),
                nth: None,
                activates: true,
            }],
            truncated: false,
        };
        let request = DecisionRequest::build("jev-latest", &input, false);
        let answers = json!({
            "operation": answer("select", probabilities(&request, "operation", "select", 0.9)),
            "target": answer("e7", probabilities(&request, "target", "e7", 0.95)),
        });
        let decision = validate_decision(&request, &answers).unwrap();
        assert_eq!(decision.operation, Operation::Select);
        assert_eq!(decision.target.as_deref(), Some("e7"));
        let reason = decision.blocked_reason.as_deref().unwrap_or_default();
        assert!(reason.contains("does not support select"), "{reason}");
        assert!(reason.contains("caller-supplied value"), "{reason}");
    }

    #[test]
    fn select_on_a_combobox_without_a_value_is_blocked() {
        let input = DecisionInput {
            instruction: "Switch the settlement currency to euros".to_string(),
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            value_keys: vec![],
            candidates: vec![Candidate {
                key: "e3".to_string(),
                role: "combobox".to_string(),
                name: "Settlement currency".to_string(),
                nth: None,
                activates: false,
            }],
            truncated: false,
        };
        let request = DecisionRequest::build("jev-latest", &input, false);
        let answers = json!({
            "operation": answer("select", probabilities(&request, "operation", "select", 0.9)),
            "target": answer("e3", probabilities(&request, "target", "e3", 0.95)),
        });
        let decision = validate_decision(&request, &answers).unwrap();
        assert_eq!(decision.operation, Operation::Select);
        assert_eq!(decision.target.as_deref(), Some("e3"));
        assert!(decision
            .blocked_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("caller-supplied value")));
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
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
        let bad = json!({ "operation": answer("nope", json!({ "nope": 1.0 })) });
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
        let bad = json!({ "operation": answer("nope", json!({ "nope": 1.0 })) });
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

    #[tokio::test]
    async fn retries_a_role_mismatch_then_uses_the_valid_pairing() {
        let mut input = textbox_input();
        input.value_keys = vec!["email".to_string()];
        let request = DecisionRequest::build("jev-latest", &input, false);
        let mismatch = json!({
            "operation": answer("click", probabilities(&request, "operation", "click", 0.9)),
            "target": answer("e1", probabilities(&request, "target", "e1", 0.95)),
        });
        let corrected = json!({
            "operation": answer("fill", probabilities(&request, "operation", "fill", 0.9)),
            "target": answer("e1", probabilities(&request, "target", "e1", 0.95)),
        });
        let decider = Arc::new(ScriptedDecider {
            answers: std::sync::Mutex::new(vec![Ok(mismatch), Ok(corrected)]),
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let decision = decide_with_retry(decider.as_ref(), &request).await.unwrap();
        assert_eq!(decision.operation, Operation::Fill);
        assert!(decision.blocked_reason.is_none());
        assert_eq!(decider.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_repeated_role_mismatch_is_blocked_not_an_error() {
        let request = DecisionRequest::build("jev-latest", &textbox_input(), false);
        let mismatch = json!({
            "operation": answer("click", probabilities(&request, "operation", "click", 0.9)),
            "target": answer("e1", probabilities(&request, "target", "e1", 0.95)),
        });
        let decider = Arc::new(ScriptedDecider {
            answers: std::sync::Mutex::new(vec![Ok(mismatch.clone()), Ok(mismatch)]),
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let decision = decide_with_retry(decider.as_ref(), &request).await.unwrap();
        assert_eq!(decision.operation, Operation::Click);
        assert_eq!(decision.target.as_deref(), Some("e1"));
        assert!(decision
            .blocked_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("does not support click")));
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
                        "scroll_up": 0.0, "back": 0.0
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

    fn decision(operation: Operation, blocked_reason: Option<&str>) -> Decision {
        Decision {
            operation,
            target: Some("e1".to_string()),
            value_key: None,
            blocked_reason: blocked_reason.map(str::to_string),
            pairing_blocked: false,
            probabilities: Map::new(),
            confidence: 0.9,
        }
    }

    #[test]
    fn proposal_outcome_defaults_to_execute() {
        let decision = decision(Operation::Click, None);
        assert_eq!(
            proposal_outcome(&decision, true, None, false, false, false),
            ProposalOutcome::Execute
        );
    }

    #[test]
    fn proposal_outcome_blocks_reason_threshold_dry_run_stale_and_confirmation() {
        let blocked = decision(Operation::Fill, Some("needs a caller-supplied value"));
        assert_eq!(
            proposal_outcome(&blocked, true, None, false, false, false),
            ProposalOutcome::Blocked
        );

        let decision = decision(Operation::Click, None);
        assert_eq!(
            proposal_outcome(&decision, true, Some(0.95), false, false, false),
            ProposalOutcome::BelowThreshold
        );
        assert_eq!(
            proposal_outcome(&decision, false, Some(0.95), false, false, false),
            ProposalOutcome::Execute,
            "an uncalibrated decider ignores the floor"
        );
        assert_eq!(
            proposal_outcome(&decision, true, None, true, false, false),
            ProposalOutcome::DryRun
        );
        assert_eq!(
            proposal_outcome(&decision, true, None, false, true, false),
            ProposalOutcome::Stale
        );
        assert_eq!(
            proposal_outcome(&decision, true, None, false, false, true),
            ProposalOutcome::ConfirmationRequired
        );
    }

    #[test]
    fn executed_mismatch_counts_as_executed_wrong() {
        let mut metrics = ProposalMetrics::default();
        metrics.record(Some(ProposalOutcome::Execute), false, false);
        assert_eq!(metrics.total, 1);
        assert_eq!(metrics.decision_wrong, 1);
        assert_eq!(metrics.executed_total, 1);
        assert_eq!(metrics.executed_wrong, 1);
    }

    #[test]
    fn blocked_mismatch_is_not_executed_wrong() {
        let mut metrics = ProposalMetrics::default();
        metrics.record(Some(ProposalOutcome::Blocked), false, false);
        assert_eq!(metrics.decision_wrong, 1);
        assert_eq!(metrics.blocked, 1);
        assert_eq!(metrics.executed_total, 0);
        assert_eq!(metrics.executed_wrong, 0);
    }

    #[test]
    fn blocked_correct_is_not_executed() {
        let mut metrics = ProposalMetrics::default();
        metrics.record(Some(ProposalOutcome::Blocked), true, true);
        assert_eq!(metrics.correct, 1);
        assert_eq!(metrics.expected_blocked, 1);
        assert_eq!(metrics.blocked_correct, 1);
        assert_eq!(metrics.executed_total, 0);
        assert_eq!(metrics.executed_wrong, 0);
    }

    #[test]
    fn executed_correct_is_not_wrong() {
        let mut metrics = ProposalMetrics::default();
        metrics.record(Some(ProposalOutcome::Execute), false, true);
        assert_eq!(metrics.correct, 1);
        assert_eq!(metrics.executed_total, 1);
        assert_eq!(metrics.executed_wrong, 0);
    }

    #[test]
    fn confirmation_required_never_counts_as_executed() {
        let mut metrics = ProposalMetrics::default();
        metrics.record(Some(ProposalOutcome::ConfirmationRequired), false, false);
        assert_eq!(metrics.decision_wrong, 1);
        assert_eq!(metrics.executed_total, 0);
        assert_eq!(metrics.executed_wrong, 0);
    }

    #[test]
    fn dry_run_stale_and_below_threshold_never_count_as_executed() {
        let mut metrics = ProposalMetrics::default();
        for outcome in [
            ProposalOutcome::DryRun,
            ProposalOutcome::Stale,
            ProposalOutcome::BelowThreshold,
        ] {
            metrics.record(Some(outcome), false, false);
        }
        assert_eq!(metrics.decision_wrong, 3);
        assert_eq!(metrics.executed_total, 0);
        assert_eq!(metrics.executed_wrong, 0);
    }

    #[test]
    fn errors_are_wrong_but_never_executed() {
        let mut metrics = ProposalMetrics::default();
        metrics.record_error(false);
        assert_eq!(metrics.errors, 1);
        assert_eq!(metrics.decision_wrong, 1);
        assert_eq!(metrics.executed_total, 0);
        assert_eq!(metrics.executed_wrong, 0);
    }

    #[test]
    fn clopper_pearson_matches_known_v3_bound() {
        let upper = clopper_pearson_upper(8, 333, 0.95);
        assert!(
            (upper - 0.0428).abs() < 0.0005,
            "8/333 upper bound was {}",
            upper
        );
        let zero = clopper_pearson_upper(0, 440, 0.95);
        let expected = 1.0 - 0.05_f64.powf(1.0 / 440.0);
        assert!((zero - expected).abs() < 1e-9, "{} vs {}", zero, expected);
        assert_eq!(clopper_pearson_upper(0, 0, 0.95), 1.0);
    }
}
