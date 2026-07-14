use serde_json::{json, Value};

use crate::intent::{
    compile_intent, ExistingChannelKey, IntentCapabilityIdV2, IntentResolutionContext,
    IntentSafetyBoundaryIdV2, PreparedIntentWorkspaceV1,
};
use crate::turn::{
    parse_interpret_intent_core, parse_interpret_intent_turn, parse_private_study_room_details,
    IntentRecipeDetailFacetV3,
};

use super::adjudicate::{
    adjudicate_intent_core_v3, adjudicate_intent_v2, IntentAdjudicationV2, IntentCoreAdjudicationV3,
};
use super::decision::{IntentRouteDecisionKindV2, IntentRouteDecisionV2};

fn core_value() -> Value {
    json!({
        "expected_revision": 0,
        "request_mode": "build",
        "automation_kind": "managed_private_study_room",
        "objective": "Create private study rooms",
        "requested_outcome": "validated_preview",
        "hub_channel": "community_hub",
        "language": "en",
        "close_policy": "disabled",
        "runtime_requirements": [],
        "explicit_boundary_requests": [],
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

fn adjudicate_core(value: &Value) -> IntentCoreAdjudicationV3 {
    let core = parse_interpret_intent_core(&value.to_string()).unwrap();
    adjudicate_intent_core_v3(core).unwrap()
}

fn core_decision(value: &Value) -> IntentRouteDecisionV2 {
    match adjudicate_core(value) {
        IntentCoreAdjudicationV3::PrivateStudyRoom(selection) => selection.decision().clone(),
        IntentCoreAdjudicationV3::TypedPlanner(permit) => permit.decision().clone(),
        IntentCoreAdjudicationV3::Terminal(permit) => permit.decision().clone(),
    }
}

fn context() -> IntentResolutionContext {
    IntentResolutionContext::from_channel_bindings([ExistingChannelKey(
        "community_hub".to_string(),
    )])
}

#[test]
fn v3_default_path_reuses_the_existing_recipe_compiler() {
    let IntentCoreAdjudicationV3::PrivateStudyRoom(selection) = adjudicate_core(&core_value())
    else {
        panic!("expected private study-room selection");
    };
    assert!(selection.detail_facets().is_empty());
    let permit = selection.finalize(None).unwrap();
    let (_, PreparedIntentWorkspaceV1::Resolved { intent, .. }) =
        permit.prepare(&context()).unwrap()
    else {
        panic!("expected resolved recipe");
    };
    let compiled = compile_intent(&intent).unwrap();
    assert_eq!(compiled.requirements.len(), 22);
    assert_eq!(compiled.manifest.recipe_id, "starring.private_study_room");
}

#[test]
fn v2_and_v3_route_the_same_semantics_to_the_same_target() {
    let interpretation = parse_interpret_intent_turn(&v2_value().to_string()).unwrap();
    let IntentAdjudicationV2::PrivateStudyRoom(v2) = adjudicate_intent_v2(interpretation).unwrap()
    else {
        panic!("expected V2 private study-room permit");
    };
    let IntentCoreAdjudicationV3::PrivateStudyRoom(v3) = adjudicate_core(&core_value()) else {
        panic!("expected V3 private study-room selection");
    };
    assert_eq!(v2.decision().kind(), v3.decision().kind());
    assert_eq!(v2.decision().blockers(), v3.decision().blockers());
    assert_eq!(
        v2.decision().boundary_violations(),
        v3.decision().boundary_violations()
    );
    assert_eq!(
        v2.decision().route_target().unwrap().recipe_id(),
        v3.decision().route_target().unwrap().recipe_id()
    );
    assert_ne!(
        v2.decision().semantic_ir_digest(),
        v3.decision().semantic_ir_digest()
    );
}

#[test]
fn v3_digest_is_transport_independent_and_detail_sensitive() {
    let baseline = core_decision(&core_value());
    assert_eq!(
        baseline.semantic_ir_digest(),
        "0be31e7271eb469b3e2f6fd26d9ad67d6a098207735126857a835c8480a225bb"
    );
    assert_eq!(
        baseline.adjudication_digest(),
        "d36e6b1032004ef49b9875515ef95f51d71e91b01f596c638625855ad34104ec"
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
fn v3_detail_path_binds_core_and_preserves_exact_literals() {
    let mut value = core_value();
    value["custom_detail_facets"] = json!(["custom_copy"]);
    let IntentCoreAdjudicationV3::PrivateStudyRoom(selection) = adjudicate_core(&value) else {
        panic!("expected private study-room selection");
    };
    let route_digest = selection.semantic_ir_digest().to_string();
    let arguments = json!({
        "expected_revision": selection.expected_revision(),
        "core_semantic_digest": route_digest,
        "copy": {"create_button_label": "Start exact focus"},
        "naming": {},
        "controls": {},
        "covered_facets": ["copy"],
        "unmapped_facets": []
    });
    let details = parse_private_study_room_details(
        &arguments.to_string(),
        &[IntentRecipeDetailFacetV3::Copy],
        selection.expected_revision(),
        selection.semantic_ir_digest(),
    )
    .unwrap();
    let detail_digest = selection.details_digest(&details).unwrap();
    assert_ne!(detail_digest, selection.semantic_ir_digest());
    let permit = selection.finalize(Some(details)).unwrap();
    let (_, PreparedIntentWorkspaceV1::Resolved { intent, .. }) =
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
fn v3_missing_spurious_and_contradictory_details_fail_closed() {
    let mut custom = core_value();
    custom["custom_detail_facets"] = json!(["custom_controls"]);
    let IntentCoreAdjudicationV3::PrivateStudyRoom(selection) = adjudicate_core(&custom) else {
        panic!("expected private study-room selection");
    };
    let Err(error) = selection.finalize(None) else {
        panic!("missing details must fail");
    };
    assert_eq!(error.code, "MISSING_RECIPE_DETAILS");

    let IntentCoreAdjudicationV3::PrivateStudyRoom(default_selection) =
        adjudicate_core(&core_value())
    else {
        panic!("expected private study-room selection");
    };
    let arguments = json!({
        "expected_revision": 0,
        "core_semantic_digest": default_selection.semantic_ir_digest(),
        "copy": {"create_button_label": "Spurious"},
        "naming": {},
        "controls": {},
        "covered_facets": ["copy"],
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

    let IntentCoreAdjudicationV3::PrivateStudyRoom(selection) = adjudicate_core(&custom) else {
        panic!("expected private study-room selection");
    };
    let arguments = json!({
        "expected_revision": 0,
        "core_semantic_digest": selection.semantic_ir_digest(),
        "copy": {},
        "naming": {},
        "controls": {"close_label": "Close now"},
        "covered_facets": ["controls"],
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
fn v3_safety_and_capability_precedence_terminate_before_details() {
    let mut creator = core_value();
    creator["close_policy"] = json!("creator_only");
    creator["custom_detail_facets"] = json!(["custom_controls"]);
    let IntentCoreAdjudicationV3::Terminal(permit) = adjudicate_core(&creator) else {
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
    boundary["explicit_boundary_requests"] = json!(["request_live_discord_mutation"]);
    boundary["custom_detail_facets"] = json!(["custom_copy"]);
    let IntentCoreAdjudicationV3::Terminal(permit) = adjudicate_core(&boundary) else {
        panic!("boundary request must terminate");
    };
    assert_eq!(permit.decision().kind(), IntentRouteDecisionKindV2::Reject);
    assert_eq!(
        permit.decision().boundary_violations()[0].id,
        IntentSafetyBoundaryIdV2::DirectLiveMutation
    );
}
