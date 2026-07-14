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
    compile_intent, CompilationManifestV1, CompilationVerificationV1, CompiledIntentV1,
};
pub(crate) use model::IntentWorkspaceV1;
pub use model::{
    ClosePolicyV1, ExistingChannelKey, FeatureId, IntentLocaleV1, IntentRequestedOutcome,
    IntentResolutionContext, MissingDecision, MissingDecisionKind, RoomNamePatternV1,
};
pub(crate) use model::{PRIVATE_STUDY_ROOM_RECIPE_ID, PRIVATE_STUDY_ROOM_RECIPE_VERSION};
pub(crate) use normalize::PreparedIntentWorkspaceV1;
pub use normalize::ValidatedIntentV1;
pub(crate) use proposal::{apply_existing_channel_decision, prepare_private_study_room};
pub use proposal::{
    propose_private_study_room, IntentProposalOutcomeV1, PrivateStudyRoomControlsProposalV1,
    PrivateStudyRoomCopyProposalV1, PrivateStudyRoomNamingProposalV1, PrivateStudyRoomProposalV1,
};
pub use provenance::{IntentCoverageV1, RequirementProvenanceV1};
