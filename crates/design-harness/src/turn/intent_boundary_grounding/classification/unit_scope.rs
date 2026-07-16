use super::action_authority::*;
use super::action_polarity::marker_is_negated;
use super::gate_control::*;
use super::live_scope::*;
use super::secret_disclosure::*;
use super::vocabulary::*;
use super::{
    closed_direct_separable_turn_off_action, closed_safety_control_action_tail,
    closed_safety_control_result_meaning, closed_safety_control_tail, safety_control_action,
    safety_control_target_length, strip_safety_control_target_modifiers, syntax, BoundaryUnit,
    IntentBoundaryRequestV2, KoreanSafetyControlClause, SafetyControlMeaning,
    SafetyControlTailEffect, TextSpan, UnitLink, ACTION_NEGATION_MODIFIERS,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum BoundaryKind {
    Gate,
    Live,
    Secret,
}

impl BoundaryKind {
    pub(in super::super) fn request(self) -> IntentBoundaryRequestV2 {
        match self {
            Self::Gate => IntentBoundaryRequestV2::BypassValidationPreviewApproval,
            Self::Live => IntentBoundaryRequestV2::DirectLiveMutation,
            Self::Secret => IntentBoundaryRequestV2::SecretDisclosure,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in super::super) struct UnitFacts {
    pub(in super::super) gate_action: bool,
    pub(in super::super) gate_target: bool,
    pub(in super::super) live_action: bool,
    pub(in super::super) live_strong_context: bool,
    pub(in super::super) live_weak_context: bool,
    pub(in super::super) immediate: bool,
    pub(in super::super) secret_action: bool,
    pub(in super::super) secret_target: bool,
    pub(in super::super) secret_unsafe_target: bool,
    pub(in super::super) secret_delivery: bool,
    pub(in super::super) secret_unprotected: bool,
}

impl UnitFacts {
    pub(in super::super) fn for_text(value: &str) -> Self {
        let gate_meaning = closed_gate_control_meaning(value);
        let gate_result_meaning =
            closed_safety_control_result_meaning(&value.split_whitespace().collect::<Vec<_>>());
        let korean_gate = korean_safety_control_clause(value);
        let gate_preservation = gate_meaning == Some(SafetyControlMeaning::PreservesControl)
            || matches!(
                korean_gate,
                Some(KoreanSafetyControlClause::Control(
                    SafetyControlMeaning::PreservesControl
                )) | Some(KoreanSafetyControlClause::BusinessOperation)
            );
        let secret_target = contains_bounded_any(value, SECRET_TARGETS);
        let secret_unprotected = has_unnegated_unprotected_secret(value);
        Self {
            gate_action: !gate_preservation
                && (gate_meaning == Some(SafetyControlMeaning::WeakensControl)
                    || korean_gate
                        == Some(KoreanSafetyControlClause::Control(
                            SafetyControlMeaning::WeakensControl,
                        ))
                    || has_optional_gate_bypass_text(value)
                    || closed_requested_rule_gate_weakening(value)),
            gate_target: has_bounded_gate_target(value) || gate_result_meaning.is_some(),
            live_action: has_unnegated_boundary_action(value, BoundaryKind::Live),
            live_strong_context: has_operational_live_context(value)
                || contains_bounded_any(value, &["live changes", "라이브 변경"]),
            live_weak_context: contains_bounded_any(value, live_weak_context()),
            immediate: contains_bounded_any(value, IMMEDIATE_CONTEXT),
            secret_action: has_unnegated_boundary_action(value, BoundaryKind::Secret),
            secret_target,
            secret_unsafe_target: has_unsafe_secret_target(value)
                || (secret_target && secret_unprotected),
            secret_delivery: contains_bounded_any(value, SECRET_DELIVERY_CONTEXT),
            secret_unprotected,
        }
    }

    pub(in super::super) fn for_unit(unit: &BoundaryUnit) -> Self {
        let mut facts = Self::for_text(&unit.text);
        facts.gate_action &= !unit.inherited_gate_action_negation;
        facts.live_action &= !unit.inherited_live_action_negation;
        facts.secret_action &= !unit.inherited_secret_action_negation;
        facts
    }

    pub(in super::super) fn is_seed(&self, kind: BoundaryKind) -> bool {
        match kind {
            BoundaryKind::Gate => self.gate_action && self.gate_target,
            BoundaryKind::Live => {
                self.live_action
                    && (self.live_strong_context || (self.live_weak_context && self.immediate))
            }
            BoundaryKind::Secret => {
                (self.secret_action && self.secret_unsafe_target)
                    || (self.secret_unsafe_target
                        && self.secret_unprotected
                        && self.secret_delivery)
            }
        }
    }

    pub(in super::super) fn has_evidence(&self, kind: BoundaryKind) -> bool {
        match kind {
            BoundaryKind::Gate => self.gate_action || self.gate_target,
            BoundaryKind::Live => {
                self.live_action
                    || self.live_strong_context
                    || self.live_weak_context
                    || self.immediate
            }
            BoundaryKind::Secret => {
                self.secret_action
                    || self.secret_target
                    || self.secret_unsafe_target
                    || self.secret_delivery
                    || self.secret_unprotected
            }
        }
    }
}

pub(super) fn closed_requested_rule_gate_weakening(value: &str) -> bool {
    if !closed_requested_observer_artifact(value) {
        return false;
    }
    GATE_ACTIONS.iter().any(|marker| {
        value.match_indices(marker).any(|(start, matched)| {
            let end = start.saturating_add(matched.len());
            marker_has_boundaries(value, start, end)
                && !marker_is_negated(value, start, end)
                && value[..start].split_whitespace().next_back() == Some("that")
                && has_bounded_gate_target(&value[end..])
        })
    })
}

pub(in super::super) fn classify_sentence_units(
    visible: &[char],
    sentence: TextSpan,
    question: bool,
) -> Vec<BoundaryUnit> {
    let (mut units, operative_start) = syntax::sentence_units(visible, sentence, question);
    merge_third_person_actor_tails(visible, &mut units);
    merge_gate_business_tails(visible, &mut units);
    merge_gate_exception_tails(visible, &mut units);
    apply_hypothetical_scope(&mut units, question);
    apply_non_authoritative_event_scope(visible, &mut units);
    apply_coordinated_negation_scope(&mut units);
    if let Some(operative_start) = operative_start {
        for unit in &mut units {
            unit.hypothetical = unit.span.start < operative_start;
        }
    }
    units
}

pub(super) fn apply_non_authoritative_event_scope(visible: &[char], units: &mut [BoundaryUnit]) {
    for index in 0..units.len() {
        if closed_non_authoritative_event_unit(&units[index].text) {
            units[index].hypothetical = true;
            units[index].non_authoritative_event = true;
        }
        let Some(previous) = index.checked_sub(1).and_then(|index| units.get(index)) else {
            continue;
        };
        let connector = visible[previous.span.end..units[index].span.start]
            .iter()
            .collect::<String>()
            .to_lowercase();
        if units[index].link == UnitLink::Barrier
            && matches!(connector.trim(), "when" | "whenever" | "while")
            && closed_requested_observer_artifact(&previous.text)
        {
            units[index].hypothetical = true;
            units[index].non_authoritative_event = true;
        }
    }
}

pub(super) fn closed_non_authoritative_event_unit(value: &str) -> bool {
    let observer_event = [" attempts to ", " attempt to ", " tries to "]
        .iter()
        .find_map(|connector| value.find(connector))
        .is_some_and(|event_start| {
            closed_observer_guard_carrier(&value[..event_start])
                && !unit_text_is_boundary_seed(&value[..event_start])
        });
    let observed_trigger = value.find(" whenever ").is_some_and(|event_start| {
        closed_requested_observer_artifact(&value[..event_start])
            && !unit_text_is_boundary_seed(&value[..event_start])
    });
    let simulated_event = [
        "simulate ",
        "simulates ",
        "simulated ",
        "simulating ",
        "simulation of ",
    ]
    .iter()
    .any(|carrier| value.starts_with(carrier));
    observer_event || observed_trigger || simulated_event
}

pub(super) fn closed_observer_guard_carrier(value: &str) -> bool {
    let artifact = [
        "alert",
        "alerts",
        "detector",
        "detectors",
        "guard",
        "guards",
        "monitor",
        "monitors",
        "rule",
        "rules",
        "warning",
        "warnings",
    ]
    .iter()
    .any(|marker| contains_bounded_any(value, &[*marker]));
    let predicate = [
        "alert", "alerts", "block", "blocks", "denies", "deny", "detect", "detects", "monitor",
        "monitors", "prevent", "prevents", "warn", "warns",
    ]
    .iter()
    .any(|marker| contains_bounded_any(value, &[*marker]));
    artifact && predicate
}

pub(super) fn closed_requested_observer_artifact(value: &str) -> bool {
    let artifact = [
        "alert",
        "alerts",
        "detector",
        "detectors",
        "guard",
        "guards",
        "monitor",
        "monitors",
        "rule",
        "rules",
        "warning",
        "warnings",
    ]
    .iter()
    .any(|marker| contains_bounded_any(value, &[*marker]));
    let requested = ["build", "create", "make"]
        .iter()
        .any(|marker| contains_bounded_any(value, &[*marker]));
    artifact && requested
}

pub(super) fn unit_text_is_boundary_seed(value: &str) -> bool {
    let facts = UnitFacts::for_text(value);
    [BoundaryKind::Gate, BoundaryKind::Live, BoundaryKind::Secret]
        .into_iter()
        .any(|kind| facts.is_seed(kind))
        || (has_bounded_gate_target(value)
            && contains_bounded_any(
                value,
                &[
                    GATE_ACTIONS,
                    GATE_DESTRUCTIVE_ACTIONS,
                    GATE_REQUIREMENT_REVERSAL_ACTIONS,
                ]
                .concat(),
            ))
}

pub(super) fn merge_third_person_actor_tails(visible: &[char], units: &mut Vec<BoundaryUnit>) {
    let original = std::mem::take(units);
    let mut merged = Vec::with_capacity(original.len());
    let mut index = 0usize;
    while index < original.len() {
        let tail_kind = original
            .get(index.saturating_add(1))
            .filter(|unit| unit.link == UnitLink::Sequential)
            .and_then(|unit| starts_with_third_person_boundary_action(&unit.text));
        if tail_kind.is_some_and(|kind| closed_boundary_actor_unit(&original[index].text, kind)) {
            let mut unit = original[index].clone();
            unit.span.end = original[index + 1].span.end;
            unit.text = syntax::normalized_text(
                &visible[unit.span.start..unit.span.end]
                    .iter()
                    .collect::<String>()
                    .to_lowercase(),
            );
            merged.push(unit);
            index = index.saturating_add(2);
        } else {
            merged.push(original[index].clone());
            index = index.saturating_add(1);
        }
    }
    *units = merged;
}

pub(super) fn closed_boundary_actor_unit(value: &str, kind: BoundaryKind) -> bool {
    let words = value.split_whitespace().collect::<Vec<_>>();
    let actor = match words.as_slice() {
        [actor] => Some(*actor),
        ["a" | "an" | "the", actor] => Some(*actor),
        ["a" | "an" | "the", "public", actor] => Some(*actor),
        _ => None,
    };
    actor.is_some_and(|actor| closed_third_person_actor(kind, actor))
}

pub(super) fn closed_public_secret_disclosure_subject(value: &str, action_start: usize) -> bool {
    if !contains_bounded_any(value, SECRET_TARGETS)
        || !contains_bounded_any(value, SECRET_DELIVERY_CONTEXT)
    {
        return false;
    }
    let prefix = value[..action_start].trim_end();
    let mut words = prefix.split_whitespace().collect::<Vec<_>>();
    while words
        .last()
        .is_some_and(|word| closed_boundary_action_adverb(word))
    {
        words.pop();
    }
    if !words
        .first()
        .is_some_and(|word| matches!(*word, "a" | "an" | "the"))
    {
        return false;
    }
    words.remove(0);
    !words.is_empty()
        && words.len() <= 4
        && words.iter().all(|word| {
            word.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && !matches!(
                    *word,
                    "can"
                        | "could"
                        | "do"
                        | "does"
                        | "may"
                        | "might"
                        | "must"
                        | "not"
                        | "should"
                        | "will"
                        | "would"
                )
        })
}

pub(super) fn starts_with_third_person_boundary_action(value: &str) -> Option<BoundaryKind> {
    let mut words = value.split_whitespace();
    let mut word = words.next()?;
    for _ in 0..2 {
        if !closed_boundary_action_adverb(word) {
            break;
        }
        word = words.next()?;
    }
    if LIVE_THIRD_PERSON_ACTIONS.contains(&word) {
        Some(BoundaryKind::Live)
    } else if SECRET_THIRD_PERSON_ACTIONS.contains(&word) {
        Some(BoundaryKind::Secret)
    } else {
        None
    }
}

pub(super) fn merge_gate_exception_tails(visible: &[char], units: &mut Vec<BoundaryUnit>) {
    let original = std::mem::take(units);
    let mut merged = Vec::with_capacity(original.len());
    let mut index = 0usize;
    while index < original.len() {
        if index.saturating_add(1) < original.len()
            && original[index + 1].link == UnitLink::ConditionalAlternative
            && closed_gate_control_meaning(&original[index].text)
                == Some(SafetyControlMeaning::PreservesControl)
            && closed_gate_exception_scope(&original[index + 1].text)
        {
            let mut unit = original[index].clone();
            unit.span.end = original[index + 1].span.end;
            unit.text = syntax::normalized_text(
                &visible[unit.span.start..unit.span.end]
                    .iter()
                    .collect::<String>()
                    .to_lowercase(),
            );
            merged.push(unit);
            index = index.saturating_add(2);
        } else {
            merged.push(original[index].clone());
            index = index.saturating_add(1);
        }
    }
    *units = merged;
}

pub(super) fn merge_gate_business_tails(visible: &[char], units: &mut Vec<BoundaryUnit>) {
    let original = std::mem::take(units);
    let mut merged: Vec<BoundaryUnit> = Vec::with_capacity(original.len());
    let mut index = 0usize;
    while index < original.len() {
        let Some(end) = gate_business_run(&original, index) else {
            merged.push(original[index].clone());
            index = index.saturating_add(1);
            continue;
        };
        let mut unit = original[index].clone();
        unit.span.end = original[end].span.end;
        unit.text = syntax::normalized_text(
            &visible[unit.span.start..unit.span.end]
                .iter()
                .collect::<String>()
                .to_lowercase(),
        );
        merged.push(unit);
        index = end.saturating_add(1);
    }
    *units = merged;
}

pub(super) fn gate_business_run(units: &[BoundaryUnit], start: usize) -> Option<usize> {
    let first = strip_exact_prefix_wrappers(&units.get(start)?.text);
    let first_words = first.split_whitespace().collect::<Vec<_>>();
    let action = safety_control_action(&first_words)?;
    if !closed_safety_control_tail(&first_words[action.length..]) {
        return None;
    }
    for (index, unit) in units.iter().enumerate().skip(start.saturating_add(1)) {
        if unit.link != UnitLink::Additive || closed_gate_control_meaning(&unit.text).is_some() {
            return None;
        }
        let words = unit.text.split_whitespace().collect::<Vec<_>>();
        let target = strip_safety_control_target_modifiers(&words);
        let target_length = safety_control_target_length(target)?;
        let remainder = &target[target_length..];
        if remainder.is_empty() {
            continue;
        }
        return closed_gate_business_object_tail(first_words[0], remainder).then_some(index);
    }
    None
}

pub(super) fn closed_gate_business_object_tail(action: &str, words: &[&str]) -> bool {
    match action {
        "drop" | "dropping" | "eliminate" | "eliminating" | "remove" | "removing" => {
            matches!(
                words,
                [
                    "artifact"
                        | "artifacts"
                        | "event"
                        | "events"
                        | "log"
                        | "logs"
                        | "record"
                        | "records"
                        | "request"
                        | "requests"
                        | "status",
                    ..
                ] | [
                    "audit",
                    "log" | "logging" | "logs" | "record" | "records",
                    ..
                ]
            )
        }
        "ignore" | "ignoring" => matches!(
            words,
            ["error" | "handling" | "latency" | "log" | "logs", ..]
        ),
        "disable" | "disabling" => matches!(
            words,
            [
                "animation" | "animations" | "notification" | "notifications",
                ..
            ]
        ),
        "omit" | "omitting" | "skip" | "skipping" => matches!(
            words,
            [
                "audit",
                "log" | "logging" | "logs" | "record" | "records",
                ..
            ]
        ),
        _ => false,
    }
}

pub(super) fn apply_hypothetical_scope(units: &mut [BoundaryUnit], question: bool) {
    let mut inherited = question;
    let mut local = false;
    for unit in units {
        if matches!(unit.link, UnitLink::Sequential | UnitLink::Barrier) {
            local = false;
        }
        let explicit_hypothetical = contains_hypothetical_marker(&unit.text);
        let explicit_local = contains_local_discussion_marker(&unit.text);
        let explicit_assertion = contains_polite_request(&unit.text);
        if explicit_hypothetical {
            inherited = true;
        } else if explicit_local {
            local = true;
        } else if explicit_assertion {
            inherited = false;
            local = false;
        }
        unit.hypothetical = inherited || local;
    }
}

pub(super) fn contains_local_discussion_marker(value: &str) -> bool {
    let value = strip_exact_prefix_wrappers(value);
    [
        "describe the risk ",
        "describe the risks ",
        "discuss ",
        "discussion only:",
        "explain the tradeoff ",
        "explain the tradeoffs ",
        "explain whether ",
        "let's discuss ",
        "let us discuss ",
    ]
    .iter()
    .any(|prefix| value.starts_with(prefix))
}

pub(super) fn apply_coordinated_negation_scope(units: &mut [BoundaryUnit]) {
    if units.len() < 2 {
        return;
    }
    let mut inherited = [false; 3];
    let mut coordinated_preservation = [None; 3];
    let mut alternative_negation = AlternativeNegationScope::None;
    for unit in units {
        let connected = matches!(
            unit.link,
            UnitLink::Additive | UnitLink::Alternative | UnitLink::NegativeAlternative
        );
        let active_alternative_negation = if unit.link == UnitLink::NegativeAlternative {
            AlternativeNegationScope::WholeClause
        } else {
            alternative_negation
        };
        let mut inherited_alternative = false;
        for kind in [BoundaryKind::Gate, BoundaryKind::Live, BoundaryKind::Secret] {
            let index = boundary_kind_index(kind);
            let inherits_alternative = matches!(
                unit.link,
                UnitLink::Alternative | UnitLink::NegativeAlternative
            ) && match active_alternative_negation {
                AlternativeNegationScope::None => false,
                AlternativeNegationScope::BareOnly => {
                    starts_with_bare_boundary_action(&unit.text, kind)
                }
                AlternativeNegationScope::WholeClause => {
                    starts_with_bare_boundary_action(&unit.text, kind)
                        || independent_positive_boundary_clause(&unit.text, kind)
                        || UnitFacts::for_text(&unit.text).is_seed(kind)
                }
            };
            inherited_alternative |= inherits_alternative;
            if !connected {
                inherited[index] = false;
                coordinated_preservation[index] = None;
            }
            if matches!(
                unit.link,
                UnitLink::Alternative | UnitLink::NegativeAlternative
            ) && !inherits_alternative
            {
                inherited[index] = false;
                coordinated_preservation[index] = None;
            }
            if independent_positive_boundary_clause(&unit.text, kind) {
                inherited[index] = false;
                coordinated_preservation[index] = None;
            }
            if inherits_alternative {
                inherited[index] = true;
                coordinated_preservation[index] = None;
            }
            if let Some(continuation) = direct_preservation_continuation(&unit.text, kind) {
                set_inherited_action_negation(unit, kind, false);
                inherited[index] = false;
                coordinated_preservation[index] = Some(continuation);
                continue;
            }
            let inherits_preservation = connected
                && coordinated_preservation[index].is_some_and(|continuation| {
                    starts_with_preservation_action(&unit.text, kind, continuation)
                });
            if inherits_preservation {
                set_inherited_action_negation(unit, kind, true);
                inherited[index] = false;
                continue;
            }
            set_inherited_action_negation(unit, kind, inherited[index]);
            coordinated_preservation[index] = None;
            if has_negated_boundary_action_with_anchor(&unit.text, kind) {
                inherited[index] = true;
            } else if !inherited_action_negation(unit, kind) {
                inherited[index] = false;
            }
        }
        let opened_scope = leading_alternative_negation_scope(&unit.text);
        alternative_negation = if opened_scope != AlternativeNegationScope::None {
            opened_scope
        } else if matches!(
            unit.link,
            UnitLink::Alternative | UnitLink::NegativeAlternative
        ) && (active_alternative_negation == AlternativeNegationScope::WholeClause
            || inherited_alternative)
        {
            active_alternative_negation
        } else if has_negated_action_marker(&unit.text) && !unit.text.starts_with("either ") {
            AlternativeNegationScope::BareOnly
        } else {
            AlternativeNegationScope::None
        };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AlternativeNegationScope {
    None,
    BareOnly,
    WholeClause,
}

pub(super) fn has_negated_action_marker(value: &str) -> bool {
    [BoundaryKind::Gate, BoundaryKind::Live, BoundaryKind::Secret]
        .into_iter()
        .any(|kind| has_negated_boundary_action_marker(value, kind))
}

pub(super) fn starts_with_bare_boundary_action(value: &str, kind: BoundaryKind) -> bool {
    let words = value.split_whitespace().collect::<Vec<_>>();
    let mut start = 0usize;
    while words
        .get(start)
        .is_some_and(|word| ACTION_NEGATION_MODIFIERS.contains(word))
    {
        start = start.saturating_add(1);
    }
    let words = &words[start..];
    if kind == BoundaryKind::Gate {
        if let Some(action) = safety_control_action(words) {
            return closed_safety_control_action_tail(&words[action.length..])
                == Some(SafetyControlTailEffect::Direct);
        }
        return closed_direct_separable_turn_off_action(words);
    }
    if boundary_governance_predicate(value) {
        return false;
    }
    boundary_action_markers(kind)
        .filter_map(|marker| marker.split_whitespace().next())
        .any(|marker| words.first() == Some(&marker))
}

pub(super) fn independent_positive_boundary_clause(value: &str, kind: BoundaryKind) -> bool {
    if explicit_positive_request_wrapper(value) {
        return UnitFacts::for_text(value).has_evidence(kind);
    }
    match kind {
        BoundaryKind::Gate => {
            closed_gate_control_meaning(value) == Some(SafetyControlMeaning::WeakensControl)
                && !starts_with_bare_boundary_action(value, kind)
        }
        BoundaryKind::Live | BoundaryKind::Secret => {
            has_unnegated_boundary_action(value, kind)
                && (boundary_governance_predicate(value)
                    || has_explicit_third_person_boundary_action(value, kind))
        }
    }
}

pub(super) fn has_explicit_third_person_boundary_action(value: &str, kind: BoundaryKind) -> bool {
    let third_person = match kind {
        BoundaryKind::Live => LIVE_THIRD_PERSON_ACTIONS,
        BoundaryKind::Secret => SECRET_THIRD_PERSON_ACTIONS,
        BoundaryKind::Gate => return false,
    };
    third_person.iter().any(|marker| {
        value.match_indices(marker).any(|(start, matched)| {
            let end = start.saturating_add(matched.len());
            marker_has_boundaries(value, start, end)
                && !marker_is_negated(value, start, end)
                && closed_third_person_boundary_actor(value, start, kind)
        })
    })
}

pub(super) fn explicit_positive_request_wrapper(value: &str) -> bool {
    GATE_EXACT_PREFIX_WRAPPERS
        .iter()
        .filter(|wrapper| {
            matches!(
                **wrapper,
                "i want you to "
                    | "i need you to "
                    | "could you "
                    | "would you "
                    | "can you "
                    | "will you "
                    | "please "
            )
        })
        .any(|wrapper| value.starts_with(wrapper))
}

pub(super) fn boundary_governance_predicate(value: &str) -> bool {
    [
        " is allowed",
        " is enabled",
        " is permitted",
        " is not disabled",
        " is not forbidden",
        " is not prohibited",
        " can be allowed",
        " may be allowed",
        " must be allowed",
        " should be allowed",
        " will be allowed",
    ]
    .iter()
    .any(|predicate| value.contains(predicate))
}

pub(super) fn leading_alternative_negation_scope(value: &str) -> AlternativeNegationScope {
    if value.starts_with("either ") {
        return AlternativeNegationScope::None;
    }
    if value.starts_with("neither ") {
        return AlternativeNegationScope::WholeClause;
    }
    let Some(control) = [
        "do not ",
        "don't ",
        "dont ",
        "don’t ",
        "never ",
        "must not ",
        "should not ",
    ]
    .iter()
    .find(|control| value.starts_with(**control)) else {
        return AlternativeNegationScope::None;
    };
    if value[control.len()..].starts_with("either ") {
        AlternativeNegationScope::WholeClause
    } else {
        AlternativeNegationScope::BareOnly
    }
}

pub(super) fn has_negated_boundary_action_with_anchor(value: &str, kind: BoundaryKind) -> bool {
    match kind {
        BoundaryKind::Gate => {
            closed_gate_control_meaning(value) == Some(SafetyControlMeaning::PreservesControl)
                && marker_sets_have_negated_action(
                    value,
                    &[
                        GATE_ACTIONS,
                        GATE_DESTRUCTIVE_ACTIONS,
                        GATE_EXACT_ACTIONS,
                        GATE_REQUIREMENT_REVERSAL_ACTIONS,
                    ],
                )
        }
        BoundaryKind::Live => marker_sets_have_negated_action(value, &[LIVE_ACTIONS]),
        BoundaryKind::Secret => marker_sets_have_negated_action(value, &[SECRET_ACTIONS]),
    }
}

pub(super) fn boundary_kind_index(kind: BoundaryKind) -> usize {
    match kind {
        BoundaryKind::Gate => 0,
        BoundaryKind::Live => 1,
        BoundaryKind::Secret => 2,
    }
}

pub(in super::super) fn inherited_action_negation(unit: &BoundaryUnit, kind: BoundaryKind) -> bool {
    match kind {
        BoundaryKind::Gate => unit.inherited_gate_action_negation,
        BoundaryKind::Live => unit.inherited_live_action_negation,
        BoundaryKind::Secret => unit.inherited_secret_action_negation,
    }
}

pub(super) fn set_inherited_action_negation(
    unit: &mut BoundaryUnit,
    kind: BoundaryKind,
    value: bool,
) {
    match kind {
        BoundaryKind::Gate => unit.inherited_gate_action_negation = value,
        BoundaryKind::Live => unit.inherited_live_action_negation = value,
        BoundaryKind::Secret => unit.inherited_secret_action_negation = value,
    }
}
