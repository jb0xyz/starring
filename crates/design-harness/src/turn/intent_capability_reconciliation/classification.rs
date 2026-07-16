use super::super::intent_interpretation::{
    EconomyRequirementV2, IntentAutomationKindV2, PersistenceRequirementV2, RuntimeRequirementsV2,
    TimerRequirementV2,
};
use super::control_restatement::enforced_safety_control_restatement;
use super::syntax::{SourceText, Span, Token};

const ECONOMY_MARKERS: &[&str] = &[
    "balance", "balances", "credits", "currency", "economy", "level", "levels", "points", "reward",
    "rewards", "xp",
];

const ECONOMY_ACTIONS: &[&str] = &[
    "award",
    "awards",
    "calculate",
    "calculates",
    "decide",
    "decides",
    "earn",
    "earns",
    "gain",
    "gains",
    "spend",
    "spends",
    "unlock",
    "unlocks",
];

const TIMER_MARKERS: &[&str] = &[
    "deadline",
    "deadlines",
    "quest",
    "quests",
    "schedule",
    "scheduler",
    "schedules",
    "timer",
    "timers",
];

const TIMER_ACTIONS: &[&str] = &[
    "advance",
    "advances",
    "complete",
    "completes",
    "expire",
    "expires",
    "run",
    "runs",
    "start",
    "starts",
    "stop",
    "stops",
    "trigger",
    "triggers",
    "update",
    "updates",
];

const LLM_ACTIONS: &[&str] = &[
    "award",
    "awards",
    "calculate",
    "calculates",
    "choose",
    "chooses",
    "decide",
    "decides",
    "evaluate",
    "evaluates",
    "execute",
    "executes",
    "generate",
    "generates",
    "score",
    "scores",
];

const EXTERNAL_ACTIONS: &[&str] = &[
    "acquire", "acquires", "obtain", "obtains", "require", "requires",
];

const TEMPORAL_CONSTRAINTS: &[&str] = &["after", "before", "prior", "until"];

pub(super) fn has_external_marker(value: &str) -> bool {
    let lowercase = value.to_lowercase();
    lowercase
        .split(|character: char| !character.is_alphanumeric() && character != '-')
        .any(|word| matches!(word, "external" | "cross-service"))
        || lowercase.contains("cross service")
}

pub(super) fn custom_automation_owns(
    source: &SourceText<'_>,
    automation_kind: IntentAutomationKindV2,
    value: &str,
) -> bool {
    if automation_kind != IntentAutomationKindV2::CustomAutomation || has_external_marker(value) {
        return false;
    }
    let lowercase = value.to_lowercase();
    let words = words(&lowercase);
    let button_opens_modal = has_any(&words, &["button", "control"])
        && has_any(
            &words,
            &["open", "opens", "show", "shows", "display", "displays"],
        )
        && has_any(&words, &["modal", "dialog"]);
    let modal_submission_response = source.contains_asserted_token("modal")
        && has_any(
            &words,
            &["submit", "submits", "submitted", "submitting", "submission"],
        )
        && has_any(
            &words,
            &[
                "reply", "replies", "respond", "responds", "return", "returns", "send", "sends",
            ],
        )
        && has_any(&words, &["ephemeral", "private", "privately"]);
    button_opens_modal || modal_submission_response
}

pub(super) fn closed_fields_or_preservation_own(
    source: &SourceText<'_>,
    value: &str,
    runtime: &RuntimeRequirementsV2,
) -> bool {
    if enforced_safety_control_restatement(source, value) {
        return true;
    }
    let lowercase = value.to_lowercase();
    let words = words(&lowercase);
    if words.is_empty() {
        return false;
    }
    let anti_weakening = contains_sequence(&words, &["do", "not", "reduce"])
        || contains_sequence(&words, &["do", "not", "replace"])
        || contains_sequence(&words, &["do", "not", "weaken"])
        || contains_sequence(&words, &["do", "not", "omit"])
        || contains_sequence(&words, &["do", "not", "simplify"])
        || contains_sequence(&words, &["do", "not", "summarize"]);
    if anti_weakening && !has_business_action(&words) {
        return true;
    }
    let generic_restart_state = runtime.persistence == PersistenceRequirementV2::RestartPersistent
        && has_any(&words, &["state", "data"])
        && has_any(&words, &["restart", "restarts"])
        && has_any(
            &words,
            &[
                "persist",
                "persists",
                "preserve",
                "preserves",
                "survive",
                "survives",
            ],
        )
        && !has_business_action(&words);
    let durable_timer = runtime.timers == TimerRequirementV2::Durable
        && has_any(&words, &["timer", "timers", "scheduler", "schedulers"])
        && has_any(&words, &["durable", "persistent"])
        && !has_business_action(&words);
    let persistent_economy = runtime.economy == EconomyRequirementV2::PersistentLedger
        && has_any(&words, ECONOMY_MARKERS)
        && has_any(&words, &["ledger", "persistent", "stored"])
        && !has_business_action(&words);
    generic_restart_state || durable_timer || persistent_economy
}

pub(super) fn runtime_business_spans(
    source: &SourceText<'_>,
    runtime: &RuntimeRequirementsV2,
) -> Vec<Span> {
    let mut spans = Vec::new();
    for clause in source.clauses() {
        if clause.hypothetical || source.overlaps_quote(clause.span) {
            continue;
        }
        let clause = clause.suffix_after("where");
        if clause.hypothetical || source.overlaps_quote(clause.span) {
            continue;
        }
        let words = token_words(&clause.tokens);
        if !has_subject_predicate(&words) || infrastructure_only(&words) {
            continue;
        }
        let economy = runtime.economy == EconomyRequirementV2::PersistentLedger
            && has_any(&words, ECONOMY_MARKERS)
            && has_any(&words, ECONOMY_ACTIONS);
        let timer = runtime.timers == TimerRequirementV2::Durable
            && has_any(&words, TIMER_MARKERS)
            && has_any(&words, TIMER_ACTIONS);
        let event_time_llm =
            runtime.event_time_llm && has_any(&words, &["llm"]) && has_any(&words, LLM_ACTIONS);
        if economy || timer || event_time_llm {
            spans.push(clause.span);
        }
    }
    spans
}

pub(super) fn external_requirement_spans(source: &SourceText<'_>) -> Vec<Span> {
    let mut spans = Vec::new();
    for clause in source.clauses() {
        if clause.hypothetical || source.overlaps_quote(clause.span) {
            continue;
        }
        let clause = clause.without_request_prefix();
        if clause.hypothetical || source.overlaps_quote(clause.span) {
            continue;
        }
        let words = token_words(&clause.tokens);
        if preservation_instruction(&words) || !external_precondition(&words) {
            continue;
        }
        let actions = words
            .iter()
            .filter(|word| EXTERNAL_ACTIONS.contains(word))
            .count();
        let markers = words
            .iter()
            .filter(|word| matches!(**word, "external" | "cross-service"))
            .count()
            .saturating_add(count_sequence(&words, &["cross", "service"]));
        let multiplicity = actions.max(markers).max(1);
        spans.extend(std::iter::repeat_n(clause.span, multiplicity));
    }
    spans
}

fn external_precondition(words: &[&str]) -> bool {
    let imperative = words
        .first()
        .is_some_and(|word| EXTERNAL_ACTIONS.contains(word));
    has_external_words(words)
        && has_any(words, EXTERNAL_ACTIONS)
        && (imperative || has_any(words, &["must", "required", "requires"]))
        && has_any(words, TEMPORAL_CONSTRAINTS)
        && (imperative || has_subject_predicate(words))
}

fn has_external_words(words: &[&str]) -> bool {
    has_any(words, &["external", "cross-service"])
        || contains_sequence(words, &["cross", "service"])
}

fn preservation_instruction(words: &[&str]) -> bool {
    let starts_preservation = words.first().is_some_and(|word| {
        matches!(
            *word,
            "keep" | "maintain" | "preserve" | "retain" | "remember"
        )
    });
    starts_preservation
        || contains_sequence(words, &["do", "not", "replace"])
        || contains_sequence(words, &["do", "not", "weaken"])
        || contains_sequence(words, &["do", "not", "omit"])
}

fn infrastructure_only(words: &[&str]) -> bool {
    let passive_requirement =
        has_any(words, &["durable", "persistent"]) && has_any(words, &["be", "must", "required"]);
    let preservation = has_any(words, &["preserve", "preserves", "survive", "survives"])
        && has_any(words, &["restart", "restarts", "state", "data"]);
    (passive_requirement || preservation) && !has_business_action(words)
}

fn has_subject_predicate(words: &[&str]) -> bool {
    words.len() >= 3
        && words.iter().skip(1).any(|word| {
            ECONOMY_ACTIONS.contains(word)
                || TIMER_ACTIONS.contains(word)
                || LLM_ACTIONS.contains(word)
                || EXTERNAL_ACTIONS.contains(word)
        })
}

fn has_business_action(words: &[&str]) -> bool {
    has_any(words, ECONOMY_ACTIONS)
        || has_any(words, TIMER_ACTIONS)
        || has_any(words, LLM_ACTIONS)
        || has_any(
            words,
            &[
                "archive", "archives", "create", "creates", "grant", "grants", "post", "posts",
                "record", "records", "sign", "signs",
            ],
        )
}

fn words(value: &str) -> Vec<&str> {
    value
        .split(|character: char| !character.is_alphanumeric() && character != '-')
        .filter(|word| !word.is_empty())
        .collect()
}

fn token_words(tokens: &[Token]) -> Vec<&str> {
    tokens.iter().map(|token| token.lower.as_str()).collect()
}

fn has_any(words: &[&str], candidates: &[&str]) -> bool {
    words.iter().any(|word| candidates.contains(word))
}

fn contains_sequence(words: &[&str], sequence: &[&str]) -> bool {
    !sequence.is_empty()
        && words
            .windows(sequence.len())
            .any(|window| window == sequence)
}

fn count_sequence(words: &[&str], sequence: &[&str]) -> usize {
    if sequence.is_empty() {
        return 0;
    }
    words
        .windows(sequence.len())
        .filter(|window| *window == sequence)
        .count()
}
