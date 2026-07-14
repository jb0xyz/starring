mod model;
mod normalize;
mod proposal;
#[cfg(test)]
mod tests;

pub use model::{
    ClosePolicyV1, ExistingChannelKey, FeatureId, IntentLocaleV1, IntentRequestedOutcome,
    IntentResolutionContext, MissingDecision, MissingDecisionKind, RoomNamePatternV1,
};
pub use normalize::ValidatedIntentV1;
pub use proposal::{
    propose_private_study_room, IntentProposalOutcomeV1, PrivateStudyRoomControlsProposalV1,
    PrivateStudyRoomCopyProposalV1, PrivateStudyRoomNamingProposalV1, PrivateStudyRoomProposalV1,
};
