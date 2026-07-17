use super::super::intent_interpretation::IntentBoundaryRequestV2;
use super::super::intent_safety_control_grammar::{
    action_permission_length, closed_active_actor_safety_control_meaning,
    closed_actor_safety_control_meaning, closed_configuration_safety_control_meaning,
    closed_direct_separable_turn_off_action, closed_inverted_subject_safety_control_meaning,
    closed_korean_safety_control_clause, closed_passive_target_safety_control_meaning,
    closed_safety_control_action_meaning, closed_safety_control_action_tail,
    closed_safety_control_result_meaning, closed_safety_control_scope,
    closed_safety_control_state_meaning, closed_safety_control_tail,
    closed_separable_turn_off_safety_control_meaning, closed_subject_safety_control_meaning,
    closed_without_safety_control_meaning, preservation_prohibition_length, safety_control_action,
    safety_control_target_length, strip_safety_control_target_modifiers, KoreanSafetyControlClause,
    SafetyControlActionEffect, SafetyControlMeaning, SafetyControlTailEffect,
};
pub(super) use super::super::intent_safety_control_grammar::{
    ACTION_NEGATION_MODIFIERS, ACTION_POLARITY_TOKEN_WINDOW,
    CLOSED_SAFETY_CONTROL_SCOPE_PREPOSITIONS, CLOSED_SAFETY_CONTROL_SCOPE_TERMS,
    CLOSED_SAFETY_CONTROL_TARGETS, CLOSED_SAFETY_CONTROL_TARGET_TERMS, ORDINARY_PREFIX_NEGATIONS,
    PRESERVATION_ACTOR_TERMS, PRESERVATION_DETERMINERS, PRESERVATION_PREFIX_NEGATIONS,
    SAFETY_CONTROL_TARGET_MODIFIERS,
};
use super::syntax::{self, word_continuation, BoundaryUnit, TextSpan, UnitLink};

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static SECRET_ACTION_POLARITY_WORK: Cell<usize> = const { Cell::new(0) };
    static MAXIMAL_SECRET_TARGET_WORK: Cell<usize> = const { Cell::new(0) };
    static UNPROTECTED_SECRET_PREFIX_STEPS: Cell<usize> = const { Cell::new(0) };
    static ROOT_SAFETY_CONTROL_PREFIX_STEPS: Cell<usize> = const { Cell::new(0) };
    static ROOT_SAFETY_CONTROL_ACTION_PROBES: Cell<usize> = const { Cell::new(0) };
}

mod action_authority;
mod action_polarity;
mod gate_control;
mod live_scope;
mod secret_disclosure;
mod unit_scope;
mod vocabulary;

#[allow(unused_imports)]
pub(super) use action_authority::{
    boundary_action_is_effectively_preserved, closed_boundary_action_adverb,
    has_negated_boundary_action_marker, has_negated_gate_action_marker,
};
pub(super) use action_polarity::{prefix_negates_action, suffix_negates_action};
pub(super) use gate_control::closed_gate_control_weakening;
#[allow(unused_imports)]
pub(super) use live_scope::{contains_any, live_weak_context};
pub(super) use live_scope::{
    live_resource_antecedent, live_resource_pronoun_continuation, LiveResourceAntecedent,
};
pub(super) use secret_disclosure::{
    closed_secret_acquisition_component, closed_secret_unprotection_continuation,
    secret_target_is_locally_safe, starts_with_secret_target_object,
};
pub(super) use unit_scope::{
    classify_sentence_units, inherited_action_negation, BoundaryKind, UnitFacts,
};
#[allow(unused_imports)]
pub(super) use vocabulary::{
    CLOSED_BOUNDARY_ACTION_ADVERBS, CLOSED_SECRET_DISCLOSURE_ACTORS,
    CLOSED_THIRD_PERSON_BOUNDARY_ACTORS, GATE_ACTIONS, GATE_ACTION_PERMISSION_PREDICATES,
    GATE_CONTROL_TERMS, GATE_DESTRUCTIVE_ACTIONS, GATE_EXACT_ACTIONS, GATE_EXACT_ACTION_TERMS,
    GATE_EXACT_PREFIX_WRAPPERS, GATE_EXACT_SUFFIX_WRAPPERS, GATE_EXACT_WRAPPER_TERMS,
    GATE_REQUIREMENT_REVERSAL_ACTIONS, GATE_REQUIREMENT_REVERSAL_PREDICATES, GATE_RESULT_ACTIONS,
    GATE_TARGETS, IMMEDIATE_CONTEXT, LIVE_ACTIONS, LIVE_CONTEXT, LIVE_CONTEXT_ALIASES,
    SAFE_REDACTION, SECRET_ACTIONS, SECRET_DELIVERY_CONTEXT, SECRET_TARGETS, SUFFIX_NEGATIONS,
    UNPROTECTED_SECRET,
};

#[cfg(test)]
use gate_control::closed_root_safety_control_action_meaning;
#[cfg(test)]
use secret_disclosure::{
    has_unnegated_unprotected_secret, maximal_secret_target_occurrences, secret_action_polarities,
};

#[cfg(test)]
mod performance_tests {
    use super::*;

    #[test]
    fn secret_target_collection_measures_the_full_linear_path() {
        fn work(repeated: usize) -> usize {
            let value = "api token ".repeat(repeated);
            MAXIMAL_SECRET_TARGET_WORK.with(|steps| steps.set(0));
            assert_eq!(maximal_secret_target_occurrences(&value).len(), repeated);
            MAXIMAL_SECRET_TARGET_WORK.with(Cell::get)
        }

        let small = work(2_048);
        let large = work(4_096);
        assert!(large <= small.saturating_mul(2).saturating_add(SECRET_TARGETS.len()));
    }

    #[test]
    fn secret_action_polarities_measure_the_full_linear_path() {
        fn work(repeated: usize) -> usize {
            let value = "publish api token ".repeat(repeated);
            SECRET_ACTION_POLARITY_WORK.with(|steps| steps.set(0));
            assert_eq!(secret_action_polarities(&value).len(), repeated);
            SECRET_ACTION_POLARITY_WORK.with(Cell::get)
        }

        let small = work(2_048);
        let large = work(4_096);
        assert!(large <= small.saturating_mul(2).saturating_add(SECRET_ACTIONS.len()));
    }

    #[test]
    fn unprotected_secret_control_queries_do_constant_prefix_work() {
        let repeated = 4_096usize;
        let value = format!("do not keep {}", "unmasked ".repeat(repeated));
        UNPROTECTED_SECRET_PREFIX_STEPS.with(|steps| steps.set(0));

        assert!(!has_unnegated_unprotected_secret(&value));
        let steps = UNPROTECTED_SECRET_PREFIX_STEPS.with(Cell::get);

        assert_eq!(
            steps,
            repeated.saturating_mul(1usize.saturating_add(PRESERVATION_PREFIX_NEGATIONS.len()))
        );
    }

    #[test]
    fn root_safety_control_scan_validates_each_prefix_word_once() {
        let repeated = 8_192usize;
        let mut words = vec!["also"; repeated];
        words.extend(["skip", "approval"]);
        ROOT_SAFETY_CONTROL_PREFIX_STEPS.with(|steps| steps.set(0));
        ROOT_SAFETY_CONTROL_ACTION_PROBES.with(|steps| steps.set(0));

        assert_eq!(
            closed_root_safety_control_action_meaning(&words),
            Some(SafetyControlMeaning::WeakensControl)
        );
        let prefix_steps = ROOT_SAFETY_CONTROL_PREFIX_STEPS.with(Cell::get);
        let action_probes = ROOT_SAFETY_CONTROL_ACTION_PROBES.with(Cell::get);

        assert_eq!(prefix_steps, repeated);
        assert_eq!(action_probes, 1);
    }
}
