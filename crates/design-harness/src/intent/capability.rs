use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::errors::StructuredError;

use super::identity::{compatibility_json_digest, IdentityErrorSpec};

pub const INTENT_CAPABILITY_MANIFEST_VERSION_V2: u16 = 1;

const CAPABILITY_MANIFEST_DIGEST_DOMAIN_V2: &[u8] = b"starring.intent.capability_manifest.v2\0";

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum IntentCapabilityIdV2 {
    DurableTimer,
    EventTimeLlmDecision,
    InstanceCreatorTeardownAuthorization,
    PersistentEconomyLedger,
    RestartPersistentState,
    UnclassifiedIntentRequirement,
}

impl IntentCapabilityIdV2 {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DurableTimer => "durable_timer",
            Self::EventTimeLlmDecision => "event_time_llm_decision",
            Self::InstanceCreatorTeardownAuthorization => "instance_creator_teardown_authorization",
            Self::PersistentEconomyLedger => "persistent_economy_ledger",
            Self::RestartPersistentState => "restart_persistent_state",
            Self::UnclassifiedIntentRequirement => "unclassified_intent_requirement",
        }
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatusV2 {
    Available,
    Unavailable,
    ForbiddenPolicy,
    Unclassified,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityPolicyIdV2 {
    EventTimeLlmExecutionForbiddenV1,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum IntentSafetyBoundaryIdV2 {
    BypassValidationPreviewApproval,
    DirectLiveMutation,
    SecretDisclosure,
}

impl IntentSafetyBoundaryIdV2 {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BypassValidationPreviewApproval => "bypass_validation_preview_approval",
            Self::DirectLiveMutation => "direct_live_mutation",
            Self::SecretDisclosure => "secret_disclosure",
        }
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum IntentRouteEffectV2 {
    CapabilityGap,
    Reject,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalizedIntentLabelV2 {
    pub en: String,
    pub ko: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityDescriptorV2 {
    pub id: IntentCapabilityIdV2,
    pub status: CapabilityStatusV2,
    pub policy_id: Option<CapabilityPolicyIdV2>,
    pub route_effect: IntentRouteEffectV2,
    pub label: LocalizedIntentLabelV2,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SafetyBoundaryDescriptorV2 {
    pub id: IntentSafetyBoundaryIdV2,
    pub route_effect: IntentRouteEffectV2,
    pub label: LocalizedIntentLabelV2,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityManifestV2 {
    pub version: u16,
    pub capabilities: Vec<CapabilityDescriptorV2>,
    pub safety_boundaries: Vec<SafetyBoundaryDescriptorV2>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IntentRequirementEvidenceV2 {
    pub semantic_path: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IntentCapabilityRequirementV2 {
    pub id: IntentCapabilityIdV2,
    pub evidence: IntentRequirementEvidenceV2,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IntentSafetyBoundaryRequestV2 {
    pub id: IntentSafetyBoundaryIdV2,
    pub evidence: IntentRequirementEvidenceV2,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IntentCapabilityBlockerV2 {
    pub id: IntentCapabilityIdV2,
    pub status: CapabilityStatusV2,
    pub policy_id: Option<CapabilityPolicyIdV2>,
    pub evidence: Vec<IntentRequirementEvidenceV2>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IntentSafetyBoundaryViolationV2 {
    pub id: IntentSafetyBoundaryIdV2,
    pub evidence: Vec<IntentRequirementEvidenceV2>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IntentCapabilityAssessmentV2 {
    pub manifest_version: u16,
    pub manifest_digest: String,
    pub blockers: Vec<IntentCapabilityBlockerV2>,
    pub boundary_violations: Vec<IntentSafetyBoundaryViolationV2>,
}

impl IntentCapabilityAssessmentV2 {
    pub fn route_effect(&self) -> Option<IntentRouteEffectV2> {
        if !self.boundary_violations.is_empty() {
            Some(IntentRouteEffectV2::Reject)
        } else if !self.blockers.is_empty() {
            Some(IntentRouteEffectV2::CapabilityGap)
        } else {
            None
        }
    }
}

pub fn intent_capability_manifest_v2() -> CapabilityManifestV2 {
    let mut capabilities = vec![
        capability_descriptor(
            IntentCapabilityIdV2::InstanceCreatorTeardownAuthorization,
            CapabilityStatusV2::Unavailable,
            None,
            "Creator-only room teardown authorization",
            "방 생성자 전용 종료 권한",
        ),
        capability_descriptor(
            IntentCapabilityIdV2::RestartPersistentState,
            CapabilityStatusV2::Unavailable,
            None,
            "State preserved across restarts",
            "재시작 후에도 보존되는 상태",
        ),
        capability_descriptor(
            IntentCapabilityIdV2::DurableTimer,
            CapabilityStatusV2::Unavailable,
            None,
            "Durable timers",
            "영속 타이머",
        ),
        capability_descriptor(
            IntentCapabilityIdV2::PersistentEconomyLedger,
            CapabilityStatusV2::Unavailable,
            None,
            "Persistent economy ledger",
            "영속 경제 원장",
        ),
        capability_descriptor(
            IntentCapabilityIdV2::EventTimeLlmDecision,
            CapabilityStatusV2::ForbiddenPolicy,
            Some(CapabilityPolicyIdV2::EventTimeLlmExecutionForbiddenV1),
            "Event-time LLM decisions",
            "이벤트 시점 LLM 결정",
        ),
        capability_descriptor(
            IntentCapabilityIdV2::UnclassifiedIntentRequirement,
            CapabilityStatusV2::Unclassified,
            None,
            "Unclassified hard requirement",
            "분류되지 않은 필수 요구사항",
        ),
    ];
    capabilities.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    let mut safety_boundaries = vec![
        safety_boundary_descriptor(
            IntentSafetyBoundaryIdV2::DirectLiveMutation,
            "Direct live mutation",
            "직접 라이브 변경",
        ),
        safety_boundary_descriptor(
            IntentSafetyBoundaryIdV2::BypassValidationPreviewApproval,
            "Bypass validation, preview, and approval",
            "검증, 미리보기, 승인 우회",
        ),
        safety_boundary_descriptor(
            IntentSafetyBoundaryIdV2::SecretDisclosure,
            "Secret disclosure",
            "비밀정보 노출",
        ),
    ];
    safety_boundaries.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    CapabilityManifestV2 {
        version: INTENT_CAPABILITY_MANIFEST_VERSION_V2,
        capabilities,
        safety_boundaries,
    }
}

pub fn intent_capability_manifest_digest_v2(
    manifest: &CapabilityManifestV2,
) -> Result<String, StructuredError> {
    let canonical = canonical_manifest(manifest)?;
    compatibility_json_digest(
        CAPABILITY_MANIFEST_DIGEST_DOMAIN_V2,
        &canonical,
        IdentityErrorSpec::new(
            "INTENT_CAPABILITY_MANIFEST_SERIALIZATION_FAILED",
            "intent.capability_manifest",
            "The capability manifest could not be serialized deterministically",
        ),
    )
}

pub fn assess_intent_capabilities_v2(
    requirements: &[IntentCapabilityRequirementV2],
    boundary_requests: &[IntentSafetyBoundaryRequestV2],
) -> Result<IntentCapabilityAssessmentV2, StructuredError> {
    let manifest = intent_capability_manifest_v2();
    let manifest_digest = intent_capability_manifest_digest_v2(&manifest)?;
    let descriptors = manifest
        .capabilities
        .iter()
        .map(|descriptor| (descriptor.id.as_str(), descriptor))
        .collect::<BTreeMap<_, _>>();
    let mut capability_evidence =
        BTreeMap::<&str, (IntentCapabilityIdV2, BTreeSet<IntentRequirementEvidenceV2>)>::new();
    for requirement in requirements {
        let descriptor = descriptors.get(requirement.id.as_str()).ok_or_else(|| {
            capability_error(
                "UNKNOWN_INTENT_CAPABILITY",
                "intent.capability_requirements",
                format!(
                    "Capability {} is not in the current manifest",
                    requirement.id.as_str()
                ),
                "Use one capability identifier from the current manifest",
            )
        })?;
        capability_evidence
            .entry(descriptor.id.as_str())
            .or_insert_with(|| (descriptor.id, BTreeSet::new()))
            .1
            .insert(requirement.evidence.clone());
    }
    let blockers = capability_evidence
        .into_values()
        .filter_map(|(id, evidence)| {
            let descriptor = descriptors.get(id.as_str())?;
            (descriptor.status != CapabilityStatusV2::Available).then(|| {
                IntentCapabilityBlockerV2 {
                    id,
                    status: descriptor.status,
                    policy_id: descriptor.policy_id,
                    evidence: evidence.into_iter().collect(),
                }
            })
        })
        .collect();
    let boundary_descriptors = manifest
        .safety_boundaries
        .iter()
        .map(|descriptor| (descriptor.id.as_str(), descriptor))
        .collect::<BTreeMap<_, _>>();
    let mut boundary_evidence = BTreeMap::<
        &str,
        (
            IntentSafetyBoundaryIdV2,
            BTreeSet<IntentRequirementEvidenceV2>,
        ),
    >::new();
    for request in boundary_requests {
        let descriptor = boundary_descriptors
            .get(request.id.as_str())
            .ok_or_else(|| {
                capability_error(
                    "UNKNOWN_INTENT_SAFETY_BOUNDARY",
                    "intent.safety_boundary_requests",
                    format!(
                        "Safety boundary {} is not in the current manifest",
                        request.id.as_str()
                    ),
                    "Use one safety-boundary identifier from the current manifest",
                )
            })?;
        boundary_evidence
            .entry(descriptor.id.as_str())
            .or_insert_with(|| (descriptor.id, BTreeSet::new()))
            .1
            .insert(request.evidence.clone());
    }
    let boundary_violations = boundary_evidence
        .into_values()
        .map(|(id, evidence)| IntentSafetyBoundaryViolationV2 {
            id,
            evidence: evidence.into_iter().collect(),
        })
        .collect();
    Ok(IntentCapabilityAssessmentV2 {
        manifest_version: manifest.version,
        manifest_digest,
        blockers,
        boundary_violations,
    })
}

fn capability_descriptor(
    id: IntentCapabilityIdV2,
    status: CapabilityStatusV2,
    policy_id: Option<CapabilityPolicyIdV2>,
    en: &str,
    ko: &str,
) -> CapabilityDescriptorV2 {
    CapabilityDescriptorV2 {
        id,
        status,
        policy_id,
        route_effect: IntentRouteEffectV2::CapabilityGap,
        label: localized_label(en, ko),
    }
}

fn safety_boundary_descriptor(
    id: IntentSafetyBoundaryIdV2,
    en: &str,
    ko: &str,
) -> SafetyBoundaryDescriptorV2 {
    SafetyBoundaryDescriptorV2 {
        id,
        route_effect: IntentRouteEffectV2::Reject,
        label: localized_label(en, ko),
    }
}

fn localized_label(en: &str, ko: &str) -> LocalizedIntentLabelV2 {
    LocalizedIntentLabelV2 {
        en: en.to_string(),
        ko: ko.to_string(),
    }
}

fn canonical_manifest(
    manifest: &CapabilityManifestV2,
) -> Result<CapabilityManifestV2, StructuredError> {
    if manifest.version != INTENT_CAPABILITY_MANIFEST_VERSION_V2 {
        return Err(capability_error(
            "UNSUPPORTED_INTENT_CAPABILITY_MANIFEST_VERSION",
            "intent.capability_manifest.version",
            format!(
                "Capability manifest version {} is not supported",
                manifest.version
            ),
            format!("Use manifest version {INTENT_CAPABILITY_MANIFEST_VERSION_V2}"),
        ));
    }
    let mut canonical = manifest.clone();
    canonical
        .capabilities
        .sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    canonical
        .safety_boundaries
        .sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    validate_capabilities(&canonical.capabilities)?;
    validate_safety_boundaries(&canonical.safety_boundaries)?;
    Ok(canonical)
}

fn validate_capabilities(capabilities: &[CapabilityDescriptorV2]) -> Result<(), StructuredError> {
    let expected = intent_capability_manifest_v2()
        .capabilities
        .into_iter()
        .map(|descriptor| (descriptor.id.as_str(), descriptor))
        .collect::<BTreeMap<_, _>>();
    let mut observed = BTreeSet::new();
    for descriptor in capabilities {
        if !observed.insert(descriptor.id.as_str()) {
            return Err(capability_error(
                "DUPLICATE_INTENT_CAPABILITY_ID",
                "intent.capability_manifest.capabilities",
                format!("Capability {} is repeated", descriptor.id.as_str()),
                "Declare each capability identifier exactly once",
            ));
        }
        let expected_descriptor = expected.get(descriptor.id.as_str()).ok_or_else(|| {
            capability_error(
                "UNKNOWN_INTENT_CAPABILITY",
                "intent.capability_manifest.capabilities",
                format!("Capability {} is not recognized", descriptor.id.as_str()),
                "Use the closed Intent V2 capability catalog",
            )
        })?;
        if descriptor.status != expected_descriptor.status
            || descriptor.policy_id != expected_descriptor.policy_id
            || descriptor.route_effect != IntentRouteEffectV2::CapabilityGap
            || descriptor.label.en.is_empty()
            || descriptor.label.ko.is_empty()
        {
            return Err(capability_error(
                "INVALID_INTENT_CAPABILITY_DESCRIPTOR",
                format!(
                    "intent.capability_manifest.capabilities.{}",
                    descriptor.id.as_str()
                ),
                format!(
                    "Capability {} does not match its fixed contract",
                    descriptor.id.as_str()
                ),
                "Preserve the capability status, policy, route effect, and localized labels",
            ));
        }
    }
    if observed.len() != expected.len() {
        return Err(capability_error(
            "INCOMPLETE_INTENT_CAPABILITY_MANIFEST",
            "intent.capability_manifest.capabilities",
            format!(
                "Capability manifest contains {} descriptors; expected {}",
                observed.len(),
                expected.len()
            ),
            "Declare every Intent V2 capability exactly once",
        ));
    }
    Ok(())
}

fn validate_safety_boundaries(
    boundaries: &[SafetyBoundaryDescriptorV2],
) -> Result<(), StructuredError> {
    let expected = intent_capability_manifest_v2()
        .safety_boundaries
        .into_iter()
        .map(|descriptor| (descriptor.id.as_str(), descriptor))
        .collect::<BTreeMap<_, _>>();
    let mut observed = BTreeSet::new();
    for descriptor in boundaries {
        if !observed.insert(descriptor.id.as_str()) {
            return Err(capability_error(
                "DUPLICATE_INTENT_SAFETY_BOUNDARY_ID",
                "intent.capability_manifest.safety_boundaries",
                format!("Safety boundary {} is repeated", descriptor.id.as_str()),
                "Declare each safety boundary exactly once",
            ));
        }
        if descriptor.route_effect != IntentRouteEffectV2::Reject
            || descriptor.label.en.is_empty()
            || descriptor.label.ko.is_empty()
        {
            return Err(capability_error(
                "INVALID_INTENT_SAFETY_BOUNDARY_DESCRIPTOR",
                format!(
                    "intent.capability_manifest.safety_boundaries.{}",
                    descriptor.id.as_str()
                ),
                format!(
                    "Safety boundary {} does not match its fixed contract",
                    descriptor.id.as_str()
                ),
                "Preserve the reject effect and localized labels",
            ));
        }
    }
    if observed.len() != expected.len() {
        return Err(capability_error(
            "INCOMPLETE_INTENT_SAFETY_BOUNDARY_MANIFEST",
            "intent.capability_manifest.safety_boundaries",
            format!(
                "Safety-boundary manifest contains {} descriptors; expected {}",
                observed.len(),
                expected.len()
            ),
            "Declare every Intent V2 safety boundary exactly once",
        ));
    }
    Ok(())
}

fn capability_error(
    code: impl Into<String>,
    location: impl Into<String>,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> StructuredError {
    StructuredError::new(code, location, message, hint)
}
