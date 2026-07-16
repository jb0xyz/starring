use super::action_authority::{without_complement_weakens_control, without_control_meaning};
use super::action_polarity::{action_prefix_polarity_words, closed_preservation_actor_term};
use super::vocabulary::*;
use super::{
    action_permission_length, closed_active_actor_safety_control_meaning,
    closed_actor_safety_control_meaning, closed_configuration_safety_control_meaning,
    closed_inverted_subject_safety_control_meaning, closed_korean_safety_control_clause,
    closed_passive_target_safety_control_meaning, closed_safety_control_action_meaning,
    closed_safety_control_result_meaning, closed_safety_control_scope,
    closed_safety_control_state_meaning, closed_separable_turn_off_safety_control_meaning,
    closed_subject_safety_control_meaning, closed_without_safety_control_meaning,
    preservation_prohibition_length, safety_control_action, safety_control_target_length,
    strip_safety_control_target_modifiers, word_continuation, KoreanSafetyControlClause,
    SafetyControlMeaning, ACTION_NEGATION_MODIFIERS, ORDINARY_PREFIX_NEGATIONS,
    PRESERVATION_DETERMINERS, PRESERVATION_PREFIX_NEGATIONS,
};
#[cfg(test)]
use super::{ROOT_SAFETY_CONTROL_ACTION_PROBES, ROOT_SAFETY_CONTROL_PREFIX_STEPS};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StructuralTail {
    Direct,
    Permitted,
    Prohibited,
}

pub(super) fn closed_gate_control_meaning(value: &str) -> Option<SafetyControlMeaning> {
    closed_gate_control_meaning_mode(value, true)
}

pub(super) fn closed_gate_control_meaning_mode(
    value: &str,
    allow_embedded_action: bool,
) -> Option<SafetyControlMeaning> {
    let core = strip_exact_prefix_wrappers(value.trim());
    if let Some((base, exception)) = split_gate_exception(core) {
        if closed_gate_control_meaning_mode(base, allow_embedded_action)
            == Some(SafetyControlMeaning::PreservesControl)
            && closed_gate_exception_scope(exception)
        {
            return Some(SafetyControlMeaning::WeakensControl);
        }
        return None;
    }
    if let Some((base, scope)) = split_restrictive_gate_scope(core) {
        if closed_gate_control_meaning_mode(base, allow_embedded_action)
            == Some(SafetyControlMeaning::PreservesControl)
            && closed_restricted_actor_scope(scope)
        {
            return Some(SafetyControlMeaning::WeakensControl);
        }
        return None;
    }
    let core = strip_exact_suffix_wrappers(core);
    let words = core.split_whitespace().collect::<Vec<_>>();
    if let Some(meaning) = closed_safety_control_result_meaning(&words) {
        return Some(meaning);
    }
    if let Some(meaning) = closed_actor_safety_control_meaning(&words) {
        return Some(meaning);
    }
    if let Some(meaning) = closed_active_actor_safety_control_meaning(&words) {
        return Some(meaning);
    }
    if let Some(meaning) = closed_passive_target_safety_control_meaning(&words) {
        return Some(meaning);
    }
    if let Some(meaning) = closed_configuration_safety_control_meaning(&words) {
        return Some(meaning);
    }
    if let Some(meaning) = closed_safety_control_state_meaning(&words) {
        return Some(meaning);
    }
    if let Some(meaning) = closed_without_safety_control_meaning(&words) {
        return Some(meaning);
    }
    if let Some(meaning) = closed_inverted_subject_safety_control_meaning(&words) {
        return Some(meaning);
    }
    if let Some(meaning) = closed_subject_safety_control_meaning(&words) {
        return Some(meaning);
    }
    if let Some(meaning) = closed_root_safety_control_action_meaning(&words) {
        return Some(meaning);
    }
    if !allow_embedded_action {
        return None;
    }
    if let Some(meaning) = without_control_meaning(core) {
        return Some(meaning);
    }
    let without_complement =
        words
            .iter()
            .rposition(|word| *word == "without")
            .and_then(|without| {
                without_complement_weakens_control(&words[without.saturating_add(1)..])
                    .map(|_| without.saturating_add(1))
            });
    (0..words.len()).find_map(|index| {
        if without_complement.is_some_and(|start| index >= start)
            || (safety_control_action(&words[index..]).is_none()
                && !words[index..]
                    .first()
                    .is_some_and(|word| matches!(*word, "turn" | "turning" | "turns")))
        {
            return None;
        }
        let negated = action_prefix_polarity_words(&words[..index]).0;
        closed_safety_control_action_meaning(&words[index..], negated)
            .or_else(|| closed_separable_turn_off_safety_control_meaning(&words[index..], negated))
    })
}

pub(super) fn closed_root_safety_control_action_meaning(
    words: &[&str],
) -> Option<SafetyControlMeaning> {
    for (index, word) in words.iter().enumerate() {
        if safety_control_action_head(word) {
            #[cfg(test)]
            ROOT_SAFETY_CONTROL_ACTION_PROBES
                .with(|steps| steps.set(steps.get().saturating_add(1)));
            let prefix = &words[..index];
            let negated = action_prefix_polarity_words(prefix).0;
            if let Some(meaning) = closed_safety_control_action_meaning(&words[index..], negated)
                .or_else(|| {
                    closed_separable_turn_off_safety_control_meaning(&words[index..], negated)
                })
            {
                return Some(meaning);
            }
        }
        #[cfg(test)]
        ROOT_SAFETY_CONTROL_PREFIX_STEPS.with(|steps| steps.set(steps.get().saturating_add(1)));
        if !closed_safety_control_action_prefix_word(word) {
            break;
        }
    }
    None
}

pub(super) fn safety_control_action_head(word: &str) -> bool {
    matches!(word, "turn" | "turning" | "turns") || safety_control_action(&[word]).is_some()
}

pub(super) fn closed_safety_control_action_prefix_word(word: &str) -> bool {
    ACTION_NEGATION_MODIFIERS.contains(&word)
        || closed_preservation_actor_term(word)
        || PRESERVATION_DETERMINERS.contains(&word)
        || matches!(word, "action" | "actions" | "all" | "every" | "from")
        || ORDINARY_PREFIX_NEGATIONS
            .iter()
            .chain(PRESERVATION_PREFIX_NEGATIONS)
            .flat_map(|control| control.split_whitespace())
            .any(|term| term == word)
}

pub(super) fn split_gate_exception(value: &str) -> Option<(&str, &str)> {
    [" unless ", " except for ", " except "]
        .iter()
        .find_map(|connector| value.split_once(connector))
}

pub(super) fn split_restrictive_gate_scope(value: &str) -> Option<(&str, &str)> {
    if let Some(parts) = value.split_once(" only for ") {
        return Some(parts);
    }
    let value = value.strip_suffix(" only")?;
    value.rsplit_once(" for ")
}

pub(super) fn closed_restricted_actor_scope(value: &str) -> bool {
    let words = value.split_whitespace().collect::<Vec<_>>();
    (1..=3).contains(&words.len())
        && words.iter().all(|word| {
            matches!(
                *word,
                "admin"
                    | "admins"
                    | "guest"
                    | "guests"
                    | "member"
                    | "members"
                    | "owner"
                    | "owners"
                    | "the"
                    | "user"
                    | "users"
            )
        })
        && words.iter().any(|word| !matches!(*word, "the"))
}

pub(super) fn closed_gate_exception_scope(value: &str) -> bool {
    let words = value.split_whitespace().collect::<Vec<_>>();
    (1..=8).contains(&words.len())
        && words.iter().all(|word| {
            matches!(
                *word,
                "a" | "action"
                    | "actions"
                    | "admin"
                    | "admins"
                    | "an"
                    | "approves"
                    | "by"
                    | "disables"
                    | "for"
                    | "from"
                    | "guest"
                    | "guests"
                    | "is"
                    | "it"
                    | "member"
                    | "members"
                    | "owner"
                    | "owners"
                    | "out"
                    | "opts"
                    | "request"
                    | "requests"
                    | "the"
                    | "user"
                    | "users"
                    | "waived"
            )
        })
        && words.iter().any(|word| {
            matches!(
                *word,
                "admin"
                    | "admins"
                    | "guest"
                    | "guests"
                    | "member"
                    | "members"
                    | "owner"
                    | "owners"
                    | "user"
                    | "users"
            )
        })
}

pub(in super::super) fn closed_gate_control_weakening(value: &str) -> bool {
    closed_gate_control_meaning_mode(value, false) == Some(SafetyControlMeaning::WeakensControl)
        || korean_safety_control_clause(value)
            == Some(KoreanSafetyControlClause::Control(
                SafetyControlMeaning::WeakensControl,
            ))
}

pub(super) fn korean_safety_control_clause(value: &str) -> Option<KoreanSafetyControlClause> {
    let words = value.split_whitespace().collect::<Vec<_>>();
    closed_korean_safety_control_clause(&words)
}

pub(super) fn has_optional_gate_bypass(words: &[&str]) -> bool {
    words.iter().enumerate().any(|(index, action)| {
        if *action != "make" {
            return false;
        }
        let remainder = strip_safety_control_target_modifiers(&words[index.saturating_add(1)..]);
        let Some(target_length) = safety_control_target_length(remainder) else {
            return false;
        };
        let remainder = &remainder[target_length..];
        remainder.first() == Some(&"optional")
            && closed_structural_remainder(&remainder[1..]).is_some()
            && !action_prefix_polarity_words(&words[..index]).0
    })
}

pub(super) fn has_optional_gate_bypass_text(value: &str) -> bool {
    let core = strip_exact_suffix_wrappers(strip_exact_prefix_wrappers(value.trim()));
    has_optional_gate_bypass(&core.split_whitespace().collect::<Vec<_>>())
}

pub(super) fn closed_structural_remainder(words: &[&str]) -> Option<StructuralTail> {
    let words = strip_exact_tail_wrappers(words);
    if words.is_empty() || closed_safety_control_scope(words) {
        return Some(StructuralTail::Direct);
    }
    if let Some(length) = preservation_prohibition_length(words) {
        let remainder = &words[length..];
        if remainder.is_empty() || closed_safety_control_scope(remainder) {
            return Some(StructuralTail::Prohibited);
        }
    }
    if let Some(length) = action_permission_length(words) {
        let remainder = &words[length..];
        if remainder.is_empty() || closed_safety_control_scope(remainder) {
            return Some(StructuralTail::Permitted);
        }
    }
    None
}

pub(super) fn strip_exact_tail_wrappers<'a>(mut words: &'a [&'a str]) -> &'a [&'a str] {
    loop {
        let Some(length) = [
            &["right", "away"][..],
            &["right", "now"][..],
            &["immediately"][..],
            &["please"][..],
            &["only"][..],
            &["now"][..],
        ]
        .iter()
        .find_map(|wrapper| words.starts_with(wrapper).then_some(wrapper.len())) else {
            return words;
        };
        words = &words[length..];
    }
}

pub(super) fn strip_exact_prefix_wrappers(mut value: &str) -> &str {
    loop {
        let Some(stripped) = GATE_EXACT_PREFIX_WRAPPERS
            .iter()
            .find_map(|wrapper| value.strip_prefix(wrapper))
        else {
            return value;
        };
        value = stripped.trim_start();
    }
}

pub(super) fn strip_exact_suffix_wrappers(mut value: &str) -> &str {
    loop {
        let Some(stripped) = GATE_EXACT_SUFFIX_WRAPPERS
            .iter()
            .find_map(|wrapper| value.strip_suffix(wrapper))
        else {
            return value;
        };
        value = stripped.trim_end();
    }
}

pub(super) fn has_bounded_gate_target(value: &str) -> bool {
    GATE_TARGETS.iter().any(|target| {
        value.match_indices(target).any(|(start, matched)| {
            marker_has_boundaries(value, start, start.saturating_add(matched.len()))
        })
    })
}

pub(super) fn marker_has_boundaries(value: &str, start: usize, end: usize) -> bool {
    let left = value[..start].chars().next_back();
    let right = value[end..].chars().next();
    let left_valid = !left.is_some_and(word_continuation);
    let right_valid =
        !right.is_some_and(word_continuation) || known_korean_marker_suffix(&value[end..]);
    left_valid && right_valid
}

pub(super) fn known_korean_marker_suffix(value: &str) -> bool {
    [
        "가",
        "게",
        "고",
        "과",
        "기",
        "도",
        "된",
        "돼",
        "들",
        "를",
        "만",
        "면",
        "로",
        "를",
        "에",
        "에서",
        "에게",
        "와",
        "으",
        "은",
        "을",
        "의",
        "이",
        "인",
        "지",
        "주",
        "줘",
        "주세요",
        "하",
        "해",
    ]
    .iter()
    .any(|suffix| value.starts_with(suffix))
}
