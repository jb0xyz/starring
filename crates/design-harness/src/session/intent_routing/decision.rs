use serde::{Deserialize, Serialize};

use crate::intent::{IntentCapabilityBlockerV2, IntentSafetyBoundaryViolationV2};

pub const INTENT_ADJUDICATOR_VERSION_V2: u16 = 1;
pub const INTENT_RECIPE_PROTOCOL_VERSION_V2: u16 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentDecisionSourceV2 {
    DeterministicIntentAdjudicator,
}

impl IntentDecisionSourceV2 {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DeterministicIntentAdjudicator => "deterministic_intent_adjudicator",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentRouteDecisionKindV2 {
    PrivateStudyRoom,
    TypedPlanner,
    CapabilityGap,
    Reject,
    Discussion,
}

impl IntentRouteDecisionKindV2 {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PrivateStudyRoom => "private_study_room",
            Self::TypedPlanner => "typed_planner",
            Self::CapabilityGap => "capability_gap",
            Self::Reject => "reject",
            Self::Discussion => "discussion",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinnedIntentRecipeV2 {
    recipe_id: String,
    recipe_version: u32,
}

impl PinnedIntentRecipeV2 {
    pub fn recipe_id(&self) -> &str {
        &self.recipe_id
    }

    pub fn recipe_version(&self) -> u32 {
        self.recipe_version
    }

    pub(super) fn private_study_room(recipe_id: &str, recipe_version: u32) -> Self {
        Self {
            recipe_id: recipe_id.to_string(),
            recipe_version,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentRouteDecisionV2 {
    kind: IntentRouteDecisionKindV2,
    decision_source: IntentDecisionSourceV2,
    adjudicator_version: u16,
    semantic_ir_digest: String,
    manifest_version: u16,
    manifest_digest: String,
    adjudication_digest: String,
    blockers: Vec<IntentCapabilityBlockerV2>,
    boundary_violations: Vec<IntentSafetyBoundaryViolationV2>,
    unclassified_requirements: Vec<String>,
    route_target: Option<PinnedIntentRecipeV2>,
}

impl IntentRouteDecisionV2 {
    pub fn kind(&self) -> IntentRouteDecisionKindV2 {
        self.kind
    }

    pub fn decision_source(&self) -> IntentDecisionSourceV2 {
        self.decision_source
    }

    pub fn adjudicator_version(&self) -> u16 {
        self.adjudicator_version
    }

    pub fn semantic_ir_digest(&self) -> &str {
        &self.semantic_ir_digest
    }

    pub fn manifest_version(&self) -> u16 {
        self.manifest_version
    }

    pub fn manifest_digest(&self) -> &str {
        &self.manifest_digest
    }

    pub fn adjudication_digest(&self) -> &str {
        &self.adjudication_digest
    }

    pub fn blockers(&self) -> &[IntentCapabilityBlockerV2] {
        &self.blockers
    }

    pub fn boundary_violations(&self) -> &[IntentSafetyBoundaryViolationV2] {
        &self.boundary_violations
    }

    pub fn unclassified_requirements(&self) -> &[String] {
        &self.unclassified_requirements
    }

    pub fn route_target(&self) -> Option<&PinnedIntentRecipeV2> {
        self.route_target.as_ref()
    }

    pub(super) fn from_parts(parts: IntentRouteDecisionPartsV2) -> Self {
        Self {
            kind: parts.kind,
            decision_source: parts.decision_source,
            adjudicator_version: parts.adjudicator_version,
            semantic_ir_digest: parts.semantic_ir_digest,
            manifest_version: parts.manifest_version,
            manifest_digest: parts.manifest_digest,
            adjudication_digest: parts.adjudication_digest,
            blockers: parts.blockers,
            boundary_violations: parts.boundary_violations,
            unclassified_requirements: parts.unclassified_requirements,
            route_target: parts.route_target,
        }
    }
}

pub(super) struct IntentRouteDecisionPartsV2 {
    pub kind: IntentRouteDecisionKindV2,
    pub decision_source: IntentDecisionSourceV2,
    pub adjudicator_version: u16,
    pub semantic_ir_digest: String,
    pub manifest_version: u16,
    pub manifest_digest: String,
    pub adjudication_digest: String,
    pub blockers: Vec<IntentCapabilityBlockerV2>,
    pub boundary_violations: Vec<IntentSafetyBoundaryViolationV2>,
    pub unclassified_requirements: Vec<String>,
    pub route_target: Option<PinnedIntentRecipeV2>,
}
