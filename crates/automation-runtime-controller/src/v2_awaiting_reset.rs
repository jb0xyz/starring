use automation_runtime_convergence::{
    DeploymentRevision, RuntimeDeployment, RuntimeDeploymentPhaseV1, RuntimeDeploymentSnapshotV1,
    TransitionOutcomeV1,
};
use chrono::{DateTime, Utc};

use crate::{
    AwaitingCertificationScopeObservationV2, RuntimeCertificationDivergenceV2,
    RuntimeCertificationOperationIdV2, RuntimeCertificationReceiptV2, RuntimeDrainIntentIdV2,
    RuntimeServingSlotV2,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeAwaitingGatewayReadyResetBasisKindV2 {
    NoOperationReserved {
        snapshot: RuntimeDeploymentSnapshotV1,
        observed_at: DateTime<Utc>,
    },
    NoAttestationForReservedOperation {
        snapshot: RuntimeDeploymentSnapshotV1,
        reserved_operation_id: RuntimeCertificationOperationIdV2,
        observed_at: DateTime<Utc>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeAwaitingGatewayReadyResetBasisV2 {
    kind: RuntimeAwaitingGatewayReadyResetBasisKindV2,
}

impl RuntimeAwaitingGatewayReadyResetBasisV2 {
    pub fn kind(&self) -> &RuntimeAwaitingGatewayReadyResetBasisKindV2 {
        &self.kind
    }

    pub fn snapshot(&self) -> &RuntimeDeploymentSnapshotV1 {
        match &self.kind {
            RuntimeAwaitingGatewayReadyResetBasisKindV2::NoOperationReserved {
                snapshot, ..
            }
            | RuntimeAwaitingGatewayReadyResetBasisKindV2::NoAttestationForReservedOperation {
                snapshot,
                ..
            } => snapshot,
        }
    }

    pub fn observed_at(&self) -> DateTime<Utc> {
        match &self.kind {
            RuntimeAwaitingGatewayReadyResetBasisKindV2::NoOperationReserved {
                observed_at,
                ..
            }
            | RuntimeAwaitingGatewayReadyResetBasisKindV2::NoAttestationForReservedOperation {
                observed_at,
                ..
            } => *observed_at,
        }
    }

    pub fn reserved_operation_id(&self) -> Option<&RuntimeCertificationOperationIdV2> {
        match &self.kind {
            RuntimeAwaitingGatewayReadyResetBasisKindV2::NoOperationReserved { .. } => None,
            RuntimeAwaitingGatewayReadyResetBasisKindV2::NoAttestationForReservedOperation {
                reserved_operation_id,
                ..
            } => Some(reserved_operation_id),
        }
    }

    pub fn serving_slot(&self) -> RuntimeServingSlotV2 {
        RuntimeServingSlotV2::from_target(&self.snapshot().target)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "classification preserves the exact committed or observed certification payload"
)]
pub enum RuntimeAwaitingGatewayReadyResetClassificationV2 {
    Eligible(RuntimeAwaitingGatewayReadyResetBasisV2),
    Committed(RuntimeCertificationReceiptV2),
    Diverged(RuntimeCertificationDivergenceV2),
}

impl RuntimeAwaitingGatewayReadyResetClassificationV2 {
    pub fn from_observation(observation: AwaitingCertificationScopeObservationV2) -> Self {
        match observation {
            AwaitingCertificationScopeObservationV2::Committed(receipt) => Self::Committed(receipt),
            AwaitingCertificationScopeObservationV2::NoOperationReserved {
                snapshot,
                observed_at,
            } => classify_basis(
                RuntimeAwaitingGatewayReadyResetBasisKindV2::NoOperationReserved {
                    snapshot,
                    observed_at,
                },
            ),
            AwaitingCertificationScopeObservationV2::NoAttestationForReservedOperation {
                snapshot,
                reserved_operation_id,
                observed_at,
            } => classify_basis(
                RuntimeAwaitingGatewayReadyResetBasisKindV2::NoAttestationForReservedOperation {
                    snapshot,
                    reserved_operation_id,
                    observed_at,
                },
            ),
            AwaitingCertificationScopeObservationV2::Diverged(divergence) => {
                Self::Diverged(divergence)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeResetAwaitingGatewayReadyV2 {
    basis: RuntimeAwaitingGatewayReadyResetBasisV2,
}

impl RuntimeResetAwaitingGatewayReadyV2 {
    pub fn new(basis: RuntimeAwaitingGatewayReadyResetBasisV2) -> Self {
        Self { basis }
    }

    pub fn basis(&self) -> &RuntimeAwaitingGatewayReadyResetBasisV2 {
        &self.basis
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeCertificationReservationResetReceiptV2 {
    NotReserved,
    Consumed {
        operation_id: RuntimeCertificationOperationIdV2,
        resulting_revision: DeploymentRevision,
        consumed_at: DateTime<Utc>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeAwaitingGatewayReadyResetReceiptErrorV2 {
    #[error("AwaitingGatewayReady reset returned an invalid successor snapshot")]
    InvalidSuccessor,
    #[error("AwaitingGatewayReady reset returned a non-successor revision")]
    RevisionMismatch,
    #[error("AwaitingGatewayReady reset did not return to ReconcilingPanels")]
    PhaseMismatch,
    #[error("AwaitingGatewayReady reset changed preserved deployment truth")]
    PreservedTruthMismatch,
    #[error("AwaitingGatewayReady reset retained cleared runtime evidence")]
    RuntimeEvidenceNotCleared,
    #[error("AwaitingGatewayReady reset retained the old controller lease")]
    ControllerLeaseRetained,
    #[error("AwaitingGatewayReady reset returned inconsistent reservation consumption")]
    ReservationMismatch,
}

impl RuntimeAwaitingGatewayReadyResetReceiptErrorV2 {
    pub const fn into_divergence(self) -> RuntimeCertificationDivergenceV2 {
        RuntimeCertificationDivergenceV2::PersistenceCorrupt
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeAwaitingGatewayReadyResetReceiptV2 {
    outcome: TransitionOutcomeV1,
    source_revision: DeploymentRevision,
    snapshot: RuntimeDeploymentSnapshotV1,
    reservation: RuntimeCertificationReservationResetReceiptV2,
    reset_at: DateTime<Utc>,
}

impl RuntimeAwaitingGatewayReadyResetReceiptV2 {
    pub fn new(
        request: &RuntimeResetAwaitingGatewayReadyV2,
        outcome: TransitionOutcomeV1,
        snapshot: RuntimeDeploymentSnapshotV1,
        reservation: RuntimeCertificationReservationResetReceiptV2,
        reset_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeAwaitingGatewayReadyResetReceiptErrorV2> {
        validate_reset_receipt(request, outcome, &snapshot, &reservation, reset_at)?;
        Ok(Self {
            outcome,
            source_revision: request.basis().snapshot().revision,
            snapshot,
            reservation,
            reset_at,
        })
    }

    pub fn outcome(&self) -> TransitionOutcomeV1 {
        self.outcome
    }

    pub fn source_revision(&self) -> DeploymentRevision {
        self.source_revision
    }

    pub fn snapshot(&self) -> &RuntimeDeploymentSnapshotV1 {
        &self.snapshot
    }

    pub fn reservation(&self) -> &RuntimeCertificationReservationResetReceiptV2 {
        &self.reservation
    }

    pub fn reset_at(&self) -> DateTime<Utc> {
        self.reset_at
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "the closed reset outcome preserves exact certification and deployment receipts"
)]
pub enum RuntimeAwaitingGatewayReadyResetOutcomeV2 {
    Reset(RuntimeAwaitingGatewayReadyResetReceiptV2),
    Committed(RuntimeCertificationReceiptV2),
    ProductDrainIntentPresent { intent_id: RuntimeDrainIntentIdV2 },
    Diverged(RuntimeCertificationDivergenceV2),
}

fn classify_basis(
    kind: RuntimeAwaitingGatewayReadyResetBasisKindV2,
) -> RuntimeAwaitingGatewayReadyResetClassificationV2 {
    let snapshot = match &kind {
        RuntimeAwaitingGatewayReadyResetBasisKindV2::NoOperationReserved { snapshot, .. }
        | RuntimeAwaitingGatewayReadyResetBasisKindV2::NoAttestationForReservedOperation {
            snapshot,
            ..
        } => snapshot,
    };
    let fenced_awaiting = snapshot
        .controller_lease
        .as_ref()
        .is_some_and(|lease| snapshot.last_fencing_token == Some(lease.fencing_token));
    if RuntimeDeployment::restore(snapshot.clone()).is_err()
        || !matches!(
            snapshot.phase,
            RuntimeDeploymentPhaseV1::AwaitingGatewayReady
        )
        || !fenced_awaiting
    {
        RuntimeAwaitingGatewayReadyResetClassificationV2::Diverged(
            RuntimeCertificationDivergenceV2::PersistenceCorrupt,
        )
    } else {
        RuntimeAwaitingGatewayReadyResetClassificationV2::Eligible(
            RuntimeAwaitingGatewayReadyResetBasisV2 { kind },
        )
    }
}

fn validate_reset_receipt(
    request: &RuntimeResetAwaitingGatewayReadyV2,
    outcome: TransitionOutcomeV1,
    successor: &RuntimeDeploymentSnapshotV1,
    reservation: &RuntimeCertificationReservationResetReceiptV2,
    reset_at: DateTime<Utc>,
) -> Result<(), RuntimeAwaitingGatewayReadyResetReceiptErrorV2> {
    let source = request.basis().snapshot();
    let expected_revision = source
        .revision
        .next()
        .map_err(|_| RuntimeAwaitingGatewayReadyResetReceiptErrorV2::RevisionMismatch)?;
    if successor.revision != expected_revision || outcome.revision() != successor.revision {
        return Err(RuntimeAwaitingGatewayReadyResetReceiptErrorV2::RevisionMismatch);
    }
    if !matches!(successor.phase, RuntimeDeploymentPhaseV1::ReconcilingPanels) {
        return Err(RuntimeAwaitingGatewayReadyResetReceiptErrorV2::PhaseMismatch);
    }
    if successor.identity != source.identity
        || successor.target != source.target
        || successor.runtime_generation != source.runtime_generation
        || successor.previous_runtime != source.previous_runtime
        || successor.requested_at != source.requested_at
        || successor.last_fencing_token != source.last_fencing_token
        || successor.preflight != source.preflight
        || successor.drain != source.drain
        || successor.activation != source.activation
        || successor.last_live_recovery != source.last_live_recovery
        || successor.last_runtime_failure != source.last_runtime_failure
    {
        return Err(RuntimeAwaitingGatewayReadyResetReceiptErrorV2::PreservedTruthMismatch);
    }
    if successor.panel_certificate.is_some()
        || successor.gateway_ready.is_some()
        || successor.live.is_some()
    {
        return Err(RuntimeAwaitingGatewayReadyResetReceiptErrorV2::RuntimeEvidenceNotCleared);
    }
    if successor.controller_lease.is_some() {
        return Err(RuntimeAwaitingGatewayReadyResetReceiptErrorV2::ControllerLeaseRetained);
    }
    RuntimeDeployment::restore(successor.clone())
        .map_err(|_| RuntimeAwaitingGatewayReadyResetReceiptErrorV2::InvalidSuccessor)?;
    let reservation_matches = match (request.basis().reserved_operation_id(), reservation) {
        (None, RuntimeCertificationReservationResetReceiptV2::NotReserved) => true,
        (
            Some(expected_operation_id),
            RuntimeCertificationReservationResetReceiptV2::Consumed {
                operation_id,
                resulting_revision,
                consumed_at,
            },
        ) => {
            operation_id == expected_operation_id
                && *resulting_revision == successor.revision
                && *consumed_at == reset_at
        }
        _ => false,
    };
    if !reservation_matches {
        return Err(RuntimeAwaitingGatewayReadyResetReceiptErrorV2::ReservationMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
    use automation_runtime_convergence::{
        ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, BindingRevision,
        ControllerId, ControllerLeaseV1, DeploymentId, DeploymentRevision, DrainAttestationV1,
        FencingToken, InstallationId, PanelCertificateId, PanelCertificateV1, PanelReportDigestV1,
        PreflightAttestationV1, ProcessInstanceId, PromotionId, RuntimeDeployment,
        RuntimeDeploymentIdentityV1, RuntimeDeploymentPhaseV1, RuntimeDeploymentSnapshotV1,
        RuntimeDeploymentTargetV1, RuntimeGeneration, TenantId, TransitionOutcomeV1,
    };
    use chrono::{DateTime, Utc};
    use discord_model::GuildId;
    use resource_resolution::ResourceBindingFingerprint;

    use super::{
        RuntimeAwaitingGatewayReadyResetBasisKindV2,
        RuntimeAwaitingGatewayReadyResetClassificationV2,
        RuntimeAwaitingGatewayReadyResetReceiptErrorV2, RuntimeAwaitingGatewayReadyResetReceiptV2,
        RuntimeCertificationReservationResetReceiptV2, RuntimeResetAwaitingGatewayReadyV2,
    };
    use crate::{
        AwaitingCertificationScopeObservationV2, RuntimeCertificationDivergenceV2,
        RuntimeCertificationOperationIdV2,
    };

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn target() -> RuntimeDeploymentTargetV1 {
        RuntimeDeploymentTargetV1 {
            guild_id: GuildId(7),
            ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
            version: RuleSetVersionId::FIRST,
            content_hash: RuleSetContentHash::parse_hex(&"b".repeat(64)).unwrap(),
            binding_revision: BindingRevision::new(3).unwrap(),
            binding_fingerprint: ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap(),
        }
    }

    fn awaiting_snapshot() -> RuntimeDeploymentSnapshotV1 {
        let identity = RuntimeDeploymentIdentityV1 {
            deployment_id: DeploymentId::parse("deployment:1").unwrap(),
            tenant_id: TenantId::parse("tenant:1").unwrap(),
            installation_id: InstallationId::parse("installation:1").unwrap(),
            promotion_id: PromotionId::parse("9".repeat(64)).unwrap(),
            activation_request_id: ActivationRequestId::parse("activation:1").unwrap(),
        };
        let target = target();
        let runtime_generation = RuntimeGeneration::new(4).unwrap();
        let fencing_token = FencingToken::new(3).unwrap();
        let snapshot = RuntimeDeploymentSnapshotV1 {
            identity: identity.clone(),
            target: target.clone(),
            runtime_generation,
            previous_runtime: None,
            requested_at: at(1),
            revision: DeploymentRevision::new(8).unwrap(),
            phase: RuntimeDeploymentPhaseV1::AwaitingGatewayReady,
            controller_lease: Some(ControllerLeaseV1 {
                controller_id: ControllerId::parse("controller:1").unwrap(),
                fencing_token,
                acquired_at: at(10),
                expires_at: at(100),
            }),
            last_fencing_token: Some(fencing_token),
            preflight: Some(PreflightAttestationV1 {
                target: target.clone(),
                runtime_generation,
                observed_runtime: None,
                checked_at: at(11),
            }),
            drain: Some(DrainAttestationV1 {
                previous_runtime: None,
                target_runtime_generation: runtime_generation,
                drained_at: at(12),
            }),
            activation: Some(ActivationAttestationV1 {
                activation_request_id: identity.activation_request_id,
                target: target.clone(),
                runtime_generation,
                kind: ActivationOutcomeKindV1::Activated,
                activated_at: at(13),
            }),
            panel_certificate: Some(PanelCertificateV1 {
                certificate_id: PanelCertificateId::parse("panel:1").unwrap(),
                report_digest: PanelReportDigestV1::parse("c".repeat(64)).unwrap(),
                target,
                runtime_generation,
                process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
                declared_count: 0,
                installed_count: 0,
                unchanged_count: 0,
                skipped_transient_count: 0,
                skipped_unresolved_channel_count: 0,
                failed_count: 0,
                ambiguous_outcome_count: 0,
                stale_message_cleanup_pending_count: 0,
                orphan_message_cleanup_pending_count: 0,
                reposted_old_message_cleanup_pending_count: 0,
                reconciled_at: at(14),
            }),
            gateway_ready: None,
            live: None,
            last_live_recovery: None,
            last_runtime_failure: None,
        };
        RuntimeDeployment::restore(snapshot.clone()).unwrap();
        snapshot
    }

    fn operation_id() -> RuntimeCertificationOperationIdV2 {
        RuntimeCertificationOperationIdV2::parse("00112233445566778899aabbccddeeff").unwrap()
    }

    fn no_operation_request() -> RuntimeResetAwaitingGatewayReadyV2 {
        let classification = RuntimeAwaitingGatewayReadyResetClassificationV2::from_observation(
            AwaitingCertificationScopeObservationV2::NoOperationReserved {
                snapshot: awaiting_snapshot(),
                observed_at: at(20),
            },
        );
        match classification {
            RuntimeAwaitingGatewayReadyResetClassificationV2::Eligible(basis) => {
                RuntimeResetAwaitingGatewayReadyV2::new(basis)
            }
            _ => panic!(),
        }
    }

    fn reserved_request() -> RuntimeResetAwaitingGatewayReadyV2 {
        let classification = RuntimeAwaitingGatewayReadyResetClassificationV2::from_observation(
            AwaitingCertificationScopeObservationV2::NoAttestationForReservedOperation {
                snapshot: awaiting_snapshot(),
                reserved_operation_id: operation_id(),
                observed_at: at(20),
            },
        );
        match classification {
            RuntimeAwaitingGatewayReadyResetClassificationV2::Eligible(basis) => {
                RuntimeResetAwaitingGatewayReadyV2::new(basis)
            }
            _ => panic!(),
        }
    }

    fn successor(source: &RuntimeDeploymentSnapshotV1) -> RuntimeDeploymentSnapshotV1 {
        let mut successor = source.clone();
        successor.revision = source.revision.next().unwrap();
        successor.phase = RuntimeDeploymentPhaseV1::ReconcilingPanels;
        successor.controller_lease = None;
        successor.panel_certificate = None;
        RuntimeDeployment::restore(successor.clone()).unwrap();
        successor
    }

    #[test]
    fn only_the_two_no_attestation_observations_become_reset_bases() {
        let no_operation = no_operation_request();
        let reserved = reserved_request();

        assert!(matches!(
            no_operation.basis().kind(),
            RuntimeAwaitingGatewayReadyResetBasisKindV2::NoOperationReserved { .. }
        ));
        assert_eq!(
            reserved.basis().reserved_operation_id(),
            Some(&operation_id())
        );
        assert_eq!(
            reserved.basis().serving_slot(),
            no_operation.basis().serving_slot()
        );
        assert_eq!(
            RuntimeAwaitingGatewayReadyResetClassificationV2::from_observation(
                AwaitingCertificationScopeObservationV2::Diverged(
                    RuntimeCertificationDivergenceV2::OwnershipLost,
                )
            ),
            RuntimeAwaitingGatewayReadyResetClassificationV2::Diverged(
                RuntimeCertificationDivergenceV2::OwnershipLost,
            )
        );
    }

    #[test]
    fn malformed_or_non_awaiting_observation_fails_closed() {
        let mut snapshot = awaiting_snapshot();
        snapshot.phase = RuntimeDeploymentPhaseV1::ReconcilingPanels;
        snapshot.panel_certificate = None;
        let classification = RuntimeAwaitingGatewayReadyResetClassificationV2::from_observation(
            AwaitingCertificationScopeObservationV2::NoOperationReserved {
                snapshot,
                observed_at: at(20),
            },
        );

        assert_eq!(
            classification,
            RuntimeAwaitingGatewayReadyResetClassificationV2::Diverged(
                RuntimeCertificationDivergenceV2::PersistenceCorrupt,
            )
        );
    }

    #[test]
    fn reset_basis_requires_a_fenced_awaiting_source_without_requiring_lease_freshness() {
        let mut missing_lease = awaiting_snapshot();
        missing_lease.controller_lease = None;
        RuntimeDeployment::restore(missing_lease.clone()).unwrap();
        assert_eq!(
            RuntimeAwaitingGatewayReadyResetClassificationV2::from_observation(
                AwaitingCertificationScopeObservationV2::NoOperationReserved {
                    snapshot: missing_lease,
                    observed_at: at(200),
                },
            ),
            RuntimeAwaitingGatewayReadyResetClassificationV2::Diverged(
                RuntimeCertificationDivergenceV2::PersistenceCorrupt,
            )
        );

        assert!(matches!(
            RuntimeAwaitingGatewayReadyResetClassificationV2::from_observation(
                AwaitingCertificationScopeObservationV2::NoOperationReserved {
                    snapshot: awaiting_snapshot(),
                    observed_at: at(200),
                },
            ),
            RuntimeAwaitingGatewayReadyResetClassificationV2::Eligible(_)
        ));
    }

    #[test]
    fn reset_receipt_accepts_exact_no_reservation_and_consumed_reservation_results() {
        let no_operation = no_operation_request();
        let no_operation_successor = successor(no_operation.basis().snapshot());
        let no_operation_receipt = RuntimeAwaitingGatewayReadyResetReceiptV2::new(
            &no_operation,
            TransitionOutcomeV1::Applied {
                revision: no_operation_successor.revision,
            },
            no_operation_successor.clone(),
            RuntimeCertificationReservationResetReceiptV2::NotReserved,
            at(21),
        )
        .unwrap();
        assert_eq!(
            no_operation_receipt.source_revision(),
            no_operation.basis().snapshot().revision
        );
        assert_eq!(no_operation_receipt.snapshot(), &no_operation_successor);

        let reserved = reserved_request();
        let reserved_successor = successor(reserved.basis().snapshot());
        let reserved_receipt = RuntimeAwaitingGatewayReadyResetReceiptV2::new(
            &reserved,
            TransitionOutcomeV1::Applied {
                revision: reserved_successor.revision,
            },
            reserved_successor.clone(),
            RuntimeCertificationReservationResetReceiptV2::Consumed {
                operation_id: operation_id(),
                resulting_revision: reserved_successor.revision,
                consumed_at: at(21),
            },
            at(21),
        )
        .unwrap();
        assert_eq!(reserved_receipt.reset_at(), at(21));
    }

    #[test]
    fn database_times_are_preserved_as_audit_values_not_used_as_authority() {
        let classification = RuntimeAwaitingGatewayReadyResetClassificationV2::from_observation(
            AwaitingCertificationScopeObservationV2::NoOperationReserved {
                snapshot: awaiting_snapshot(),
                observed_at: at(30),
            },
        );
        let RuntimeAwaitingGatewayReadyResetClassificationV2::Eligible(basis) = classification
        else {
            panic!();
        };
        let request = RuntimeResetAwaitingGatewayReadyV2::new(basis);
        let reset_snapshot = successor(request.basis().snapshot());
        let receipt = RuntimeAwaitingGatewayReadyResetReceiptV2::new(
            &request,
            TransitionOutcomeV1::Replayed {
                revision: reset_snapshot.revision,
            },
            reset_snapshot,
            RuntimeCertificationReservationResetReceiptV2::NotReserved,
            at(21),
        )
        .unwrap();

        assert_eq!(request.basis().observed_at(), at(30));
        assert_eq!(receipt.reset_at(), at(21));
    }

    #[test]
    fn reset_receipt_rejects_changed_truth_retained_evidence_and_wrong_consumption() {
        let request = reserved_request();
        let valid = successor(request.basis().snapshot());

        let mut changed = valid.clone();
        changed.target.content_hash = RuleSetContentHash::parse_hex(&"d".repeat(64)).unwrap();
        assert!(matches!(
            RuntimeAwaitingGatewayReadyResetReceiptV2::new(
                &request,
                TransitionOutcomeV1::Applied {
                    revision: changed.revision,
                },
                changed,
                RuntimeCertificationReservationResetReceiptV2::Consumed {
                    operation_id: operation_id(),
                    resulting_revision: valid.revision,
                    consumed_at: at(21),
                },
                at(21),
            ),
            Err(RuntimeAwaitingGatewayReadyResetReceiptErrorV2::PreservedTruthMismatch)
        ));

        let mut retained = valid.clone();
        retained.panel_certificate = request.basis().snapshot().panel_certificate.clone();
        assert_eq!(
            RuntimeAwaitingGatewayReadyResetReceiptV2::new(
                &request,
                TransitionOutcomeV1::Applied {
                    revision: retained.revision,
                },
                retained,
                RuntimeCertificationReservationResetReceiptV2::Consumed {
                    operation_id: operation_id(),
                    resulting_revision: valid.revision,
                    consumed_at: at(21),
                },
                at(21),
            ),
            Err(RuntimeAwaitingGatewayReadyResetReceiptErrorV2::RuntimeEvidenceNotCleared)
        );

        let wrong_id =
            RuntimeCertificationOperationIdV2::parse("ffeeddccbbaa99887766554433221100").unwrap();
        let error = RuntimeAwaitingGatewayReadyResetReceiptV2::new(
            &request,
            TransitionOutcomeV1::Applied {
                revision: valid.revision,
            },
            valid.clone(),
            RuntimeCertificationReservationResetReceiptV2::Consumed {
                operation_id: wrong_id,
                resulting_revision: valid.revision,
                consumed_at: at(21),
            },
            at(21),
        )
        .unwrap_err();
        assert_eq!(
            error,
            RuntimeAwaitingGatewayReadyResetReceiptErrorV2::ReservationMismatch
        );
        assert_eq!(
            error.into_divergence(),
            RuntimeCertificationDivergenceV2::PersistenceCorrupt
        );
    }

    #[test]
    fn reset_receipt_rejects_revision_phase_and_controller_lease_mismatches() {
        let request = reserved_request();
        let valid = successor(request.basis().snapshot());

        let mut wrong_revision = valid.clone();
        wrong_revision.revision = valid.revision.next().unwrap();
        assert_eq!(
            RuntimeAwaitingGatewayReadyResetReceiptV2::new(
                &request,
                TransitionOutcomeV1::Applied {
                    revision: wrong_revision.revision,
                },
                wrong_revision,
                RuntimeCertificationReservationResetReceiptV2::Consumed {
                    operation_id: operation_id(),
                    resulting_revision: valid.revision,
                    consumed_at: at(21),
                },
                at(21),
            ),
            Err(RuntimeAwaitingGatewayReadyResetReceiptErrorV2::RevisionMismatch)
        );

        let mut wrong_phase = valid.clone();
        wrong_phase.phase = RuntimeDeploymentPhaseV1::AwaitingGatewayReady;
        assert_eq!(
            RuntimeAwaitingGatewayReadyResetReceiptV2::new(
                &request,
                TransitionOutcomeV1::Applied {
                    revision: wrong_phase.revision,
                },
                wrong_phase,
                RuntimeCertificationReservationResetReceiptV2::Consumed {
                    operation_id: operation_id(),
                    resulting_revision: valid.revision,
                    consumed_at: at(21),
                },
                at(21),
            ),
            Err(RuntimeAwaitingGatewayReadyResetReceiptErrorV2::PhaseMismatch)
        );

        let mut retained_lease = valid.clone();
        retained_lease.controller_lease = request.basis().snapshot().controller_lease.clone();
        assert_eq!(
            RuntimeAwaitingGatewayReadyResetReceiptV2::new(
                &request,
                TransitionOutcomeV1::Applied {
                    revision: retained_lease.revision,
                },
                retained_lease,
                RuntimeCertificationReservationResetReceiptV2::Consumed {
                    operation_id: operation_id(),
                    resulting_revision: valid.revision,
                    consumed_at: at(21),
                },
                at(21),
            ),
            Err(RuntimeAwaitingGatewayReadyResetReceiptErrorV2::ControllerLeaseRetained)
        );
    }

    #[test]
    fn reset_receipt_requires_exact_terminal_reservation_revision_and_time() {
        let request = reserved_request();
        let valid = successor(request.basis().snapshot());

        for reservation in [
            RuntimeCertificationReservationResetReceiptV2::Consumed {
                operation_id: operation_id(),
                resulting_revision: request.basis().snapshot().revision,
                consumed_at: at(21),
            },
            RuntimeCertificationReservationResetReceiptV2::Consumed {
                operation_id: operation_id(),
                resulting_revision: valid.revision,
                consumed_at: at(22),
            },
            RuntimeCertificationReservationResetReceiptV2::NotReserved,
        ] {
            assert_eq!(
                RuntimeAwaitingGatewayReadyResetReceiptV2::new(
                    &request,
                    TransitionOutcomeV1::Applied {
                        revision: valid.revision,
                    },
                    valid.clone(),
                    reservation,
                    at(21),
                ),
                Err(RuntimeAwaitingGatewayReadyResetReceiptErrorV2::ReservationMismatch)
            );
        }
    }
}
