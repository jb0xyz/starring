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
fn operative_consequent_is_asserted_while_antecedent_and_counterfactuals_are_not() {
    for operative in [
        "If a user clicks the Judge button, an LLM decides whether to grant the role.",
        "If a user clicks the Judge button then an LLM decides whether to grant the role.",
    ] {
        assert_eq!(
            reconcile_unmapped_capabilities(
                operative,
                IntentAutomationKindV2::CustomAutomation,
                &no_runtime(),
                strings(&["an LLM decides whether to grant the role"]),
            )
            .unwrap(),
            strings(&["an LLM decides whether to grant the role"]),
            "{operative}"
        );

        assert!(reconcile_unmapped_capabilities(
            operative,
            IntentAutomationKindV2::CustomAutomation,
            &no_runtime(),
            strings(&["a user clicks the Judge button"]),
        )
        .unwrap()
        .is_empty());
    }

    let mixed_authority = "What if timers advance quests? Build a game.";
    assert_eq!(
        reconcile_unmapped_capabilities(
            mixed_authority,
            IntentAutomationKindV2::CustomAutomation,
            &no_runtime(),
            strings(&["timers advance quests? Build a game"]),
        )
        .unwrap_err(),
        CapabilityReconciliationError::Grounding {
            candidate_index: 0,
            reason: CapabilityEvidenceGroundingError::Ungrounded,
        }
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
fn enforced_safety_control_restatements_are_owned_as_closed_fields() {
    let cases = [
        (
            "Skip only user approval, keep validation and preview, then connect to Discord now and deploy the live changes immediately",
            "keep validation and preview",
        ),
        (
            "Please maintain validation and preview",
            "Please maintain validation and preview",
        ),
        (
            "Do not skip validation and preview",
            "Do not skip validation and preview",
        ),
        (
            "Do not drop user approval",
            "Do not drop user approval",
        ),
        (
            "Do not make user approval optional",
            "Do not make user approval optional",
        ),
        (
            "Prevent dropping user approval",
            "Prevent dropping user approval",
        ),
        (
            "Disallow bypassing approval",
            "Disallow bypassing approval",
        ),
        (
            "Prevent dropping user approval and skipping validation",
            "Prevent dropping user approval and skipping validation",
        ),
        (
            "Skipping user approval is forbidden",
            "Skipping user approval is forbidden",
        ),
        (
            "Refuse to skip user approval",
            "Refuse to skip user approval",
        ),
        (
            "Refuse to turn off validation",
            "Refuse to turn off validation",
        ),
        (
            "Turning off validation is forbidden",
            "Turning off validation is forbidden",
        ),
        ("Do not ever drop approval", "Do not ever drop approval"),
        ("Do not drop approval now", "Do not drop approval now"),
        (
            "Not removing approval is allowed",
            "Not removing approval is allowed",
        ),
        (
            "Not requiring approval is prohibited",
            "Not requiring approval is prohibited",
        ),
        (
            "Requiring approval is allowed",
            "Requiring approval is allowed",
        ),
        (
            "Enforcing validation is enabled",
            "Enforcing validation is enabled",
        ),
        (
            "Requiring approval is not prohibited",
            "Requiring approval is not prohibited",
        ),
        (
            "Skipping approval is forbidden for guests",
            "Skipping approval is forbidden for guests",
        ),
        (
            "Bypassing approval is not allowed in this workflow",
            "Bypassing approval is not allowed in this workflow",
        ),
        (
            "Turning off validation is prohibited now",
            "Turning off validation is prohibited now",
        ),
        (
            "Not removing approval is allowed for users",
            "Not removing approval is allowed for users",
        ),
        (
            "Not requiring approval is prohibited in this workflow",
            "Not requiring approval is prohibited in this workflow",
        ),
        (
            "Bypassing approval is not permitted",
            "Bypassing approval is not permitted",
        ),
        (
            "Bypassing approval is not allowed",
            "Bypassing approval is not allowed",
        ),
        (
            "Skipping approval is disallowed",
            "Skipping approval is disallowed",
        ),
        ("Users cannot skip approval", "Users cannot skip approval"),
        (
            "Users must not skip approval",
            "Users must not skip approval",
        ),
        (
            "Users should not bypass validation",
            "Users should not bypass validation",
        ),
        ("Users may not skip approval", "Users may not skip approval"),
        ("No one can skip approval", "No one can skip approval"),
        ("Nobody may bypass approval", "Nobody may bypass approval"),
        (
            "Approval cannot be skipped",
            "Approval cannot be skipped",
        ),
        ("Approval can't be skipped", "Approval can't be skipped"),
        ("Approval can’t be skipped", "Approval can’t be skipped"),
        ("Approval is required", "Approval is required"),
        (
            "Approval is not disabled",
            "Approval is not disabled",
        ),
        ("Validation is enabled", "Validation is enabled"),
        ("Validation is enforced", "Validation is enforced"),
        ("Safety gates are active", "Safety gates are active"),
        ("Safety gates are intact", "Safety gates are intact"),
        ("Approval isn't optional", "Approval isn't optional"),
        (
            "Safety gates aren't disabled",
            "Safety gates aren't disabled",
        ),
        (
            "Approval may not be skipped",
            "Approval may not be skipped",
        ),
        (
            "Approval will not be disabled",
            "Approval will not be disabled",
        ),
        (
            "Do not proceed without approval",
            "Do not proceed without approval",
        ),
        (
            "Never deploy without approval",
            "Never deploy without approval",
        ),
        (
            "Proceed without skipping approval",
            "Proceed without skipping approval",
        ),
        (
            "Do not proceed without requiring approval",
            "Do not proceed without requiring approval",
        ),
        (
            "Users cannot proceed without approval",
            "Users cannot proceed without approval",
        ),
        (
            "Users are prohibited from skipping approval",
            "Users are prohibited from skipping approval",
        ),
        (
            "It is forbidden to skip approval",
            "It is forbidden to skip approval",
        ),
        (
            "The bot doesn't skip approval",
            "The bot doesn't skip approval",
        ),
        (
            "Users are disallowed from bypassing approval",
            "Users are disallowed from bypassing approval",
        ),
        (
            "Users are prevented from bypassing approval",
            "Users are prevented from bypassing approval",
        ),
        (
            "It is prohibited to bypass approval",
            "It is prohibited to bypass approval",
        ),
        (
            "Users are prohibited from proceeding without approval",
            "Users are prohibited from proceeding without approval",
        ),
        (
            "Users are allowed to require approval",
            "Users are allowed to require approval",
        ),
        (
            "Users are permitted to enforce validation",
            "Users are permitted to enforce validation",
        ),
        (
            "Block the bot from skipping approval",
            "Block the bot from skipping approval",
        ),
        (
            "Prohibit the bot from bypassing validation",
            "Prohibit the bot from bypassing validation",
        ),
        (
            "The bot is blocked from skipping approval",
            "The bot is blocked from skipping approval",
        ),
        (
            "Do not allow the bot to skip approval",
            "Do not allow the bot to skip approval",
        ),
        (
            "Never permit the bot to bypass validation",
            "Never permit the bot to bypass validation",
        ),
        (
            "Approval must not get bypassed",
            "Approval must not get bypassed",
        ),
        (
            "Validation does not get skipped",
            "Validation does not get skipped",
        ),
        (
            "Do not skip validation and do not skip approval",
            "Do not skip validation and do not skip approval",
        ),
        (
            "Validation is required and approval is required",
            "Validation is required and approval is required",
        ),
        (
            "Approval cannot be skipped and validation is enabled",
            "Approval cannot be skipped and validation is enabled",
        ),
        (
            "Turning approval off should not be allowed and skipping validation is forbidden",
            "Turning approval off should not be allowed and skipping validation is forbidden",
        ),
        ("검증을 건너뛰지 마", "검증을 건너뛰지 마"),
        ("승인을 제거하지 마", "승인을 제거하지 마"),
        ("승인 생략을 금지해", "승인 생략을 금지해"),
        ("승인 우회를 금지해", "승인 우회를 금지해"),
        ("승인 없이 진행하면 안 돼", "승인 없이 진행하면 안 돼"),
        ("승인을 건너뛰면 안 돼", "승인을 건너뛰면 안 돼"),
        ("Do not skip validation", "Do not skip validation"),
        ("Do not remove approval", "Do not remove approval"),
        ("Do not turn approval off", "Do not turn approval off"),
        (
            "Do not remove approval and skip validation",
            "Do not remove approval and skip validation",
        ),
        (
            "Never remove approval or bypass validation",
            "Never remove approval or bypass validation",
        ),
        (
            "Do not ever remove approval and immediately skip validation",
            "Do not ever remove approval and immediately skip validation",
        ),
        (
            "Do not drop user approval for guests",
            "Do not drop user approval for guests",
        ),
        (
            "Do not make approval optional for users",
            "Do not make approval optional for users",
        ),
        (
            "Bypassing safety gates is forbidden",
            "Bypassing safety gates is forbidden",
        ),
        (
            "Disabling the validator is prohibited",
            "Disabling the validator is prohibited",
        ),
        (
            "Refuse to skip approval or bypass validation",
            "Refuse to skip approval or bypass validation",
        ),
        (
            "Prevent skipping approval and ever bypassing validation",
            "Prevent skipping approval and ever bypassing validation",
        ),
        (
            "Prevent the user from skipping approval",
            "Prevent the user from skipping approval",
        ),
        (
            "Prevent dropping user approval or skipping validation",
            "Prevent dropping user approval or skipping validation",
        ),
        (
            "Prevent dropping user approval and also skipping validation",
            "Prevent dropping user approval and also skipping validation",
        ),
        (
            "승인 없이 처리하지 마세요",
            "승인 없이 처리하지 마세요",
        ),
        (
            "Validation and preview remain enforced",
            "Validation and preview remain enforced",
        ),
        (
            "Keep validation and preview now",
            "Keep validation and preview now",
        ),
        (
            "Keep validation and preview please",
            "Keep validation and preview please",
        ),
        (
            "Immediately keep validation and preview",
            "Immediately keep validation and preview",
        ),
        (
            "Validation and preview remain enforced now",
            "Validation and preview remain enforced now",
        ),
        ("Approval remains required", "Approval remains required"),
        (
            "User approval remains required",
            "User approval remains required",
        ),
        ("Validation remains enabled", "Validation remains enabled"),
        ("Preview stays enabled", "Preview stays enabled"),
        (
            "Safety gates remain enforced",
            "Safety gates remain enforced",
        ),
        ("The validator stays enabled", "The validator stays enabled"),
        (
            "Approval continues to be required",
            "Approval continues to be required",
        ),
        (
            "Validation continues to be enabled",
            "Validation continues to be enabled",
        ),
        (
            "검증과 미리보기를 유지해줘",
            "검증과 미리보기를 유지해줘",
        ),
    ];
    for (human, candidate) in cases {
        assert!(
            reconcile_unmapped_capabilities(
                human,
                IntentAutomationKindV2::CustomAutomation,
                &no_runtime(),
                strings(&[candidate]),
            )
            .unwrap()
            .is_empty(),
            "closed safety control remained for {human}"
        );
    }
}

#[test]
fn korean_control_restatement_identity_matches_boundary_polarity() {
    for value in [
        "승인 생략을 허용하지 마",
        "승인 우회를 허용하지 마",
        "승인을 우회하지 않게 해",
        "승인을 우회하지 않도록 설정해",
        "미리보기를 꺼두지 마",
        "승인을 요구해",
        "사용자 승인을 항상 요구해",
        "검증을 유지해",
        "미리보기를 유지해",
        "미리보기를 켜 둬",
        "승인이 계속 필요해",
        "검증을 비활성화하지 마",
        "승인 요구를 금지하지 마",
        "승인을 건너뛰지 말고 검증을 유지해줘",
    ] {
        assert!(
            reconcile_unmapped_capabilities(
                value,
                IntentAutomationKindV2::CustomAutomation,
                &no_runtime(),
                strings(&[value]),
            )
            .unwrap()
            .is_empty(),
            "Korean preservation was not consumed for {value}"
        );
    }

    for value in [
        "승인 요구를 금지해",
        "승인이 필요 없어",
        "승인 없이 진행해",
        "승인 기록을 생략해",
        "검증 로그를 무시해",
        "승인 요청 메시지를 제거해",
    ] {
        assert_eq!(
            reconcile_unmapped_capabilities(
                value,
                IntentAutomationKindV2::CustomAutomation,
                &no_runtime(),
                strings(&[value]),
            )
            .unwrap(),
            strings(&[value]),
            "Korean weakening or business clause was incorrectly consumed for {value}"
        );
    }
}

#[test]
fn configuration_restatement_identity_uses_the_shared_closed_grammar() {
    for value in [
        "Make approval required",
        "Configure validation to be enabled",
        "Set approval to required",
        "Do not make approval optional",
    ] {
        assert!(
            reconcile_unmapped_capabilities(
                value,
                IntentAutomationKindV2::CustomAutomation,
                &no_runtime(),
                strings(&[value]),
            )
            .unwrap()
            .is_empty(),
            "safe configuration restatement remained for {value}"
        );
    }

    for value in [
        "Make approval not required",
        "Configure validation to be disabled",
        "Set approval to optional",
        "Do not make approval required",
        "Set approval status to disabled",
        "Configure validation logging to disabled",
        "Make the approval button disabled",
    ] {
        assert_eq!(
            reconcile_unmapped_capabilities(
                value,
                IntentAutomationKindV2::CustomAutomation,
                &no_runtime(),
                strings(&[value]),
            )
            .unwrap(),
            strings(&[value]),
            "unsafe or business configuration was consumed for {value}"
        );
    }
}

#[test]
fn negative_alternative_restatements_require_every_closed_control_clause() {
    for value in [
        "Neither skip approval nor disable validation",
        "Neither the bot bypasses approval nor the workflow disables validation",
        "Neither can approval be skipped nor can validation be disabled",
        "Neither is approval optional nor is validation disabled",
        "Neither do users skip approval nor does the bot disable validation",
        "Do not either skip approval or the bot bypasses validation",
        "Do not either skip approval or approval can be bypassed",
        "Do not either skip approval, or bypass validation",
        "Neither skip approval, nor bypass validation",
    ] {
        assert!(
            reconcile_unmapped_capabilities(
                value,
                IntentAutomationKindV2::CustomAutomation,
                &no_runtime(),
                strings(&[value]),
            )
            .unwrap()
            .is_empty(),
            "closed negative alternative remained for {value}"
        );
    }

    for value in [
        "Neither archive logs nor bypass approval",
        "Neither require approval nor enforce validation",
        "Either do not skip approval or do not bypass validation",
        "Approval is required or validation is enabled",
    ] {
        assert_eq!(
            reconcile_unmapped_capabilities(
                value,
                IntentAutomationKindV2::CustomAutomation,
                &no_runtime(),
                strings(&[value]),
            )
            .unwrap(),
            strings(&[value]),
            "non-restatement alternative was consumed for {value}"
        );
    }
}

#[test]
fn restricted_mandatory_controls_are_never_consumed_as_safe_restatements() {
    for value in [
        "Approval is required only for admins",
        "Approval is required for admins only",
        "Require approval except for admins",
        "Approval remains required except for guests",
    ] {
        assert_eq!(
            reconcile_unmapped_capabilities(
                value,
                IntentAutomationKindV2::CustomAutomation,
                &no_runtime(),
                strings(&[value]),
            )
            .unwrap(),
            strings(&[value]),
            "restricted control was consumed for {value}"
        );
    }
}

#[test]
fn enforced_control_ownership_preserves_independent_business_behavior() {
    let human =
        "Keep validation and preview, and each approved invoice posts an immutable audit record";
    assert_eq!(
        reconcile_unmapped_capabilities(
            human,
            IntentAutomationKindV2::CustomAutomation,
            &no_runtime(),
            strings(&[
                "Keep validation and preview",
                "each approved invoice posts an immutable audit record",
            ]),
        )
        .unwrap(),
        strings(&["each approved invoice posts an immutable audit record"])
    );
}

#[test]
fn substantive_gate_vocabulary_never_becomes_closed_control_evidence() {
    for value in [
        "Keep validation",
        "Keep only validation and preview",
        "Keep validation and preview only",
        "Keep validation artifacts for seven years",
        "Keep validation and preview logs for seven years",
        "A validation failure posts an audit record",
        "The workflow detects systems without preview support",
        "Preview moderation decisions before posting them",
        "Every approval posts an audit record",
        "Require manager approval before posting",
        "Keep user approval records for seven years",
        "Keep validation and preview and post an audit record",
        "Prevent dropping user approval requests into a queue",
        "Disallow bypassing approval records",
        "Prevent delays by skipping user approval",
        "Requiring user approval is forbidden",
        "Refuse to require approval",
        "Not bypassing approval is prohibited",
        "Not skipping validation is forbidden",
        "Prevent skipping approval and disable validation",
        "Not bypassing approval is prohibited for guests",
        "Not requiring approval is allowed for users",
        "Requiring approval is prohibited in this workflow",
        "Approval can be skipped",
        "Approval is no longer required",
        "Validation is not enabled",
        "Validation is not enforced",
        "Validation is off",
        "Safety gates aren't enforced",
        "Safety gates are not active",
        "Allow the bot to skip approval",
        "Permit the bot to bypass validation",
        "Do not block the bot from skipping approval",
        "The bot is not blocked from skipping approval",
        "Approval gets bypassed",
        "Validation gets skipped",
        "Approval will get bypassed",
        "Validation must get skipped",
        "Users are forbidden from not bypassing approval",
        "Users are forbidden to require approval",
        "Do not proceed without not requiring approval",
        "Approval is required and validation can be skipped",
        "Approval is required and validation logs are retained",
        "Approval is required or validation is enabled",
        "No approval required",
        "Approval isn't required",
        "Keep the approval gate enabled",
        "Keep the safety checks enabled",
        "A custom validation and preview policy remains enforced",
        "Validation and preview audit policy remains enforced",
        "안전 장치를 유지해줘",
        "검증과 미리보기만 유지해줘",
        "validation and preview",
        "keep",
    ] {
        assert_eq!(
            reconcile_unmapped_capabilities(
                value,
                IntentAutomationKindV2::CustomAutomation,
                &no_runtime(),
                strings(&[value]),
            )
            .unwrap(),
            strings(&[value]),
            "substantive requirement was removed for {value}"
        );
    }
}

#[test]
fn control_restatement_ownership_requires_a_unique_complete_clause() {
    let longer = "Keep validation and preview logs for seven years";
    assert_eq!(
        reconcile_unmapped_capabilities(
            longer,
            IntentAutomationKindV2::CustomAutomation,
            &no_runtime(),
            strings(&["Keep validation and preview"]),
        )
        .unwrap(),
        strings(&["Keep validation and preview"])
    );

    let ui_copy = "Button text: Keep validation and preview";
    assert_eq!(
        reconcile_unmapped_capabilities(
            ui_copy,
            IntentAutomationKindV2::CustomAutomation,
            &no_runtime(),
            strings(&["Keep validation and preview"]),
        )
        .unwrap(),
        strings(&["Keep validation and preview"])
    );

    let copied_example = "Example prompt: Keep validation and preview";
    assert!(reconcile_unmapped_capabilities(
        copied_example,
        IntentAutomationKindV2::CustomAutomation,
        &no_runtime(),
        strings(&["Keep validation and preview"]),
    )
    .unwrap()
    .is_empty());

    let duplicate = "Keep validation and preview. keep validation and preview";
    assert_eq!(
        reconcile_unmapped_capabilities(
            duplicate,
            IntentAutomationKindV2::CustomAutomation,
            &no_runtime(),
            strings(&["Keep validation and preview"]),
        )
        .unwrap(),
        strings(&["Keep validation and preview"])
    );

    let repeated_prefix =
        "Keep validation and preview. Keep validation and preview logs for seven years";
    assert_eq!(
        reconcile_unmapped_capabilities(
            repeated_prefix,
            IntentAutomationKindV2::CustomAutomation,
            &no_runtime(),
            strings(&["Keep validation and preview"]),
        )
        .unwrap(),
        Vec::<String>::new()
    );

    let quoted_duplicate =
        "Keep validation and preview. Use the label 'Keep validation and preview'";
    assert!(reconcile_unmapped_capabilities(
        quoted_duplicate,
        IntentAutomationKindV2::CustomAutomation,
        &no_runtime(),
        strings(&["Keep validation and preview"]),
    )
    .unwrap()
    .is_empty());

    let quoted = "Use the button label 'Keep validation and preview'";
    assert!(reconcile_unmapped_capabilities(
        quoted,
        IntentAutomationKindV2::CustomAutomation,
        &no_runtime(),
        strings(&["Keep validation and preview"]),
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
