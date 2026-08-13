#![forbid(unsafe_code)]

//! Pure typed proof boundary for interaction preflight and effect dispatch.
//!
//! Stateful/live dispatch is intentionally unavailable in this milestone. The opaque prepared
//! dispatch has no public constructor or deserializer; only its bounded typed views and
//! domain-separated output digests are public.

mod dispatch;
mod journal_plan;
mod preflight_certificate;

#[cfg(test)]
mod test_support;

pub use dispatch::{
    require_stateful_interaction_effect_dispatch_integration_v1,
    InteractionEffectDispatchActionDigestV1, InteractionEffectDispatchActionV1,
    InteractionEffectDispatchBodyDigestV1, InteractionEffectDispatchBodyRefV1,
    InteractionEffectDispatchBodyV1, InteractionEffectDispatchButtonV1,
    InteractionEffectDispatchChannelResourceV1, InteractionEffectDispatchMessageResourceV1,
    InteractionEffectDispatchPreparedDigestV1, InteractionEffectDispatchReceiptRequirementV1,
    InteractionEffectDispatchRegistrationIdentityV1, InteractionEffectDispatchResourceDependencyV1,
    InteractionEffectDispatchRoleResourceV1, InteractionEffectDispatchSourceIdentityV1,
    InteractionEffectDispatchTeardownManifestV1, PreparedInteractionEffectDispatchV1,
    StatefulInteractionEffectDispatchUnavailableV1,
    MAX_INTERACTION_EFFECT_DISPATCH_CANONICAL_BYTES_V1,
    STATEFUL_INTERACTION_EFFECT_DISPATCH_INTEGRATED_V1,
};
pub use journal_plan::{
    InteractionEffectJournalPlanBindErrorV1, InteractionEffectJournalPlanEntryV1,
    InteractionEffectJournalPlanV1,
};
pub use preflight_certificate::{
    InteractionActionPreflightCertificateErrorV1, InteractionActionPreflightCertificateV1,
};
