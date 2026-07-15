use serde_json::{json, Value};

use crate::intent::{
    compile_intent, CapabilityPolicyIdV2, CapabilityStatusV2, ExistingChannelKey,
    IntentCapabilityIdV2, IntentRequestedOutcome, IntentResolutionContext,
    IntentSafetyBoundaryIdV2, PreparedIntentWorkspaceV2,
};
use crate::turn::parse_interpret_intent_turn;

use super::adjudicate::{
    adjudicate_intent_v2, validate_persisted_private_study_room_decision_v2, IntentAdjudicationV2,
};
use super::decision::{IntentDecisionSourceV2, IntentRouteDecisionKindV2, IntentRouteDecisionV2};

const MANIFEST_DIGEST: &str = "68de3f4d9355c99b213ba7546f41a772cd21e59ac4f750cc5ff33d99a0cc5d53";

fn base_value() -> Value {
    json!({
        "expected_revision": 0,
        "request_mode": "build",
        "automation_kind": "managed_private_study_room",
        "objective": "Create private study rooms",
        "requested_outcome": "validated_preview",
        "hub_channel": "community_hub",
        "locale": "en",
        "close_authorization": "disabled",
        "runtime_requirements": {
            "persistence": "none",
            "timers": "none",
            "economy": "none",
            "event_time_llm": false
        },
        "boundary_requests": [],
        "unclassified_requirements": [],
        "response": "",
        "response_locale": "en"
    })
}

fn adjudicate(value: &Value) -> IntentAdjudicationV2 {
    let interpretation = parse_interpret_intent_turn(&value.to_string()).unwrap();
    adjudicate_intent_v2(interpretation).unwrap()
}

fn context() -> IntentResolutionContext {
    IntentResolutionContext::from_channel_bindings([ExistingChannelKey(
        "community_hub".to_string(),
    )])
}

fn terminal_parts(value: &Value) -> (IntentRouteDecisionV2, String) {
    let IntentAdjudicationV2::Terminal(permit) = adjudicate(value) else {
        panic!("expected a terminal adjudication");
    };
    permit.into_parts()
}

fn route_decision(value: &Value) -> IntentRouteDecisionV2 {
    match adjudicate(value) {
        IntentAdjudicationV2::PrivateStudyRoom(permit) => permit.decision().clone(),
        IntentAdjudicationV2::TypedPlanner(permit) => permit.decision().clone(),
        IntentAdjudicationV2::Terminal(permit) => permit.decision().clone(),
    }
}

#[test]
fn supported_recipe_gets_one_pinned_consuming_permit() {
    let IntentAdjudicationV2::PrivateStudyRoom(permit) = adjudicate(&base_value()) else {
        panic!("expected a private study-room permit");
    };
    assert_eq!(
        permit.decision().kind(),
        IntentRouteDecisionKindV2::PrivateStudyRoom
    );
    assert_eq!(
        permit.decision().decision_source(),
        IntentDecisionSourceV2::DeterministicIntentAdjudicator
    );
    assert_eq!(permit.decision().manifest_digest(), MANIFEST_DIGEST);
    assert!(permit.decision().blockers().is_empty());
    assert!(permit.decision().boundary_violations().is_empty());
    let target = permit.decision().route_target().unwrap();
    assert_eq!(target.recipe_id(), "starring.private_study_room");
    assert_eq!(target.recipe_version(), 1);

    let original_digest = permit.decision().adjudication_digest().to_string();
    let (decision, prepared) = permit.prepare(&context()).unwrap();
    assert_eq!(decision.adjudication_digest(), original_digest);
    let PreparedIntentWorkspaceV2::Resolved { intent, .. } = prepared else {
        panic!("expected a resolved recipe");
    };
    let compiled = compile_intent(&intent).unwrap();
    assert_eq!(compiled.requirements.len(), 22);
    assert_eq!(compiled.manifest.recipe_id, "starring.private_study_room");
}

#[test]
fn any_member_close_lowers_but_creator_only_never_gets_a_recipe_permit() {
    let mut any_member = base_value();
    any_member["close_authorization"] = Value::String("any_member".to_string());
    let IntentAdjudicationV2::PrivateStudyRoom(permit) = adjudicate(&any_member) else {
        panic!("expected a private study-room permit");
    };
    let (_, PreparedIntentWorkspaceV2::Resolved { intent, .. }) =
        permit.prepare(&context()).unwrap()
    else {
        panic!("expected a resolved recipe");
    };
    let compiled = compile_intent(&intent).unwrap();
    assert_eq!(compiled.requirements.len(), 26);

    let mut creator = base_value();
    creator["hub_channel"] = Value::Null;
    creator["close_authorization"] = Value::String("creator_only".to_string());
    creator["response"] = Value::String("I built it without the restriction.".to_string());
    let (decision, response) = terminal_parts(&creator);
    assert_eq!(decision.kind(), IntentRouteDecisionKindV2::CapabilityGap);
    assert_eq!(decision.blockers().len(), 1);
    assert_eq!(
        decision.blockers()[0].id,
        IntentCapabilityIdV2::InstanceCreatorTeardownAuthorization
    );
    assert_eq!(
        decision.blockers()[0].status,
        CapabilityStatusV2::Unavailable
    );
    assert!(!response.contains("built it"));
    assert!(response.contains("Creator-only room teardown authorization"));
}

#[test]
fn stateful_build_has_the_exact_sorted_four_blockers_and_policy() {
    let mut value = base_value();
    value["automation_kind"] = Value::String("custom_automation".to_string());
    value["objective"] = Value::String("Create a persistent timed economy game".to_string());
    value["runtime_requirements"] = json!({
        "persistence": "restart_persistent",
        "timers": "durable",
        "economy": "persistent_ledger",
        "event_time_llm": true
    });
    value["response"] = Value::String("The game is ready.".to_string());
    let (decision, response) = terminal_parts(&value);
    assert_eq!(decision.kind(), IntentRouteDecisionKindV2::CapabilityGap);
    assert_eq!(
        decision
            .blockers()
            .iter()
            .map(|blocker| blocker.id)
            .collect::<Vec<_>>(),
        vec![
            IntentCapabilityIdV2::DurableTimer,
            IntentCapabilityIdV2::EventTimeLlmDecision,
            IntentCapabilityIdV2::PersistentEconomyLedger,
            IntentCapabilityIdV2::RestartPersistentState,
        ]
    );
    let event_time = decision
        .blockers()
        .iter()
        .find(|blocker| blocker.id == IntentCapabilityIdV2::EventTimeLlmDecision)
        .unwrap();
    assert_eq!(event_time.status, CapabilityStatusV2::ForbiddenPolicy);
    assert_eq!(
        event_time.policy_id,
        Some(CapabilityPolicyIdV2::EventTimeLlmExecutionForbiddenV1)
    );
    assert!(!response.contains("game is ready"));
    assert!(response.contains("Event-time LLM decisions"));
}

#[test]
fn safety_boundary_rejects_before_gap_without_losing_build_findings() {
    let mut value = base_value();
    value["automation_kind"] = Value::String("custom_automation".to_string());
    value["runtime_requirements"] = json!({
        "persistence": "restart_persistent",
        "timers": "durable",
        "economy": "none",
        "event_time_llm": false
    });
    value["boundary_requests"] = json!(["secret_disclosure", "direct_live_mutation"]);
    value["response"] = Value::String("Secrets sent and changes deployed.".to_string());
    let (decision, response) = terminal_parts(&value);
    assert_eq!(decision.kind(), IntentRouteDecisionKindV2::Reject);
    assert_eq!(
        decision
            .boundary_violations()
            .iter()
            .map(|violation| violation.id)
            .collect::<Vec<_>>(),
        vec![
            IntentSafetyBoundaryIdV2::DirectLiveMutation,
            IntentSafetyBoundaryIdV2::SecretDisclosure,
        ]
    );
    assert_eq!(decision.blockers().len(), 2);
    assert!(!response.contains("deployed"));
    assert!(response.contains("Direct live mutation"));
    assert!(response.contains("Secret disclosure"));
}

#[test]
fn discussion_precedes_runtime_gaps_and_alone_surfaces_model_prose() {
    let mut value = base_value();
    value["request_mode"] = Value::String("discussion".to_string());
    value["automation_kind"] = Value::String("none".to_string());
    value["requested_outcome"] = Value::String("discussion".to_string());
    value["runtime_requirements"] = json!({
        "persistence": "restart_persistent",
        "timers": "durable",
        "economy": "persistent_ledger",
        "event_time_llm": true
    });
    value["unclassified_requirements"] = json!(["external consensus lease"]);
    value["response"] = Value::String("Let us compare durable game designs.".to_string());
    let (decision, response) = terminal_parts(&value);
    assert_eq!(decision.kind(), IntentRouteDecisionKindV2::Discussion);
    assert!(decision.blockers().is_empty());
    assert!(decision.boundary_violations().is_empty());
    assert_eq!(response, "Let us compare durable game designs.");
}

#[test]
fn custom_static_build_gets_typed_permit_and_ignores_recipe_overrides() {
    let mut value = base_value();
    value["automation_kind"] = Value::String("custom_automation".to_string());
    value["objective"] = Value::String("Create a static feedback flow".to_string());
    value["copy"] = json!({"create_button_label": "Private override"});
    value["response"] = Value::String("I already deployed the bot.".to_string());
    value["response_locale"] = Value::String("ko".to_string());
    let IntentAdjudicationV2::TypedPlanner(permit) = adjudicate(&value) else {
        panic!("expected a typed-planner permit");
    };
    assert_eq!(
        permit.decision().kind(),
        IntentRouteDecisionKindV2::TypedPlanner
    );
    assert!(permit.decision().blockers().is_empty());
    assert_eq!(permit.reason(), "Create a static feedback flow");
    assert_eq!(
        permit.requested_outcome(),
        IntentRequestedOutcome::ValidatedPreview
    );
    assert!(!permit.response().contains("deployed"));
    assert!(permit.response().contains("타입 기반 플래너"));
    let (objective, requested_outcome, decision, response) = permit.into_parts();
    assert_eq!(objective, "Create a static feedback flow");
    assert_eq!(requested_outcome, IntentRequestedOutcome::ValidatedPreview);
    assert_eq!(decision.kind(), IntentRouteDecisionKindV2::TypedPlanner);
    assert!(response.contains("라이브 시스템은 변경하지 않았습니다"));
}

#[test]
fn unclassified_requirement_fails_closed_with_sorted_evidence() {
    let mut value = base_value();
    value["automation_kind"] = Value::String("custom_automation".to_string());
    value["unclassified_requirements"] = json!([
        "zeta quorum contract",
        "alpha scheduler lease",
        "zeta quorum contract"
    ]);
    let (decision, _) = terminal_parts(&value);
    assert_eq!(decision.kind(), IntentRouteDecisionKindV2::CapabilityGap);
    assert_eq!(
        decision.unclassified_requirements(),
        &["alpha scheduler lease", "zeta quorum contract"]
    );
    assert_eq!(decision.blockers().len(), 1);
    assert_eq!(
        decision.blockers()[0].id,
        IntentCapabilityIdV2::UnclassifiedIntentRequirement
    );
    assert_eq!(decision.blockers()[0].evidence.len(), 2);
    assert_eq!(
        decision.blockers()[0]
            .evidence
            .iter()
            .map(|evidence| evidence.description.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha scheduler lease", "zeta quorum contract"]
    );
}

#[test]
fn build_without_an_automation_or_finding_is_inconsistent() {
    let mut value = base_value();
    value["automation_kind"] = Value::String("none".to_string());
    let interpretation = parse_interpret_intent_turn(&value.to_string()).unwrap();
    let Err(error) = adjudicate_intent_v2(interpretation) else {
        panic!("expected an inconsistent adjudication");
    };
    assert_eq!(error.code, "INCONSISTENT_INTENT_ADJUDICATION");
    assert_eq!(error.location, "intent.interpretation.automation_kind");
}

#[test]
fn digests_are_golden_stable_transport_independent_and_semantically_sensitive() {
    let IntentAdjudicationV2::PrivateStudyRoom(first) = adjudicate(&base_value()) else {
        panic!("expected a private study-room permit");
    };
    let mut transport_variant = base_value();
    transport_variant["expected_revision"] = Value::from(93);
    transport_variant["response"] = Value::String("Ignored build prose".to_string());
    let IntentAdjudicationV2::PrivateStudyRoom(second) = adjudicate(&transport_variant) else {
        panic!("expected a private study-room permit");
    };
    assert_eq!(
        first.decision().semantic_ir_digest(),
        second.decision().semantic_ir_digest()
    );
    assert_eq!(
        first.decision().adjudication_digest(),
        second.decision().adjudication_digest()
    );
    assert_eq!(
        first.decision().semantic_ir_digest(),
        "aa503846a83bfea486de99fff3fbff2a547a871d3d11b64f3f26a6d8794980ed"
    );
    assert_eq!(
        first.decision().adjudication_digest(),
        "607a5075ecde88f3b8b288f5d54f7b9e42afac05ef36805c87f7e7e6c0856711"
    );

    let mut semantic_variant = base_value();
    semantic_variant["copy"] = json!({"create_button_label": "Open a room"});
    let IntentAdjudicationV2::PrivateStudyRoom(changed) = adjudicate(&semantic_variant) else {
        panic!("expected a private study-room permit");
    };
    assert_ne!(
        first.decision().semantic_ir_digest(),
        changed.decision().semantic_ir_digest()
    );
    assert_ne!(
        first.decision().adjudication_digest(),
        changed.decision().adjudication_digest()
    );
    for digest in [
        first.decision().semantic_ir_digest(),
        first.decision().adjudication_digest(),
    ] {
        assert_eq!(digest.len(), 64);
        assert!(digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    }
}

#[test]
fn set_order_and_duplicates_do_not_change_digests() {
    let mut first = base_value();
    first["automation_kind"] = Value::String("custom_automation".to_string());
    first["boundary_requests"] = json!([
        "secret_disclosure",
        "direct_live_mutation",
        "secret_disclosure"
    ]);
    first["unclassified_requirements"] = json!(["zeta", "alpha", "zeta"]);
    let mut second = first.clone();
    second["boundary_requests"] = json!(["direct_live_mutation", "secret_disclosure"]);
    second["unclassified_requirements"] = json!(["alpha", "zeta"]);
    let (first, _) = terminal_parts(&first);
    let (second, _) = terminal_parts(&second);
    assert_eq!(first.semantic_ir_digest(), second.semantic_ir_digest());
    assert_eq!(first.adjudication_digest(), second.adjudication_digest());
}

#[test]
fn every_authoritative_semantic_surface_changes_the_digest() {
    let base = base_value();
    let baseline = route_decision(&base);
    let variants = [
        ("request_mode", {
            let mut value = base.clone();
            value["request_mode"] = Value::String("discussion".to_string());
            value["automation_kind"] = Value::String("none".to_string());
            value["requested_outcome"] = Value::String("discussion".to_string());
            value["response"] = Value::String("Compare the design.".to_string());
            value
        }),
        ("automation_kind", {
            let mut value = base.clone();
            value["automation_kind"] = Value::String("custom_automation".to_string());
            value
        }),
        ("objective", {
            let mut value = base.clone();
            value["objective"] = Value::String("Create focused private rooms".to_string());
            value
        }),
        ("requested_outcome", {
            let mut value = base.clone();
            value["requested_outcome"] = Value::String("working_draft".to_string());
            value
        }),
        ("hub", {
            let mut value = base.clone();
            value["hub_channel"] = Value::String("other_hub".to_string());
            value
        }),
        ("locale", {
            let mut value = base.clone();
            value["locale"] = Value::String("ko".to_string());
            value
        }),
        ("close", {
            let mut value = base.clone();
            value["close_authorization"] = Value::String("any_member".to_string());
            value
        }),
        ("runtime", {
            let mut value = base.clone();
            value["runtime_requirements"]["timers"] = Value::String("durable".to_string());
            value
        }),
        ("boundary", {
            let mut value = base.clone();
            value["boundary_requests"] = json!(["direct_live_mutation"]);
            value
        }),
        ("unclassified", {
            let mut value = base.clone();
            value["unclassified_requirements"] = json!(["external lease"]);
            value
        }),
        ("response_locale", {
            let mut value = base.clone();
            value["response_locale"] = Value::String("ko".to_string());
            value
        }),
        ("copy", {
            let mut value = base.clone();
            value["copy"] = json!({"create_button_label": "Open room"});
            value
        }),
        ("naming", {
            let mut value = base.clone();
            value["naming"] = json!({"channel_name": {"prefix": "room-", "suffix": ""}});
            value
        }),
        ("controls", {
            let mut value = base.clone();
            value["controls"] = json!({"help_label": "Room help"});
            value
        }),
    ];
    for (name, value) in variants {
        let changed = route_decision(&value);
        assert_ne!(
            baseline.semantic_ir_digest(),
            changed.semantic_ir_digest(),
            "{name} did not change semantic identity"
        );
        assert_ne!(
            baseline.adjudication_digest(),
            changed.adjudication_digest(),
            "{name} did not change adjudication identity"
        );
    }
}

#[test]
fn discussion_prose_has_no_route_or_digest_authority() {
    let mut first = base_value();
    first["request_mode"] = Value::String("discussion".to_string());
    first["automation_kind"] = Value::String("none".to_string());
    first["requested_outcome"] = Value::String("discussion".to_string());
    first["response"] = Value::String("Compare the options.".to_string());
    let mut second = first.clone();
    second["response"] = Value::String("A completely different answer.".to_string());
    let (first_decision, first_response) = terminal_parts(&first);
    let (second_decision, second_response) = terminal_parts(&second);
    assert_ne!(first_response, second_response);
    assert_eq!(
        first_decision.semantic_ir_digest(),
        second_decision.semantic_ir_digest()
    );
    assert_eq!(
        first_decision.adjudication_digest(),
        second_decision.adjudication_digest()
    );
}

#[test]
fn missing_hub_preserves_the_decision_across_deterministic_question_creation() {
    let mut value = base_value();
    value["hub_channel"] = Value::Null;
    let IntentAdjudicationV2::PrivateStudyRoom(permit) = adjudicate(&value) else {
        panic!("expected a private study-room permit");
    };
    let digest = permit.decision().adjudication_digest().to_string();
    let (decision, prepared) = permit.prepare(&context()).unwrap();
    assert_eq!(decision.adjudication_digest(), digest);
    let PreparedIntentWorkspaceV2::NeedsInput { decisions, .. } = prepared else {
        panic!("expected one deterministic hub question");
    };
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].options, vec!["community_hub"]);
}

#[test]
fn public_decision_json_roundtrip_preserves_the_audit_record() {
    let IntentAdjudicationV2::PrivateStudyRoom(permit) = adjudicate(&base_value()) else {
        panic!("expected a private study-room permit");
    };
    let json = serde_json::to_string(permit.decision()).unwrap();
    let restored = serde_json::from_str::<IntentRouteDecisionV2>(&json).unwrap();
    assert_eq!(&restored, permit.decision());
    validate_persisted_private_study_room_decision_v2(&restored).unwrap();

    let mut tampered = serde_json::to_value(&restored).unwrap();
    tampered["adjudication_digest"] = Value::String("0".repeat(64));
    let tampered = serde_json::from_value::<IntentRouteDecisionV2>(tampered).unwrap();
    assert_eq!(
        validate_persisted_private_study_room_decision_v2(&tampered)
            .unwrap_err()
            .code,
        "INVALID_PERSISTED_INTENT_DECISION_DIGEST"
    );
}
