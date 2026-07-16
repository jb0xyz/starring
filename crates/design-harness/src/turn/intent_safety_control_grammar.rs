pub(super) const ORDINARY_PREFIX_NEGATIONS: &[&str] = &[
    "do not",
    "don't",
    "dont",
    "don’t",
    "does not",
    "doesn't",
    "doesnt",
    "doesn’t",
    "did not",
    "didn't",
    "didnt",
    "didn’t",
    "isn't",
    "isnt",
    "isn’t",
    "aren't",
    "arent",
    "aren’t",
    "wasn't",
    "wasnt",
    "wasn’t",
    "weren't",
    "werent",
    "weren’t",
    "won't",
    "wont",
    "won’t",
    "wouldn't",
    "wouldnt",
    "wouldn’t",
    "couldn't",
    "couldnt",
    "couldn’t",
    "never",
    "neither",
    "must not",
    "mustn't",
    "mustn’t",
    "should not",
    "shouldn't",
    "shouldn’t",
    "cannot",
    "can't",
    "cant",
    "can’t",
    "not",
    "without",
    "no",
    "안",
    "못",
    "절대",
    "금지",
];

pub(super) const PRESERVATION_PREFIX_NEGATIONS: &[&str] = &[
    "avoid",
    "prevent",
    "preventing",
    "disallow",
    "disallowing",
    "forbid",
    "forbidding",
    "refuse to",
    "stop",
];

pub(super) const ACTION_NEGATION_MODIFIERS: &[&str] =
    &["also", "either", "ever", "immediately", "just", "only"];

pub(super) const ACTION_POLARITY_TOKEN_WINDOW: usize = 16;

const MAX_KOREAN_CONTROL_CLAUSES: usize = 32;

pub(super) const SAFETY_CONTROL_TARGET_MODIFIERS: &[&str] =
    &["all", "any", "each", "every", "just", "only"];

pub(super) const PRESERVATION_ACTOR_TERMS: &[&str] = &[
    "admin",
    "admins",
    "anyone",
    "automation",
    "bot",
    "bots",
    "everyone",
    "guest",
    "guests",
    "member",
    "members",
    "people",
    "someone",
    "system",
    "user",
    "users",
    "workflow",
];

pub(super) const PRESERVATION_DETERMINERS: &[&str] =
    &["a", "all", "an", "any", "each", "every", "the"];

pub(super) const CLOSED_SAFETY_CONTROL_TARGETS: &[&str] = &[
    "the design safety gates",
    "the human confirmation step",
    "the human approval",
    "the review gate",
    "the approval checkpoint",
    "the validation/preview stage",
    "the user approval",
    "the approval gate",
    "the approvals",
    "the approval",
    "the safety gates",
    "the safety gate",
    "the safety checks",
    "the safety check",
    "the safeguards",
    "the safeguard",
    "the validation",
    "the validator",
    "the preview",
    "design safety gates",
    "user approval",
    "user-approval",
    "human approval",
    "human confirmation step",
    "review gate",
    "consent",
    "approval checkpoint",
    "validation/preview stage",
    "approval gate",
    "approvals",
    "safety gates",
    "safety gate",
    "safety checks",
    "safety check",
    "safeguards",
    "safeguard",
    "validation",
    "validator",
    "preview",
    "approval",
];

pub(super) const CLOSED_SAFETY_CONTROL_TARGET_TERMS: &[&str] = &[
    "approval",
    "check",
    "checkpoint",
    "checks",
    "gate",
    "gates",
    "preview",
    "confirmation",
    "consent",
    "human",
    "review",
    "stage",
    "safeguard",
    "safeguards",
    "safety",
    "user",
    "validation",
    "validator",
];

pub(super) const CLOSED_SAFETY_CONTROL_SCOPE_PREPOSITIONS: &[&str] = &[
    "after", "before", "by", "during", "for", "from", "in", "on", "within",
];

pub(super) const CLOSED_SAFETY_CONTROL_SCOPE_TERMS: &[&str] = &[
    "a",
    "actions",
    "admin",
    "admins",
    "all",
    "an",
    "any",
    "automation",
    "bot",
    "bots",
    "creation",
    "current",
    "design",
    "disabled",
    "each",
    "every",
    "feature",
    "flow",
    "game",
    "guest",
    "guests",
    "is",
    "member",
    "members",
    "anyone",
    "everyone",
    "people",
    "someone",
    "preview",
    "requested",
    "room",
    "rule",
    "system",
    "that",
    "the",
    "this",
    "user",
    "users",
    "workflow",
];

pub(super) const CLOSED_SAFETY_CONTROL_SCOPE_HEADS: &[&str] = &[
    "actions",
    "admin",
    "admins",
    "automation",
    "bot",
    "bots",
    "creation",
    "design",
    "feature",
    "flow",
    "game",
    "guest",
    "guests",
    "member",
    "members",
    "anyone",
    "everyone",
    "people",
    "someone",
    "preview",
    "room",
    "rule",
    "system",
    "user",
    "users",
    "workflow",
];

pub(super) fn safety_control_target_length(words: &[&str]) -> Option<usize> {
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

pub(super) fn strip_safety_control_target_modifiers<'a>(mut words: &'a [&'a str]) -> &'a [&'a str] {
    while words
        .first()
        .is_some_and(|word| SAFETY_CONTROL_TARGET_MODIFIERS.contains(word))
    {
        words = &words[1..];
    }
    words
}

pub(super) fn closed_safety_control_scope(words: &[&str]) -> bool {
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

pub(super) fn closed_safety_control_tail(words: &[&str]) -> bool {
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

pub(super) fn preservation_prohibition_length(words: &[&str]) -> Option<usize> {
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

pub(super) fn closed_preservation_prohibition_tail(words: &[&str]) -> bool {
    preservation_prohibition_length(words).is_some_and(|length| {
        let remainder = strip_safety_control_tail_prefix(&words[length..]);
        remainder.is_empty() || closed_safety_control_scope(remainder)
    })
}

pub(super) fn action_permission_length(words: &[&str]) -> Option<usize> {
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

fn safety_control_modal(word: &str) -> bool {
    matches!(
        word,
        "can" | "could" | "may" | "might" | "must" | "should" | "will" | "would"
    )
}

pub(super) fn closed_action_permission_tail(words: &[&str]) -> bool {
    action_permission_length(words).is_some_and(|length| {
        let remainder = strip_safety_control_tail_prefix(&words[length..]);
        remainder.is_empty() || closed_safety_control_scope(remainder)
    })
}

fn strip_safety_control_tail_prefix<'a>(mut words: &'a [&'a str]) -> &'a [&'a str] {
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
pub(super) fn reset_tail_prefix_steps() {
    TAIL_PREFIX_STEPS.with(|steps| steps.set(0));
}

#[cfg(test)]
pub(super) fn tail_prefix_steps() -> usize {
    TAIL_PREFIX_STEPS.with(Cell::get)
}

fn strip_safety_control_tail_suffix<'a>(mut words: &'a [&'a str]) -> &'a [&'a str] {
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

pub(super) fn preservation_gerund(word: &str) -> bool {
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

pub(super) fn preservation_action(word: &str) -> bool {
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
pub(super) enum SafetyControlActionEffect {
    WeakensControl,
    EnforcesControl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SafetyControlMeaning {
    PreservesControl,
    WeakensControl,
}

pub(super) fn closed_safety_control_result_meaning(words: &[&str]) -> Option<SafetyControlMeaning> {
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
pub(super) enum SafetyControlTailEffect {
    Direct,
    Permitted,
    Prohibited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SafetyControlAction {
    pub(super) length: usize,
    pub(super) effect: SafetyControlActionEffect,
    pub(super) form: SafetyControlActionForm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SafetyControlActionForm {
    Gerund,
    Infinitive,
    ThirdPerson,
}

impl SafetyControlAction {
    pub(super) fn matches_gerund(self, gerund: bool) -> bool {
        self.form
            == if gerund {
                SafetyControlActionForm::Gerund
            } else {
                SafetyControlActionForm::Infinitive
            }
    }
}

pub(super) fn safety_control_action(words: &[&str]) -> Option<SafetyControlAction> {
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

pub(super) fn closed_safety_control_action_meaning(
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

pub(super) fn closed_separable_turn_off_safety_control_meaning(
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

pub(super) fn closed_direct_separable_turn_off_action(words: &[&str]) -> bool {
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

pub(super) fn safety_control_action_effect_meaning(
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

pub(super) fn closed_safety_control_action_tail(words: &[&str]) -> Option<SafetyControlTailEffect> {
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

pub(super) fn closed_subject_safety_control_meaning(
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

pub(super) fn closed_inverted_subject_safety_control_meaning(
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

pub(super) fn closed_safety_control_state_meaning(words: &[&str]) -> Option<SafetyControlMeaning> {
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

pub(super) fn closed_configuration_safety_control_meaning(
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

pub(super) fn closed_passive_target_safety_control_meaning(
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

pub(super) fn closed_active_actor_safety_control_meaning(
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

pub(super) fn closed_actor_safety_control_meaning(words: &[&str]) -> Option<SafetyControlMeaning> {
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

pub(super) fn closed_actor_safety_control_preservation(words: &[&str]) -> bool {
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

pub(super) fn closed_without_safety_control_meaning(
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KoreanSafetyControlClause {
    Control(SafetyControlMeaning),
    BusinessOperation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KoreanActionClass {
    Ha,
    Geonneottwi,
    Native,
}

pub(super) fn closed_korean_safety_control_clause(
    words: &[&str],
) -> Option<KoreanSafetyControlClause> {
    if !words.contains(&"말고") {
        return closed_korean_safety_control_atom(words);
    }
    let mut start = 0usize;
    let mut clauses = 0usize;
    let mut aggregate = SafetyControlMeaning::PreservesControl;
    for connector in words
        .iter()
        .enumerate()
        .filter_map(|(index, word)| (*word == "말고").then_some(index))
    {
        if connector == start || connector.saturating_add(1) >= words.len() {
            return None;
        }
        clauses = clauses.saturating_add(1);
        if clauses >= MAX_KOREAN_CONTROL_CLAUSES {
            return None;
        }
        aggregate = merge_korean_control_meaning(
            aggregate,
            closed_korean_control_atom_meaning(&words[start..=connector])?,
        );
        start = connector.saturating_add(1);
    }
    aggregate = merge_korean_control_meaning(
        aggregate,
        closed_korean_control_atom_meaning(&words[start..])?,
    );
    Some(KoreanSafetyControlClause::Control(aggregate))
}

fn closed_korean_control_atom_meaning(words: &[&str]) -> Option<SafetyControlMeaning> {
    match closed_korean_safety_control_atom(words)? {
        KoreanSafetyControlClause::Control(meaning) => Some(meaning),
        KoreanSafetyControlClause::BusinessOperation => None,
    }
}

fn merge_korean_control_meaning(
    left: SafetyControlMeaning,
    right: SafetyControlMeaning,
) -> SafetyControlMeaning {
    if left == SafetyControlMeaning::WeakensControl || right == SafetyControlMeaning::WeakensControl
    {
        SafetyControlMeaning::WeakensControl
    } else {
        SafetyControlMeaning::PreservesControl
    }
}

fn closed_korean_safety_control_atom(words: &[&str]) -> Option<KoreanSafetyControlClause> {
    let words = words.strip_prefix(&["보안", "모드에서"]).unwrap_or(words);
    let target_length = korean_control_target_prefix_length(words)?;
    let remainder = strip_korean_control_modifiers(&words[target_length..]);
    if korean_safety_control_business_operation(remainder) {
        return Some(KoreanSafetyControlClause::BusinessOperation);
    }
    let meaning = korean_without_safety_control_meaning(remainder)
        .or_else(|| korean_nominal_safety_control_meaning(remainder))
        .or_else(|| korean_safety_control_state_meaning(remainder))
        .or_else(|| korean_direct_safety_control_meaning(remainder))?;
    Some(KoreanSafetyControlClause::Control(meaning))
}

fn strip_korean_control_modifiers<'a>(mut words: &'a [&'a str]) -> &'a [&'a str] {
    while words.first().is_some_and(|word| {
        matches!(
            *word,
            "항상" | "계속" | "그대로" | "반드시" | "모두" | "전부"
        )
    }) {
        words = &words[1..];
    }
    words
}

fn korean_safety_control_business_operation(words: &[&str]) -> bool {
    if words.len() < 2
        || !words[..words.len().saturating_sub(1)]
            .iter()
            .all(|word| korean_business_object(word))
    {
        return false;
    }
    korean_direct_action_effect(&words[words.len().saturating_sub(1)..]).is_some()
}

fn korean_business_object(word: &str) -> bool {
    korean_stem_with_particle(
        word,
        &[
            "기록",
            "로그",
            "지연",
            "애니메이션",
            "알림",
            "요청",
            "메시지",
        ],
    )
}

fn korean_without_safety_control_meaning(words: &[&str]) -> Option<SafetyControlMeaning> {
    if words.first() != Some(&"없이") {
        return None;
    }
    let negated = korean_process_negated(&words[1..])?;
    Some(if negated {
        SafetyControlMeaning::PreservesControl
    } else {
        SafetyControlMeaning::WeakensControl
    })
}

fn korean_process_negated(words: &[&str]) -> Option<bool> {
    for process in ["진행", "처리", "배포", "적용", "실행"] {
        let Some(suffix) = words.first()?.strip_prefix(process) else {
            continue;
        };
        if words.len() == 1
            && matches!(
                suffix,
                "" | "해" | "해줘" | "해주세요" | "하세요" | "한다" | "해야해" | "해야합니다"
            )
        {
            return Some(false);
        }
        let suffix = suffix.strip_prefix("하").unwrap_or(suffix);
        if korean_closed_negative_suffix(suffix, &words[1..]) {
            return Some(true);
        }
    }
    None
}

fn korean_nominal_safety_control_meaning(words: &[&str]) -> Option<SafetyControlMeaning> {
    let (effect, nominal) = korean_nominal_action_effect(words.first()?)?;
    let governance = &words[1..];
    let tail = if governance.is_empty() && nominal {
        SafetyControlTailEffect::Direct
    } else {
        korean_governance_tail(governance)?
    };
    Some(safety_control_action_effect_meaning(effect, tail, false))
}

fn korean_nominal_action_effect(word: &str) -> Option<(SafetyControlActionEffect, bool)> {
    for (stem, effect) in korean_action_stems() {
        if word == *stem {
            return Some((*effect, true));
        }
        if word
            .strip_prefix(stem)
            .is_some_and(|suffix| matches!(suffix, "을" | "를"))
        {
            return Some((*effect, false));
        }
    }
    if korean_stem_with_particle(word, &["건너뛰기"]) {
        return Some((SafetyControlActionEffect::WeakensControl, false));
    }
    None
}

fn korean_governance_tail(words: &[&str]) -> Option<SafetyControlTailEffect> {
    if words.len() == 1 && korean_ha_command(words[0], "허용")
        || korean_action_negated(words, "금지", KoreanActionClass::Ha)
    {
        return Some(SafetyControlTailEffect::Permitted);
    }
    if words.len() == 1 && korean_ha_command(words[0], "금지")
        || korean_action_negated(words, "허용", KoreanActionClass::Ha)
    {
        return Some(SafetyControlTailEffect::Prohibited);
    }
    None
}

fn korean_safety_control_state_meaning(words: &[&str]) -> Option<SafetyControlMeaning> {
    match words {
        ["필요해"] | ["필요합니다"] => Some(SafetyControlMeaning::PreservesControl),
        ["필요", "없어"]
        | ["필요", "없습니다"]
        | ["필요없어"]
        | ["필요없습니다"]
        | ["필요하지", "않아"]
        | ["필요하지", "않습니다"] => Some(SafetyControlMeaning::WeakensControl),
        ["선택", "사항으로", "해"]
        | ["선택", "사항으로", "해줘"]
        | ["선택", "사항으로", "해주세요"]
        | ["선택사항으로", "해"]
        | ["선택사항으로", "해줘"]
        | ["선택사항으로", "해주세요"] => Some(SafetyControlMeaning::WeakensControl),
        _ => None,
    }
}

fn korean_direct_safety_control_meaning(words: &[&str]) -> Option<SafetyControlMeaning> {
    if korean_forced_action(words) {
        let (effect, _) = korean_action_surface(words.first()?)?;
        return Some(safety_control_action_effect_meaning(
            effect,
            SafetyControlTailEffect::Direct,
            false,
        ));
    }
    if let Some((effect, class)) = korean_action_surface(words.first()?) {
        if korean_action_negated(words, korean_action_stem(words.first()?)?, class) {
            return Some(safety_control_action_effect_meaning(
                effect,
                SafetyControlTailEffect::Direct,
                true,
            ));
        }
    }
    let effect = korean_direct_action_effect(words)?;
    Some(safety_control_action_effect_meaning(
        effect,
        SafetyControlTailEffect::Direct,
        false,
    ))
}

fn korean_direct_action_effect(words: &[&str]) -> Option<SafetyControlActionEffect> {
    if words == ["건너뛴"] {
        return Some(SafetyControlActionEffect::WeakensControl);
    }
    let (effect, class) = korean_action_surface(words.first()?)?;
    let stem = korean_action_stem(words.first()?)?;
    let suffix = words.first()?.strip_prefix(stem)?;
    let direct = match class {
        KoreanActionClass::Ha => {
            words.len() == 1
                && matches!(
                    suffix,
                    "" | "해" | "해줘" | "해주세요" | "하세요" | "해야해" | "해야합니다" | "한다"
                )
        }
        KoreanActionClass::Geonneottwi => {
            words.len() == 1 && matches!(suffix, "" | "어" | "어줘" | "어주세요" | "세요" | "ㄴ")
        }
        KoreanActionClass::Native => {
            (words.len() == 1 && matches!(suffix, "" | "줘" | "주세요" | "둬"))
                || (stem == "켜" && words == ["켜", "둬"])
        }
    };
    direct.then_some(effect)
}

fn korean_action_surface(word: &str) -> Option<(SafetyControlActionEffect, KoreanActionClass)> {
    korean_action_stems().iter().find_map(|(stem, effect)| {
        word.strip_prefix(stem).map(|_| {
            let class = if *stem == "건너뛰" {
                KoreanActionClass::Geonneottwi
            } else if matches!(*stem, "꺼두" | "없애" | "빼" | "끄" | "꺼" | "켜") {
                KoreanActionClass::Native
            } else {
                KoreanActionClass::Ha
            };
            (*effect, class)
        })
    })
}

fn korean_action_stem(word: &str) -> Option<&'static str> {
    korean_action_stems()
        .iter()
        .find_map(|(stem, _)| word.strip_prefix(stem).map(|_| *stem))
}

fn korean_action_stems() -> &'static [(&'static str, SafetyControlActionEffect)] {
    &[
        ("비활성화", SafetyControlActionEffect::WeakensControl),
        ("건너뛰", SafetyControlActionEffect::WeakensControl),
        ("꺼두", SafetyControlActionEffect::WeakensControl),
        ("생략", SafetyControlActionEffect::WeakensControl),
        ("우회", SafetyControlActionEffect::WeakensControl),
        ("제거", SafetyControlActionEffect::WeakensControl),
        ("무시", SafetyControlActionEffect::WeakensControl),
        ("없애", SafetyControlActionEffect::WeakensControl),
        ("요구", SafetyControlActionEffect::EnforcesControl),
        ("유지", SafetyControlActionEffect::EnforcesControl),
        ("보존", SafetyControlActionEffect::EnforcesControl),
        ("활성화", SafetyControlActionEffect::EnforcesControl),
        ("빼", SafetyControlActionEffect::WeakensControl),
        ("끄", SafetyControlActionEffect::WeakensControl),
        ("꺼", SafetyControlActionEffect::WeakensControl),
        ("켜", SafetyControlActionEffect::EnforcesControl),
    ]
}

fn korean_action_negated(words: &[&str], stem: &str, class: KoreanActionClass) -> bool {
    let Some(suffix) = words.first().and_then(|word| word.strip_prefix(stem)) else {
        return false;
    };
    let suffix = if class == KoreanActionClass::Ha {
        suffix.strip_prefix("하").unwrap_or(suffix)
    } else {
        suffix
    };
    korean_closed_negative_suffix(suffix, &words[1..])
}

fn korean_closed_negative_suffix(suffix: &str, remainder: &[&str]) -> bool {
    (matches!(suffix, "지마" | "지마세요" | "지않아" | "지않고") && remainder.is_empty())
        || (suffix == "지"
            && matches!(
                remainder,
                ["마"]
                    | ["마세요"]
                    | ["말고"]
                    | ["않아"]
                    | ["않고"]
                    | ["않게", "해"]
                    | ["않도록", "설정해"]
            ))
        || (suffix == "면"
            && matches!(
                remainder,
                ["안", "돼"] | ["안", "돼요"] | ["안", "됩니다"] | ["안", "됨"]
            ))
}

fn korean_forced_action(words: &[&str]) -> bool {
    let Some(stem) = words.first().and_then(|word| korean_action_stem(word)) else {
        return false;
    };
    let Some(suffix) = words.first().and_then(|word| word.strip_prefix(stem)) else {
        return false;
    };
    let suffix = suffix.strip_prefix("하").unwrap_or(suffix);
    suffix == "지"
        && matches!(
            &words[1..],
            ["않으면", "안", "돼"] | ["않으면", "안", "돼요"] | ["않으면", "안", "됩니다"]
        )
}

fn korean_ha_command(word: &str, stem: &str) -> bool {
    word.strip_prefix(stem).is_some_and(|suffix| {
        matches!(
            suffix,
            "" | "해" | "해줘" | "해주세요" | "하세요" | "해야해" | "해야합니다" | "한다"
        )
    })
}

fn korean_control_target_prefix_length(words: &[&str]) -> Option<usize> {
    let mut index = 0usize;
    loop {
        let (length, coordinated) = korean_control_target_at(&words[index..])?;
        index = index.saturating_add(length);
        if !coordinated {
            break;
        }
        if index >= words.len() {
            return None;
        }
    }
    Some(index)
}

fn korean_control_target_at(words: &[&str]) -> Option<(usize, bool)> {
    for (first, second) in [
        ("사용자", "승인"),
        ("안전", "게이트"),
        ("안전", "장치"),
        ("보호", "장치"),
    ] {
        if words.first() == Some(&first) {
            let particle = korean_target_particle(words.get(1)?, second)?;
            return Some((2, matches!(particle, "과" | "와")));
        }
    }
    for target in [
        "안전게이트",
        "안전장치",
        "보호장치",
        "미리보기",
        "검증",
        "승인",
    ] {
        if let Some(particle) = words
            .first()
            .and_then(|word| korean_target_particle(word, target))
        {
            return Some((1, matches!(particle, "과" | "와")));
        }
    }
    None
}

fn korean_target_particle<'a>(word: &'a str, target: &str) -> Option<&'a str> {
    word.strip_prefix(target).filter(|suffix| {
        matches!(
            *suffix,
            "" | "을" | "를" | "이" | "가" | "은" | "는" | "만" | "도" | "과" | "와"
        )
    })
}

fn korean_stem_with_particle(word: &str, stems: &[&str]) -> bool {
    stems.iter().any(|stem| {
        word.strip_prefix(stem)
            .is_some_and(|suffix| matches!(suffix, "" | "을" | "를"))
    })
}
#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static TAIL_PREFIX_STEPS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
mod tests {
    use super::{
        closed_actor_safety_control_meaning, closed_korean_safety_control_clause,
        closed_subject_safety_control_meaning, KoreanSafetyControlClause, SafetyControlMeaning,
        MAX_KOREAN_CONTROL_CLAUSES,
    };

    #[test]
    fn governed_actor_frames_preserve_polarity_symmetry() {
        for words in [
            &["block", "the", "bot", "from", "skipping", "approval"][..],
            &["prohibit", "the", "bot", "from", "bypassing", "validation"][..],
            &[
                "the", "bot", "is", "blocked", "from", "skipping", "approval",
            ][..],
            &["do", "not", "allow", "the", "bot", "to", "skip", "approval"][..],
            &[
                "never",
                "permit",
                "the",
                "bot",
                "to",
                "bypass",
                "validation",
            ][..],
        ] {
            assert_eq!(
                closed_actor_safety_control_meaning(words),
                Some(SafetyControlMeaning::PreservesControl),
                "preserving governance frame was not classified for {words:?}"
            );
        }
        for words in [
            &["allow", "the", "bot", "to", "skip", "approval"][..],
            &[
                "do", "not", "block", "the", "bot", "from", "skipping", "approval",
            ][..],
            &[
                "the", "bot", "is", "not", "blocked", "from", "skipping", "approval",
            ][..],
        ] {
            assert_eq!(
                closed_actor_safety_control_meaning(words),
                Some(SafetyControlMeaning::WeakensControl),
                "weakening governance frame was not classified for {words:?}"
            );
        }
    }

    #[test]
    fn get_passives_are_bounded_and_polarity_symmetric() {
        for words in [
            &["approval", "gets", "bypassed"][..],
            &["validation", "gets", "skipped"][..],
            &["approval", "will", "get", "bypassed"][..],
        ] {
            assert_eq!(
                closed_subject_safety_control_meaning(words),
                Some(SafetyControlMeaning::WeakensControl),
                "weakening get-passive was not classified for {words:?}"
            );
        }
        for words in [
            &["approval", "must", "not", "get", "bypassed"][..],
            &["validation", "does", "not", "get", "skipped"][..],
            &["approval", "never", "gets", "bypassed"][..],
        ] {
            assert_eq!(
                closed_subject_safety_control_meaning(words),
                Some(SafetyControlMeaning::PreservesControl),
                "preserving get-passive was not classified for {words:?}"
            );
        }
        for words in [
            &["approval", "gets", "bypassed", "by", "the", "ledger"][..],
            &["approval", "budget", "gets", "bypassed"][..],
            &["approval", "gets", "reviewed"][..],
        ] {
            assert_eq!(
                closed_subject_safety_control_meaning(words),
                None,
                "unbounded or non-control get-passive was classified for {words:?}"
            );
        }
    }

    #[test]
    fn korean_negative_coordination_is_iterative_and_budgeted() {
        let mut within_budget = Vec::new();
        for _ in 1..MAX_KOREAN_CONTROL_CLAUSES {
            within_budget.extend(["승인을", "건너뛰지", "말고"]);
        }
        within_budget.extend(["검증을", "유지해줘"]);
        assert_eq!(
            closed_korean_safety_control_clause(&within_budget),
            Some(KoreanSafetyControlClause::Control(
                SafetyControlMeaning::PreservesControl
            ))
        );

        let mut over_budget = Vec::new();
        for _ in 0..MAX_KOREAN_CONTROL_CLAUSES {
            over_budget.extend(["승인을", "건너뛰지", "말고"]);
        }
        over_budget.extend(["검증을", "유지해줘"]);
        assert_eq!(closed_korean_safety_control_clause(&over_budget), None);
    }

    #[test]
    fn copular_state_meaning_is_symmetric_and_fully_consuming() {
        for words in [
            &["approval", "is", "no", "longer", "required"][..],
            &["validation", "is", "not", "enabled"][..],
            &["validation", "is", "not", "enforced"][..],
            &["validation", "is", "off"][..],
            &["safety", "gates", "aren't", "enforced"][..],
            &["safety", "gates", "aren", "t", "enforced"][..],
            &["safety", "gates", "are", "not", "active"][..],
        ] {
            assert_eq!(
                closed_subject_safety_control_meaning(words),
                Some(SafetyControlMeaning::WeakensControl),
                "weakening state was not classified for {words:?}"
            );
        }
        for words in [
            &["validation", "is", "enforced"][..],
            &["safety", "gates", "are", "active"][..],
            &["safety", "gates", "are", "intact"][..],
            &["approval", "isn't", "optional"][..],
            &["approval", "isn", "t", "optional"][..],
            &["safety", "gates", "aren't", "disabled"][..],
            &["safety", "gates", "aren", "t", "disabled"][..],
        ] {
            assert_eq!(
                closed_subject_safety_control_meaning(words),
                Some(SafetyControlMeaning::PreservesControl),
                "preserving state was not classified for {words:?}"
            );
        }
        for words in [
            &["validation", "is", "enforced", "for", "invoice", "routing"][..],
            &[
                "safety", "gates", "are", "active", "and", "publish", "a", "report",
            ][..],
            &[
                "approval", "is", "no", "longer", "required", "by", "the", "ledger",
            ][..],
        ] {
            assert_eq!(
                closed_subject_safety_control_meaning(words),
                None,
                "business tail was consumed for {words:?}"
            );
        }
    }
}
