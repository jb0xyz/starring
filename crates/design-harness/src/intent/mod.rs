mod candidate;
mod catalog;
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
pub use compile::{
    compile_intent, CompilationManifestV1, CompilationVerificationV1, CompiledIntentV1,
};
pub use model::{
    ClosePolicyV1, ExistingChannelKey, FeatureId, IntentLocaleV1, IntentRequestedOutcome,
    IntentResolutionContext, MissingDecision, MissingDecisionKind, RoomNamePatternV1,
};
pub use normalize::ValidatedIntentV1;
pub use proposal::{
    propose_private_study_room, IntentProposalOutcomeV1, PrivateStudyRoomControlsProposalV1,
    PrivateStudyRoomCopyProposalV1, PrivateStudyRoomNamingProposalV1, PrivateStudyRoomProposalV1,
};
pub use provenance::{IntentCoverageV1, RequirementProvenanceV1};
