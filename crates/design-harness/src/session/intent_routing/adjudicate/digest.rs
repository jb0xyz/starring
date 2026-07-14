use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::errors::StructuredError;
use crate::intent::{
    IntentCapabilityBlockerV2, IntentRequestedOutcome, IntentSafetyBoundaryViolationV2,
};
#[cfg(test)]
use crate::turn::IntentInterpretationV2;
use crate::turn::{
    CloseAuthorizationV2, EconomyRequirementV2, IntentAutomationKindV2, IntentBoundaryRequestV2,
    IntentCoreInterpretationV3, IntentLocaleHintV2, IntentRecipeDetailFacetV3, IntentRequestModeV2,
    PersistenceRequirementV2, PrivateStudyRoomDetailsV1, TimerRequirementV2,
};

#[cfg(test)]
use super::super::decision::INTENT_RECIPE_PROTOCOL_VERSION_V2;
use super::super::decision::{PinnedIntentRecipeV2, INTENT_RECIPE_PROTOCOL_VERSION_V3};
use super::adjudication_error;

#[cfg(test)]
const SEMANTIC_IR_DIGEST_DOMAIN_V2: &[u8] = b"starring.intent.semantic_ir.v2\0";
#[cfg(test)]
const ADJUDICATION_DIGEST_DOMAIN_V2: &[u8] = b"starring.intent.adjudication.v2\0";
const SEMANTIC_IR_DIGEST_DOMAIN_V3: &[u8] = b"starring.intent.semantic_ir.v3\0";
const ADJUDICATION_DIGEST_DOMAIN_V3: &[u8] = b"starring.intent.adjudication.v3\0";
const RECIPE_DETAILS_DIGEST_DOMAIN_V1: &[u8] = b"starring.intent.recipe_details.v1\0";

pub(super) struct AdjudicationDigestInputV2<'a> {
    pub(super) decision_source: &'static str,
    pub(super) adjudicator_version: u16,
    pub(super) kind: &'static str,
    pub(super) semantic_ir_digest: &'a str,
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

pub(super) fn semantic_ir_digest_v3(
    core: &IntentCoreInterpretationV3,
) -> Result<String, StructuredError> {
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
    let projection = CanonicalSemanticIrV3 {
        protocol_version: INTENT_RECIPE_PROTOCOL_VERSION_V3,
        request_mode: request_mode_wire(core.request_mode()),
        automation_kind: automation_kind_wire(core.automation_kind()),
        objective: core.objective(),
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
    };
    digest_serializable(
        SEMANTIC_IR_DIGEST_DOMAIN_V3,
        &projection,
        "INTENT_SEMANTIC_IR_SERIALIZATION_FAILED",
        "intent.semantic_ir",
    )
}

pub(super) fn adjudication_digest_v3(
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
        ADJUDICATION_DIGEST_DOMAIN_V3,
        &projection,
        "INTENT_ADJUDICATION_SERIALIZATION_FAILED",
        "intent.adjudication",
    )
}

pub(super) fn private_study_room_details_digest_v1(
    core_semantic_digest: &str,
    details: &PrivateStudyRoomDetailsV1,
) -> Result<String, StructuredError> {
    let projection = CanonicalRecipeDetailsV1 {
        recipe_id: crate::intent::PRIVATE_STUDY_ROOM_RECIPE_ID,
        recipe_version: crate::intent::PRIVATE_STUDY_ROOM_RECIPE_VERSION,
        core_semantic_digest,
        covered_facets: canonical_detail_facets(details.covered_facets()),
        copy: details.copy(),
        naming: details.naming(),
        controls: details.controls(),
    };
    digest_serializable(
        RECIPE_DETAILS_DIGEST_DOMAIN_V1,
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
    let value = serde_json::to_value(value).map_err(|error| {
        adjudication_error(
            code,
            location,
            "The deterministic intent projection could not be serialized",
            error.to_string(),
        )
    })?;
    let canonical = canonicalize_json(value);
    let bytes = serde_json::to_vec(&canonical).map_err(|error| {
        adjudication_error(
            code,
            location,
            "The canonical intent projection could not be serialized",
            error.to_string(),
        )
    })?;
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(bytes);
    Ok(hex_digest(hasher.finalize()))
}

fn canonicalize_json(value: Value) -> Value {
    match value {
        Value::Object(values) => {
            let mut entries = values.into_iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonicalize_json(value)))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize_json).collect()),
        value => value,
    }
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

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
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
struct CanonicalSemanticIrV3<'a> {
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
    detail_facets: Vec<&'static str>,
}

#[derive(Serialize)]
struct CanonicalRuntimeRequirementsV2 {
    persistence: &'static str,
    timers: &'static str,
    economy: &'static str,
    event_time_llm: bool,
}

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
struct CanonicalRecipeDetailsV1<'a> {
    recipe_id: &'static str,
    recipe_version: u32,
    core_semantic_digest: &'a str,
    covered_facets: Vec<&'static str>,
    copy: &'a crate::intent::PrivateStudyRoomCopyProposalV1,
    naming: &'a crate::intent::PrivateStudyRoomNamingProposalV1,
    controls: &'a crate::turn::PrivateStudyRoomControlsInterpretationV2,
}
