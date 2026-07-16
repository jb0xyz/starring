use super::core::*;
use super::lexicon::*;

pub(in crate::turn) fn closed_safety_control_action_meaning(
    words: &[&str],
    mut negated: bool,
) -> Option<SafetyControlMeaning> {
    let action = safety_control_action(words)?;
    let mut tail_words = &words[action.length..];
    if action.effect == SafetyControlActionEffect::EnforcesControl
        && tail_words.first() == Some(&"no")
    {
        negated = !negated;
        tail_words = &tail_words[1..];
    }
    let tail = closed_safety_control_action_tail(tail_words)?;
    Some(safety_control_action_effect_meaning(
        action.effect,
        tail,
        negated,
    ))
}

pub(in crate::turn) fn closed_separable_turn_off_safety_control_meaning(
    words: &[&str],
    negated: bool,
) -> Option<SafetyControlMeaning> {
    let tail = separable_turn_off_safety_control_tail(words)?;
    Some(safety_control_action_effect_meaning(
        SafetyControlActionEffect::WeakensControl,
        tail,
        negated,
    ))
}

pub(in crate::turn) fn closed_direct_separable_turn_off_action(words: &[&str]) -> bool {
    separable_turn_off_safety_control_tail(words) == Some(SafetyControlTailEffect::Direct)
}

fn separable_turn_off_safety_control_tail(words: &[&str]) -> Option<SafetyControlTailEffect> {
    if !words
        .first()
        .is_some_and(|word| matches!(*word, "turn" | "turning" | "turns"))
    {
        return None;
    }
    let target = strip_safety_control_target_modifiers(&words[1..]);
    let target_length = safety_control_target_length(target)?;
    if target.get(target_length) != Some(&"off") {
        return None;
    }
    closed_safety_control_governance_tail(&target[target_length.saturating_add(1)..])
}

pub(in crate::turn) fn safety_control_action_effect_meaning(
    effect: SafetyControlActionEffect,
    tail: SafetyControlTailEffect,
    negated: bool,
) -> SafetyControlMeaning {
    let weakens = match (effect, tail) {
        (
            SafetyControlActionEffect::WeakensControl,
            SafetyControlTailEffect::Direct | SafetyControlTailEffect::Permitted,
        ) => !negated,
        (SafetyControlActionEffect::WeakensControl, SafetyControlTailEffect::Prohibited) => negated,
        (
            SafetyControlActionEffect::EnforcesControl,
            SafetyControlTailEffect::Direct | SafetyControlTailEffect::Permitted,
        ) => negated,
        (SafetyControlActionEffect::EnforcesControl, SafetyControlTailEffect::Prohibited) => {
            !negated
        }
    };
    if weakens {
        SafetyControlMeaning::WeakensControl
    } else {
        SafetyControlMeaning::PreservesControl
    }
}

pub(in crate::turn) fn closed_safety_control_action_tail(
    words: &[&str],
) -> Option<SafetyControlTailEffect> {
    let remainder = safety_control_targets_remainder(words)?;
    closed_safety_control_governance_tail(remainder)
}

fn closed_safety_control_governance_tail(words: &[&str]) -> Option<SafetyControlTailEffect> {
    let remainder = strip_safety_control_tail_prefix(words);
    if remainder.is_empty() || closed_safety_control_scope(remainder) {
        return Some(SafetyControlTailEffect::Direct);
    }
    if closed_preservation_prohibition_tail(remainder) {
        return Some(SafetyControlTailEffect::Prohibited);
    }
    closed_action_permission_tail(remainder).then_some(SafetyControlTailEffect::Permitted)
}

pub(in crate::turn) fn closed_subject_safety_control_meaning(
    words: &[&str],
) -> Option<SafetyControlMeaning> {
    let words = strip_safety_control_tail_suffix(words);
    if words.first() == Some(&"no") {
        let remainder = safety_control_targets_remainder(&words[1..])?;
        let length = if remainder.starts_with(&["is", "required"]) {
            2
        } else if remainder.first() == Some(&"required") {
            1
        } else {
            return None;
        };
        return closed_subject_predicate_remainder(&remainder[length..])
            .then_some(SafetyControlMeaning::WeakensControl);
    }
    let remainder = safety_control_targets_remainder(words)?;
    let (length, meaning) = subject_safety_control_predicate(remainder)?;
    closed_subject_predicate_remainder(&remainder[length..]).then_some(meaning)
}

pub(in crate::turn) fn closed_inverted_subject_safety_control_meaning(
    words: &[&str],
) -> Option<SafetyControlMeaning> {
    let words = if words.starts_with(&["not", "only"]) || words.starts_with(&["not", "just"]) {
        &words[2..]
    } else {
        words
    };
    let predicate_head = *words.first()?;
    if !matches!(
        predicate_head,
        "are" | "can" | "could" | "is" | "may" | "might" | "must" | "should" | "will" | "would"
    ) {
        return None;
    }
    let remainder = safety_control_targets_remainder(&words[1..])?;
    let mut predicate = Vec::with_capacity(remainder.len().saturating_add(1));
    predicate.push(predicate_head);
    predicate.extend_from_slice(remainder);
    let (length, meaning) = subject_safety_control_predicate(&predicate)?;
    (length == predicate.len()).then_some(meaning)
}

pub(in crate::turn) fn closed_safety_control_state_meaning(
    words: &[&str],
) -> Option<SafetyControlMeaning> {
    let words = strip_safety_control_tail_suffix(words);
    if words
        .first()
        .is_some_and(|word| matches!(*word, "keep" | "maintain" | "preserve" | "retain"))
    {
        let remainder = safety_control_targets_remainder(&words[1..])?;
        return closed_state_predicate(remainder);
    }
    let remainder = safety_control_targets_remainder(words)?;
    for prefix in [
        &["remain"][..],
        &["remains"][..],
        &["stay"][..],
        &["stays"][..],
        &["continue", "to", "be"][..],
        &["continues", "to", "be"][..],
    ] {
        if remainder.starts_with(prefix) {
            return closed_state_predicate(&remainder[prefix.len()..]);
        }
    }
    None
}

pub(in crate::turn) fn closed_configuration_safety_control_meaning(
    words: &[&str],
) -> Option<SafetyControlMeaning> {
    let (words, outer_negated) = strip_command_outer_negation(words);
    let action = *words.first()?;
    if !matches!(action, "configure" | "make" | "set") {
        return None;
    }
    let remainder = safety_control_targets_remainder(&words[1..])?;
    let state = match action {
        "make" => closed_configuration_state(remainder),
        "configure" => remainder
            .strip_prefix(&["to"])
            .and_then(closed_configuration_be_state),
        "set" => remainder
            .strip_prefix(&["to"])
            .or_else(|| remainder.strip_prefix(&["as"]))
            .and_then(|state| {
                closed_configuration_state(state).or_else(|| closed_configuration_be_state(state))
            }),
        _ => None,
    }?;
    Some(if outer_negated {
        invert_safety_control_meaning(state)
    } else {
        state
    })
}

fn strip_command_outer_negation<'a>(words: &'a [&'a str]) -> (&'a [&'a str], bool) {
    if words.starts_with(&["do", "not"]) || words.starts_with(&["don", "t"]) {
        return (&words[2..], true);
    }
    if words
        .first()
        .is_some_and(|word| matches!(*word, "don't" | "dont" | "don’t" | "never"))
    {
        return (&words[1..], true);
    }
    (words, false)
}

fn closed_configuration_be_state(words: &[&str]) -> Option<SafetyControlMeaning> {
    let words = words.strip_prefix(&["be"]).unwrap_or(words);
    if let Some(state) = words.strip_prefix(&["not", "be"]) {
        return closed_configuration_state(state).map(invert_safety_control_meaning);
    }
    closed_configuration_state(words)
}

fn closed_configuration_state(words: &[&str]) -> Option<SafetyControlMeaning> {
    match words {
        ["active" | "enabled" | "enforced" | "required"] => {
            Some(SafetyControlMeaning::PreservesControl)
        }
        ["disabled" | "off" | "optional"] | ["turned", "off"] => {
            Some(SafetyControlMeaning::WeakensControl)
        }
        ["not", "active" | "enabled" | "enforced" | "required"]
        | ["no", "longer", "active" | "enabled" | "enforced" | "required"] => {
            Some(SafetyControlMeaning::WeakensControl)
        }
        ["not", "disabled" | "off" | "optional"] => Some(SafetyControlMeaning::PreservesControl),
        _ => None,
    }
}

fn closed_state_predicate(words: &[&str]) -> Option<SafetyControlMeaning> {
    let (length, meaning) = safety_control_state_expression(words)?;
    closed_subject_predicate_remainder(&words[length..]).then_some(meaning)
}

fn safety_control_state_expression(words: &[&str]) -> Option<(usize, SafetyControlMeaning)> {
    let (prefix_length, negated) = if words.starts_with(&["no", "longer"]) {
        (2, true)
    } else if words.first() == Some(&"not") {
        (1, true)
    } else {
        (0, false)
    };
    let (state_length, meaning) = safety_control_state_atom(&words[prefix_length..])?;
    Some((
        prefix_length.saturating_add(state_length),
        if negated {
            invert_safety_control_meaning(meaning)
        } else {
            meaning
        },
    ))
}

fn safety_control_state_atom(words: &[&str]) -> Option<(usize, SafetyControlMeaning)> {
    let preserved = SafetyControlMeaning::PreservesControl;
    let weakened = SafetyControlMeaning::WeakensControl;
    match words {
        ["active" | "enabled" | "enforced" | "intact" | "needed" | "required", ..] => {
            Some((1, preserved))
        }
        ["disabled" | "off" | "optional", ..] => Some((1, weakened)),
        ["turned", "off", ..] => Some((2, weakened)),
        _ => None,
    }
}

fn copular_safety_control_state_predicate(words: &[&str]) -> Option<(usize, SafetyControlMeaning)> {
    if words
        .first()
        .is_some_and(|word| matches!(*word, "is" | "are"))
    {
        let (length, meaning) = safety_control_state_expression(&words[1..])?;
        return Some((length.saturating_add(1), meaning));
    }
    let prefix_length = if words.first().is_some_and(|word| {
        matches!(
            *word,
            "isn't" | "isnt" | "isn’t" | "aren't" | "arent" | "aren’t"
        )
    }) {
        1
    } else if matches!(words, ["isn" | "aren", "t", ..]) {
        2
    } else {
        return None;
    };
    let (length, meaning) = safety_control_state_atom(&words[prefix_length..])?;
    Some((
        prefix_length.saturating_add(length),
        invert_safety_control_meaning(meaning),
    ))
}

pub(in crate::turn) fn closed_passive_target_safety_control_meaning(
    words: &[&str],
) -> Option<SafetyControlMeaning> {
    let (words, outer_negated) = if words.starts_with(&["do", "not"]) {
        (&words[2..], true)
    } else if words.first() == Some(&"not") {
        (&words[1..], true)
    } else {
        (words, false)
    };
    if !words
        .first()
        .is_some_and(|word| matches!(*word, "prevent" | "stop"))
    {
        return None;
    }
    let remainder = safety_control_targets_remainder(&words[1..])?;
    if !remainder.starts_with(&["from", "being"]) {
        return None;
    }
    let inner = closed_state_predicate(&remainder[2..])?;
    Some(apply_outer_process_negation(
        invert_safety_control_meaning(inner),
        outer_negated,
    ))
}

pub(in crate::turn) fn closed_active_actor_safety_control_meaning(
    words: &[&str],
) -> Option<SafetyControlMeaning> {
    if let Some(meaning) = closed_without_safety_control_meaning(words) {
        return Some(meaning);
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
        return None;
    }
    let predicate = &words[index..];
    let (action_words, negated, expected_form) = if predicate.first() == Some(&"never") {
        (&predicate[1..], true, SafetyControlActionForm::ThirdPerson)
    } else if let Some(length) = active_actor_negative_auxiliary_length(predicate) {
        (
            &predicate[length..],
            true,
            SafetyControlActionForm::Infinitive,
        )
    } else if predicate.len() >= 2 && safety_control_modal(predicate[0]) && predicate[1] == "not" {
        (&predicate[2..], true, SafetyControlActionForm::Infinitive)
    } else if predicate
        .first()
        .is_some_and(|word| safety_control_modal(word))
    {
        (&predicate[1..], false, SafetyControlActionForm::Infinitive)
    } else {
        (predicate, false, SafetyControlActionForm::ThirdPerson)
    };
    let action = safety_control_action(action_words)?;
    if action.form != expected_form {
        return None;
    }
    closed_safety_control_action_meaning(action_words, negated)
}

fn active_actor_negative_auxiliary_length(words: &[&str]) -> Option<usize> {
    if words.first().is_some_and(|word| {
        matches!(
            *word,
            "doesn't" | "doesnt" | "doesn’t" | "didn't" | "didnt" | "didn’t"
        )
    }) {
        return Some(1);
    }
    (words.starts_with(&["does", "not"])
        || words.starts_with(&["did", "not"])
        || words.starts_with(&["doesn", "t"])
        || words.starts_with(&["didn", "t"]))
    .then_some(2)
}

pub(in crate::turn) fn closed_actor_safety_control_meaning(
    words: &[&str],
) -> Option<SafetyControlMeaning> {
    if let Some(meaning) = closed_governed_actor_safety_control_meaning(words) {
        return Some(meaning);
    }
    for (frame, gerund, prohibited) in [
        (&["are", "blocked", "from"][..], true, true),
        (&["are", "prohibited", "from"][..], true, true),
        (&["are", "forbidden", "from"][..], true, true),
        (&["are", "disallowed", "from"][..], true, true),
        (&["are", "prevented", "from"][..], true, true),
        (&["are", "not", "prohibited", "from"][..], true, false),
        (&["are", "not", "forbidden", "from"][..], true, false),
        (&["are", "not", "blocked", "from"][..], true, false),
        (&["are", "forbidden", "to"][..], false, true),
        (&["are", "prohibited", "to"][..], false, true),
        (&["are", "not", "allowed", "to"][..], false, true),
        (&["are", "allowed", "to"][..], false, false),
        (&["are", "permitted", "to"][..], false, false),
        (&["is", "blocked", "from"][..], true, true),
        (&["is", "prohibited", "from"][..], true, true),
        (&["is", "forbidden", "from"][..], true, true),
        (&["is", "disallowed", "from"][..], true, true),
        (&["is", "prevented", "from"][..], true, true),
        (&["is", "not", "prohibited", "from"][..], true, false),
        (&["is", "not", "forbidden", "from"][..], true, false),
        (&["is", "not", "blocked", "from"][..], true, false),
        (&["is", "forbidden", "to"][..], false, true),
        (&["is", "prohibited", "to"][..], false, true),
        (&["is", "not", "allowed", "to"][..], false, true),
        (&["is", "allowed", "to"][..], false, false),
        (&["is", "permitted", "to"][..], false, false),
    ] {
        let Some(frame_start) = words
            .windows(frame.len())
            .position(|candidate| candidate == frame)
        else {
            continue;
        };
        if !closed_preservation_actor(&words[..frame_start]) {
            continue;
        }
        let action_words = &words[frame_start.saturating_add(frame.len())..];
        let Some(inner) = closed_governed_safety_control_action(action_words, gerund) else {
            continue;
        };
        return Some(if prohibited {
            invert_safety_control_meaning(inner)
        } else {
            inner
        });
    }
    None
}

fn closed_governed_actor_safety_control_meaning(words: &[&str]) -> Option<SafetyControlMeaning> {
    let (words, outer_negated) = strip_command_outer_negation(words);
    let (connector, gerund, prohibited) = match words.first()? {
        &"block" | &"prohibit" | &"prevent" | &"stop" => ("from", true, true),
        &"allow" | &"permit" => ("to", false, false),
        _ => return None,
    };
    let connector_index = words[1..]
        .iter()
        .position(|word| *word == connector)?
        .saturating_add(1);
    if !closed_preservation_actor(&words[1..connector_index]) {
        return None;
    }
    let inner =
        closed_governed_safety_control_action(&words[connector_index.saturating_add(1)..], gerund)?;
    let governed = if prohibited {
        invert_safety_control_meaning(inner)
    } else {
        inner
    };
    Some(apply_outer_process_negation(governed, outer_negated))
}

fn closed_governed_safety_control_action(
    action_words: &[&str],
    gerund: bool,
) -> Option<SafetyControlMeaning> {
    if let Some(meaning) = closed_without_safety_control_meaning(action_words) {
        let process_words = action_words.strip_prefix(&["not"]).unwrap_or(action_words);
        return process_words
            .first()
            .is_some_and(|process| process.ends_with("ing") == gerund)
            .then_some(meaning);
    }
    let (action_words, negated) = optional_action_negation(action_words);
    let action = safety_control_action(action_words)?;
    if !action.matches_gerund(gerund)
        || closed_safety_control_action_tail(&action_words[action.length..])
            != Some(SafetyControlTailEffect::Direct)
    {
        return None;
    }
    closed_safety_control_action_meaning(action_words, negated)
}

pub(in crate::turn) fn closed_actor_safety_control_preservation(words: &[&str]) -> bool {
    closed_actor_safety_control_meaning(words) == Some(SafetyControlMeaning::PreservesControl)
}

fn invert_safety_control_meaning(meaning: SafetyControlMeaning) -> SafetyControlMeaning {
    match meaning {
        SafetyControlMeaning::PreservesControl => SafetyControlMeaning::WeakensControl,
        SafetyControlMeaning::WeakensControl => SafetyControlMeaning::PreservesControl,
    }
}

fn optional_action_negation<'a>(words: &'a [&'a str]) -> (&'a [&'a str], bool) {
    if words.first() == Some(&"not") {
        (&words[1..], true)
    } else {
        (words, false)
    }
}

fn complement_safety_control_meaning(words: &[&str]) -> Option<SafetyControlMeaning> {
    if closed_safety_control_tail(words) {
        return Some(SafetyControlMeaning::WeakensControl);
    }
    let (action_words, negated) = optional_action_negation(words);
    closed_safety_control_action_meaning(action_words, negated).map(invert_safety_control_meaning)
}

fn apply_outer_process_negation(
    meaning: SafetyControlMeaning,
    negated: bool,
) -> SafetyControlMeaning {
    if negated {
        invert_safety_control_meaning(meaning)
    } else {
        meaning
    }
}

fn closed_without_complement_meaning(words: &[&str]) -> Option<SafetyControlMeaning> {
    let (words, negated) = optional_action_negation(words);
    if negated {
        if closed_safety_control_tail(words) {
            return Some(SafetyControlMeaning::PreservesControl);
        }
        return closed_safety_control_action_meaning(words, true)
            .map(invert_safety_control_meaning);
    }
    complement_safety_control_meaning(words)
}

fn closed_preservation_actor(words: &[&str]) -> bool {
    if words == ["it"] {
        return true;
    }
    let words = if words
        .first()
        .is_some_and(|word| PRESERVATION_DETERMINERS.contains(word))
    {
        &words[1..]
    } else {
        words
    };
    (1..=2).contains(&words.len())
        && words
            .iter()
            .all(|word| PRESERVATION_ACTOR_TERMS.contains(word))
}

pub(in crate::turn) fn closed_without_safety_control_meaning(
    words: &[&str],
) -> Option<SafetyControlMeaning> {
    let mut occurrences = words
        .iter()
        .enumerate()
        .filter(|(_, word)| **word == "without");
    let without = occurrences.next()?.0;
    if occurrences.next().is_some() {
        return None;
    }
    let outer_negated = closed_without_process_prefix(&words[..without])?;
    let complement = &words[without.saturating_add(1)..];
    let complement_meaning = closed_without_complement_meaning(complement)?;
    Some(apply_outer_process_negation(
        complement_meaning,
        outer_negated,
    ))
}

fn closed_without_process_prefix(words: &[&str]) -> Option<bool> {
    let words = if words.first() == Some(&"please") {
        &words[1..]
    } else {
        words
    };
    for control in [
        &["do", "not"][..],
        &["don't"][..],
        &["dont"][..],
        &["don’t"][..],
        &["never"][..],
        &["must", "not"][..],
        &["should", "not"][..],
    ] {
        if words.starts_with(control) && closed_without_process_action(&words[control.len()..]) {
            return Some(true);
        }
    }
    if closed_without_process_action(words) {
        return Some(false);
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
        return None;
    }
    let modal = &words[index..];
    if let Some(length) = active_actor_negative_auxiliary_length(modal) {
        if closed_without_process_action(&modal[length..]) {
            return Some(true);
        }
    }
    for control in [
        &["cannot"][..],
        &["can't"][..],
        &["cant"][..],
        &["can’t"][..],
        &["can", "not"][..],
        &["may", "not"][..],
        &["must", "not"][..],
        &["should", "not"][..],
    ] {
        if modal.starts_with(control) && closed_without_process_action(&modal[control.len()..]) {
            return Some(true);
        }
    }
    for control in ["can", "may", "must", "should"] {
        if modal.first() == Some(&control) && closed_without_process_action(&modal[1..]) {
            return Some(false);
        }
    }
    closed_without_process_action(modal).then_some(false)
}

fn closed_without_process_action(words: &[&str]) -> bool {
    matches!(
        words,
        ["apply"
            | "applies"
            | "applying"
            | "continue"
            | "continues"
            | "continuing"
            | "deploy"
            | "deploys"
            | "deploying"
            | "execute"
            | "executes"
            | "executing"
            | "operate"
            | "operates"
            | "operating"
            | "proceed"
            | "proceeds"
            | "proceeding"
            | "run"
            | "runs"
            | "running"]
    )
}

fn safety_control_targets_remainder<'a>(words: &'a [&'a str]) -> Option<&'a [&'a str]> {
    let words = strip_safety_control_tail_suffix(words);
    let words = strip_safety_control_target_modifiers(words);
    let mut consumed = safety_control_target_length(words)?;
    while words.get(consumed) == Some(&"and") {
        let next = strip_safety_control_target_modifiers(&words[consumed.saturating_add(1)..]);
        let modifiers = words[consumed.saturating_add(1)..]
            .len()
            .saturating_sub(next.len());
        let next_length = safety_control_target_length(next)?;
        consumed = consumed
            .saturating_add(1)
            .saturating_add(modifiers)
            .saturating_add(next_length);
    }
    Some(&words[consumed..])
}

fn subject_safety_control_predicate(words: &[&str]) -> Option<(usize, SafetyControlMeaning)> {
    let preserved = SafetyControlMeaning::PreservesControl;
    let weakened = SafetyControlMeaning::WeakensControl;
    if let Some(state) = copular_safety_control_state_predicate(words) {
        return Some(state);
    }
    if let Some((prefix_length, negated)) = modal_be_prefix(words) {
        let predicate = &words[prefix_length..];
        let (predicate_length, positive_meaning) = if predicate.first() == Some(&"optional") {
            (1, weakened)
        } else if predicate
            .first()
            .is_some_and(|word| matches!(*word, "enabled" | "needed" | "required"))
        {
            (1, preserved)
        } else if predicate.starts_with(&["turned", "off"]) {
            (2, weakened)
        } else if predicate.first().is_some_and(|word| {
            matches!(
                *word,
                "bypassed" | "disabled" | "ignored" | "omitted" | "removed" | "skipped"
            )
        }) {
            (1, weakened)
        } else {
            return None;
        };
        let meaning = if negated {
            match positive_meaning {
                SafetyControlMeaning::PreservesControl => weakened,
                SafetyControlMeaning::WeakensControl => preserved,
            }
        } else {
            positive_meaning
        };
        return Some((prefix_length.saturating_add(predicate_length), meaning));
    }
    if let Some((prefix_length, negated)) = get_passive_prefix(words) {
        let (predicate_length, positive_meaning) =
            get_passive_safety_control_predicate(&words[prefix_length..])?;
        return Some((
            prefix_length.saturating_add(predicate_length),
            if negated {
                invert_safety_control_meaning(positive_meaning)
            } else {
                positive_meaning
            },
        ));
    }
    for (prefix, negated) in [
        (&["is", "not"][..], true),
        (&["are", "not"][..], true),
        (&["isn't"][..], true),
        (&["isnt"][..], true),
        (&["isn’t"][..], true),
        (&["aren't"][..], true),
        (&["arent"][..], true),
        (&["aren’t"][..], true),
        (&["is"][..], false),
        (&["are"][..], false),
    ] {
        if !words.starts_with(prefix) {
            continue;
        }
        let action = &words[prefix.len()..];
        let action_length = if action.starts_with(&["turned", "off"]) {
            2
        } else if action.first().is_some_and(|word| {
            matches!(
                *word,
                "bypassed" | "disabled" | "ignored" | "omitted" | "removed" | "skipped"
            )
        }) {
            1
        } else {
            continue;
        };
        return Some((
            prefix.len().saturating_add(action_length),
            if negated { preserved } else { weakened },
        ));
    }
    None
}

fn modal_be_prefix(words: &[&str]) -> Option<(usize, bool)> {
    if matches!(words, ["cannot" | "can't" | "cant" | "can’t", "be", ..]) {
        return Some((2, true));
    }
    if words.len() >= 3 && safety_control_modal(words[0]) && words[1..3] == ["not", "be"] {
        return Some((3, true));
    }
    (words.len() >= 2 && safety_control_modal(words[0]) && words[1] == "be").then_some((2, false))
}

fn get_passive_prefix(words: &[&str]) -> Option<(usize, bool)> {
    if words.len() >= 2
        && words
            .first()
            .is_some_and(|word| matches!(*word, "cannot" | "can't" | "cant" | "can’t"))
        && words[1] == "get"
    {
        return Some((2, true));
    }
    if words.len() >= 3 && safety_control_modal(words[0]) && words[1] == "not" && words[2] == "get"
    {
        return Some((3, true));
    }
    if words.len() >= 2 && safety_control_modal(words[0]) && words[1] == "get" {
        return Some((2, false));
    }
    if words.len() >= 3
        && matches!(words[0], "do" | "does" | "did")
        && words[1] == "not"
        && words[2] == "get"
    {
        return Some((3, true));
    }
    if words.len() >= 2
        && matches!(
            words[0],
            "don't"
                | "dont"
                | "don’t"
                | "doesn't"
                | "doesnt"
                | "doesn’t"
                | "didn't"
                | "didnt"
                | "didn’t"
        )
        && words[1] == "get"
    {
        return Some((2, true));
    }
    if words.len() >= 2 && words[0] == "never" && matches!(words[1], "get" | "gets") {
        return Some((2, true));
    }
    words
        .first()
        .is_some_and(|word| matches!(*word, "get" | "gets" | "got"))
        .then_some((1, false))
}

fn get_passive_safety_control_predicate(words: &[&str]) -> Option<(usize, SafetyControlMeaning)> {
    if words.starts_with(&["turned", "off"]) {
        return Some((2, SafetyControlMeaning::WeakensControl));
    }
    words
        .first()
        .is_some_and(|word| {
            matches!(
                *word,
                "bypassed" | "disabled" | "ignored" | "omitted" | "removed" | "skipped"
            )
        })
        .then_some((1, SafetyControlMeaning::WeakensControl))
}

fn closed_subject_predicate_remainder(words: &[&str]) -> bool {
    let words = strip_safety_control_tail_prefix(words);
    words.is_empty() || closed_safety_control_scope(words)
}
