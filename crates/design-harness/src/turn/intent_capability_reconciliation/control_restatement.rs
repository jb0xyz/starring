use super::super::intent_safety_control_grammar::{
    closed_active_actor_safety_control_meaning, closed_actor_safety_control_meaning,
    closed_actor_safety_control_preservation, closed_configuration_safety_control_meaning,
    closed_inverted_subject_safety_control_meaning, closed_korean_safety_control_clause,
    closed_passive_target_safety_control_meaning, closed_safety_control_action_meaning,
    closed_safety_control_result_meaning, closed_safety_control_scope,
    closed_safety_control_state_meaning, closed_safety_control_tail,
    closed_separable_turn_off_safety_control_meaning, closed_subject_safety_control_meaning,
    closed_without_safety_control_meaning, safety_control_action, safety_control_target_length,
    strip_safety_control_target_modifiers, KoreanSafetyControlClause, SafetyControlActionEffect,
    SafetyControlMeaning, ACTION_NEGATION_MODIFIERS, PRESERVATION_ACTOR_TERMS,
    PRESERVATION_DETERMINERS, PRESERVATION_PREFIX_NEGATIONS,
};
use super::syntax::SourceText;

pub(super) fn enforced_safety_control_restatement(source: &SourceText<'_>, value: &str) -> bool {
    let Some(words) = source.unique_complete_asserted_clause_tokens(value) else {
        return false;
    };
    english_safety_control_restatement(&words) || korean_safety_control_restatement(&words)
}

fn english_safety_control_restatement(words: &[String]) -> bool {
    let words = words.iter().map(String::as_str).collect::<Vec<_>>();
    let words = strip_english_control_request_prefix(&words);
    if words.is_empty() {
        return false;
    }
    if neither_safety_control_restatement(words) {
        return true;
    }
    if shared_negative_either_safety_control_restatement(words) {
        return true;
    }
    coordinated_english_safety_control_restatement(words)
}

fn shared_negative_either_safety_control_restatement(words: &[&str]) -> bool {
    let clauses = if words.starts_with(&["do", "not", "either"])
        || words.starts_with(&["don", "t", "either"])
    {
        &words[3..]
    } else if words
        .first()
        .is_some_and(|word| matches!(*word, "don't" | "dont" | "don’t" | "never"))
        && words.get(1) == Some(&"either")
    {
        &words[2..]
    } else {
        return false;
    };
    let mut start = 0usize;
    let mut count = 0usize;
    for end in 0..=clauses.len() {
        if end < clauses.len() && clauses[end] != "or" {
            continue;
        }
        if end == start
            || closed_positive_safety_control_meaning(&clauses[start..end])
                != Some(SafetyControlMeaning::WeakensControl)
        {
            return false;
        }
        count = count.saturating_add(1);
        start = end.saturating_add(1);
    }
    count >= 2 && start == clauses.len().saturating_add(1)
}

fn neither_safety_control_restatement(words: &[&str]) -> bool {
    if words.first() != Some(&"neither") {
        return false;
    }
    let mut start = 1usize;
    let mut clauses = 0usize;
    for end in 1..=words.len() {
        if end < words.len() && words[end] != "nor" {
            continue;
        }
        if end == start
            || closed_positive_safety_control_meaning(&words[start..end])
                != Some(SafetyControlMeaning::WeakensControl)
        {
            return false;
        }
        clauses = clauses.saturating_add(1);
        start = end.saturating_add(1);
    }
    clauses >= 2 && start == words.len().saturating_add(1)
}

fn closed_positive_safety_control_meaning(words: &[&str]) -> Option<SafetyControlMeaning> {
    closed_actor_safety_control_meaning(words)
        .or_else(|| closed_active_actor_safety_control_meaning(words))
        .or_else(|| closed_inverted_actor_safety_control_meaning(words))
        .or_else(|| closed_passive_target_safety_control_meaning(words))
        .or_else(|| closed_configuration_safety_control_meaning(words))
        .or_else(|| closed_safety_control_result_meaning(words))
        .or_else(|| closed_safety_control_state_meaning(words))
        .or_else(|| closed_without_safety_control_meaning(words))
        .or_else(|| closed_inverted_subject_safety_control_meaning(words))
        .or_else(|| closed_subject_safety_control_meaning(words))
        .or_else(|| closed_safety_control_action_meaning(words, false))
        .or_else(|| closed_separable_turn_off_safety_control_meaning(words, false))
}

fn closed_inverted_actor_safety_control_meaning(words: &[&str]) -> Option<SafetyControlMeaning> {
    if !words
        .first()
        .is_some_and(|word| matches!(*word, "do" | "does" | "did"))
    {
        return None;
    }
    let mut index = 1usize;
    if words
        .get(index)
        .is_some_and(|word| PRESERVATION_DETERMINERS.contains(word))
    {
        index = index.saturating_add(1);
    }
    let actor_start = index;
    while index < words.len()
        && index.saturating_sub(actor_start) < 2
        && PRESERVATION_ACTOR_TERMS.contains(&words[index])
    {
        index = index.saturating_add(1);
    }
    if index == actor_start {
        return None;
    }
    let action_words = &words[index..];
    let action = safety_control_action(action_words)?;
    if !action.matches_gerund(false) {
        return None;
    }
    closed_safety_control_action_meaning(action_words, false)
}

fn coordinated_english_safety_control_restatement(words: &[&str]) -> bool {
    if single_english_safety_control_restatement(words) {
        return true;
    }
    words.iter().enumerate().any(|(index, word)| {
        *word == "and"
            && index > 0
            && index.saturating_add(1) < words.len()
            && single_english_safety_control_restatement(&words[..index])
            && coordinated_english_safety_control_restatement(&words[index.saturating_add(1)..])
    })
}

fn single_english_safety_control_restatement(words: &[&str]) -> bool {
    if restricted_safety_control_clause(words) {
        return false;
    }
    if preservation_wrapper_control_restatement(words) {
        return true;
    }
    if action_control_restatement(words) {
        return true;
    }
    if actor_cannot_control_restatement(words) {
        return true;
    }
    let positive = words
        .first()
        .is_some_and(|word| matches!(*word, "keep" | "maintain" | "preserve" | "retain"));
    let negative_bypass = words.starts_with(&["do", "not"])
        || words.starts_with(&["don", "t"])
        || words.first() == Some(&"never");
    let predicate_first = words
        .iter()
        .position(|word| matches!(*word, "remain" | "stay"));
    if positive {
        return closed_english_control_tail(&words[1..]);
    }
    if negative_bypass {
        let prefix = if words.first() == Some(&"never") {
            1
        } else {
            2
        };
        if negative_optional_control_restatement(&words[prefix..]) {
            return true;
        }
        return closed_english_preservation_sequence(&words[prefix..], None);
    }
    let Some(predicate_index) = predicate_first else {
        return false;
    };
    let predicate_tail =
        strip_positive_control_tail_suffix(&words[predicate_index.saturating_add(1)..]);
    predicate_index > 0
        && closed_english_control_tail(&words[..predicate_index])
        && predicate_tail
            .iter()
            .all(|word| matches!(*word, "active" | "enabled" | "enforced" | "intact"))
        && !predicate_tail.is_empty()
}

fn restricted_safety_control_clause(words: &[&str]) -> bool {
    words
        .iter()
        .any(|word| matches!(*word, "unless" | "except"))
        || words.windows(2).any(|pair| pair == ["only", "for"])
        || (words.last() == Some(&"only")
            && words[..words.len().saturating_sub(1)].contains(&"for"))
}

fn strip_english_control_request_prefix<'a>(words: &'a [&'a str]) -> &'a [&'a str] {
    let mut remaining = words;
    loop {
        if remaining.first() == Some(&"please") {
            remaining = &remaining[1..];
            continue;
        }
        if remaining.first() == Some(&"immediately") {
            remaining = &remaining[1..];
            continue;
        }
        if remaining.len() >= 2
            && matches!(remaining[0], "can" | "could" | "will" | "would")
            && remaining[1] == "you"
        {
            remaining = &remaining[2..];
            continue;
        }
        break;
    }
    remaining
}

fn preservation_wrapper_control_restatement(words: &[&str]) -> bool {
    let Some((mut remaining, gerund)) = strip_preservation_wrapper(words) else {
        return false;
    };
    if let Some(stripped) = strip_preservation_object_frame(remaining) {
        remaining = stripped;
    }
    closed_english_preservation_sequence(remaining, Some(gerund))
}

fn action_control_restatement(words: &[&str]) -> bool {
    if closed_actor_safety_control_preservation(words)
        || closed_without_safety_control_meaning(words)
            == Some(SafetyControlMeaning::PreservesControl)
        || closed_active_actor_safety_control_meaning(words)
            == Some(SafetyControlMeaning::PreservesControl)
        || closed_passive_target_safety_control_meaning(words)
            == Some(SafetyControlMeaning::PreservesControl)
        || closed_configuration_safety_control_meaning(words)
            == Some(SafetyControlMeaning::PreservesControl)
        || closed_inverted_subject_safety_control_meaning(words)
            == Some(SafetyControlMeaning::PreservesControl)
        || (!words
            .first()
            .is_some_and(|word| matches!(*word, "keep" | "maintain" | "preserve" | "retain"))
            && closed_safety_control_state_meaning(words)
                == Some(SafetyControlMeaning::PreservesControl))
        || closed_subject_safety_control_meaning(words)
            == Some(SafetyControlMeaning::PreservesControl)
    {
        return true;
    }
    let (words, negated) = if words.first() == Some(&"not") {
        (&words[1..], true)
    } else {
        (words, false)
    };
    closed_safety_control_action_meaning(words, negated)
        == Some(SafetyControlMeaning::PreservesControl)
        || closed_separable_turn_off_safety_control_meaning(words, negated)
            == Some(SafetyControlMeaning::PreservesControl)
}

fn actor_cannot_control_restatement(words: &[&str]) -> bool {
    if negative_actor_quantifier_control_restatement(words) {
        return true;
    }
    let mut index = usize::from(
        words
            .first()
            .is_some_and(|word| PRESERVATION_DETERMINERS.contains(word)),
    );
    let actor_start = index;
    while index < words.len()
        && index.saturating_sub(actor_start) < 2
        && PRESERVATION_ACTOR_TERMS.contains(&words[index])
    {
        index = index.saturating_add(1);
    }
    if index == actor_start {
        return false;
    }
    let Some(control_length) = actor_negative_modal_length(&words[index..]) else {
        return false;
    };
    closed_english_preservation_sequence(&words[index.saturating_add(control_length)..], None)
}

fn negative_actor_quantifier_control_restatement(words: &[&str]) -> bool {
    let modal = if words
        .first()
        .is_some_and(|word| matches!(*word, "nobody" | "none"))
    {
        1
    } else if words.first() == Some(&"no")
        && words
            .get(1)
            .is_some_and(|word| *word == "one" || PRESERVATION_ACTOR_TERMS.contains(word))
    {
        2
    } else {
        return false;
    };
    if !words
        .get(modal)
        .is_some_and(|word| matches!(*word, "can" | "could" | "may" | "might"))
    {
        return false;
    }
    closed_english_preservation_sequence(&words[modal.saturating_add(1)..], None)
}

fn actor_negative_modal_length(words: &[&str]) -> Option<usize> {
    if words
        .first()
        .is_some_and(|word| matches!(*word, "cannot" | "can't" | "cant" | "can’t"))
    {
        return Some(1);
    }
    (["can", "may", "must", "should"].contains(words.first()?)
        && words
            .get(1)
            .is_some_and(|word| matches!(*word, "not" | "t")))
    .then_some(2)
}

fn closed_english_preservation_sequence(words: &[&str], required_gerund: Option<bool>) -> bool {
    let words = strip_preservation_modifiers(words);
    let next = words.iter().enumerate().find_map(|(index, word)| {
        matches!(*word, "and" | "or")
            .then(|| strip_preservation_modifiers(&words[index.saturating_add(1)..]))
            .filter(|continuation| starts_preservation_action(continuation, required_gerund))
            .map(|_| index)
    });
    let Some(next) = next else {
        return closed_single_english_preservation(words, required_gerund);
    };
    closed_single_english_preservation(&words[..next], required_gerund)
        && closed_english_preservation_sequence(&words[next.saturating_add(1)..], required_gerund)
}

fn starts_preservation_action(words: &[&str], required_gerund: Option<bool>) -> bool {
    safety_control_action(words).is_some_and(|action| {
        action.effect == SafetyControlActionEffect::WeakensControl
            && required_gerund.is_none_or(|gerund| action.matches_gerund(gerund))
    }) || (words.first() == Some(&"turn") && required_gerund.is_none_or(|gerund| !gerund))
}

fn closed_single_english_preservation(words: &[&str], required_gerund: Option<bool>) -> bool {
    if let Some(action) = safety_control_action(words).filter(|action| {
        action.effect == SafetyControlActionEffect::WeakensControl
            && required_gerund.is_none_or(|gerund| action.matches_gerund(gerund))
    }) {
        return closed_english_preservation_tail(&words[action.length..]);
    }
    if words.first() != Some(&"turn") || required_gerund.is_some_and(|gerund| gerund) {
        return false;
    }
    let target = strip_safety_control_target_modifiers(&words[1..]);
    let Some(target_length) = safety_control_target_length(target) else {
        return false;
    };
    target.get(target_length) == Some(&"off")
        && (target.len() == target_length.saturating_add(1)
            || closed_safety_control_scope(&target[target_length.saturating_add(1)..]))
}

fn strip_preservation_wrapper<'a>(words: &'a [&'a str]) -> Option<(&'a [&'a str], bool)> {
    if words.starts_with(&["refuse", "to"]) {
        return Some((&words[2..], false));
    }
    words.first().and_then(|wrapper| {
        PRESERVATION_PREFIX_NEGATIONS
            .iter()
            .filter_map(|candidate| candidate.split_whitespace().next())
            .any(|candidate| candidate == *wrapper)
            .then_some((&words[1..], true))
    })
}

fn strip_preservation_object_frame<'a>(words: &'a [&'a str]) -> Option<&'a [&'a str]> {
    let mut index = usize::from(
        words
            .first()
            .is_some_and(|word| PRESERVATION_DETERMINERS.contains(word)),
    );
    let start = index;
    while index < words.len()
        && index.saturating_sub(start) < 2
        && PRESERVATION_ACTOR_TERMS.contains(&words[index])
    {
        index = index.saturating_add(1);
    }
    (index > start && words.get(index) == Some(&"from"))
        .then_some(&words[index.saturating_add(1)..])
}

fn strip_preservation_modifiers<'a>(mut words: &'a [&'a str]) -> &'a [&'a str] {
    while words
        .first()
        .is_some_and(|word| ACTION_NEGATION_MODIFIERS.contains(word))
    {
        words = &words[1..];
    }
    words
}

fn negative_optional_control_restatement(words: &[&str]) -> bool {
    if words.first() != Some(&"make") {
        return false;
    }
    let words = strip_safety_control_target_modifiers(&words[1..]);
    let Some(target_length) = safety_control_target_length(words) else {
        return false;
    };
    let remainder = &words[target_length..];
    remainder.first() == Some(&"optional")
        && (remainder.len() == 1 || closed_safety_control_scope(&remainder[1..]))
}

fn closed_english_control_tail(words: &[&str]) -> bool {
    let words = strip_positive_control_tail_suffix(words);
    closed_english_control_words(words) && closed_english_control_targets(words)
}

fn strip_positive_control_tail_suffix<'a>(mut words: &'a [&'a str]) -> &'a [&'a str] {
    loop {
        let length = if words.ends_with(&["right", "away"]) || words.ends_with(&["right", "now"]) {
            2
        } else if words
            .last()
            .is_some_and(|word| matches!(*word, "immediately" | "now" | "please"))
        {
            1
        } else {
            return words;
        };
        words = &words[..words.len().saturating_sub(length)];
    }
}

fn closed_english_preservation_tail(words: &[&str]) -> bool {
    closed_safety_control_tail(words)
}

fn closed_english_control_words(words: &[&str]) -> bool {
    !words.is_empty()
        && words.iter().all(|word| {
            matches!(
                *word,
                "active"
                    | "all"
                    | "and"
                    | "approval"
                    | "approvals"
                    | "check"
                    | "checks"
                    | "design"
                    | "enabled"
                    | "enforced"
                    | "gate"
                    | "gates"
                    | "in"
                    | "intact"
                    | "place"
                    | "preview"
                    | "safeguard"
                    | "safeguards"
                    | "safety"
                    | "the"
                    | "user"
                    | "validation"
            )
        })
}

fn closed_english_control_targets(words: &[&str]) -> bool {
    let validation_and_preview = words.contains(&"validation") && words.contains(&"preview");
    let explicit_user_approval = words.contains(&"approval") && words.contains(&"user");
    validation_and_preview || explicit_user_approval
}

fn korean_safety_control_restatement(words: &[String]) -> bool {
    let words = words.iter().map(String::as_str).collect::<Vec<_>>();
    if words.iter().any(|word| {
        [
            "검증만",
            "미리보기만",
            "승인만",
            "안전게이트만",
            "안전장치만",
            "보호장치만",
        ]
        .contains(word)
    }) {
        return false;
    }
    closed_korean_safety_control_clause(&words)
        == Some(KoreanSafetyControlClause::Control(
            SafetyControlMeaning::PreservesControl,
        ))
}
