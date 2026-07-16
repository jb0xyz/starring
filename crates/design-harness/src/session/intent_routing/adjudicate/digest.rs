use std::collections::BTreeSet;

use serde::Serialize;

use crate::errors::StructuredError;
use crate::intent::identity::{canonical_json_digest, IdentityErrorSpec};
use crate::intent::{
    IntentCapabilityBlockerV2, IntentRequestedOutcome, IntentSafetyBoundaryViolationV2,
};
#[cfg(test)]
use crate::turn::IntentInterpretationV2;
use crate::turn::{
    CloseAuthorizationV2, EconomyRequirementV2, IntentAutomationKindV2, IntentBoundaryRequestV2,
    IntentCoreInterpretationV4, IntentLocaleHintV2, IntentRecipeDetailFacetV3, IntentRequestModeV2,
    PersistenceRequirementV2, PrivateStudyRoomDetailsV1, TimerRequirementV2,
};

#[cfg(test)]
use super::super::decision::INTENT_RECIPE_PROTOCOL_VERSION_V2;
use super::super::decision::{PinnedIntentRecipeV2, INTENT_RECIPE_PROTOCOL_VERSION_V4};
use super::adjudication_error;

#[cfg(test)]
const SEMANTIC_IR_DIGEST_DOMAIN_V2: &[u8] = b"starring.intent.semantic_ir.v2\0";
#[cfg(test)]
const ADJUDICATION_DIGEST_DOMAIN_V2: &[u8] = b"starring.intent.adjudication.v2\0";
const SEMANTIC_IR_DIGEST_DOMAIN_V4: &[u8] = b"starring.intent.semantic_ir.v4\0";
const ADJUDICATION_DIGEST_DOMAIN_V4: &[u8] = b"starring.intent.adjudication.v4\0";
const RECIPE_DETAILS_DIGEST_DOMAIN_V2: &[u8] = b"starring.intent.recipe_details.v2\0";

pub(super) struct AdjudicationDigestInputV2<'a> {
    pub(super) decision_source: &'static str,
    pub(super) adjudicator_version: u16,
    pub(super) kind: &'static str,
    pub(super) semantic_ir_digest: &'a str,
    pub(super) request_evidence_hash: Option<&'a str>,
    pub(super) manifest_version: u16,
    pub(super) manifest_digest: &'a str,
    pub(super) blockers: &'a [IntentCapabilityBlockerV2],
    pub(super) boundary_violations: &'a [IntentSafetyBoundaryViolationV2],
    pub(super) unclassified_requirements: &'a [String],
    pub(super) route_target: Option<&'a PinnedIntentRecipeV2>,
}

#[cfg(test)]
pub(super) fn semantic_ir_digest_v2(
    interpretation: &IntentInterpretationV2,
) -> Result<String, StructuredError> {
    let mut boundary_requests = interpretation
        .boundary_requests()
        .iter()
        .map(|value| boundary_request_wire(*value))
        .collect::<Vec<_>>();
    boundary_requests.sort_unstable();
    boundary_requests.dedup();
    let unclassified_requirements = canonical_unclassified_requirements(interpretation);
    let projection = CanonicalSemanticIrV2 {
        protocol_version: INTENT_RECIPE_PROTOCOL_VERSION_V2,
        request_mode: request_mode_wire(interpretation.request_mode()),
        automation_kind: automation_kind_wire(interpretation.automation_kind()),
        objective: interpretation.objective(),
        requested_outcome: requested_outcome_wire(interpretation.requested_outcome()),
        hub_channel: interpretation.hub_channel().map(|value| value.as_str()),
        locale: locale_wire(interpretation.locale()),
        close_authorization: close_authorization_wire(interpretation.close_authorization()),
        runtime_requirements: CanonicalRuntimeRequirementsV2 {
            persistence: persistence_wire(interpretation.runtime_requirements().persistence),
            timers: timer_wire(interpretation.runtime_requirements().timers),
            economy: economy_wire(interpretation.runtime_requirements().economy),
            event_time_llm: interpretation.runtime_requirements().event_time_llm,
        },
        boundary_requests,
        unclassified_requirements,
        response_locale: locale_wire(interpretation.response_locale()),
        copy: interpretation.copy(),
        naming: interpretation.naming(),
        controls: interpretation.controls(),
    };
    digest_serializable(
        SEMANTIC_IR_DIGEST_DOMAIN_V2,
        &projection,
        "INTENT_SEMANTIC_IR_SERIALIZATION_FAILED",
        "intent.semantic_ir",
    )
}

#[cfg(test)]
pub(super) fn adjudication_digest_v2(
    input: AdjudicationDigestInputV2<'_>,
) -> Result<String, StructuredError> {
    let projection = CanonicalAdjudicationV2 {
        decision_source: input.decision_source,
        adjudicator_version: input.adjudicator_version,
        kind: input.kind,
        semantic_ir_digest: input.semantic_ir_digest,
        manifest_version: input.manifest_version,
        manifest_digest: input.manifest_digest,
        blockers: input.blockers,
        boundary_violations: input.boundary_violations,
        unclassified_requirements: input.unclassified_requirements,
        route_target: input.route_target.map(|target| CanonicalRouteTargetV2 {
            kind: "recipe",
            recipe_id: target.recipe_id(),
            recipe_version: target.recipe_version(),
        }),
    };
    digest_serializable(
        ADJUDICATION_DIGEST_DOMAIN_V2,
        &projection,
        "INTENT_ADJUDICATION_SERIALIZATION_FAILED",
        "intent.adjudication",
    )
}

pub(super) fn semantic_ir_digest_v4(
    core: &IntentCoreInterpretationV4,
) -> Result<String, StructuredError> {
    let projection = canonical_semantic_ir_v4(core);
    digest_serializable(
        SEMANTIC_IR_DIGEST_DOMAIN_V4,
        &projection,
        "INTENT_SEMANTIC_IR_SERIALIZATION_FAILED",
        "intent.semantic_ir",
    )
}

fn canonical_semantic_ir_v4(core: &IntentCoreInterpretationV4) -> CanonicalSemanticIrV4<'_> {
    let mut boundary_requests = core
        .boundary_requests()
        .iter()
        .map(|value| boundary_request_wire(*value))
        .collect::<Vec<_>>();
    boundary_requests.sort_unstable();
    boundary_requests.dedup();
    let unclassified_requirements = core
        .unclassified_requirements()
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let detail_facets = canonical_detail_facets(core.recipe_detail_facets());
    CanonicalSemanticIrV4 {
        protocol_version: INTENT_RECIPE_PROTOCOL_VERSION_V4,
        request_mode: request_mode_wire(core.request_mode()),
        automation_kind: automation_kind_wire(core.automation_kind()),
        requested_outcome: requested_outcome_wire(core.requested_outcome()),
        hub_channel: core.selected_existing_channel().map(|value| value.as_str()),
        locale: locale_wire(core.locale()),
        close_authorization: close_authorization_wire(core.close_authorization()),
        runtime_requirements: CanonicalRuntimeRequirementsV2 {
            persistence: persistence_wire(core.runtime_requirements().persistence),
            timers: timer_wire(core.runtime_requirements().timers),
            economy: economy_wire(core.runtime_requirements().economy),
            event_time_llm: core.runtime_requirements().event_time_llm,
        },
        boundary_requests,
        unclassified_requirements,
        detail_facets,
    }
}

pub(super) fn adjudication_digest_v4(
    input: AdjudicationDigestInputV2<'_>,
) -> Result<String, StructuredError> {
    let projection = canonical_adjudication_v4(input)?;
    digest_serializable(
        ADJUDICATION_DIGEST_DOMAIN_V4,
        &projection,
        "INTENT_ADJUDICATION_SERIALIZATION_FAILED",
        "intent.adjudication",
    )
}

fn canonical_adjudication_v4<'a>(
    input: AdjudicationDigestInputV2<'a>,
) -> Result<CanonicalAdjudicationV4<'a>, StructuredError> {
    let request_evidence_hash = input.request_evidence_hash.ok_or_else(|| {
        adjudication_error(
            "INTENT_REQUEST_EVIDENCE_MISSING",
            "intent.adjudication.request_evidence_hash",
            "Protocol V4 adjudication requires initial request evidence",
            "Bind the decision to the accepted initial human evidence chain",
        )
    })?;
    Ok(CanonicalAdjudicationV4 {
        decision_source: input.decision_source,
        adjudicator_version: input.adjudicator_version,
        kind: input.kind,
        semantic_ir_digest: input.semantic_ir_digest,
        request_evidence_hash,
        manifest_version: input.manifest_version,
        manifest_digest: input.manifest_digest,
        blockers: input.blockers,
        boundary_violations: input.boundary_violations,
        unclassified_requirements: input.unclassified_requirements,
        route_target: input.route_target.map(|target| CanonicalRouteTargetV2 {
            kind: "recipe",
            recipe_id: target.recipe_id(),
            recipe_version: target.recipe_version(),
        }),
    })
}

pub(super) fn private_study_room_details_digest_v2(
    core_semantic_digest: &str,
    source_human_turn_digest: &str,
    details: &PrivateStudyRoomDetailsV1,
) -> Result<String, StructuredError> {
    let projection = CanonicalRecipeDetailsV2 {
        recipe_id: crate::intent::PRIVATE_STUDY_ROOM_RECIPE_ID,
        recipe_version: crate::intent::PRIVATE_STUDY_ROOM_RECIPE_VERSION,
        core_semantic_digest,
        source_human_turn_digest,
        covered_facets: canonical_detail_facets(details.covered_facets()),
        copy: details.copy(),
        naming: details.naming(),
        controls: details.controls(),
    };
    digest_serializable(
        RECIPE_DETAILS_DIGEST_DOMAIN_V2,
        &projection,
        "INTENT_RECIPE_DETAILS_SERIALIZATION_FAILED",
        "intent.recipe_details",
    )
}

#[cfg(test)]
pub(super) fn canonical_unclassified_requirements(
    interpretation: &IntentInterpretationV2,
) -> Vec<String> {
    interpretation
        .unclassified_requirements()
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn digest_serializable(
    domain: &[u8],
    value: &impl Serialize,
    code: &str,
    location: &str,
) -> Result<String, StructuredError> {
    canonical_json_digest(
        domain,
        value,
        IdentityErrorSpec::new(
            code,
            location,
            "The deterministic intent projection could not be serialized",
        ),
    )
}

fn request_mode_wire(value: IntentRequestModeV2) -> &'static str {
    match value {
        IntentRequestModeV2::Discussion => "discussion",
        IntentRequestModeV2::Build => "build",
    }
}

fn automation_kind_wire(value: IntentAutomationKindV2) -> &'static str {
    match value {
        IntentAutomationKindV2::ManagedPrivateStudyRoom => "managed_private_study_room",
        IntentAutomationKindV2::CustomAutomation => "custom_automation",
        IntentAutomationKindV2::None => "none",
    }
}

fn requested_outcome_wire(value: IntentRequestedOutcome) -> &'static str {
    match value {
        IntentRequestedOutcome::Discussion => "discussion",
        IntentRequestedOutcome::WorkingDraft => "working_draft",
        IntentRequestedOutcome::ValidatedPreview => "validated_preview",
    }
}

fn locale_wire(value: IntentLocaleHintV2) -> &'static str {
    match value {
        IntentLocaleHintV2::En => "en",
        IntentLocaleHintV2::Ko => "ko",
        IntentLocaleHintV2::Unspecified => "unspecified",
    }
}

fn close_authorization_wire(value: CloseAuthorizationV2) -> &'static str {
    match value {
        CloseAuthorizationV2::NotRequested => "not_requested",
        CloseAuthorizationV2::Disabled => "disabled",
        CloseAuthorizationV2::AnyMember => "any_member",
        CloseAuthorizationV2::CreatorOnly => "creator_only",
    }
}

fn persistence_wire(value: PersistenceRequirementV2) -> &'static str {
    match value {
        PersistenceRequirementV2::None => "none",
        PersistenceRequirementV2::RestartPersistent => "restart_persistent",
    }
}

fn timer_wire(value: TimerRequirementV2) -> &'static str {
    match value {
        TimerRequirementV2::None => "none",
        TimerRequirementV2::Durable => "durable",
    }
}

fn economy_wire(value: EconomyRequirementV2) -> &'static str {
    match value {
        EconomyRequirementV2::None => "none",
        EconomyRequirementV2::PersistentLedger => "persistent_ledger",
    }
}

fn boundary_request_wire(value: IntentBoundaryRequestV2) -> &'static str {
    match value {
        IntentBoundaryRequestV2::DirectLiveMutation => "direct_live_mutation",
        IntentBoundaryRequestV2::BypassValidationPreviewApproval => {
            "bypass_validation_preview_approval"
        }
        IntentBoundaryRequestV2::SecretDisclosure => "secret_disclosure",
    }
}

fn canonical_detail_facets(values: &[IntentRecipeDetailFacetV3]) -> Vec<&'static str> {
    values
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(detail_facet_wire)
        .collect()
}

fn detail_facet_wire(value: IntentRecipeDetailFacetV3) -> &'static str {
    match value {
        IntentRecipeDetailFacetV3::Copy => "copy",
        IntentRecipeDetailFacetV3::Naming => "naming",
        IntentRecipeDetailFacetV3::Controls => "controls",
    }
}

#[cfg(test)]
#[derive(Serialize)]
struct CanonicalSemanticIrV2<'a> {
    protocol_version: u16,
    request_mode: &'static str,
    automation_kind: &'static str,
    objective: &'a str,
    requested_outcome: &'static str,
    hub_channel: Option<&'a str>,
    locale: &'static str,
    close_authorization: &'static str,
    runtime_requirements: CanonicalRuntimeRequirementsV2,
    boundary_requests: Vec<&'static str>,
    unclassified_requirements: Vec<String>,
    response_locale: &'static str,
    copy: &'a crate::intent::PrivateStudyRoomCopyProposalV1,
    naming: &'a crate::intent::PrivateStudyRoomNamingProposalV1,
    controls: &'a crate::turn::PrivateStudyRoomControlsInterpretationV2,
}

#[derive(Serialize)]
struct CanonicalSemanticIrV4<'a> {
    protocol_version: u16,
    request_mode: &'static str,
    automation_kind: &'static str,
    requested_outcome: &'static str,
    hub_channel: Option<&'a str>,
    locale: &'static str,
    close_authorization: &'static str,
    runtime_requirements: CanonicalRuntimeRequirementsV2,
    boundary_requests: Vec<&'static str>,
    unclassified_requirements: Vec<String>,
    detail_facets: Vec<&'static str>,
}

#[derive(Serialize)]
struct CanonicalAdjudicationV4<'a> {
    decision_source: &'static str,
    adjudicator_version: u16,
    kind: &'static str,
    semantic_ir_digest: &'a str,
    request_evidence_hash: &'a str,
    manifest_version: u16,
    manifest_digest: &'a str,
    blockers: &'a [IntentCapabilityBlockerV2],
    boundary_violations: &'a [IntentSafetyBoundaryViolationV2],
    unclassified_requirements: &'a [String],
    route_target: Option<CanonicalRouteTargetV2<'a>>,
}

#[derive(Serialize)]
struct CanonicalRuntimeRequirementsV2 {
    persistence: &'static str,
    timers: &'static str,
    economy: &'static str,
    event_time_llm: bool,
}

#[cfg(test)]
#[derive(Serialize)]
struct CanonicalAdjudicationV2<'a> {
    decision_source: &'static str,
    adjudicator_version: u16,
    kind: &'static str,
    semantic_ir_digest: &'a str,
    manifest_version: u16,
    manifest_digest: &'a str,
    blockers: &'a [IntentCapabilityBlockerV2],
    boundary_violations: &'a [IntentSafetyBoundaryViolationV2],
    unclassified_requirements: &'a [String],
    route_target: Option<CanonicalRouteTargetV2<'a>>,
}

#[derive(Serialize)]
struct CanonicalRouteTargetV2<'a> {
    kind: &'static str,
    recipe_id: &'a str,
    recipe_version: u32,
}

#[derive(Serialize)]
struct CanonicalRecipeDetailsV2<'a> {
    recipe_id: &'static str,
    recipe_version: u32,
    core_semantic_digest: &'a str,
    source_human_turn_digest: &'a str,
    covered_facets: Vec<&'static str>,
    copy: &'a crate::intent::PrivateStudyRoomCopyProposalV1,
    naming: &'a crate::intent::PrivateStudyRoomNamingProposalV1,
    controls: &'a crate::turn::PrivateStudyRoomControlsInterpretationV2,
}

#[cfg(test)]
mod v4_golden_tests {
    use serde_json::{json, Value};

    use crate::session::intent_routing::IntentRouteDecisionV2;
    use crate::turn::parse_interpret_intent_core_compatibility;

    use super::super::{adjudicate_intent_core_v4, IntentCoreAdjudicationV4};
    use super::{
        canonical_adjudication_v4, canonical_semantic_ir_v4, AdjudicationDigestInputV2,
        ADJUDICATION_DIGEST_DOMAIN_V4, SEMANTIC_IR_DIGEST_DOMAIN_V4,
    };

    const REQUEST_EVIDENCE_HASH: &str =
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    fn core_value() -> Value {
        json!({
            "expected_revision": 0,
            "request_mode": "build",
            "automation_kind": "managed_private_study_room",
            "requested_outcome": "validated_preview",
            "hub_channel": "community_hub",
            "language": "en",
            "close_policy": "disabled",
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

    fn decision(value: &Value) -> IntentRouteDecisionV2 {
        let core = parse_interpret_intent_core_compatibility(&value.to_string()).unwrap();
        match adjudicate_intent_core_v4(core, REQUEST_EVIDENCE_HASH).unwrap() {
            IntentCoreAdjudicationV4::PrivateStudyRoom(selection) => selection.decision().clone(),
            _ => panic!("expected private study-room selection"),
        }
    }

    fn adjudication_projection(decision: &IntentRouteDecisionV2) -> Value {
        serde_json::to_value(
            canonical_adjudication_v4(AdjudicationDigestInputV2 {
                decision_source: decision.decision_source().as_str(),
                adjudicator_version: decision.adjudicator_version(),
                kind: decision.kind().as_str(),
                semantic_ir_digest: decision.semantic_ir_digest(),
                request_evidence_hash: decision.request_evidence_hash(),
                manifest_version: decision.manifest_version(),
                manifest_digest: decision.manifest_digest(),
                blockers: decision.blockers(),
                boundary_violations: decision.boundary_violations(),
                unclassified_requirements: decision.unclassified_requirements(),
                route_target: decision.route_target(),
            })
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn v4_identity_domains_projections_and_digests_are_exact_goldens() {
        assert_eq!(
            SEMANTIC_IR_DIGEST_DOMAIN_V4,
            b"starring.intent.semantic_ir.v4\0"
        );
        assert_eq!(
            ADJUDICATION_DIGEST_DOMAIN_V4,
            b"starring.intent.adjudication.v4\0"
        );

        let value = core_value();
        let core = parse_interpret_intent_core_compatibility(&value.to_string()).unwrap();
        assert_eq!(
            serde_json::to_value(canonical_semantic_ir_v4(&core)).unwrap(),
            json!({
                "protocol_version": 4,
                "request_mode": "build",
                "automation_kind": "managed_private_study_room",
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
                "detail_facets": []
            })
        );
        let baseline = decision(&value);
        assert_eq!(
            baseline.semantic_ir_digest(),
            "9edb6a99acc1119ebbeaca929c6c26c11e00919dd217e278c520f65272df115e"
        );
        assert_eq!(
            adjudication_projection(&baseline),
            json!({
                "decision_source": "deterministic_intent_adjudicator",
                "adjudicator_version": 3,
                "kind": "private_study_room",
                "semantic_ir_digest": "9edb6a99acc1119ebbeaca929c6c26c11e00919dd217e278c520f65272df115e",
                "request_evidence_hash": REQUEST_EVIDENCE_HASH,
                "manifest_version": 1,
                "manifest_digest": "68de3f4d9355c99b213ba7546f41a772cd21e59ac4f750cc5ff33d99a0cc5d53",
                "blockers": [],
                "boundary_violations": [],
                "unclassified_requirements": [],
                "route_target": {
                    "kind": "recipe",
                    "recipe_id": "starring.private_study_room",
                    "recipe_version": 1
                }
            })
        );
        assert_eq!(
            baseline.adjudication_digest(),
            "45ea6fb31cc2f205995f040d352d5cabe7d1ed803cffc92249a4efa54c57fe4a"
        );

        let mut detailed_value = core_value();
        detailed_value["custom_detail_facets"] = json!(["custom_copy"]);
        let detailed = decision(&detailed_value);
        assert_eq!(
            detailed.semantic_ir_digest(),
            "1342e9ed2325f2db5318f9b18c5f7eed830d07ca28ef3834f4134bccc17394d2"
        );
        assert_eq!(
            detailed.adjudication_digest(),
            "0ecfc354461649409df44e50e2296ff0332188455336aae848ca7e7d52489b99"
        );
        assert_ne!(baseline.semantic_ir_digest(), detailed.semantic_ir_digest());
        assert_ne!(
            baseline.adjudication_digest(),
            detailed.adjudication_digest()
        );
    }
}
