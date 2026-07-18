mod bridge;
mod digest;
mod id;
mod model;
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
