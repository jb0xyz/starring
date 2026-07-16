use super::lexicon::*;

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static TAIL_PREFIX_STEPS: Cell<usize> = const { Cell::new(0) };
}

pub(in crate::turn) fn safety_control_target_length(words: &[&str]) -> Option<usize> {
    CLOSED_SAFETY_CONTROL_TARGETS
        .iter()
        .filter_map(|target| {
            let length = target.split_whitespace().count();
            (words.len() >= length
                && words[..length]
                    .iter()
                    .copied()
                    .eq(target.split_whitespace()))
            .then_some(length)
        })
        .max()
}

pub(in crate::turn) fn strip_safety_control_target_modifiers<'a>(
    mut words: &'a [&'a str],
) -> &'a [&'a str] {
    while words
        .first()
        .is_some_and(|word| SAFETY_CONTROL_TARGET_MODIFIERS.contains(word))
    {
        words = &words[1..];
    }
    words
}

pub(in crate::turn) fn closed_safety_control_scope(words: &[&str]) -> bool {
    let Some((preposition, scope)) = words.split_first() else {
        return false;
    };
    CLOSED_SAFETY_CONTROL_SCOPE_PREPOSITIONS.contains(preposition)
        && (1..=4).contains(&scope.len())
        && scope
            .iter()
            .all(|word| CLOSED_SAFETY_CONTROL_SCOPE_TERMS.contains(word))
        && scope
            .iter()
            .any(|word| CLOSED_SAFETY_CONTROL_SCOPE_HEADS.contains(word))
}

pub(in crate::turn) fn closed_safety_control_tail(words: &[&str]) -> bool {
    let words = strip_safety_control_tail_suffix(words);
    let words = strip_safety_control_target_modifiers(words);
    let Some(target_length) = safety_control_target_length(words) else {
        return false;
    };
    let mut remainder = &words[target_length..];
    while remainder.first() == Some(&"and") {
        let next = strip_safety_control_target_modifiers(&remainder[1..]);
        let Some(next_length) = safety_control_target_length(next) else {
            return false;
        };
        remainder = &next[next_length..];
    }
    let remainder = strip_safety_control_tail_prefix(remainder);
    remainder.is_empty() || closed_safety_control_scope(remainder)
}

pub(in crate::turn) fn preservation_prohibition_length(words: &[&str]) -> Option<usize> {
    if matches!(
        words,
        [
            "is",
            "denied" | "disabled" | "disallowed" | "forbidden" | "prohibited",
            ..
        ] | ["isn't" | "isnt" | "isn’t", "allowed" | "permitted", ..]
    ) {
        return Some(2);
    }
    if words.len() >= 3
        && safety_control_modal(words[0])
        && words[1] == "be"
        && matches!(
            words[2],
            "denied" | "disabled" | "disallowed" | "forbidden" | "prohibited"
        )
    {
        return Some(3);
    }
    if words.len() >= 4
        && safety_control_modal(words[0])
        && words[1..3] == ["not", "be"]
        && matches!(words[3], "allowed" | "enabled" | "permitted")
    {
        return Some(4);
    }
    (words.starts_with(&["is", "not", "allowed"])
        || words.starts_with(&["is", "not", "permitted"])
        || words.starts_with(&["isn", "t", "allowed"])
        || words.starts_with(&["isn", "t", "permitted"]))
    .then_some(3)
}

pub(in crate::turn) fn closed_preservation_prohibition_tail(words: &[&str]) -> bool {
    preservation_prohibition_length(words).is_some_and(|length| {
        let remainder = strip_safety_control_tail_prefix(&words[length..]);
        remainder.is_empty() || closed_safety_control_scope(remainder)
    })
}

pub(in crate::turn) fn action_permission_length(words: &[&str]) -> Option<usize> {
    if matches!(words, ["is", "allowed" | "enabled" | "permitted", ..]) {
        return Some(2);
    }
    if words.len() >= 3
        && safety_control_modal(words[0])
        && words[1] == "be"
        && matches!(words[2], "allowed" | "enabled" | "permitted")
    {
        return Some(3);
    }
    if words.len() >= 4
        && safety_control_modal(words[0])
        && words[1..3] == ["not", "be"]
        && matches!(
            words[3],
            "denied" | "disabled" | "disallowed" | "forbidden" | "prohibited"
        )
    {
        return Some(4);
    }
    if matches!(
        words,
        [
            "is",
            "not",
            "disabled" | "disallowed" | "forbidden" | "prohibited",
            ..
        ]
    ) {
        return Some(3);
    }
    matches!(
        words,
        [
            "isn't" | "isnt" | "isn’t",
            "disabled" | "disallowed" | "forbidden" | "prohibited",
            ..
        ]
    )
    .then_some(2)
}

pub(super) fn safety_control_modal(word: &str) -> bool {
    matches!(
        word,
        "can" | "could" | "may" | "might" | "must" | "should" | "will" | "would"
    )
}

pub(in crate::turn) fn closed_action_permission_tail(words: &[&str]) -> bool {
    action_permission_length(words).is_some_and(|length| {
        let remainder = strip_safety_control_tail_prefix(&words[length..]);
        remainder.is_empty() || closed_safety_control_scope(remainder)
    })
}

pub(super) fn strip_safety_control_tail_prefix<'a>(mut words: &'a [&'a str]) -> &'a [&'a str] {
    loop {
        let length =
            if words.starts_with(&["right", "away"]) || words.starts_with(&["right", "now"]) {
                2
            } else if words
                .first()
                .is_some_and(|word| matches!(*word, "immediately" | "now" | "only" | "please"))
            {
                1
            } else {
                return words;
            };
        #[cfg(test)]
        TAIL_PREFIX_STEPS.with(|steps| steps.set(steps.get().saturating_add(length)));
        words = &words[length..];
    }
}

#[cfg(test)]
pub(in crate::turn) fn reset_tail_prefix_steps() {
    TAIL_PREFIX_STEPS.with(|steps| steps.set(0));
}

#[cfg(test)]
pub(in crate::turn) fn tail_prefix_steps() -> usize {
    TAIL_PREFIX_STEPS.with(Cell::get)
}

pub(super) fn strip_safety_control_tail_suffix<'a>(mut words: &'a [&'a str]) -> &'a [&'a str] {
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

pub(in crate::turn) fn preservation_gerund(word: &str) -> bool {
    matches!(
        word,
        "bypassing"
            | "disabling"
            | "dropping"
            | "eliminating"
            | "ignoring"
            | "omitting"
            | "removing"
            | "skipping"
    )
}

pub(in crate::turn) fn preservation_action(word: &str) -> bool {
    preservation_gerund(word)
        || matches!(
            word,
            "bypass"
                | "bypasses"
                | "disable"
                | "disables"
                | "drop"
                | "drops"
                | "eliminate"
                | "eliminates"
                | "ignore"
                | "ignores"
                | "omit"
                | "omits"
                | "remove"
                | "removes"
                | "skip"
                | "skips"
        )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::turn) enum SafetyControlActionEffect {
    WeakensControl,
    EnforcesControl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::turn) enum SafetyControlMeaning {
    PreservesControl,
    WeakensControl,
}

pub(in crate::turn) fn closed_safety_control_result_meaning(
    words: &[&str],
) -> Option<SafetyControlMeaning> {
    let (words, negated) = strip_result_control_negation(words);
    if words.is_empty() {
        return None;
    }
    if matches!(
        words,
        ["validator", "must", "actually", "run"]
            | ["validation", "must", "actually", "run"]
            | ["approval", "must", "be", "requested"]
            | ["approval", "must", "be", "explicit"]
    ) {
        return Some(if negated {
            SafetyControlMeaning::WeakensControl
        } else {
            SafetyControlMeaning::PreservesControl
        });
    }
    let weakens = closed_automatic_control_result(words)
        || closed_assumed_control_result(words)
        || closed_noop_validator(words);
    weakens.then_some(if negated {
        SafetyControlMeaning::PreservesControl
    } else {
        SafetyControlMeaning::WeakensControl
    })
}

fn strip_result_control_negation<'a>(words: &'a [&'a str]) -> (&'a [&'a str], bool) {
    for prefix in [
        &["do", "not"][..],
        &["don't"][..],
        &["dont"][..],
        &["don’t"][..],
        &["must", "not"][..],
        &["never"][..],
        &["should", "not"][..],
    ] {
        if words.starts_with(prefix) {
            return (&words[prefix.len()..], true);
        }
    }
    (words, false)
}

fn closed_automatic_control_result(words: &[&str]) -> bool {
    if matches!(
        words,
        ["auto-approve"] | ["automatically", "approve" | "accept"]
    ) {
        return true;
    }
    let action_tail = match words {
        ["auto-approve", tail @ ..]
        | ["automatically", "approve", tail @ ..]
        | ["automatically", "accept", tail @ ..]
        | ["always", "accept", tail @ ..] => tail,
        _ => return closed_control_result_assignment(words),
    };
    action_tail.len() <= 7
        && action_tail.iter().all(|word| {
            matches!(
                *word,
                "approval"
                    | "asking"
                    | "design"
                    | "deployment"
                    | "every"
                    | "preview"
                    | "the"
                    | "user"
                    | "validation"
                    | "without"
            )
        })
}

fn closed_control_result_assignment(words: &[&str]) -> bool {
    matches!(
        words,
        ["treat", "approval", "as", "granted", "by", "default"]
            | ["default", "approval", "to", "yes" | "true"]
            | ["use", "implicit", "approval"]
            | ["assume", "the", "user", "approved"]
            | ["mark", "validation", "successful", "automatically"]
            | ["mark", "validation", "as", "successful"]
            | ["treat", "validator", "as", "passed"]
            | ["set", "approval", "to", "true" | "yes" | "granted"]
    )
}

fn closed_assumed_control_result(words: &[&str]) -> bool {
    matches!(
        words,
        ["assume", "approval", "was", "granted"]
            | ["assume", "validation", "passed"]
            | ["treat", "validation", "as", "passed" | "successful"]
    )
}

fn closed_noop_validator(words: &[&str]) -> bool {
    matches!(
        words,
        ["use", "a" | "an" | "the", "no-op" | "noop", "validator"]
            | ["use", "no-op" | "noop", "validation" | "validator"]
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::turn) enum SafetyControlTailEffect {
    Direct,
    Permitted,
    Prohibited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::turn) struct SafetyControlAction {
    pub(in crate::turn) length: usize,
    pub(in crate::turn) effect: SafetyControlActionEffect,
    pub(in crate::turn) form: SafetyControlActionForm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::turn) enum SafetyControlActionForm {
    Gerund,
    Infinitive,
    ThirdPerson,
}

impl SafetyControlAction {
    pub(in crate::turn) fn matches_gerund(self, gerund: bool) -> bool {
        self.form
            == if gerund {
                SafetyControlActionForm::Gerund
            } else {
                SafetyControlActionForm::Infinitive
            }
    }
}

pub(in crate::turn) fn safety_control_action(words: &[&str]) -> Option<SafetyControlAction> {
    if words.starts_with(&["turn", "off"]) {
        return Some(SafetyControlAction {
            length: 2,
            effect: SafetyControlActionEffect::WeakensControl,
            form: SafetyControlActionForm::Infinitive,
        });
    }
    if words.starts_with(&["turning", "off"]) {
        return Some(SafetyControlAction {
            length: 2,
            effect: SafetyControlActionEffect::WeakensControl,
            form: SafetyControlActionForm::Gerund,
        });
    }
    if words.starts_with(&["turns", "off"]) {
        return Some(SafetyControlAction {
            length: 2,
            effect: SafetyControlActionEffect::WeakensControl,
            form: SafetyControlActionForm::ThirdPerson,
        });
    }
    let word = *words.first()?;
    if preservation_action(word) {
        return Some(SafetyControlAction {
            length: 1,
            effect: SafetyControlActionEffect::WeakensControl,
            form: if preservation_gerund(word) {
                SafetyControlActionForm::Gerund
            } else if matches!(
                word,
                "bypasses"
                    | "disables"
                    | "drops"
                    | "eliminates"
                    | "ignores"
                    | "omits"
                    | "removes"
                    | "skips"
            ) {
                SafetyControlActionForm::ThirdPerson
            } else {
                SafetyControlActionForm::Infinitive
            },
        });
    }
    matches!(
        word,
        "enforce" | "enforces" | "enforcing" | "require" | "requires" | "requiring"
    )
    .then_some(SafetyControlAction {
        length: 1,
        effect: SafetyControlActionEffect::EnforcesControl,
        form: if word.ends_with("ing") {
            SafetyControlActionForm::Gerund
        } else if word.ends_with('s') {
            SafetyControlActionForm::ThirdPerson
        } else {
            SafetyControlActionForm::Infinitive
        },
    })
}
