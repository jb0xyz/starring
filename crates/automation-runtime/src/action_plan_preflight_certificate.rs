use std::fmt::{Debug, Formatter};

use automation_runtime_interaction::{
    build_interaction_preflight_certificate_digest_v1, InteractionActionPlanDigestV1,
    InteractionPreflightCertificateDigestInputV1, InteractionPreflightCertificateDigestV1,
    InteractionPreflightPlanDigestV1, InteractionPreflightSnapshotDigestV1,
    InteractionReceiptClaimRootV1, InteractionReceiptIdentityV1, InteractionRouteBindingV1,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionActionPreflightCertificateErrorV1 {
    #[error("interaction preflight receipt authority changed")]
    AuthorityDrift,
    #[error("interaction preflight action plan changed")]
    PlanDrift,
    #[error("interaction preflight Discord snapshot changed")]
    SnapshotDrift,
    #[error("interaction preflight certificate is corrupt")]
    CertificateCorrupt,
}

#[derive(Clone, PartialEq, Eq)]
pub struct InteractionActionPreflightCertificateV1 {
    receipt_identity: InteractionReceiptIdentityV1,
    route: InteractionRouteBindingV1,
    request_digest: automation_runtime_interaction::InteractionRequestDigestV1,
    action_plan_digest: InteractionActionPlanDigestV1,
    preflight_plan_digest: InteractionPreflightPlanDigestV1,
    snapshot_digest: InteractionPreflightSnapshotDigestV1,
    certificate_digest: InteractionPreflightCertificateDigestV1,
}

impl InteractionActionPreflightCertificateV1 {
    pub fn issue(
        claim_root: &InteractionReceiptClaimRootV1,
        action_plan_digest: InteractionActionPlanDigestV1,
        preflight_plan_digest: InteractionPreflightPlanDigestV1,
        snapshot_digest: InteractionPreflightSnapshotDigestV1,
    ) -> Self {
        let certificate_digest = build_interaction_preflight_certificate_digest_v1(
            InteractionPreflightCertificateDigestInputV1 {
                claim_root,
                action_plan_digest: &action_plan_digest,
                preflight_plan_digest: &preflight_plan_digest,
                snapshot_digest: &snapshot_digest,
            },
        );
        Self {
            receipt_identity: claim_root.identity(),
            route: claim_root.route().clone(),
            request_digest: claim_root.request_digest().clone(),
            action_plan_digest,
            preflight_plan_digest,
            snapshot_digest,
            certificate_digest,
        }
    }

    pub fn verify(
        &self,
        claim_root: &InteractionReceiptClaimRootV1,
        action_plan_digest: &InteractionActionPlanDigestV1,
        preflight_plan_digest: &InteractionPreflightPlanDigestV1,
        snapshot_digest: &InteractionPreflightSnapshotDigestV1,
    ) -> Result<(), InteractionActionPreflightCertificateErrorV1> {
        if self.receipt_identity != claim_root.identity()
            || self.route != *claim_root.route()
            || self.request_digest != *claim_root.request_digest()
        {
            return Err(InteractionActionPreflightCertificateErrorV1::AuthorityDrift);
        }
        if &self.action_plan_digest != action_plan_digest
            || &self.preflight_plan_digest != preflight_plan_digest
        {
            return Err(InteractionActionPreflightCertificateErrorV1::PlanDrift);
        }
        if &self.snapshot_digest != snapshot_digest {
            return Err(InteractionActionPreflightCertificateErrorV1::SnapshotDrift);
        }
        let expected = build_interaction_preflight_certificate_digest_v1(
            InteractionPreflightCertificateDigestInputV1 {
                claim_root,
                action_plan_digest,
                preflight_plan_digest,
                snapshot_digest,
            },
        );
        if self.certificate_digest != expected {
            return Err(InteractionActionPreflightCertificateErrorV1::CertificateCorrupt);
        }
        Ok(())
    }

    pub fn digest(&self) -> &InteractionPreflightCertificateDigestV1 {
        &self.certificate_digest
    }

    pub fn action_plan_digest(&self) -> &InteractionActionPlanDigestV1 {
        &self.action_plan_digest
    }

    pub fn preflight_plan_digest(&self) -> &InteractionPreflightPlanDigestV1 {
        &self.preflight_plan_digest
    }

    pub fn snapshot_digest(&self) -> &InteractionPreflightSnapshotDigestV1 {
        &self.snapshot_digest
    }
}

impl Debug for InteractionActionPreflightCertificateV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InteractionActionPreflightCertificateV1(<redacted>)")
    }
}
