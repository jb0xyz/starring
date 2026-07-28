use chrono::{DateTime, Utc};

use crate::{
    ControllerId, DeploymentRevision, FencingToken, RuntimeDeploymentPhaseKindV1, RuntimeGeneration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PanelIneligibilityV1 {
    #[error("panel outcome counts overflow")]
    CountOverflow,
    #[error("panel reconciliation contains transient skips")]
    TransientSkipped,
    #[error("panel reconciliation contains unresolved-channel skips")]
    UnresolvedChannelSkipped,
    #[error("panel reconciliation contains failures")]
    Failed,
    #[error("panel reconciliation contains ambiguous outcomes")]
    AmbiguousOutcome,
    #[error("panel reconciliation has stale-message cleanup pending")]
    StaleCleanupPending,
    #[error("panel reconciliation has orphan-message cleanup pending")]
    OrphanCleanupPending,
    #[error("reposted panel has old-message cleanup pending")]
    RepostedOldMessageCleanupPending,
    #[error("panel reconciliation outcome count differs from declared count")]
    Incomplete,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeDeploymentError {
    #[error("runtime generation must be newer than the previous runtime")]
    RuntimeGenerationNotMonotonic,
    #[error("previous runtime belongs to a different guild or ruleset key")]
    PreviousRuntimeSlotMismatch,
    #[error("superseding deployment must have a distinct deployment identity")]
    SupersedingDeploymentIdentityConflict,
    #[error("superseding deployment belongs to a different tenant or installation")]
    SupersedingDeploymentScopeMismatch,
    #[error("deployment revision conflict: expected {expected}, actual {actual}")]
    RevisionConflict {
        expected: DeploymentRevision,
        actual: DeploymentRevision,
    },
    #[error("runtime generation conflict: expected {expected}, actual {actual}")]
    RuntimeGenerationConflict {
        expected: RuntimeGeneration,
        actual: RuntimeGeneration,
    },
    #[error("controller lease expiry must be after the acquisition time")]
    InvalidLeaseWindow,
    #[error("controller lease is held by {controller_id} until {expires_at}")]
    LeaseHeld {
        controller_id: ControllerId,
        expires_at: DateTime<Utc>,
    },
    #[error("controller lease is required")]
    LeaseRequired,
    #[error("controller lease expired at {expires_at}")]
    LeaseExpired { expires_at: DateTime<Utc> },
    #[error("controller lease belongs to a different controller")]
    ControllerMismatch,
    #[error("controller fencing token conflict: expected {expected}, actual {actual}")]
    FencingTokenConflict {
        expected: FencingToken,
        actual: FencingToken,
    },
    #[error("controller fencing token must increase monotonically")]
    FencingTokenNotMonotonic,
    #[error("operation {operation} is invalid in phase {current:?}")]
    InvalidTransition {
        current: RuntimeDeploymentPhaseKindV1,
        operation: &'static str,
    },
    #[error("attestation target differs from the immutable deployment target")]
    TargetMismatch,
    #[error("attestation previous runtime differs from the requested baseline")]
    PreviousRuntimeMismatch,
    #[error("activation attestation differs from the bound activation request")]
    ActivationRequestMismatch,
    #[error("attestation process instance differs from the accepted runtime process")]
    ProcessInstanceMismatch,
    #[error("panel certificate is not eligible for Live: {0}")]
    PanelIneligible(PanelIneligibilityV1),
    #[error("attestation timestamp precedes its prerequisite")]
    AttestationTimeRegression,
    #[error("runtime failure code or message is invalid")]
    InvalidFailure,
    #[error("terminal reason is invalid")]
    InvalidReason,
    #[error("Product drain supersession source differs from the acknowledged source")]
    ProductDrainSupersessionSourceMismatch,
    #[error("deployment revision overflow")]
    RevisionOverflow,
    #[error("runtime deployment snapshot violates state invariants")]
    InvalidSnapshot,
}
