mod candidate;
mod capability;
#[cfg(test)]
mod capability_tests;
mod catalog;
#[cfg(test)]
mod catalog_tests;
mod compile;
#[cfg(test)]
mod compiler_tests;
pub(crate) mod identity;
mod keyspace;
mod model;
mod normalize;
mod private_study_room;
mod proposal;
mod provenance;
mod semantic;
mod simulation;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod workspace_tests;

pub(crate) use candidate::{candidate_ruleset_hash, draft_state_hash};
pub use candidate::{
    prepare_intent_candidate, CommittedIntentCandidateV1, IntentExecutionReportV1,
    PreparedIntentCandidateV1,
};
pub use capability::{
    assess_intent_capabilities_v2, intent_capability_manifest_digest_v2,
    intent_capability_manifest_v2, CapabilityDescriptorV2, CapabilityManifestV2,
    CapabilityPolicyIdV2, CapabilityStatusV2, IntentCapabilityAssessmentV2,
    IntentCapabilityBlockerV2, IntentCapabilityIdV2, IntentCapabilityRequirementV2,
    IntentRequirementEvidenceV2, IntentRouteEffectV2, IntentSafetyBoundaryIdV2,
    IntentSafetyBoundaryRequestV2, IntentSafetyBoundaryViolationV2, LocalizedIntentLabelV2,
    SafetyBoundaryDescriptorV2, INTENT_CAPABILITY_MANIFEST_VERSION_V2,
};
pub use catalog::{
    recipe_descriptor_digest_v1, recipe_descriptor_v1, recipe_registry_digest_v1,
    recipe_registry_v1, RecipeDescriptorV1, RecipeKindV1,
};
pub use compile::{
    compile_intent, CompilationManifestV2, CompilationVerificationV1, CompiledIntentV2,
    INTENT_IDENTITY_REVISION,
};
pub(crate) use model::IntentWorkspaceV2;
pub use model::{
    ClosePolicyV1, ExistingChannelKey, FeatureId, IntentLocaleV1, IntentRequestedOutcome,
    IntentResolutionContext, MissingDecision, MissingDecisionKind, RoomNamePatternV1,
};
pub(crate) use model::{PRIVATE_STUDY_ROOM_RECIPE_ID, PRIVATE_STUDY_ROOM_RECIPE_VERSION};
pub use normalize::ValidatedIntentV2;
pub(crate) use normalize::{prepare_intent_workspace, PreparedIntentWorkspaceV2};
pub(crate) use proposal::{apply_existing_channel_decision, prepare_private_study_room};
pub use proposal::{
    propose_private_study_room, IntentProposalOutcomeV2, PrivateStudyRoomControlsProposalV1,
    PrivateStudyRoomCopyProposalV1, PrivateStudyRoomNamingProposalV1, PrivateStudyRoomProposalV2,
};
pub use provenance::{IntentCoverageV1, RequirementProvenanceV1};
