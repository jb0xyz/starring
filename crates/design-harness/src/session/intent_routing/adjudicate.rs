use crate::errors::StructuredError;
use crate::intent::{
    assess_intent_capabilities_v2, intent_capability_manifest_digest_v2,
    intent_capability_manifest_v2, ClosePolicyV1, IntentCapabilityAssessmentV2,
    IntentCapabilityIdV2, IntentCapabilityRequirementV2, IntentLocaleV1, IntentRequestedOutcome,
    IntentRequirementEvidenceV2, IntentResolutionContext, IntentSafetyBoundaryIdV2,
    IntentSafetyBoundaryRequestV2, PreparedIntentWorkspaceV1, PrivateStudyRoomControlsProposalV1,
    PrivateStudyRoomProposalV1, PRIVATE_STUDY_ROOM_RECIPE_ID, PRIVATE_STUDY_ROOM_RECIPE_VERSION,
};
use crate::turn::{
    CloseAuthorizationV2, EconomyRequirementV2, IntentAutomationKindV2, IntentBoundaryRequestV2,
    IntentInterpretationV2, IntentLocaleHintV2, IntentRequestModeV2, PersistenceRequirementV2,
    TimerRequirementV2,
};

use super::decision::{
    IntentDecisionSourceV2, IntentRouteDecisionKindV2, IntentRouteDecisionPartsV2,
    IntentRouteDecisionV2, PinnedIntentRecipeV2, INTENT_ADJUDICATOR_VERSION_V2,
};

mod digest;
mod response;

use digest::{
    adjudication_digest_v2, canonical_unclassified_requirements, semantic_ir_digest_v2,
    AdjudicationDigestInputV2,
};
use response::{capability_gap_response, reject_response, typed_planner_response};

pub(super) enum IntentAdjudicationV2 {
    PrivateStudyRoom(Box<PrivateStudyRoomPermitV2>),
    TypedPlanner(TypedPlannerPermitV2),
    Terminal(TerminalIntentPermitV2),
}

pub(super) struct PrivateStudyRoomPermitV2 {
    proposal: PrivateStudyRoomProposalV1,
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
    ) -> Result<(IntentRouteDecisionV2, PreparedIntentWorkspaceV1), StructuredError> {
        let prepared = crate::intent::prepare_private_study_room(self.proposal, context)?;
        Ok((self.decision, prepared))
    }
}

pub(super) struct TypedPlannerPermitV2 {
    objective: String,
    requested_outcome: IntentRequestedOutcome,
    decision: IntentRouteDecisionV2,
    response: String,
}

impl TypedPlannerPermitV2 {
    #[cfg(test)]
    pub(super) fn objective(&self) -> &str {
        &self.objective
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
            self.objective,
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

pub(super) fn adjudicate_intent_v2(
    interpretation: IntentInterpretationV2,
) -> Result<IntentAdjudicationV2, StructuredError> {
    let semantic_ir_digest = semantic_ir_digest_v2(&interpretation)?;
    let requirements = derive_capability_requirements(&interpretation);
    let boundary_requests = derive_boundary_requests(&interpretation);
    let assessment = assess_intent_capabilities_v2(&requirements, &boundary_requests)?;
    let unclassified_requirements = canonical_unclassified_requirements(&interpretation);

    if !assessment.boundary_violations.is_empty() {
        let response = reject_response(
            &assessment.boundary_violations,
            interpretation.response_locale(),
        )?;
        let decision = decision_for(
            IntentRouteDecisionKindV2::Reject,
            semantic_ir_digest,
            assessment,
            unclassified_requirements,
            None,
        )?;
        return Ok(IntentAdjudicationV2::Terminal(TerminalIntentPermitV2 {
            decision,
            response,
        }));
    }

    if interpretation.request_mode() == IntentRequestModeV2::Discussion {
        let decision = decision_for(
            IntentRouteDecisionKindV2::Discussion,
            semantic_ir_digest,
            assessment,
            unclassified_requirements,
            None,
        )?;
        return Ok(IntentAdjudicationV2::Terminal(TerminalIntentPermitV2 {
            decision,
            response: interpretation.response().to_string(),
        }));
    }

    if !assessment.blockers.is_empty() {
        let response =
            capability_gap_response(&assessment.blockers, interpretation.response_locale())?;
        let decision = decision_for(
            IntentRouteDecisionKindV2::CapabilityGap,
            semantic_ir_digest,
            assessment,
            unclassified_requirements,
            None,
        )?;
        return Ok(IntentAdjudicationV2::Terminal(TerminalIntentPermitV2 {
            decision,
            response,
        }));
    }

    match interpretation.automation_kind() {
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
            )?;
            let proposal = private_study_room_proposal(&interpretation)?;
            Ok(IntentAdjudicationV2::PrivateStudyRoom(Box::new(
                PrivateStudyRoomPermitV2 { proposal, decision },
            )))
        }
        IntentAutomationKindV2::CustomAutomation => {
            let response = typed_planner_response(interpretation.response_locale()).to_string();
            let objective = interpretation.objective().to_string();
            let requested_outcome = interpretation.requested_outcome();
            let decision = decision_for(
                IntentRouteDecisionKindV2::TypedPlanner,
                semantic_ir_digest,
                assessment,
                unclassified_requirements,
                None,
            )?;
            Ok(IntentAdjudicationV2::TypedPlanner(TypedPlannerPermitV2 {
                objective,
                requested_outcome,
                decision,
                response,
            }))
        }
        IntentAutomationKindV2::None => Err(adjudication_error(
            "INCONSISTENT_INTENT_ADJUDICATION",
            "intent.interpretation.automation_kind",
            "A build interpretation has no automation kind and no blocking capability or safety boundary",
            "Classify the complete build as managed_private_study_room or custom_automation",
        )),
    }
}

fn derive_capability_requirements(
    interpretation: &IntentInterpretationV2,
) -> Vec<IntentCapabilityRequirementV2> {
    if interpretation.request_mode() != IntentRequestModeV2::Build {
        return Vec::new();
    }
    let mut requirements = Vec::new();
    if interpretation.close_authorization() == CloseAuthorizationV2::CreatorOnly {
        requirements.push(requirement(
            IntentCapabilityIdV2::InstanceCreatorTeardownAuthorization,
            "intent.interpretation.close_authorization",
            "creator_only",
        ));
    }
    let runtime = interpretation.runtime_requirements();
    if runtime.persistence == PersistenceRequirementV2::RestartPersistent {
        requirements.push(requirement(
            IntentCapabilityIdV2::RestartPersistentState,
            "intent.interpretation.runtime_requirements.persistence",
            "restart_persistent",
        ));
    }
    if runtime.timers == TimerRequirementV2::Durable {
        requirements.push(requirement(
            IntentCapabilityIdV2::DurableTimer,
            "intent.interpretation.runtime_requirements.timers",
            "durable",
        ));
    }
    if runtime.economy == EconomyRequirementV2::PersistentLedger {
        requirements.push(requirement(
            IntentCapabilityIdV2::PersistentEconomyLedger,
            "intent.interpretation.runtime_requirements.economy",
            "persistent_ledger",
        ));
    }
    if runtime.event_time_llm {
        requirements.push(requirement(
            IntentCapabilityIdV2::EventTimeLlmDecision,
            "intent.interpretation.runtime_requirements.event_time_llm",
            "true",
        ));
    }
    for (index, value) in interpretation
        .unclassified_requirements()
        .iter()
        .enumerate()
    {
        requirements.push(requirement(
            IntentCapabilityIdV2::UnclassifiedIntentRequirement,
            &format!("intent.interpretation.unclassified_requirements.{index}"),
            value,
        ));
    }
    requirements
}

fn derive_boundary_requests(
    interpretation: &IntentInterpretationV2,
) -> Vec<IntentSafetyBoundaryRequestV2> {
    interpretation
        .boundary_requests()
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
                evidence: evidence(
                    &format!("intent.interpretation.boundary_requests.{value}"),
                    value,
                ),
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
    let adjudication_digest = adjudication_digest_v2(AdjudicationDigestInputV2 {
        decision_source: decision_source.as_str(),
        adjudicator_version: INTENT_ADJUDICATOR_VERSION_V2,
        kind: kind.as_str(),
        semantic_ir_digest: &semantic_ir_digest,
        manifest_version: assessment.manifest_version,
        manifest_digest: &assessment.manifest_digest,
        blockers: &blockers,
        boundary_violations: &boundary_violations,
        unclassified_requirements: &unclassified_requirements,
        route_target: route_target.as_ref(),
    })?;
    Ok(IntentRouteDecisionV2::from_parts(
        IntentRouteDecisionPartsV2 {
            kind,
            decision_source,
            adjudicator_version: INTENT_ADJUDICATOR_VERSION_V2,
            semantic_ir_digest,
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

pub(super) fn validate_persisted_private_study_room_decision_v2(
    decision: &IntentRouteDecisionV2,
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
    .all(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|value| value.is_ascii_digit() || (b'a'..=b'f').contains(&value))
    });
    if decision.kind() != IntentRouteDecisionKindV2::PrivateStudyRoom
        || decision.decision_source() != IntentDecisionSourceV2::DeterministicIntentAdjudicator
        || decision.adjudicator_version() != INTENT_ADJUDICATOR_VERSION_V2
        || decision.manifest_version() != manifest.version
        || decision.manifest_digest() != manifest_digest
        || !digests_are_hex
    {
        return Err(adjudication_error(
            "INVALID_PERSISTED_INTENT_DECISION",
            "intent.adjudication",
            "The persisted private study-room decision does not match the current deterministic contract",
            "Restart the intent with the current protocol and capability manifest",
        ));
    }
    let expected_digest = adjudication_digest_v2(AdjudicationDigestInputV2 {
        decision_source: decision.decision_source().as_str(),
        adjudicator_version: decision.adjudicator_version(),
        kind: decision.kind().as_str(),
        semantic_ir_digest: decision.semantic_ir_digest(),
        manifest_version: decision.manifest_version(),
        manifest_digest: decision.manifest_digest(),
        blockers: decision.blockers(),
        boundary_violations: decision.boundary_violations(),
        unclassified_requirements: decision.unclassified_requirements(),
        route_target: decision.route_target(),
    })?;
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

fn private_study_room_proposal(
    interpretation: &IntentInterpretationV2,
) -> Result<PrivateStudyRoomProposalV1, StructuredError> {
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
    Ok(PrivateStudyRoomProposalV1 {
        objective: interpretation.objective().to_string(),
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

fn adjudication_error(
    code: impl Into<String>,
    location: impl Into<String>,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> StructuredError {
    StructuredError::new(code, location, message, hint)
}
