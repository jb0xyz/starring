use std::collections::BTreeSet;

use crate::errors::StructuredError;
use crate::intent::identity::is_lowercase_sha256_hex;
use crate::intent::{
    assess_intent_capabilities_v2, intent_capability_manifest_digest_v2,
    intent_capability_manifest_v2, ClosePolicyV1, IntentCapabilityAssessmentV2,
    IntentCapabilityIdV2, IntentCapabilityRequirementV2, IntentLocaleV1, IntentRequestedOutcome,
    IntentRequirementEvidenceV2, IntentResolutionContext, IntentSafetyBoundaryIdV2,
    IntentSafetyBoundaryRequestV2, PreparedIntentWorkspaceV2, PrivateStudyRoomControlsProposalV1,
    PrivateStudyRoomProposalV2, PRIVATE_STUDY_ROOM_RECIPE_ID, PRIVATE_STUDY_ROOM_RECIPE_VERSION,
};
#[cfg(test)]
use crate::turn::IntentInterpretationV2;
use crate::turn::{
    CloseAuthorizationV2, EconomyRequirementV2, IntentAutomationKindV2, IntentBoundaryRequestV2,
    IntentCoreInterpretationV4, IntentLocaleHintV2, IntentRecipeDetailFacetV3, IntentRequestModeV2,
    PersistenceRequirementV2, PrivateStudyRoomControlsInterpretationV2, PrivateStudyRoomDetailsV1,
    RuntimeRequirementsV2, TimerRequirementV2,
};

#[cfg(test)]
use super::decision::INTENT_ADJUDICATOR_VERSION_V2;
use super::decision::{
    IntentDecisionSourceV2, IntentRouteDecisionKindV2, IntentRouteDecisionPartsV2,
    IntentRouteDecisionV2, PinnedIntentRecipeV2, INTENT_ADJUDICATOR_VERSION_V4,
};

mod digest;
mod response;

#[cfg(test)]
use digest::{adjudication_digest_v2, semantic_ir_digest_v2};
use digest::{
    adjudication_digest_v4, private_study_room_details_digest_v2, semantic_ir_digest_v4,
    AdjudicationDigestInputV2,
};
use response::{capability_gap_response, reject_response, typed_planner_response};

#[cfg(test)]
pub(super) enum IntentAdjudicationV2 {
    PrivateStudyRoom(Box<PrivateStudyRoomPermitV2>),
    TypedPlanner(TypedPlannerPermitV2),
    Terminal(TerminalIntentPermitV2),
}

pub(super) enum IntentCoreAdjudicationV4 {
    PrivateStudyRoom(Box<PrivateStudyRoomSelectionV4>),
    TypedPlanner(TypedPlannerPermitV2),
    Terminal(TerminalIntentPermitV2),
}

#[derive(Clone)]
pub(super) struct PrivateStudyRoomSelectionV4 {
    core: IntentCoreInterpretationV4,
    decision: IntentRouteDecisionV2,
}

impl PrivateStudyRoomSelectionV4 {
    pub(super) fn expected_revision(&self) -> u64 {
        self.core.expected_revision()
    }

    pub(super) fn semantic_ir_digest(&self) -> &str {
        self.decision.semantic_ir_digest()
    }

    pub(super) fn detail_facets(&self) -> &[IntentRecipeDetailFacetV3] {
        self.core.recipe_detail_facets()
    }

    pub(super) fn decision(&self) -> &IntentRouteDecisionV2 {
        &self.decision
    }

    pub(super) fn details_digest(
        &self,
        source_human_turn_digest: &str,
        details: &PrivateStudyRoomDetailsV1,
    ) -> Result<String, StructuredError> {
        private_study_room_details_digest_v2(
            self.semantic_ir_digest(),
            source_human_turn_digest,
            details,
        )
    }

    pub(super) fn finalize(
        self,
        details: Option<PrivateStudyRoomDetailsV1>,
    ) -> Result<PrivateStudyRoomPermitV2, StructuredError> {
        let proposal =
            private_study_room_proposal_v4(&self.core, details.as_ref(), &self.decision)?;
        Ok(PrivateStudyRoomPermitV2 {
            proposal,
            decision: self.decision,
        })
    }
}

pub(super) struct PrivateStudyRoomPermitV2 {
    proposal: PrivateStudyRoomProposalV2,
    decision: IntentRouteDecisionV2,
}

impl PrivateStudyRoomPermitV2 {
    #[cfg(test)]
    pub(super) fn decision(&self) -> &IntentRouteDecisionV2 {
        &self.decision
    }

    pub(super) fn prepare(
        self,
        context: &IntentResolutionContext,
    ) -> Result<(IntentRouteDecisionV2, PreparedIntentWorkspaceV2), StructuredError> {
        let prepared = crate::intent::prepare_private_study_room(self.proposal, context)?;
        Ok((self.decision, prepared))
    }
}

pub(super) struct TypedPlannerPermitV2 {
    reason: String,
    requested_outcome: IntentRequestedOutcome,
    decision: IntentRouteDecisionV2,
    response: String,
}

impl TypedPlannerPermitV2 {
    #[cfg(test)]
    pub(super) fn reason(&self) -> &str {
        &self.reason
    }

    #[cfg(test)]
    pub(super) fn requested_outcome(&self) -> IntentRequestedOutcome {
        self.requested_outcome
    }

    #[cfg(test)]
    pub(super) fn decision(&self) -> &IntentRouteDecisionV2 {
        &self.decision
    }

    #[cfg(test)]
    pub(super) fn response(&self) -> &str {
        &self.response
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        String,
        IntentRequestedOutcome,
        IntentRouteDecisionV2,
        String,
    ) {
        (
            self.reason,
            self.requested_outcome,
            self.decision,
            self.response,
        )
    }
}

pub(super) struct TerminalIntentPermitV2 {
    decision: IntentRouteDecisionV2,
    response: String,
}

impl TerminalIntentPermitV2 {
    #[cfg(test)]
    pub(super) fn decision(&self) -> &IntentRouteDecisionV2 {
        &self.decision
    }

    pub(super) fn into_parts(self) -> (IntentRouteDecisionV2, String) {
        (self.decision, self.response)
    }
}

#[cfg(test)]
pub(super) fn adjudicate_intent_v2(
    interpretation: IntentInterpretationV2,
) -> Result<IntentAdjudicationV2, StructuredError> {
    let semantic_ir_digest = semantic_ir_digest_v2(&interpretation)?;
    let adjudication = adjudicate_semantics(
        SemanticIntentView::from_interpretation(&interpretation),
        semantic_ir_digest,
        AdjudicationProtocol::V2,
    )?;
    match adjudication {
        SemanticAdjudication::PrivateStudyRoom(decision) => {
            let proposal = private_study_room_proposal(&interpretation)?;
            Ok(IntentAdjudicationV2::PrivateStudyRoom(Box::new(
                PrivateStudyRoomPermitV2 { proposal, decision },
            )))
        }
        SemanticAdjudication::TypedPlanner(permit) => {
            Ok(IntentAdjudicationV2::TypedPlanner(permit))
        }
        SemanticAdjudication::Terminal(permit) => Ok(IntentAdjudicationV2::Terminal(permit)),
    }
}

pub(super) fn adjudicate_intent_core_v4(
    core: IntentCoreInterpretationV4,
    request_evidence_hash: &str,
) -> Result<IntentCoreAdjudicationV4, StructuredError> {
    let semantic_ir_digest = semantic_ir_digest_v4(&core)?;
    let adjudication = adjudicate_semantics(
        SemanticIntentView::from_core(&core, request_evidence_hash),
        semantic_ir_digest,
        AdjudicationProtocol::V4,
    )?;
    match adjudication {
        SemanticAdjudication::PrivateStudyRoom(decision) => {
            Ok(IntentCoreAdjudicationV4::PrivateStudyRoom(Box::new(
                PrivateStudyRoomSelectionV4 { core, decision },
            )))
        }
        SemanticAdjudication::TypedPlanner(permit) => {
            Ok(IntentCoreAdjudicationV4::TypedPlanner(permit))
        }
        SemanticAdjudication::Terminal(permit) => Ok(IntentCoreAdjudicationV4::Terminal(permit)),
    }
}

#[derive(Clone, Copy)]
struct SemanticIntentView<'a> {
    root: &'static str,
    request_mode: IntentRequestModeV2,
    automation_kind: IntentAutomationKindV2,
    typed_planner_reason: Option<&'a str>,
    request_evidence_hash: Option<&'a str>,
    requested_outcome: IntentRequestedOutcome,
    close_authorization: CloseAuthorizationV2,
    runtime_requirements: &'a RuntimeRequirementsV2,
    boundary_requests: &'a [IntentBoundaryRequestV2],
    unclassified_requirements: &'a [String],
    response: &'a str,
    response_locale: IntentLocaleHintV2,
}

impl<'a> SemanticIntentView<'a> {
    #[cfg(test)]
    fn from_interpretation(interpretation: &'a IntentInterpretationV2) -> Self {
        Self {
            root: "intent.interpretation",
            request_mode: interpretation.request_mode(),
            automation_kind: interpretation.automation_kind(),
            typed_planner_reason: Some(interpretation.objective()),
            request_evidence_hash: None,
            requested_outcome: interpretation.requested_outcome(),
            close_authorization: interpretation.close_authorization(),
            runtime_requirements: interpretation.runtime_requirements(),
            boundary_requests: interpretation.boundary_requests(),
            unclassified_requirements: interpretation.unclassified_requirements(),
            response: interpretation.response(),
            response_locale: interpretation.response_locale(),
        }
    }

    fn from_core(core: &'a IntentCoreInterpretationV4, request_evidence_hash: &'a str) -> Self {
        Self {
            root: "intent.core",
            request_mode: core.request_mode(),
            automation_kind: core.automation_kind(),
            typed_planner_reason: None,
            request_evidence_hash: Some(request_evidence_hash),
            requested_outcome: core.requested_outcome(),
            close_authorization: core.close_authorization(),
            runtime_requirements: core.runtime_requirements(),
            boundary_requests: core.boundary_requests(),
            unclassified_requirements: core.unclassified_requirements(),
            response: core.response(),
            response_locale: core.locale(),
        }
    }
}

#[derive(Clone, Copy)]
enum AdjudicationProtocol {
    #[cfg(test)]
    V2,
    V4,
}

enum SemanticAdjudication {
    PrivateStudyRoom(IntentRouteDecisionV2),
    TypedPlanner(TypedPlannerPermitV2),
    Terminal(TerminalIntentPermitV2),
}

fn adjudicate_semantics(
    input: SemanticIntentView<'_>,
    semantic_ir_digest: String,
    protocol: AdjudicationProtocol,
) -> Result<SemanticAdjudication, StructuredError> {
    let requirements = derive_capability_requirements(input);
    let boundary_requests = derive_boundary_requests(input);
    let assessment = assess_intent_capabilities_v2(&requirements, &boundary_requests)?;
    let unclassified_requirements = input
        .unclassified_requirements
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    if !assessment.boundary_violations.is_empty() {
        let response = reject_response(&assessment.boundary_violations, input.response_locale)?;
        let decision = decision_for(
            IntentRouteDecisionKindV2::Reject,
            semantic_ir_digest,
            assessment,
            unclassified_requirements,
            None,
            protocol,
            input.request_evidence_hash,
        )?;
        return Ok(SemanticAdjudication::Terminal(TerminalIntentPermitV2 {
            decision,
            response,
        }));
    }

    if input.request_mode == IntentRequestModeV2::Discussion {
        let decision = decision_for(
            IntentRouteDecisionKindV2::Discussion,
            semantic_ir_digest,
            assessment,
            unclassified_requirements,
            None,
            protocol,
            input.request_evidence_hash,
        )?;
        return Ok(SemanticAdjudication::Terminal(TerminalIntentPermitV2 {
            decision,
            response: input.response.to_string(),
        }));
    }

    if !assessment.blockers.is_empty() {
        let response = capability_gap_response(&assessment.blockers, input.response_locale)?;
        let decision = decision_for(
            IntentRouteDecisionKindV2::CapabilityGap,
            semantic_ir_digest,
            assessment,
            unclassified_requirements,
            None,
            protocol,
            input.request_evidence_hash,
        )?;
        return Ok(SemanticAdjudication::Terminal(TerminalIntentPermitV2 {
            decision,
            response,
        }));
    }

    match input.automation_kind {
        IntentAutomationKindV2::ManagedPrivateStudyRoom => {
            let route_target = PinnedIntentRecipeV2::private_study_room(
                PRIVATE_STUDY_ROOM_RECIPE_ID,
                PRIVATE_STUDY_ROOM_RECIPE_VERSION,
            );
            let decision = decision_for(
                IntentRouteDecisionKindV2::PrivateStudyRoom,
                semantic_ir_digest,
                assessment,
                unclassified_requirements,
                Some(route_target),
                protocol,
                input.request_evidence_hash,
            )?;
            Ok(SemanticAdjudication::PrivateStudyRoom(decision))
        }
        IntentAutomationKindV2::CustomAutomation => {
            let response = typed_planner_response(input.response_locale).to_string();
            let decision = decision_for(
                IntentRouteDecisionKindV2::TypedPlanner,
                semantic_ir_digest,
                assessment,
                unclassified_requirements,
                None,
                protocol,
                input.request_evidence_hash,
            )?;
            Ok(SemanticAdjudication::TypedPlanner(TypedPlannerPermitV2 {
                reason: input
                    .typed_planner_reason
                    .unwrap_or("Supported custom static automation")
                    .to_string(),
                requested_outcome: input.requested_outcome,
                decision,
                response,
            }))
        }
        IntentAutomationKindV2::None => Err(adjudication_error(
            "INCONSISTENT_INTENT_ADJUDICATION",
            format!("{}.automation_kind", input.root),
            "A build interpretation has no automation kind and no blocking capability or safety boundary",
            "Classify the complete build as managed_private_study_room or custom_automation",
        )),
    }
}

fn derive_capability_requirements(
    input: SemanticIntentView<'_>,
) -> Vec<IntentCapabilityRequirementV2> {
    if input.request_mode != IntentRequestModeV2::Build {
        return Vec::new();
    }
    let mut requirements = Vec::new();
    if input.close_authorization == CloseAuthorizationV2::CreatorOnly {
        requirements.push(requirement(
            IntentCapabilityIdV2::InstanceCreatorTeardownAuthorization,
            &format!("{}.close_authorization", input.root),
            "creator_only",
        ));
    }
    let runtime = input.runtime_requirements;
    if runtime.persistence == PersistenceRequirementV2::RestartPersistent {
        requirements.push(requirement(
            IntentCapabilityIdV2::RestartPersistentState,
            &format!("{}.runtime_requirements.persistence", input.root),
            "restart_persistent",
        ));
    }
    if runtime.timers == TimerRequirementV2::Durable {
        requirements.push(requirement(
            IntentCapabilityIdV2::DurableTimer,
            &format!("{}.runtime_requirements.timers", input.root),
            "durable",
        ));
    }
    if runtime.economy == EconomyRequirementV2::PersistentLedger {
        requirements.push(requirement(
            IntentCapabilityIdV2::PersistentEconomyLedger,
            &format!("{}.runtime_requirements.economy", input.root),
            "persistent_ledger",
        ));
    }
    if runtime.event_time_llm {
        requirements.push(requirement(
            IntentCapabilityIdV2::EventTimeLlmDecision,
            &format!("{}.runtime_requirements.event_time_llm", input.root),
            "true",
        ));
    }
    for (index, value) in input.unclassified_requirements.iter().enumerate() {
        requirements.push(requirement(
            IntentCapabilityIdV2::UnclassifiedIntentRequirement,
            &format!("{}.unclassified_requirements.{index}", input.root),
            value,
        ));
    }
    requirements
}

fn derive_boundary_requests(input: SemanticIntentView<'_>) -> Vec<IntentSafetyBoundaryRequestV2> {
    input
        .boundary_requests
        .iter()
        .map(|request| {
            let (id, value) = match request {
                IntentBoundaryRequestV2::DirectLiveMutation => (
                    IntentSafetyBoundaryIdV2::DirectLiveMutation,
                    "direct_live_mutation",
                ),
                IntentBoundaryRequestV2::BypassValidationPreviewApproval => (
                    IntentSafetyBoundaryIdV2::BypassValidationPreviewApproval,
                    "bypass_validation_preview_approval",
                ),
                IntentBoundaryRequestV2::SecretDisclosure => (
                    IntentSafetyBoundaryIdV2::SecretDisclosure,
                    "secret_disclosure",
                ),
            };
            IntentSafetyBoundaryRequestV2 {
                id,
                evidence: evidence(&format!("{}.boundary_requests.{value}", input.root), value),
            }
        })
        .collect()
}

fn requirement(
    id: IntentCapabilityIdV2,
    semantic_path: &str,
    description: &str,
) -> IntentCapabilityRequirementV2 {
    IntentCapabilityRequirementV2 {
        id,
        evidence: evidence(semantic_path, description),
    }
}

fn evidence(semantic_path: &str, description: &str) -> IntentRequirementEvidenceV2 {
    IntentRequirementEvidenceV2 {
        semantic_path: semantic_path.to_string(),
        description: description.to_string(),
    }
}

fn decision_for(
    kind: IntentRouteDecisionKindV2,
    semantic_ir_digest: String,
    assessment: IntentCapabilityAssessmentV2,
    unclassified_requirements: Vec<String>,
    route_target: Option<PinnedIntentRecipeV2>,
    protocol: AdjudicationProtocol,
    request_evidence_hash: Option<&str>,
) -> Result<IntentRouteDecisionV2, StructuredError> {
    let mut blockers = assessment.blockers;
    blockers.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for blocker in &mut blockers {
        blocker.evidence.sort();
        blocker.evidence.dedup();
    }
    let mut boundary_violations = assessment.boundary_violations;
    boundary_violations.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for violation in &mut boundary_violations {
        violation.evidence.sort();
        violation.evidence.dedup();
    }
    validate_decision_shape(
        kind,
        &blockers,
        &boundary_violations,
        &unclassified_requirements,
        route_target.as_ref(),
    )?;
    let decision_source = IntentDecisionSourceV2::DeterministicIntentAdjudicator;
    let adjudicator_version = match protocol {
        #[cfg(test)]
        AdjudicationProtocol::V2 => INTENT_ADJUDICATOR_VERSION_V2,
        AdjudicationProtocol::V4 => INTENT_ADJUDICATOR_VERSION_V4,
    };
    let digest_input = AdjudicationDigestInputV2 {
        decision_source: decision_source.as_str(),
        adjudicator_version,
        kind: kind.as_str(),
        semantic_ir_digest: &semantic_ir_digest,
        request_evidence_hash,
        manifest_version: assessment.manifest_version,
        manifest_digest: &assessment.manifest_digest,
        blockers: &blockers,
        boundary_violations: &boundary_violations,
        unclassified_requirements: &unclassified_requirements,
        route_target: route_target.as_ref(),
    };
    let adjudication_digest = match protocol {
        #[cfg(test)]
        AdjudicationProtocol::V2 => adjudication_digest_v2(digest_input),
        AdjudicationProtocol::V4 => adjudication_digest_v4(digest_input),
    }?;
    Ok(IntentRouteDecisionV2::from_parts(
        IntentRouteDecisionPartsV2 {
            kind,
            decision_source,
            adjudicator_version,
            semantic_ir_digest,
            request_evidence_hash: request_evidence_hash.map(str::to_string),
            manifest_version: assessment.manifest_version,
            manifest_digest: assessment.manifest_digest,
            adjudication_digest,
            blockers,
            boundary_violations,
            unclassified_requirements,
            route_target,
        },
    ))
}

fn validate_decision_shape(
    kind: IntentRouteDecisionKindV2,
    blockers: &[crate::intent::IntentCapabilityBlockerV2],
    boundary_violations: &[crate::intent::IntentSafetyBoundaryViolationV2],
    unclassified_requirements: &[String],
    route_target: Option<&PinnedIntentRecipeV2>,
) -> Result<(), StructuredError> {
    let valid = match kind {
        IntentRouteDecisionKindV2::PrivateStudyRoom => {
            blockers.is_empty()
                && boundary_violations.is_empty()
                && unclassified_requirements.is_empty()
                && route_target.is_some_and(|target| {
                    target.recipe_id() == PRIVATE_STUDY_ROOM_RECIPE_ID
                        && target.recipe_version() == PRIVATE_STUDY_ROOM_RECIPE_VERSION
                })
        }
        IntentRouteDecisionKindV2::TypedPlanner => {
            blockers.is_empty()
                && boundary_violations.is_empty()
                && unclassified_requirements.is_empty()
                && route_target.is_none()
        }
        IntentRouteDecisionKindV2::CapabilityGap => {
            !blockers.is_empty() && boundary_violations.is_empty() && route_target.is_none()
        }
        IntentRouteDecisionKindV2::Reject => {
            !boundary_violations.is_empty() && route_target.is_none()
        }
        IntentRouteDecisionKindV2::Discussion => {
            blockers.is_empty() && boundary_violations.is_empty() && route_target.is_none()
        }
    };
    if valid {
        return Ok(());
    }
    Err(adjudication_error(
        "INVALID_INTENT_DECISION_SHAPE",
        "intent.adjudication",
        "The deterministic route decision has an impossible combination of findings and target",
        "Construct the decision through the route-specific adjudicator branch",
    ))
}

#[cfg(test)]
pub(super) fn validate_persisted_private_study_room_decision_v2(
    decision: &IntentRouteDecisionV2,
) -> Result<(), StructuredError> {
    validate_persisted_private_study_room_decision(decision, AdjudicationProtocol::V2)
}

pub(super) fn validate_persisted_private_study_room_decision_v4(
    decision: &IntentRouteDecisionV2,
) -> Result<(), StructuredError> {
    validate_persisted_private_study_room_decision(decision, AdjudicationProtocol::V4)
}

fn validate_persisted_private_study_room_decision(
    decision: &IntentRouteDecisionV2,
    protocol: AdjudicationProtocol,
) -> Result<(), StructuredError> {
    let manifest = intent_capability_manifest_v2();
    let manifest_digest = intent_capability_manifest_digest_v2(&manifest)?;
    validate_decision_shape(
        decision.kind(),
        decision.blockers(),
        decision.boundary_violations(),
        decision.unclassified_requirements(),
        decision.route_target(),
    )?;
    let digests_are_hex = [
        decision.semantic_ir_digest(),
        decision.manifest_digest(),
        decision.adjudication_digest(),
    ]
    .into_iter()
    .all(is_lowercase_sha256_hex);
    let adjudicator_version = match protocol {
        #[cfg(test)]
        AdjudicationProtocol::V2 => INTENT_ADJUDICATOR_VERSION_V2,
        AdjudicationProtocol::V4 => INTENT_ADJUDICATOR_VERSION_V4,
    };
    let request_evidence_valid = match protocol {
        AdjudicationProtocol::V4 => decision
            .request_evidence_hash()
            .is_some_and(valid_digest_shape),
        #[cfg(test)]
        AdjudicationProtocol::V2 => decision.request_evidence_hash().is_none(),
    };
    if decision.kind() != IntentRouteDecisionKindV2::PrivateStudyRoom
        || decision.decision_source() != IntentDecisionSourceV2::DeterministicIntentAdjudicator
        || decision.adjudicator_version() != adjudicator_version
        || decision.manifest_version() != manifest.version
        || decision.manifest_digest() != manifest_digest
        || !digests_are_hex
        || !request_evidence_valid
    {
        return Err(adjudication_error(
            "INVALID_PERSISTED_INTENT_DECISION",
            "intent.adjudication",
            "The persisted private study-room decision does not match the current deterministic contract",
            "Restart the intent with the current protocol and capability manifest",
        ));
    }
    let input = AdjudicationDigestInputV2 {
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
    };
    let expected_digest = match protocol {
        #[cfg(test)]
        AdjudicationProtocol::V2 => adjudication_digest_v2(input),
        AdjudicationProtocol::V4 => adjudication_digest_v4(input),
    }?;
    if decision.adjudication_digest() != expected_digest {
        return Err(adjudication_error(
            "INVALID_PERSISTED_INTENT_DECISION_DIGEST",
            "intent.adjudication.adjudication_digest",
            "The persisted adjudication digest does not match its canonical decision fields",
            "Discard the tampered snapshot and resume from a trusted intent decision",
        ));
    }
    Ok(())
}

#[cfg(test)]
fn private_study_room_proposal(
    interpretation: &IntentInterpretationV2,
) -> Result<PrivateStudyRoomProposalV2, StructuredError> {
    let close_policy = match interpretation.close_authorization() {
        CloseAuthorizationV2::NotRequested => None,
        CloseAuthorizationV2::Disabled => Some(ClosePolicyV1::Disabled),
        CloseAuthorizationV2::AnyMember => Some(ClosePolicyV1::AnyMember),
        CloseAuthorizationV2::CreatorOnly => {
            return Err(adjudication_error(
                "UNSUPPORTED_CREATOR_ONLY_TEARDOWN_AUTHORIZATION",
                "intent.interpretation.close_authorization",
                "Creator-only teardown authorization cannot be lowered into the private study-room recipe",
                "Return a capability-gap decision without compiling the recipe",
            ));
        }
    };
    let locale = match interpretation.locale() {
        IntentLocaleHintV2::En => Some(IntentLocaleV1::En),
        IntentLocaleHintV2::Ko => Some(IntentLocaleV1::Ko),
        IntentLocaleHintV2::Unspecified => None,
    };
    let controls = interpretation.controls();
    Ok(PrivateStudyRoomProposalV2 {
        requested_outcome: interpretation.requested_outcome(),
        hub_channel: interpretation.hub_channel().cloned(),
        locale,
        copy: interpretation.copy().clone(),
        naming: interpretation.naming().clone(),
        controls: PrivateStudyRoomControlsProposalV1 {
            help_label: controls.help_label.clone(),
            help_response: controls.help_response.clone(),
            join_label: controls.join_label.clone(),
            joined_response: controls.joined_response.clone(),
            close_label: controls.close_label.clone(),
            closed_response: controls.closed_response.clone(),
            close_policy,
        },
    })
}

fn private_study_room_proposal_v4(
    core: &IntentCoreInterpretationV4,
    details: Option<&PrivateStudyRoomDetailsV1>,
    decision: &IntentRouteDecisionV2,
) -> Result<PrivateStudyRoomProposalV2, StructuredError> {
    validate_detail_selection(core, details, decision)?;
    let close_policy = close_policy(
        core.close_authorization(),
        "intent.core.close_authorization",
    )?;
    let locale = locale(core.locale());
    let copy = details
        .map(|value| value.copy().clone())
        .unwrap_or_default();
    let naming = details
        .map(|value| value.naming().clone())
        .unwrap_or_default();
    let controls = details
        .map(|value| value.controls().clone())
        .unwrap_or_default();
    validate_close_control_consistency(core.close_authorization(), &controls)?;
    Ok(PrivateStudyRoomProposalV2 {
        requested_outcome: core.requested_outcome(),
        hub_channel: core.selected_existing_channel().cloned(),
        locale,
        copy,
        naming,
        controls: PrivateStudyRoomControlsProposalV1 {
            help_label: controls.help_label,
            help_response: controls.help_response,
            join_label: controls.join_label,
            joined_response: controls.joined_response,
            close_label: controls.close_label,
            closed_response: controls.closed_response,
            close_policy,
        },
    })
}

fn validate_detail_selection(
    core: &IntentCoreInterpretationV4,
    details: Option<&PrivateStudyRoomDetailsV1>,
    decision: &IntentRouteDecisionV2,
) -> Result<(), StructuredError> {
    let required = core
        .recipe_detail_facets()
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    match (required.is_empty(), details) {
        (true, None) => return Ok(()),
        (true, Some(_)) => {
            return Err(adjudication_error(
                "UNEXPECTED_RECIPE_DETAILS",
                "intent.details",
                "Recipe details were supplied for the deterministic default path",
                "Omit the detail call when Core has no active detail facets",
            ));
        }
        (false, None) => {
            return Err(adjudication_error(
                "MISSING_RECIPE_DETAILS",
                "intent.details",
                "The selected recipe detail facets have no extracted values",
                "Complete the single recipe detail frontier before compilation",
            ));
        }
        (false, Some(_)) => {}
    }
    let Some(details) = details else {
        return Err(adjudication_error(
            "MISSING_RECIPE_DETAILS",
            "intent.details",
            "The selected recipe detail facets have no extracted values",
            "Complete the single recipe detail frontier before compilation",
        ));
    };
    let covered = details
        .covered_facets()
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    if details.expected_revision() != core.expected_revision() {
        return Err(adjudication_error(
            "STALE_RECIPE_DETAIL_REVISION",
            "intent.details.expected_revision",
            "Recipe details do not use the Core IR revision",
            "Retry the detail frontier with the active expected revision",
        ));
    }
    if details.core_semantic_digest() != decision.semantic_ir_digest() {
        return Err(adjudication_error(
            "RECIPE_DETAIL_CORE_DIGEST_MISMATCH",
            "intent.details.core_semantic_digest",
            "Recipe details are not bound to the selected Core IR",
            "Copy the exact harness-provided Core semantic digest",
        ));
    }
    if covered != required {
        return Err(adjudication_error(
            "RECIPE_DETAIL_COVERAGE_MISMATCH",
            "intent.details.covered_facets",
            "Recipe detail coverage does not match the selected Core facets",
            "Cover each selected detail facet exactly once",
        ));
    }
    Ok(())
}

fn validate_close_control_consistency(
    authorization: CloseAuthorizationV2,
    controls: &PrivateStudyRoomControlsInterpretationV2,
) -> Result<(), StructuredError> {
    let has_close_override = controls.close_label.is_some() || controls.closed_response.is_some();
    if !has_close_override || authorization == CloseAuthorizationV2::AnyMember {
        return Ok(());
    }
    Err(adjudication_error(
        "INCONSISTENT_RECIPE_CLOSE_CONTROL",
        "intent.details.controls",
        "Custom close controls contradict the adjudicated close authorization",
        "Use close controls only with any_member authorization",
    ))
}

fn close_policy(
    authorization: CloseAuthorizationV2,
    location: &str,
) -> Result<Option<ClosePolicyV1>, StructuredError> {
    match authorization {
        CloseAuthorizationV2::NotRequested => Ok(None),
        CloseAuthorizationV2::Disabled => Ok(Some(ClosePolicyV1::Disabled)),
        CloseAuthorizationV2::AnyMember => Ok(Some(ClosePolicyV1::AnyMember)),
        CloseAuthorizationV2::CreatorOnly => Err(adjudication_error(
            "UNSUPPORTED_CREATOR_ONLY_TEARDOWN_AUTHORIZATION",
            location,
            "Creator-only teardown authorization cannot be lowered into the private study-room recipe",
            "Return a capability-gap decision without compiling the recipe",
        )),
    }
}

fn locale(value: IntentLocaleHintV2) -> Option<IntentLocaleV1> {
    match value {
        IntentLocaleHintV2::En => Some(IntentLocaleV1::En),
        IntentLocaleHintV2::Ko => Some(IntentLocaleV1::Ko),
        IntentLocaleHintV2::Unspecified => None,
    }
}

fn valid_digest_shape(value: &str) -> bool {
    is_lowercase_sha256_hex(value)
}

fn adjudication_error(
    code: impl Into<String>,
    location: impl Into<String>,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> StructuredError {
    StructuredError::new(code, location, message, hint)
}
