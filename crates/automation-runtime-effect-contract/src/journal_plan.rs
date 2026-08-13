use std::fmt::{Debug, Formatter};

use automation_runtime_interaction::{
    InteractionEffectExpectedPostimageDigestV1, InteractionEffectKindV1,
    InteractionEffectPlanDefinitionV1, InteractionPreflightCertificateDigestV1,
    InteractionPreflightSnapshotDigestV1, MAX_INTERACTION_EFFECT_ACTIONS_V1,
};

use crate::InteractionActionPreflightCertificateV1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionEffectJournalPlanBindErrorV1 {
    #[error("interaction effect journal plan contains too many effects")]
    EffectCount,
    #[error("interaction effect journal plan action indices are not canonical")]
    ActionOrder,
    #[error("interaction effect journal plan is not bound to the certified receipt")]
    ReceiptBinding,
    #[error("interaction effect journal plan is not bound to the certified action plan")]
    ActionPlanBinding,
    #[error("interaction effect journal plan is not bound to the preflight certificate")]
    CertificateBinding,
    #[error("interaction effect journal response tail is missing, duplicated, or not final")]
    ResponseTail,
}

#[derive(Clone, PartialEq, Eq)]
pub struct InteractionEffectJournalPlanEntryV1 {
    definition: InteractionEffectPlanDefinitionV1,
    expected_postimage_digest: InteractionEffectExpectedPostimageDigestV1,
}

impl InteractionEffectJournalPlanEntryV1 {
    pub fn new(
        definition: InteractionEffectPlanDefinitionV1,
        expected_postimage_digest: InteractionEffectExpectedPostimageDigestV1,
    ) -> Self {
        Self {
            definition,
            expected_postimage_digest,
        }
    }

    pub fn definition(&self) -> &InteractionEffectPlanDefinitionV1 {
        &self.definition
    }

    pub fn expected_postimage_digest(&self) -> &InteractionEffectExpectedPostimageDigestV1 {
        &self.expected_postimage_digest
    }
}

impl Debug for InteractionEffectJournalPlanEntryV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InteractionEffectJournalPlanEntryV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct InteractionEffectJournalPlanV1 {
    preflight_certificate_digest: InteractionPreflightCertificateDigestV1,
    snapshot_digest: InteractionPreflightSnapshotDigestV1,
    entries: Vec<InteractionEffectJournalPlanEntryV1>,
}

impl InteractionEffectJournalPlanV1 {
    pub fn bind(
        certificate: &InteractionActionPreflightCertificateV1,
        entries: Vec<InteractionEffectJournalPlanEntryV1>,
    ) -> Result<Self, InteractionEffectJournalPlanBindErrorV1> {
        if entries.len() > usize::from(MAX_INTERACTION_EFFECT_ACTIONS_V1) {
            return Err(InteractionEffectJournalPlanBindErrorV1::EffectCount);
        }

        let mut response_tail = None;
        for (position, entry) in entries.iter().enumerate() {
            let action = entry.definition.action();
            if usize::from(action.action_index().get()) != position {
                return Err(InteractionEffectJournalPlanBindErrorV1::ActionOrder);
            }
            if action.receipt_identity() != certificate.receipt_identity() {
                return Err(InteractionEffectJournalPlanBindErrorV1::ReceiptBinding);
            }
            if action.action_plan_digest() != certificate.action_plan_digest() {
                return Err(InteractionEffectJournalPlanBindErrorV1::ActionPlanBinding);
            }
            if action.preflight_certificate_digest() != certificate.digest() {
                return Err(InteractionEffectJournalPlanBindErrorV1::CertificateBinding);
            }
            if action.kind() == InteractionEffectKindV1::EditResponse
                && response_tail.replace(position).is_some()
            {
                return Err(InteractionEffectJournalPlanBindErrorV1::ResponseTail);
            }
        }

        if !entries.is_empty() && response_tail != Some(entries.len() - 1) {
            return Err(InteractionEffectJournalPlanBindErrorV1::ResponseTail);
        }

        Ok(Self {
            preflight_certificate_digest: certificate.digest().clone(),
            snapshot_digest: certificate.snapshot_digest().clone(),
            entries,
        })
    }

    pub fn preflight_certificate_digest(&self) -> &InteractionPreflightCertificateDigestV1 {
        &self.preflight_certificate_digest
    }

    pub fn snapshot_digest(&self) -> &InteractionPreflightSnapshotDigestV1 {
        &self.snapshot_digest
    }

    pub fn entries(&self) -> &[InteractionEffectJournalPlanEntryV1] {
        &self.entries
    }
}

impl Debug for InteractionEffectJournalPlanV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InteractionEffectJournalPlanV1")
            .field("effect_count", &self.entries.len())
            .field("contents", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        certificate_v1, certificate_with_seed_v1, create_role_entry_v1, edit_response_entry_v1,
        edit_response_entry_with_binding_v1,
    };

    #[test]
    fn certified_plan_accepts_canonical_effect_order_and_final_response_tail() {
        let certificate = certificate_v1();
        let plan = InteractionEffectJournalPlanV1::bind(
            &certificate,
            vec![
                create_role_entry_v1(&certificate, 0),
                edit_response_entry_v1(&certificate, 1),
            ],
        )
        .unwrap();
        assert_eq!(plan.entries().len(), 2);
        assert_eq!(plan.preflight_certificate_digest(), certificate.digest());
        assert_eq!(plan.snapshot_digest(), certificate.snapshot_digest());
    }

    #[test]
    fn plan_rejects_receipt_action_plan_and_certificate_binding_drift() {
        let certificate = certificate_v1();
        let foreign = certificate_with_seed_v1(2);

        let receipt_drift = edit_response_entry_with_binding_v1(
            &certificate,
            0,
            foreign.receipt_identity(),
            certificate.action_plan_digest().clone(),
            certificate.digest().clone(),
        );
        assert_eq!(
            InteractionEffectJournalPlanV1::bind(&certificate, vec![receipt_drift]),
            Err(InteractionEffectJournalPlanBindErrorV1::ReceiptBinding)
        );

        let action_plan_drift = edit_response_entry_with_binding_v1(
            &certificate,
            0,
            certificate.receipt_identity(),
            foreign.action_plan_digest().clone(),
            certificate.digest().clone(),
        );
        assert_eq!(
            InteractionEffectJournalPlanV1::bind(&certificate, vec![action_plan_drift]),
            Err(InteractionEffectJournalPlanBindErrorV1::ActionPlanBinding)
        );

        let certificate_drift = edit_response_entry_with_binding_v1(
            &certificate,
            0,
            certificate.receipt_identity(),
            certificate.action_plan_digest().clone(),
            foreign.digest().clone(),
        );
        assert_eq!(
            InteractionEffectJournalPlanV1::bind(&certificate, vec![certificate_drift]),
            Err(InteractionEffectJournalPlanBindErrorV1::CertificateBinding)
        );
    }

    #[test]
    fn plan_rejects_noncanonical_action_indices() {
        let certificate = certificate_v1();
        assert_eq!(
            InteractionEffectJournalPlanV1::bind(
                &certificate,
                vec![edit_response_entry_v1(&certificate, 1)],
            ),
            Err(InteractionEffectJournalPlanBindErrorV1::ActionOrder)
        );
    }

    #[test]
    fn plan_requires_exactly_one_final_response_tail_for_effectful_dispatch() {
        let certificate = certificate_v1();
        assert_eq!(
            InteractionEffectJournalPlanV1::bind(
                &certificate,
                vec![create_role_entry_v1(&certificate, 0)],
            ),
            Err(InteractionEffectJournalPlanBindErrorV1::ResponseTail)
        );
        assert_eq!(
            InteractionEffectJournalPlanV1::bind(
                &certificate,
                vec![
                    edit_response_entry_v1(&certificate, 0),
                    create_role_entry_v1(&certificate, 1),
                ],
            ),
            Err(InteractionEffectJournalPlanBindErrorV1::ResponseTail)
        );
        assert_eq!(
            InteractionEffectJournalPlanV1::bind(
                &certificate,
                vec![
                    edit_response_entry_v1(&certificate, 0),
                    edit_response_entry_v1(&certificate, 1),
                ],
            ),
            Err(InteractionEffectJournalPlanBindErrorV1::ResponseTail)
        );
    }
}
