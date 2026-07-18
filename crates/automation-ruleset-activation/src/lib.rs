pub mod approval;
pub mod id;
pub mod model;
pub mod service;
pub mod store;

pub use approval::{
    approval_policy_digest_v1, product_approval_context_digest_v1, ActivationApprovalContextV1,
    ActivationLinkStateV1, ApprovalBindingContextV1, ApprovalPolicyBindingV1,
    ExpectedActiveBaselineV1, ProductApprovalContextV1,
};
pub use id::{
    ActivationDigest, ActivationHashError, ActivationIdError, ActivationPromotionId,
    ActivationRequestId, ApplyAttemptId,
};
pub use model::{
    ActivationRequest, ActivationRequestState, ActivationTarget, ActivationTerminationV1,
    ApplyErrorRecord, ApplyFailureKind, Approval, ApprovalDecisionError, ClaimDecision, Completion,
    CompletionKind, CreateActivationRequest, CreateProductActivationRequest, LinkDecision,
    LinkDecisionError, ObservedActive, Rejection, RejectionDecisionError, SupersessionReasonV1,
    TransitionError, WithdrawDecisionError,
};
#[cfg(feature = "unsafe-dev-activation")]
pub use service::unsafe_dev_activate;
pub use service::{
    ActivationEnvironment, ActivationEnvironmentError, ActivationEnvironmentProvider,
    ActivationService, ApplyError, ApplyOutcome, RecoveryDisposition, RecoveryEntry,
    RecoveryReport, RequestActivation, RequestActivationError,
};
pub use store::{
    ActivationClock, ActivationRequestStore, ActivationStoreError, ApproveError, ClaimOutcome,
    InMemoryActivationRequestStore, LinkProductActivation, LinkProductError, ManualActivationClock,
    RejectError, UtcActivationClock, WithdrawError,
};
