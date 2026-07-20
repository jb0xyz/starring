mod bridge;
mod digest;
mod id;
mod model;
mod planner;
mod ports;
mod service;
mod store;

pub use bridge::{
    ProductActivationBridge, ProductApprovalEnvironmentError, ProductApprovalEnvironmentProvider,
    ProductApprovalEnvironmentV1,
};
pub use digest::{approval_payload_digest_v1, DigestError};
pub use id::{
    AuthoringHash, AuthoringSessionId, AutomationInstallationId, BindingRevision, IdempotencyKey,
    IdempotencyScopeDigest, OpaqueIdError, PolicyRevision, PrincipalId, PromotionId,
    PromotionIdError, PromotionRequestDigest, PromotionRevision, RevisionError, SessionGeneration,
    TenantId,
};
pub use model::{
    ApprovalPolicyV1, AuthenticatedPromotionContext, AuthoringEvidenceV1,
    AuthoringPreviewSummaryV1, AuthoringPreviewV1, NewPromotionV1, PendingActivationDispositionV1,
    PendingActivationLinkV1, ProductApprovalPayloadV1, PromotionIntentV1, PromotionRecordV1,
    PromotionRecordValidationError, PromotionStageV1, PublicationDispositionV1,
    PublicationRecordV1,
};
pub use planner::{
    derive_promotion_identity_from_secret_v1, derive_promotion_identity_v1,
    plan_activation_link_v1, plan_approval_environment_v1, plan_pending_activation_v1,
    plan_ruleset_publication_v1, plan_start_promotion_ref_v1, plan_start_promotion_v1,
    validate_exact_planned_record_v1, ActivationLinkProposalV1, ApprovalEnvironmentProposalV1,
    LinkedActivationTransitionV1, PendingActivationProposalV1, PendingActivationTransitionV1,
    PreparedPromotionPlanV1, PromotionIdentityV1, PromotionPlanValidationErrorV1,
    PublicationTransitionV1, RuleSetPublicationProposalV1,
};
pub use ports::{
    EnsurePendingActivationV1, LinkPendingActivationV1, PendingActivationPort,
    PendingActivationPortError, PendingActivationReceiptV1, PublicationPortOutcomeV1,
    PublishAuthoringRuleSetV1, PublishedAuthoringRuleSetV1, ResolveProductApprovalContextV1,
    ResolvedProductApprovalContextV1, RuleSetPublicationPort,
};
pub use service::{PromotionError, PromotionService, ResumePromotionOutcomeV1, StartPromotionV1};
pub use store::{
    CreatePromotionOutcomeV1, InMemoryPromotionStore, ManualPromotionClock, PromotionClock,
    PromotionStore, PromotionStoreError, UtcPromotionClock,
};
