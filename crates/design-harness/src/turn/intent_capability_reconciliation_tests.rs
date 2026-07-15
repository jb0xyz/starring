use super::intent_capability_grounding::CapabilityEvidenceGroundingError;
use super::intent_capability_reconciliation::{
    reconcile_unmapped_capabilities, CapabilityReconciliationError,
};
use super::{
    EconomyRequirementV2, IntentAutomationKindV2, PersistenceRequirementV2, RuntimeRequirementsV2,
    TimerRequirementV2,
};

fn no_runtime() -> RuntimeRequirementsV2 {
    RuntimeRequirementsV2 {
        persistence: PersistenceRequirementV2::None,
        timers: TimerRequirementV2::None,
        economy: EconomyRequirementV2::None,
        event_time_llm: false,
    }
}

fn stateful_runtime() -> RuntimeRequirementsV2 {
    RuntimeRequirementsV2 {
        persistence: PersistenceRequirementV2::RestartPersistent,
        timers: TimerRequirementV2::Durable,
        economy: EconomyRequirementV2::PersistentLedger,
        event_time_llm: true,
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn custom_modal_and_private_submission_response_are_owned() {
    let human = "Design a feedback automation where a button opens a paragraph modal and submitting it sends a private thank-you response";
    assert_eq!(
        reconcile_unmapped_capabilities(
            human,
            IntentAutomationKindV2::CustomAutomation,
            &no_runtime(),
            strings(&[
                "a button opens a paragraph modal",
                "submitting it sends a private thank-you response",
            ]),
        )
        .unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn static_ownership_does_not_consume_an_external_precondition() {
    let human = "Build a modal submission that must acquire an external consensus lease before returning a private response";
    assert_eq!(
        reconcile_unmapped_capabilities(
            human,
            IntentAutomationKindV2::CustomAutomation,
            &no_runtime(),
            strings(&["external consensus lease"]),
        )
        .unwrap(),
        strings(&[
            "a modal submission that must acquire an external consensus lease before returning a private response"
        ])
    );
}

#[test]
fn managed_recipe_recovers_an_imperative_external_precondition() {
    let human =
        "Build a managed private study room. Acquire an external consensus lease before responding";
    assert_eq!(
        reconcile_unmapped_capabilities(
            human,
            IntentAutomationKindV2::ManagedPrivateStudyRoom,
            &no_runtime(),
            strings(&["external consensus lease"]),
        )
        .unwrap(),
        strings(&["Acquire an external consensus lease before responding"])
    );
}

#[test]
fn closed_runtime_fields_recover_dependent_stateful_behaviors() {
    let human = "Build a persistent Discord game where every message earns XP, levels unlock an economy, timers advance quests, and an LLM decides rewards at event time. Quest timers must be durable, and the economy ledger must be persistent. Preserve state across restarts and do not reduce the request to static responses";
    assert_eq!(
        reconcile_unmapped_capabilities(
            human,
            IntentAutomationKindV2::CustomAutomation,
            &stateful_runtime(),
            strings(&[
                "Preserve state across restarts and do not reduce the request to static responses"
            ]),
        )
        .unwrap(),
        strings(&[
            "an LLM decides rewards at event time",
            "every message earns XP",
            "levels unlock an economy",
            "timers advance quests",
        ])
    );
}

#[test]
fn inactive_runtime_axes_never_trigger_behavior_derivation() {
    let human = "Build a game where every message earns XP, timers advance quests, and an LLM decides rewards at event time. Preserve state across restarts";
    let runtime = RuntimeRequirementsV2 {
        persistence: PersistenceRequirementV2::RestartPersistent,
        ..no_runtime()
    };
    assert!(reconcile_unmapped_capabilities(
        human,
        IntentAutomationKindV2::CustomAutomation,
        &runtime,
        strings(&["Preserve state across restarts"]),
    )
    .unwrap()
    .is_empty());
}

#[test]
fn prompt_example_external_candidate_recovers_the_unique_human_clause() {
    let human = "Build a static Discord button flow that must acquire an external consensus lease before responding. Preserve the external consensus lease requirement and do not replace it with a local approximation";
    assert_eq!(
        reconcile_unmapped_capabilities(
            human,
            IntentAutomationKindV2::CustomAutomation,
            &no_runtime(),
            strings(&["a worker that must obtain a cross-service lease before replying"]),
        )
        .unwrap(),
        strings(&[
            "a static Discord button flow that must acquire an external consensus lease before responding"
        ])
    );
}

#[test]
fn explicit_external_precondition_is_recovered_when_the_model_omits_it() {
    let human = "Create a worker that must obtain a cross-service lease before replying";
    assert_eq!(
        reconcile_unmapped_capabilities(
            human,
            IntentAutomationKindV2::CustomAutomation,
            &no_runtime(),
            Vec::new(),
        )
        .unwrap(),
        strings(&["a worker that must obtain a cross-service lease before replying"])
    );
}

#[test]
fn multiple_external_preconditions_fail_closed() {
    let human = "Build a flow that must acquire an external lease before responding, and a worker must obtain a cross-service lock before posting";
    assert_eq!(
        reconcile_unmapped_capabilities(
            human,
            IntentAutomationKindV2::CustomAutomation,
            &no_runtime(),
            Vec::new(),
        )
        .unwrap_err(),
        CapabilityReconciliationError::AmbiguousExternalEvidence { count: 2 }
    );
}

#[test]
fn synonym_recovery_is_never_attempted() {
    let human = "Build a flow that must acquire a third-party consensus lease before responding";
    assert_eq!(
        reconcile_unmapped_capabilities(
            human,
            IntentAutomationKindV2::CustomAutomation,
            &no_runtime(),
            strings(&["a worker must obtain a cross-service lease before replying"]),
        )
        .unwrap_err(),
        CapabilityReconciliationError::Grounding {
            candidate_index: 0,
            reason: CapabilityEvidenceGroundingError::Ungrounded,
        }
    );
}

#[test]
fn non_external_paraphrases_fail_without_recovery() {
    let human = "Build a game where every message earns XP";
    assert_eq!(
        reconcile_unmapped_capabilities(
            human,
            IntentAutomationKindV2::CustomAutomation,
            &stateful_runtime(),
            strings(&["each post grants experience"]),
        )
        .unwrap_err(),
        CapabilityReconciliationError::Grounding {
            candidate_index: 0,
            reason: CapabilityEvidenceGroundingError::Ungrounded,
        }
    );
}

#[test]
fn quoted_and_hypothetical_text_never_derives_capabilities() {
    let quoted = "Build a button that posts the message 'every message earns XP'. The economy ledger must be persistent";
    let economy = RuntimeRequirementsV2 {
        economy: EconomyRequirementV2::PersistentLedger,
        ..no_runtime()
    };
    assert!(reconcile_unmapped_capabilities(
        quoted,
        IntentAutomationKindV2::CustomAutomation,
        &economy,
        strings(&["every message earns XP"]),
    )
    .unwrap()
    .is_empty());

    let hypothetical =
        "What if a worker must acquire an external consensus lease before responding?";
    assert_eq!(
        reconcile_unmapped_capabilities(
            hypothetical,
            IntentAutomationKindV2::CustomAutomation,
            &no_runtime(),
            strings(&["a worker must acquire an external consensus lease before responding"]),
        )
        .unwrap_err(),
        CapabilityReconciliationError::IncompleteExternalEvidence {
            candidate_index: Some(0)
        }
    );
}

#[test]
fn unbalanced_quotes_fail_before_any_reconciliation() {
    assert_eq!(
        reconcile_unmapped_capabilities(
            "Build a flow with 'an external lease before responding",
            IntentAutomationKindV2::CustomAutomation,
            &no_runtime(),
            Vec::new(),
        )
        .unwrap_err(),
        CapabilityReconciliationError::UnbalancedQuote
    );
}

#[test]
fn runtime_recovery_is_idempotent_and_candidate_order_independent() {
    let human = "Build a game where every message earns XP, levels unlock an economy, timers advance quests, and an LLM decides rewards at event time. Preserve state across restarts";
    let first = reconcile_unmapped_capabilities(
        human,
        IntentAutomationKindV2::CustomAutomation,
        &stateful_runtime(),
        strings(&["timers advance quests", "every message earns XP"]),
    )
    .unwrap();
    let reversed = reconcile_unmapped_capabilities(
        human,
        IntentAutomationKindV2::CustomAutomation,
        &stateful_runtime(),
        strings(&["every message earns XP", "timers advance quests"]),
    )
    .unwrap();
    assert_eq!(first, reversed);
    assert_eq!(
        reconcile_unmapped_capabilities(
            human,
            IntentAutomationKindV2::CustomAutomation,
            &stateful_runtime(),
            first.clone(),
        )
        .unwrap(),
        first
    );
}

#[test]
fn every_reconciled_value_is_an_exact_contiguous_source_span() {
    let human = "Build a game where every message earns XP, timers advance quests, and an LLM decides rewards at event time";
    let output = reconcile_unmapped_capabilities(
        human,
        IntentAutomationKindV2::CustomAutomation,
        &stateful_runtime(),
        Vec::new(),
    )
    .unwrap();
    assert!(output.iter().all(|value| human.contains(value)));
    assert!(output
        .iter()
        .all(|value| value.encode_utf16().count() <= 160));
}

#[test]
fn derived_external_evidence_obeys_the_utf16_limit() {
    let actor = "😀".repeat(70);
    let human =
        format!("Build a {actor} worker that must acquire an external lease before responding");
    assert!(matches!(
        reconcile_unmapped_capabilities(
            &human,
            IntentAutomationKindV2::CustomAutomation,
            &no_runtime(),
            Vec::new(),
        ),
        Err(CapabilityReconciliationError::EvidenceTooLong { utf16_len }) if utf16_len > 160
    ));
}

#[test]
fn output_count_limit_fails_closed_after_deduplication() {
    let candidates = strings(&[
        "worker one signs records",
        "worker two signs records",
        "worker three signs records",
        "worker four signs records",
        "worker five signs records",
        "worker six signs records",
        "worker seven signs records",
        "worker eight signs records",
        "worker nine signs records",
    ]);
    let human = candidates.join(", ");
    assert_eq!(
        reconcile_unmapped_capabilities(
            &human,
            IntentAutomationKindV2::CustomAutomation,
            &no_runtime(),
            candidates,
        )
        .unwrap_err(),
        CapabilityReconciliationError::TooManyCapabilities { count: 9 }
    );
}
