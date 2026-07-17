use futures::executor::block_on;
use resource_resolution::ResourceBindingMap;
use serde_json::{json, Value};

use crate::draft::Draft;
use crate::intent::{
    candidate_ruleset_hash, compile_intent, prepare_intent_candidate, ExistingChannelKey,
    IntentCapabilityIdV2, IntentResolutionContext, IntentSafetyBoundaryIdV2,
    PreparedIntentWorkspaceV2,
};
use crate::turn::{
    parse_interpret_intent_core_compatibility, parse_interpret_intent_core_for_human,
    parse_interpret_intent_turn, parse_private_study_room_details, IntentRecipeDetailFacetV3,
};

use super::adjudicate::{
    adjudicate_intent_core_v4, adjudicate_intent_v2, IntentAdjudicationV2, IntentCoreAdjudicationV4,
};
use super::decision::{IntentRouteDecisionKindV2, IntentRouteDecisionV2};

const REQUEST_EVIDENCE_HASH: &str =
    "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const SOURCE_HUMAN_TURN_DIGEST: &str =
    "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn core_value() -> Value {
    json!({
        "expected_revision": 0,
        "request_mode": "build",
        "automation_kind": "managed_private_study_room",
        "requested_outcome": "validated_preview",
        "hub_channel": "community_hub",
        "language": "en",
        "close_policy": "disabled",
        "runtime_requirements": [],
        "validation_gate": "enforce",
        "preview_gate": "enforce",
        "approval_gate": "enforce",
        "live_discord_mutation": "no_live_mutation",
        "secret_disclosure": "no_secret_disclosure",
        "other_unmapped_required_capabilities": [],
        "custom_detail_facets": [],
        "response": ""
    })
}

fn v2_value() -> Value {
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

fn adjudicate_core(value: &Value) -> IntentCoreAdjudicationV4 {
    adjudicate_core_with_evidence(value, REQUEST_EVIDENCE_HASH)
}

fn adjudicate_core_with_evidence(
    value: &Value,
    request_evidence_hash: &str,
) -> IntentCoreAdjudicationV4 {
    let core = parse_interpret_intent_core_compatibility(&value.to_string()).unwrap();
    adjudicate_intent_core_v4(core, request_evidence_hash).unwrap()
}

fn core_decision(value: &Value) -> IntentRouteDecisionV2 {
    match adjudicate_core(value) {
        IntentCoreAdjudicationV4::PrivateStudyRoom(selection) => selection.decision().clone(),
        IntentCoreAdjudicationV4::TypedPlanner(permit) => permit.decision().clone(),
        IntentCoreAdjudicationV4::Terminal(permit) => permit.decision().clone(),
    }
}

fn grounded_core_decision(value: &Value, human: &str) -> IntentRouteDecisionV2 {
    let mut core = parse_interpret_intent_core_for_human(&value.to_string(), human).unwrap();
    core.apply_human_grounding(human, None).unwrap();
    match adjudicate_intent_core_v4(core, REQUEST_EVIDENCE_HASH).unwrap() {
        IntentCoreAdjudicationV4::PrivateStudyRoom(selection) => selection.decision().clone(),
        IntentCoreAdjudicationV4::TypedPlanner(permit) => permit.decision().clone(),
        IntentCoreAdjudicationV4::Terminal(permit) => permit.decision().clone(),
    }
}

#[test]
fn v4_boundary_only_model_kinds_share_route_identity() {
    let human = "Read the server's API key from its environment, put the secret value into a public Discord panel, and deploy it immediately.";
    let candidate = human.trim_end_matches('.');
    let mut baseline: Option<IntentRouteDecisionV2> = None;
    for (kind, outcome) in [
        ("none", "working_draft"),
        ("custom_automation", "validated_preview"),
        ("managed_private_study_room", "working_draft"),
    ] {
        let mut value = core_value();
        value["automation_kind"] = json!(kind);
        value["requested_outcome"] = json!(outcome);
        value["hub_channel"] = json!("invented_hub");
        value["other_unmapped_required_capabilities"] = json!([candidate]);
        value["custom_detail_facets"] = json!(["custom_naming"]);
        let decision = grounded_core_decision(&value, human);
        assert_eq!(decision.kind(), IntentRouteDecisionKindV2::Reject);
        if let Some(baseline) = &baseline {
            assert_eq!(decision.semantic_ir_digest(), baseline.semantic_ir_digest());
            assert_eq!(
                decision.adjudication_digest(),
                baseline.adjudication_digest()
            );
        } else {
            baseline = Some(decision);
        }
    }
}

#[test]
fn v4_runtime_only_objective_head_does_not_rotate_decision_identity() {
    let human = "Build a persistent Discord game where every message earns XP, levels unlock an economy, timers advance quests, and an LLM decides rewards at event time. Quest timers must be durable, and the economy ledger must be persistent. Preserve state across restarts and do not reduce the request to static responses.";
    let requirements = [
        "an LLM decides rewards at event time",
        "every message earns XP",
        "levels unlock an economy",
        "timers advance quests",
    ];
    let mut baseline = core_value();
    baseline["automation_kind"] = json!("none");
    baseline["other_unmapped_required_capabilities"] = json!(requirements);
    let baseline = grounded_core_decision(&baseline, human);

    let mut summarized = core_value();
    summarized["automation_kind"] = json!("none");
    summarized["other_unmapped_required_capabilities"] = json!([
        "Build a persistent Discord game",
        "an LLM decides rewards at event time",
        "every message earns XP",
        "levels unlock an economy",
        "timers advance quests"
    ]);
    let summarized = grounded_core_decision(&summarized, human);

    assert_eq!(baseline.kind(), IntentRouteDecisionKindV2::CapabilityGap);
    assert_eq!(summarized.kind(), IntentRouteDecisionKindV2::CapabilityGap);
    assert_eq!(baseline.unclassified_requirements(), requirements);
    assert_eq!(summarized.unclassified_requirements(), requirements);
    assert_eq!(
        baseline.semantic_ir_digest(),
        summarized.semantic_ir_digest()
    );
    assert_eq!(
        baseline.adjudication_digest(),
        summarized.adjudication_digest()
    );
}

fn context() -> IntentResolutionContext {
    IntentResolutionContext::from_channel_bindings([ExistingChannelKey(
        "community_hub".to_string(),
    )])
}

fn bindings() -> ResourceBindingMap {
    let mut bindings = ResourceBindingMap::default();
    bindings.channel_bindings.insert(
        serde_json::from_value(json!("community_hub")).unwrap(),
        "700".parse().unwrap(),
    );
    bindings
}

#[test]
fn v4_default_path_reuses_the_existing_recipe_compiler() {
    let IntentCoreAdjudicationV4::PrivateStudyRoom(selection) = adjudicate_core(&core_value())
    else {
        panic!("expected private study-room selection");
    };
    assert!(selection.detail_facets().is_empty());
    let permit = selection.finalize(None).unwrap();
    let (_, PreparedIntentWorkspaceV2::Resolved { intent, .. }) =
        permit.prepare(&context()).unwrap()
    else {
        panic!("expected resolved recipe");
    };
    let compiled = compile_intent(&intent).unwrap();
    assert_eq!(compiled.requirements.len(), 22);
    assert_eq!(compiled.manifest.recipe_id, "starring.private_study_room");
}

#[test]
fn v2_and_v4_route_the_same_semantics_to_the_same_target() {
    let interpretation = parse_interpret_intent_turn(&v2_value().to_string()).unwrap();
    let IntentAdjudicationV2::PrivateStudyRoom(v2) = adjudicate_intent_v2(interpretation).unwrap()
    else {
        panic!("expected V2 private study-room permit");
    };
    let IntentCoreAdjudicationV4::PrivateStudyRoom(v4) = adjudicate_core(&core_value()) else {
        panic!("expected V4 private study-room selection");
    };
    assert_eq!(v2.decision().kind(), v4.decision().kind());
    assert_eq!(v2.decision().blockers(), v4.decision().blockers());
    assert_eq!(
        v2.decision().boundary_violations(),
        v4.decision().boundary_violations()
    );
    assert_eq!(
        v2.decision().route_target().unwrap().recipe_id(),
        v4.decision().route_target().unwrap().recipe_id()
    );
    assert_ne!(
        v2.decision().semantic_ir_digest(),
        v4.decision().semantic_ir_digest()
    );
    assert_eq!(
        v4.decision().request_evidence_hash(),
        Some(REQUEST_EVIDENCE_HASH)
    );
}

#[test]
fn pre_v4_legacy_equivalent_and_v4_emit_byte_identical_requirements_and_candidates() {
    block_on(async {
        let interpretation = parse_interpret_intent_turn(&v2_value().to_string()).unwrap();
        let IntentAdjudicationV2::PrivateStudyRoom(legacy_permit) =
            adjudicate_intent_v2(interpretation).unwrap()
        else {
            panic!("expected legacy private study-room permit");
        };
        let (_, PreparedIntentWorkspaceV2::Resolved { intent: legacy, .. }) =
            legacy_permit.prepare(&context()).unwrap()
        else {
            panic!("expected resolved legacy recipe");
        };
        let IntentCoreAdjudicationV4::PrivateStudyRoom(selection) = adjudicate_core(&core_value())
        else {
            panic!("expected V4 private study-room selection");
        };
        let (_, PreparedIntentWorkspaceV2::Resolved { intent: v4, .. }) = selection
            .finalize(None)
            .unwrap()
            .prepare(&context())
            .unwrap()
        else {
            panic!("expected resolved V4 recipe");
        };

        let legacy_compilation = compile_intent(&legacy).unwrap();
        let v4_compilation = compile_intent(&v4).unwrap();
        assert_eq!(
            serde_json::to_vec(&legacy_compilation.requirements).unwrap(),
            serde_json::to_vec(&v4_compilation.requirements).unwrap()
        );
        assert_eq!(
            legacy_compilation.manifest.semantic_intent_hash,
            v4_compilation.manifest.semantic_intent_hash
        );
        assert_eq!(
            legacy_compilation.manifest.compiled_plan_hash,
            v4_compilation.manifest.compiled_plan_hash
        );

        let root = Draft::new();
        let bindings = bindings();
        let legacy_candidate = prepare_intent_candidate(&root, &legacy, &bindings)
            .await
            .unwrap();
        let v4_candidate = prepare_intent_candidate(&root, &v4, &bindings)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_vec(&legacy_candidate.compilation().requirements).unwrap(),
            serde_json::to_vec(&v4_candidate.compilation().requirements).unwrap()
        );
        assert_eq!(
            serde_json::to_vec(&legacy_candidate.candidate().ruleset).unwrap(),
            serde_json::to_vec(&v4_candidate.candidate().ruleset).unwrap()
        );
        assert_eq!(
            candidate_ruleset_hash(legacy_candidate.candidate()).unwrap(),
            candidate_ruleset_hash(v4_candidate.candidate()).unwrap()
        );
        assert_eq!(legacy_candidate.execution(), v4_candidate.execution());
    });
}

#[test]
fn v4_digest_is_transport_independent_and_detail_sensitive() {
    let baseline = core_decision(&core_value());
    assert_eq!(baseline.semantic_ir_digest().len(), 64);
    assert_eq!(baseline.adjudication_digest().len(), 64);
    assert_eq!(
        baseline.request_evidence_hash(),
        Some(REQUEST_EVIDENCE_HASH)
    );
    let mut transport = core_value();
    transport["expected_revision"] = json!(91);
    transport["response"] = json!("ignored build prose");
    let transport = core_decision(&transport);
    assert_eq!(
        baseline.semantic_ir_digest(),
        transport.semantic_ir_digest()
    );
    assert_eq!(
        baseline.adjudication_digest(),
        transport.adjudication_digest()
    );

    let mut detailed = core_value();
    detailed["custom_detail_facets"] = json!(["custom_copy"]);
    let detailed = core_decision(&detailed);
    assert_ne!(baseline.semantic_ir_digest(), detailed.semantic_ir_digest());
    assert_ne!(
        baseline.adjudication_digest(),
        detailed.adjudication_digest()
    );
    assert_eq!(baseline.semantic_ir_digest().len(), 64);
    assert_eq!(baseline.adjudication_digest().len(), 64);
}

#[test]
fn v4_request_evidence_changes_audited_adjudication_not_route_semantics() {
    let baseline = core_decision(&core_value());
    let alternate_hash = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    let alternate = match adjudicate_core_with_evidence(&core_value(), alternate_hash) {
        IntentCoreAdjudicationV4::PrivateStudyRoom(selection) => selection.decision().clone(),
        _ => panic!("expected private study-room selection"),
    };
    assert_eq!(
        baseline.semantic_ir_digest(),
        alternate.semantic_ir_digest()
    );
    assert_ne!(
        baseline.adjudication_digest(),
        alternate.adjudication_digest()
    );
    assert_eq!(
        baseline.request_evidence_hash(),
        Some(REQUEST_EVIDENCE_HASH)
    );
    assert_eq!(alternate.request_evidence_hash(), Some(alternate_hash));
}

#[test]
fn v4_detail_path_binds_core_human_evidence_and_preserves_exact_literals() {
    let mut value = core_value();
    value["custom_detail_facets"] = json!(["custom_copy"]);
    let IntentCoreAdjudicationV4::PrivateStudyRoom(selection) = adjudicate_core(&value) else {
        panic!("expected private study-room selection");
    };
    let arguments = json!({
        "copy": {"create_button_label": "Start exact focus"},
        "naming": {},
        "controls": {},
        "unmapped_facets": []
    });
    let details = parse_private_study_room_details(
        &arguments.to_string(),
        &[IntentRecipeDetailFacetV3::Copy],
        selection.expected_revision(),
        selection.semantic_ir_digest(),
    )
    .unwrap();
    let detail_digest = selection
        .details_digest(SOURCE_HUMAN_TURN_DIGEST, &details)
        .unwrap();
    assert_eq!(detail_digest.len(), 64);
    assert_ne!(
        detail_digest,
        selection
            .details_digest(REQUEST_EVIDENCE_HASH, &details)
            .unwrap()
    );
    let permit = selection.finalize(Some(details)).unwrap();
    let (_, PreparedIntentWorkspaceV2::Resolved { intent, .. }) =
        permit.prepare(&context()).unwrap()
    else {
        panic!("expected resolved recipe");
    };
    let compiled = compile_intent(&intent).unwrap();
    assert!(serde_json::to_string(&compiled)
        .unwrap()
        .contains("Start exact focus"));
}

#[test]
fn v4_missing_spurious_and_contradictory_details_fail_closed() {
    let mut custom = core_value();
    custom["custom_detail_facets"] = json!(["custom_controls"]);
    let IntentCoreAdjudicationV4::PrivateStudyRoom(selection) = adjudicate_core(&custom) else {
        panic!("expected private study-room selection");
    };
    let Err(error) = selection.finalize(None) else {
        panic!("missing details must fail");
    };
    assert_eq!(error.code, "MISSING_RECIPE_DETAILS");

    let IntentCoreAdjudicationV4::PrivateStudyRoom(default_selection) =
        adjudicate_core(&core_value())
    else {
        panic!("expected private study-room selection");
    };
    let arguments = json!({
        "copy": {"create_button_label": "Spurious"},
        "naming": {},
        "controls": {},
        "unmapped_facets": []
    });
    let details = parse_private_study_room_details(
        &arguments.to_string(),
        &[IntentRecipeDetailFacetV3::Copy],
        0,
        default_selection.semantic_ir_digest(),
    )
    .unwrap();
    let Err(error) = default_selection.finalize(Some(details)) else {
        panic!("spurious details must fail");
    };
    assert_eq!(error.code, "UNEXPECTED_RECIPE_DETAILS");

    let IntentCoreAdjudicationV4::PrivateStudyRoom(selection) = adjudicate_core(&custom) else {
        panic!("expected private study-room selection");
    };
    let arguments = json!({
        "copy": {},
        "naming": {},
        "controls": {"close_label": "Close now"},
        "unmapped_facets": []
    });
    let details = parse_private_study_room_details(
        &arguments.to_string(),
        &[IntentRecipeDetailFacetV3::Controls],
        0,
        selection.semantic_ir_digest(),
    )
    .unwrap();
    let Err(error) = selection.finalize(Some(details)) else {
        panic!("contradictory close control must fail");
    };
    assert_eq!(error.code, "INCONSISTENT_RECIPE_CLOSE_CONTROL");
}

#[test]
fn v4_safety_and_capability_precedence_terminate_before_details() {
    let mut creator = core_value();
    creator["close_policy"] = json!("creator_only");
    creator["custom_detail_facets"] = json!(["custom_controls"]);
    let IntentCoreAdjudicationV4::Terminal(permit) = adjudicate_core(&creator) else {
        panic!("creator-only must terminate");
    };
    assert_eq!(
        permit.decision().kind(),
        IntentRouteDecisionKindV2::CapabilityGap
    );
    assert_eq!(
        permit.decision().blockers()[0].id,
        IntentCapabilityIdV2::InstanceCreatorTeardownAuthorization
    );

    let mut boundary = core_value();
    boundary["live_discord_mutation"] = json!("mutate_live_now");
    boundary["custom_detail_facets"] = json!(["custom_copy"]);
    let IntentCoreAdjudicationV4::Terminal(permit) = adjudicate_core(&boundary) else {
        panic!("boundary request must terminate");
    };
    assert_eq!(permit.decision().kind(), IntentRouteDecisionKindV2::Reject);
    assert_eq!(
        permit.decision().boundary_violations()[0].id,
        IntentSafetyBoundaryIdV2::DirectLiveMutation
    );
}
