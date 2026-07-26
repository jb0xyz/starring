use std::num::NonZeroU32;

use automation_runtime_convergence::{
    DeploymentRevision, RuntimeDeployment, RuntimeDeploymentPhaseV1, RuntimeDeploymentSnapshotV1,
};
use chrono::{DateTime, Utc};

use crate::{
    RuntimeCanonicalCertificationIntentV2, RuntimeCertificationCanonicalErrorV2,
    RuntimeCertificationDivergenceV2, RuntimeCertificationIntentFingerprintV2,
    RuntimeCertificationOperationIdV2, RuntimeDeploymentScopeV1, RuntimeExecutionReceiptV1,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCertificationOperationScopeV2 {
    scope: RuntimeDeploymentScopeV1,
    deployment_revision: DeploymentRevision,
    convergence_attempt: NonZeroU32,
}

impl RuntimeCertificationOperationScopeV2 {
    pub fn from_awaiting_execution(
        execution: &RuntimeExecutionReceiptV1,
    ) -> Result<Self, RuntimeCertificationOperationBuildErrorV2> {
        validate_execution_receipt(execution)?;
        if !matches!(
            execution.snapshot.phase,
            RuntimeDeploymentPhaseV1::AwaitingGatewayReady
        ) {
            return Err(RuntimeCertificationOperationBuildErrorV2::NotAwaitingGatewayReady);
        }
        Ok(Self {
            scope: RuntimeDeploymentScopeV1::from_identity(&execution.snapshot.identity),
            deployment_revision: execution.snapshot.revision,
            convergence_attempt: execution.convergence_attempt,
        })
    }

    pub fn scope(&self) -> &RuntimeDeploymentScopeV1 {
        &self.scope
    }

    pub fn deployment_revision(&self) -> DeploymentRevision {
        self.deployment_revision
    }

    pub fn convergence_attempt(&self) -> NonZeroU32 {
        self.convergence_attempt
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeCertificationOperationFieldV2 {
    Scope,
    DeploymentRevision,
    ConvergenceAttempt,
    ControllerId,
    FencingToken,
    RuntimeGeneration,
    Target,
    PanelEvidence,
    OperationId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeCertificationOperationBuildErrorV2 {
    #[error("runtime certification execution receipt is invalid")]
    InvalidExecutionReceipt,
    #[error("runtime certification operation requires AwaitingGatewayReady")]
    NotAwaitingGatewayReady,
    #[error("runtime certification intent disagrees with Awaiting execution on {field:?}")]
    IntentCorrelationMismatch {
        field: RuntimeCertificationOperationFieldV2,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeCertificationOperationPersistenceErrorV2 {
    #[error(transparent)]
    Canonical(#[from] RuntimeCertificationCanonicalErrorV2),
    #[error("persisted certification operation disagrees on {field:?}")]
    PersistedCorrelationMismatch {
        field: RuntimeCertificationOperationFieldV2,
    },
}

impl RuntimeCertificationOperationPersistenceErrorV2 {
    pub const fn into_divergence(self) -> RuntimeCertificationDivergenceV2 {
        RuntimeCertificationDivergenceV2::PersistenceCorrupt
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeReservedCertificationIntentV2 {
    operation_scope: RuntimeCertificationOperationScopeV2,
    canonical_intent: RuntimeCanonicalCertificationIntentV2,
}

impl RuntimeReservedCertificationIntentV2 {
    pub fn new(
        execution: &RuntimeExecutionReceiptV1,
        canonical_intent: RuntimeCanonicalCertificationIntentV2,
    ) -> Result<Self, RuntimeCertificationOperationBuildErrorV2> {
        let operation_scope =
            RuntimeCertificationOperationScopeV2::from_awaiting_execution(execution)?;
        validate_intent_against_execution(execution, &canonical_intent)?;
        Ok(Self {
            operation_scope,
            canonical_intent,
        })
    }

    pub fn from_persisted(
        persisted_scope: RuntimeDeploymentScopeV1,
        persisted_deployment_revision: DeploymentRevision,
        persisted_convergence_attempt: NonZeroU32,
        persisted_operation_id: &RuntimeCertificationOperationIdV2,
        certification_intent_bytes: &[u8],
        persisted_fingerprint: &RuntimeCertificationIntentFingerprintV2,
    ) -> Result<Self, RuntimeCertificationOperationPersistenceErrorV2> {
        let canonical_intent = RuntimeCanonicalCertificationIntentV2::from_persisted(
            certification_intent_bytes,
            persisted_fingerprint,
        )?;
        let intent = canonical_intent.intent();
        if persisted_scope != intent.guard.scope {
            return Err(
                RuntimeCertificationOperationPersistenceErrorV2::PersistedCorrelationMismatch {
                    field: RuntimeCertificationOperationFieldV2::Scope,
                },
            );
        }
        if persisted_deployment_revision != intent.guard.expected_revision {
            return Err(
                RuntimeCertificationOperationPersistenceErrorV2::PersistedCorrelationMismatch {
                    field: RuntimeCertificationOperationFieldV2::DeploymentRevision,
                },
            );
        }
        if persisted_convergence_attempt != intent.guard.convergence_attempt {
            return Err(
                RuntimeCertificationOperationPersistenceErrorV2::PersistedCorrelationMismatch {
                    field: RuntimeCertificationOperationFieldV2::ConvergenceAttempt,
                },
            );
        }
        if persisted_operation_id != &intent.operation_id {
            return Err(
                RuntimeCertificationOperationPersistenceErrorV2::PersistedCorrelationMismatch {
                    field: RuntimeCertificationOperationFieldV2::OperationId,
                },
            );
        }
        let operation_scope = RuntimeCertificationOperationScopeV2 {
            scope: persisted_scope,
            deployment_revision: persisted_deployment_revision,
            convergence_attempt: persisted_convergence_attempt,
        };
        Ok(Self {
            operation_scope,
            canonical_intent,
        })
    }

    pub fn operation_scope(&self) -> &RuntimeCertificationOperationScopeV2 {
        &self.operation_scope
    }

    pub fn operation_id(&self) -> &RuntimeCertificationOperationIdV2 {
        &self.canonical_intent.intent().operation_id
    }

    pub fn canonical_intent(&self) -> &RuntimeCanonicalCertificationIntentV2 {
        &self.canonical_intent
    }

    pub fn certification_intent_bytes(&self) -> &[u8] {
        self.canonical_intent.certification_intent_bytes()
    }

    pub fn intent_fingerprint(&self) -> &RuntimeCertificationIntentFingerprintV2 {
        self.canonical_intent.intent_fingerprint()
    }

    #[expect(
        clippy::result_large_err,
        reason = "replay divergence uses the accepted closed certification outcome"
    )]
    pub fn require_byte_exact_replay(
        &self,
        proposed: &Self,
    ) -> Result<(), RuntimeCertificationDivergenceV2> {
        if self.operation_scope == proposed.operation_scope
            && self.operation_id() == proposed.operation_id()
            && self.certification_intent_bytes() == proposed.certification_intent_bytes()
            && self.intent_fingerprint() == proposed.intent_fingerprint()
        {
            Ok(())
        } else {
            Err(RuntimeCertificationDivergenceV2::ReservationMismatch)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCertificationReservationScopeLookupV2 {
    operation_scope: RuntimeCertificationOperationScopeV2,
}

impl RuntimeCertificationReservationScopeLookupV2 {
    pub fn from_awaiting_execution(
        execution: &RuntimeExecutionReceiptV1,
    ) -> Result<Self, RuntimeCertificationOperationBuildErrorV2> {
        Ok(Self {
            operation_scope: RuntimeCertificationOperationScopeV2::from_awaiting_execution(
                execution,
            )?,
        })
    }

    pub fn operation_scope(&self) -> &RuntimeCertificationOperationScopeV2 {
        &self.operation_scope
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeCertificationReservationObservationErrorV2 {
    #[error("certification reservation observation snapshot is invalid")]
    InvalidSnapshot,
    #[error("certification reservation observation is not AwaitingGatewayReady")]
    NotAwaitingGatewayReady,
    #[error("certification reservation observation scope does not match its snapshot")]
    ScopeMismatch,
    #[error("certification reservation observation revision does not match its snapshot")]
    DeploymentRevisionMismatch,
    #[error("persisted certification reservation does not match its Awaiting snapshot")]
    ReservationMismatch,
}

impl RuntimeCertificationReservationObservationErrorV2 {
    pub const fn into_divergence(self) -> RuntimeCertificationDivergenceV2 {
        RuntimeCertificationDivergenceV2::PersistenceCorrupt
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "scope observations retain the exact locked deployment snapshot"
)]
pub enum RuntimeCertificationReservationScopeObservationKindV2 {
    Absent {
        lookup: RuntimeCertificationReservationScopeLookupV2,
        snapshot: RuntimeDeploymentSnapshotV1,
        observed_at: DateTime<Utc>,
    },
    Reserved {
        lookup: RuntimeCertificationReservationScopeLookupV2,
        snapshot: RuntimeDeploymentSnapshotV1,
        reservation: RuntimeReservedCertificationIntentV2,
        observed_at: DateTime<Utc>,
    },
    Diverged(RuntimeCertificationDivergenceV2),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCertificationReservationScopeObservationV2 {
    kind: RuntimeCertificationReservationScopeObservationKindV2,
}

impl RuntimeCertificationReservationScopeObservationV2 {
    pub fn absent(
        lookup: RuntimeCertificationReservationScopeLookupV2,
        snapshot: RuntimeDeploymentSnapshotV1,
        observed_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeCertificationReservationObservationErrorV2> {
        validate_observation_scope(&snapshot, lookup.operation_scope())?;
        Ok(Self {
            kind: RuntimeCertificationReservationScopeObservationKindV2::Absent {
                lookup,
                snapshot,
                observed_at,
            },
        })
    }

    pub fn reserved(
        lookup: RuntimeCertificationReservationScopeLookupV2,
        snapshot: RuntimeDeploymentSnapshotV1,
        reservation: RuntimeReservedCertificationIntentV2,
        observed_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeCertificationReservationObservationErrorV2> {
        validate_observation_scope(&snapshot, lookup.operation_scope())?;
        if lookup.operation_scope() != reservation.operation_scope() {
            return Err(RuntimeCertificationReservationObservationErrorV2::ReservationMismatch);
        }
        validate_reservation_against_snapshot(&snapshot, &reservation)?;
        Ok(Self {
            kind: RuntimeCertificationReservationScopeObservationKindV2::Reserved {
                lookup,
                snapshot,
                reservation,
                observed_at,
            },
        })
    }

    pub fn diverged(divergence: RuntimeCertificationDivergenceV2) -> Self {
        Self {
            kind: RuntimeCertificationReservationScopeObservationKindV2::Diverged(divergence),
        }
    }

    pub fn kind(&self) -> &RuntimeCertificationReservationScopeObservationKindV2 {
        &self.kind
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "the exact reservation receipt remains inline in the closed outcome"
)]
pub enum RuntimeCertificationIntentReservationOutcomeV2 {
    Reserved(RuntimeReservedCertificationIntentV2),
    Diverged(RuntimeCertificationDivergenceV2),
}

fn validate_execution_receipt(
    execution: &RuntimeExecutionReceiptV1,
) -> Result<(), RuntimeCertificationOperationBuildErrorV2> {
    RuntimeDeployment::restore(execution.snapshot.clone())
        .map_err(|_| RuntimeCertificationOperationBuildErrorV2::InvalidExecutionReceipt)?;
    let lease = execution
        .snapshot
        .controller_lease
        .as_ref()
        .ok_or(RuntimeCertificationOperationBuildErrorV2::InvalidExecutionReceipt)?;
    if lease.controller_id != execution.controller_id
        || lease.fencing_token != execution.fencing_token
        || lease.acquired_at != execution.acquired_at
        || lease.expires_at != execution.expires_at
        || execution.snapshot.last_fencing_token != Some(execution.fencing_token)
        || execution.expires_at <= execution.acquired_at
    {
        return Err(RuntimeCertificationOperationBuildErrorV2::InvalidExecutionReceipt);
    }
    Ok(())
}

fn validate_intent_against_execution(
    execution: &RuntimeExecutionReceiptV1,
    canonical_intent: &RuntimeCanonicalCertificationIntentV2,
) -> Result<(), RuntimeCertificationOperationBuildErrorV2> {
    let intent = canonical_intent.intent();
    let guard = &intent.guard;
    let expected_scope = RuntimeDeploymentScopeV1::from_identity(&execution.snapshot.identity);
    let panel = execution
        .snapshot
        .panel_certificate
        .as_ref()
        .ok_or(RuntimeCertificationOperationBuildErrorV2::InvalidExecutionReceipt)?;
    let mismatch = if guard.scope != expected_scope {
        Some(RuntimeCertificationOperationFieldV2::Scope)
    } else if guard.expected_revision != execution.snapshot.revision {
        Some(RuntimeCertificationOperationFieldV2::DeploymentRevision)
    } else if guard.convergence_attempt != execution.convergence_attempt {
        Some(RuntimeCertificationOperationFieldV2::ConvergenceAttempt)
    } else if guard.controller_id != execution.controller_id {
        Some(RuntimeCertificationOperationFieldV2::ControllerId)
    } else if guard.fencing_token != execution.fencing_token {
        Some(RuntimeCertificationOperationFieldV2::FencingToken)
    } else if guard.runtime_generation != execution.snapshot.runtime_generation {
        Some(RuntimeCertificationOperationFieldV2::RuntimeGeneration)
    } else if intent.target != execution.snapshot.target {
        Some(RuntimeCertificationOperationFieldV2::Target)
    } else if !panel_evidence_matches(intent, panel) {
        Some(RuntimeCertificationOperationFieldV2::PanelEvidence)
    } else {
        None
    };
    if let Some(field) = mismatch {
        Err(RuntimeCertificationOperationBuildErrorV2::IntentCorrelationMismatch { field })
    } else {
        Ok(())
    }
}

fn validate_observation_scope(
    snapshot: &RuntimeDeploymentSnapshotV1,
    operation_scope: &RuntimeCertificationOperationScopeV2,
) -> Result<(), RuntimeCertificationReservationObservationErrorV2> {
    RuntimeDeployment::restore(snapshot.clone())
        .map_err(|_| RuntimeCertificationReservationObservationErrorV2::InvalidSnapshot)?;
    if !matches!(
        snapshot.phase,
        RuntimeDeploymentPhaseV1::AwaitingGatewayReady
    ) {
        return Err(RuntimeCertificationReservationObservationErrorV2::NotAwaitingGatewayReady);
    }
    if !operation_scope.scope.matches(&snapshot.identity) {
        return Err(RuntimeCertificationReservationObservationErrorV2::ScopeMismatch);
    }
    if operation_scope.deployment_revision != snapshot.revision {
        return Err(RuntimeCertificationReservationObservationErrorV2::DeploymentRevisionMismatch);
    }
    Ok(())
}

fn validate_reservation_against_snapshot(
    snapshot: &RuntimeDeploymentSnapshotV1,
    reservation: &RuntimeReservedCertificationIntentV2,
) -> Result<(), RuntimeCertificationReservationObservationErrorV2> {
    let intent = reservation.canonical_intent().intent();
    let guard = &intent.guard;
    let Some(lease) = snapshot.controller_lease.as_ref() else {
        return Err(RuntimeCertificationReservationObservationErrorV2::ReservationMismatch);
    };
    let Some(panel) = snapshot.panel_certificate.as_ref() else {
        return Err(RuntimeCertificationReservationObservationErrorV2::ReservationMismatch);
    };
    if !guard.scope.matches(&snapshot.identity)
        || guard.expected_revision != snapshot.revision
        || guard.convergence_attempt != reservation.operation_scope().convergence_attempt()
        || guard.controller_id != lease.controller_id
        || guard.fencing_token != lease.fencing_token
        || guard.runtime_generation != snapshot.runtime_generation
        || intent.target != snapshot.target
        || !panel_evidence_matches(intent, panel)
    {
        return Err(RuntimeCertificationReservationObservationErrorV2::ReservationMismatch);
    }
    Ok(())
}

fn panel_evidence_matches(
    intent: &crate::RuntimeCertificationIntentV2,
    panel: &automation_runtime_convergence::PanelCertificateV1,
) -> bool {
    intent.panel.certificate_id == panel.certificate_id
        && intent.panel.report_digest == panel.report_digest
        && intent.panel.process_identity.target == panel.target
        && intent.panel.process_identity.runtime_generation == panel.runtime_generation
        && intent.panel.process_identity.process_instance_id == panel.process_instance_id
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU32, NonZeroU64};
    use std::time::Duration;

    use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
    use automation_runtime_convergence::{
        ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, BindingRevision,
        CommandGuardV1, ControllerId, DeploymentId, DeploymentRevision, DrainAttestationV1,
        FencingToken, InstallationId, LeaseRequestV1, PanelCertificateId, PanelCertificateV1,
        PanelReportDigestV1, PreflightAttestationV1, ProcessInstanceId, PromotionId,
        RuntimeDeployment, RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1,
        RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
    };
    use chrono::{DateTime, Utc};
    use discord_model::GuildId;
    use resource_resolution::ResourceBindingFingerprint;

    use super::{
        RuntimeCertificationIntentReservationOutcomeV2, RuntimeCertificationOperationBuildErrorV2,
        RuntimeCertificationOperationFieldV2, RuntimeCertificationOperationPersistenceErrorV2,
        RuntimeCertificationReservationObservationErrorV2,
        RuntimeCertificationReservationScopeLookupV2,
        RuntimeCertificationReservationScopeObservationKindV2,
        RuntimeCertificationReservationScopeObservationV2, RuntimeReservedCertificationIntentV2,
    };
    use crate::{
        GatewayShardIdV1, RuntimeBindingPinV1, RuntimeBuildRevisionV1,
        RuntimeCanonicalCertificationIntentV2, RuntimeCertificationCanonicalErrorV2,
        RuntimeCertificationDivergenceV2, RuntimeCertificationIntentFingerprintV2,
        RuntimeCertificationIntentV2, RuntimeCertificationOperationIdV2, RuntimeDeploymentScopeV1,
        RuntimeExecutionGuardV1, RuntimeExecutionReceiptV1, RuntimeGatewayOwnerLeaseIdV1,
        RuntimePanelEvidenceV2, RuntimeSessionActionIdV1,
    };

    fn non_zero(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).unwrap()
    }

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

    fn deployment_identity() -> RuntimeDeploymentIdentityV1 {
        RuntimeDeploymentIdentityV1 {
            deployment_id: DeploymentId::parse("deployment:1").unwrap(),
            tenant_id: TenantId::parse("tenant:1").unwrap(),
            installation_id: InstallationId::parse("installation:1").unwrap(),
            promotion_id: PromotionId::parse("9".repeat(64)).unwrap(),
            activation_request_id: ActivationRequestId::parse("activation:1").unwrap(),
        }
    }

    fn command_guard(
        deployment: &RuntimeDeployment,
        controller_id: &ControllerId,
        fencing_token: FencingToken,
        now: DateTime<Utc>,
    ) -> CommandGuardV1 {
        CommandGuardV1 {
            expected_revision: deployment.revision(),
            controller_id: controller_id.clone(),
            fencing_token,
            runtime_generation: deployment.runtime_generation(),
            now,
        }
    }

    fn claimed_deployment() -> (RuntimeDeployment, ControllerId, FencingToken) {
        let mut deployment = RuntimeDeployment::request(
            deployment_identity(),
            target(),
            RuntimeGeneration::new(4).unwrap(),
            None,
            at(1),
        )
        .unwrap();
        let controller_id = ControllerId::parse("controller:1").unwrap();
        let fencing_token = FencingToken::new(3).unwrap();
        deployment
            .acquire_lease(LeaseRequestV1 {
                expected_revision: deployment.revision(),
                controller_id: controller_id.clone(),
                fencing_token,
                now: at(10),
                expires_at: at(100),
            })
            .unwrap();
        (deployment, controller_id, fencing_token)
    }

    fn requested_execution() -> RuntimeExecutionReceiptV1 {
        let (deployment, controller_id, fencing_token) = claimed_deployment();
        RuntimeExecutionReceiptV1 {
            snapshot: deployment.snapshot(),
            controller_id,
            fencing_token,
            convergence_attempt: NonZeroU32::new(5).unwrap(),
            acquired_at: at(10),
            expires_at: at(100),
        }
    }

    fn awaiting_execution() -> RuntimeExecutionReceiptV1 {
        let (mut deployment, controller_id, fencing_token) = claimed_deployment();
        deployment
            .accept_preflight(
                &command_guard(&deployment, &controller_id, fencing_token, at(11)),
                PreflightAttestationV1 {
                    target: deployment.target().clone(),
                    runtime_generation: deployment.runtime_generation(),
                    observed_runtime: None,
                    checked_at: at(11),
                },
            )
            .unwrap();
        deployment
            .request_drain(&command_guard(
                &deployment,
                &controller_id,
                fencing_token,
                at(12),
            ))
            .unwrap();
        deployment
            .accept_drain(
                &command_guard(&deployment, &controller_id, fencing_token, at(13)),
                DrainAttestationV1 {
                    previous_runtime: None,
                    target_runtime_generation: deployment.runtime_generation(),
                    drained_at: at(13),
                },
            )
            .unwrap();
        deployment
            .begin_activation(&command_guard(
                &deployment,
                &controller_id,
                fencing_token,
                at(14),
            ))
            .unwrap();
        deployment
            .accept_activation(
                &command_guard(&deployment, &controller_id, fencing_token, at(15)),
                ActivationAttestationV1 {
                    activation_request_id: deployment_identity().activation_request_id,
                    target: deployment.target().clone(),
                    runtime_generation: deployment.runtime_generation(),
                    kind: ActivationOutcomeKindV1::Activated,
                    activated_at: at(15),
                },
            )
            .unwrap();
        deployment
            .begin_panel_reconciliation(&command_guard(
                &deployment,
                &controller_id,
                fencing_token,
                at(16),
            ))
            .unwrap();
        deployment
            .accept_panel_certificate(
                &command_guard(&deployment, &controller_id, fencing_token, at(17)),
                PanelCertificateV1 {
                    certificate_id: PanelCertificateId::parse("panel:1").unwrap(),
                    report_digest: PanelReportDigestV1::parse("c".repeat(64)).unwrap(),
                    target: deployment.target().clone(),
                    runtime_generation: deployment.runtime_generation(),
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
                    reconciled_at: at(17),
                },
            )
            .unwrap();
        RuntimeExecutionReceiptV1 {
            snapshot: deployment.snapshot(),
            controller_id,
            fencing_token,
            convergence_attempt: NonZeroU32::new(5).unwrap(),
            acquired_at: at(10),
            expires_at: at(100),
        }
    }

    fn canonical_intent(
        execution: &RuntimeExecutionReceiptV1,
        operation_id: &str,
    ) -> RuntimeCanonicalCertificationIntentV2 {
        let target = execution.snapshot.target.clone();
        let process_identity = RuntimeProcessIdentityV1 {
            target: target.clone(),
            runtime_generation: execution.snapshot.runtime_generation,
            process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
        };
        RuntimeCanonicalCertificationIntentV2::new(RuntimeCertificationIntentV2 {
            action_id: RuntimeSessionActionIdV1::new(non_zero(1)),
            operation_id: RuntimeCertificationOperationIdV2::parse(operation_id).unwrap(),
            guard: RuntimeExecutionGuardV1 {
                scope: RuntimeDeploymentScopeV1::from_identity(&execution.snapshot.identity),
                expected_revision: execution.snapshot.revision,
                controller_id: execution.controller_id.clone(),
                fencing_token: execution.fencing_token,
                runtime_generation: execution.snapshot.runtime_generation,
                convergence_attempt: execution.convergence_attempt,
            },
            target: target.clone(),
            binding_pin: RuntimeBindingPinV1 {
                tenant_id: execution.snapshot.identity.tenant_id.clone(),
                installation_id: execution.snapshot.identity.installation_id.clone(),
                installation_authority_revision: non_zero(6),
                binding_revision: target.binding_revision,
                binding_fingerprint: target.binding_fingerprint.clone(),
            },
            process_identity: process_identity.clone(),
            gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1 {
                gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
                process_instance_id: process_identity.process_instance_id.clone(),
                lease_epoch: non_zero(5),
                expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
            },
            observed_owner_revision: non_zero(7),
            runtime_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
            panel: RuntimePanelEvidenceV2 {
                certificate_id: PanelCertificateId::parse("panel:1").unwrap(),
                report_digest: PanelReportDigestV1::parse("c".repeat(64)).unwrap(),
                process_identity,
                controller_fencing_token: execution.fencing_token,
            },
            serving_lease_for: Duration::from_secs(30),
        })
        .unwrap()
    }

    fn mismatch(
        field: RuntimeCertificationOperationFieldV2,
    ) -> RuntimeCertificationOperationPersistenceErrorV2 {
        RuntimeCertificationOperationPersistenceErrorV2::PersistedCorrelationMismatch { field }
    }

    fn reservation() -> (
        RuntimeExecutionReceiptV1,
        RuntimeReservedCertificationIntentV2,
    ) {
        let execution = awaiting_execution();
        let canonical = canonical_intent(&execution, "00112233445566778899aabbccddeeff");
        let reservation = RuntimeReservedCertificationIntentV2::new(&execution, canonical).unwrap();
        (execution, reservation)
    }

    #[test]
    fn new_reservation_derives_its_complete_natural_scope() {
        let execution = awaiting_execution();
        let canonical = canonical_intent(&execution, "00112233445566778899aabbccddeeff");
        let expected_intent = canonical.intent().clone();
        let expected_bytes = canonical.certification_intent_bytes().to_vec();
        let expected_fingerprint = canonical.intent_fingerprint().clone();
        let reserved = RuntimeReservedCertificationIntentV2::new(&execution, canonical).unwrap();

        assert_eq!(
            reserved.operation_scope().scope(),
            &expected_intent.guard.scope
        );
        assert_eq!(
            reserved.operation_scope().deployment_revision(),
            expected_intent.guard.expected_revision
        );
        assert_eq!(
            reserved.operation_scope().convergence_attempt(),
            expected_intent.guard.convergence_attempt
        );
        assert_eq!(reserved.operation_id(), &expected_intent.operation_id);
        assert_eq!(reserved.certification_intent_bytes(), expected_bytes);
        assert_eq!(reserved.intent_fingerprint(), &expected_fingerprint);
    }

    #[test]
    fn persisted_reservation_reconstructs_only_the_byte_exact_root() {
        let (_, expected) = reservation();
        let reconstructed = RuntimeReservedCertificationIntentV2::from_persisted(
            expected.operation_scope().scope().clone(),
            expected.operation_scope().deployment_revision(),
            expected.operation_scope().convergence_attempt(),
            expected.operation_id(),
            expected.certification_intent_bytes(),
            expected.intent_fingerprint(),
        )
        .unwrap();

        assert_eq!(reconstructed, expected);
    }

    #[test]
    fn persisted_reservation_rejects_every_natural_scope_and_id_mismatch() {
        let (_, expected) = reservation();
        let mut wrong_scope = expected.operation_scope().scope().clone();
        wrong_scope.deployment_id = DeploymentId::parse("deployment:2").unwrap();
        let wrong_id =
            RuntimeCertificationOperationIdV2::parse("ffeeddccbbaa99887766554433221100").unwrap();

        for (scope, revision, attempt, operation_id, error) in [
            (
                wrong_scope,
                expected.operation_scope().deployment_revision(),
                expected.operation_scope().convergence_attempt(),
                expected.operation_id(),
                mismatch(RuntimeCertificationOperationFieldV2::Scope),
            ),
            (
                expected.operation_scope().scope().clone(),
                DeploymentRevision::new(3).unwrap(),
                expected.operation_scope().convergence_attempt(),
                expected.operation_id(),
                mismatch(RuntimeCertificationOperationFieldV2::DeploymentRevision),
            ),
            (
                expected.operation_scope().scope().clone(),
                expected.operation_scope().deployment_revision(),
                NonZeroU32::new(6).unwrap(),
                expected.operation_id(),
                mismatch(RuntimeCertificationOperationFieldV2::ConvergenceAttempt),
            ),
            (
                expected.operation_scope().scope().clone(),
                expected.operation_scope().deployment_revision(),
                expected.operation_scope().convergence_attempt(),
                &wrong_id,
                mismatch(RuntimeCertificationOperationFieldV2::OperationId),
            ),
        ] {
            assert_eq!(
                RuntimeReservedCertificationIntentV2::from_persisted(
                    scope,
                    revision,
                    attempt,
                    operation_id,
                    expected.certification_intent_bytes(),
                    expected.intent_fingerprint(),
                ),
                Err(error)
            );
        }
    }

    #[test]
    fn persisted_reservation_rejects_noncanonical_bytes_and_wrong_fingerprint() {
        let (_, expected) = reservation();
        let mut noncanonical_bytes = expected.certification_intent_bytes().to_vec();
        noncanonical_bytes.push(b' ');
        let wrong_fingerprint =
            RuntimeCertificationIntentFingerprintV2::parse("f".repeat(64)).unwrap();

        assert!(matches!(
            RuntimeReservedCertificationIntentV2::from_persisted(
                expected.operation_scope().scope().clone(),
                expected.operation_scope().deployment_revision(),
                expected.operation_scope().convergence_attempt(),
                expected.operation_id(),
                &noncanonical_bytes,
                expected.intent_fingerprint(),
            ),
            Err(RuntimeCertificationOperationPersistenceErrorV2::Canonical(
                RuntimeCertificationCanonicalErrorV2::NonCanonicalEncoding { .. }
            ))
        ));
        assert!(matches!(
            RuntimeReservedCertificationIntentV2::from_persisted(
                expected.operation_scope().scope().clone(),
                expected.operation_scope().deployment_revision(),
                expected.operation_scope().convergence_attempt(),
                expected.operation_id(),
                expected.certification_intent_bytes(),
                &wrong_fingerprint,
            ),
            Err(RuntimeCertificationOperationPersistenceErrorV2::Canonical(
                RuntimeCertificationCanonicalErrorV2::PersistedFingerprintMismatch { .. }
            ))
        ));
    }

    #[test]
    fn scope_lookup_requires_a_structurally_valid_awaiting_execution() {
        let requested = requested_execution();
        assert_eq!(
            RuntimeCertificationReservationScopeLookupV2::from_awaiting_execution(&requested),
            Err(RuntimeCertificationOperationBuildErrorV2::NotAwaitingGatewayReady)
        );

        let mut invalid = awaiting_execution();
        invalid.expires_at = at(9);
        assert_eq!(
            RuntimeCertificationReservationScopeLookupV2::from_awaiting_execution(&invalid),
            Err(RuntimeCertificationOperationBuildErrorV2::InvalidExecutionReceipt)
        );
    }

    #[test]
    fn reservation_rejects_an_intent_from_another_execution_attempt() {
        let execution = awaiting_execution();
        let canonical = canonical_intent(&execution, "00112233445566778899aabbccddeeff");
        let mut successor_attempt = execution.clone();
        successor_attempt.convergence_attempt = NonZeroU32::new(6).unwrap();

        assert_eq!(
            RuntimeReservedCertificationIntentV2::new(&successor_attempt, canonical),
            Err(
                RuntimeCertificationOperationBuildErrorV2::IntentCorrelationMismatch {
                    field: RuntimeCertificationOperationFieldV2::ConvergenceAttempt,
                }
            )
        );
    }

    #[test]
    fn reservation_rejects_panel_evidence_from_another_reconciliation() {
        let execution = awaiting_execution();
        let canonical = canonical_intent(&execution, "00112233445566778899aabbccddeeff");
        let mut mismatched_intent = canonical.intent().clone();
        mismatched_intent.panel.report_digest = PanelReportDigestV1::parse("d".repeat(64)).unwrap();
        let mismatched = RuntimeCanonicalCertificationIntentV2::new(mismatched_intent).unwrap();

        assert_eq!(
            RuntimeReservedCertificationIntentV2::new(&execution, mismatched),
            Err(
                RuntimeCertificationOperationBuildErrorV2::IntentCorrelationMismatch {
                    field: RuntimeCertificationOperationFieldV2::PanelEvidence,
                }
            )
        );
    }

    #[test]
    fn replay_requires_the_same_scope_id_bytes_and_fingerprint() {
        let (execution, expected) = reservation();
        assert_eq!(
            expected.require_byte_exact_replay(&expected.clone()),
            Ok(())
        );

        let competing = RuntimeReservedCertificationIntentV2::new(
            &execution,
            canonical_intent(&execution, "ffeeddccbbaa99887766554433221100"),
        )
        .unwrap();
        assert_eq!(
            expected.require_byte_exact_replay(&competing),
            Err(RuntimeCertificationDivergenceV2::ReservationMismatch)
        );
    }

    #[test]
    fn persisted_corruption_has_one_closed_divergence() {
        let (_, expected) = reservation();
        let wrong_id =
            RuntimeCertificationOperationIdV2::parse("ffeeddccbbaa99887766554433221100").unwrap();
        let error = RuntimeReservedCertificationIntentV2::from_persisted(
            expected.operation_scope().scope().clone(),
            expected.operation_scope().deployment_revision(),
            expected.operation_scope().convergence_attempt(),
            &wrong_id,
            expected.certification_intent_bytes(),
            expected.intent_fingerprint(),
        )
        .unwrap_err();

        assert_eq!(
            error.into_divergence(),
            RuntimeCertificationDivergenceV2::PersistenceCorrupt
        );
    }

    #[test]
    fn scope_observation_returns_the_persisted_reservation_without_an_id_lookup() {
        let (execution, reservation) = reservation();
        let lookup =
            RuntimeCertificationReservationScopeLookupV2::from_awaiting_execution(&execution)
                .unwrap();
        let absent = RuntimeCertificationReservationScopeObservationV2::absent(
            lookup.clone(),
            execution.snapshot.clone(),
            at(20),
        )
        .unwrap();
        let reserved = RuntimeCertificationReservationScopeObservationV2::reserved(
            lookup.clone(),
            execution.snapshot.clone(),
            reservation.clone(),
            at(21),
        )
        .unwrap();
        let outcome = RuntimeCertificationIntentReservationOutcomeV2::Reserved(reservation.clone());

        assert_eq!(
            lookup.operation_scope().deployment_revision(),
            execution.snapshot.revision
        );
        assert!(matches!(
            absent.kind(),
            RuntimeCertificationReservationScopeObservationKindV2::Absent {
                lookup: observed_lookup,
                observed_at,
                ..
            } if observed_lookup == &lookup && *observed_at == at(20)
        ));
        assert!(matches!(
            reserved.kind(),
            RuntimeCertificationReservationScopeObservationKindV2::Reserved {
                reservation: observed,
                observed_at,
                ..
            } if observed == &reservation && *observed_at == at(21)
        ));
        assert!(matches!(
            outcome,
            RuntimeCertificationIntentReservationOutcomeV2::Reserved(observed)
                if observed == reservation
        ));
    }

    #[test]
    fn scope_observation_rejects_a_mismatched_snapshot() {
        let execution = awaiting_execution();
        let lookup =
            RuntimeCertificationReservationScopeLookupV2::from_awaiting_execution(&execution)
                .unwrap();
        let mut advanced = execution.snapshot.clone();
        advanced.revision = advanced.revision.next().unwrap();
        let error =
            RuntimeCertificationReservationScopeObservationV2::absent(lookup, advanced, at(20))
                .unwrap_err();

        assert_eq!(
            error,
            RuntimeCertificationReservationObservationErrorV2::DeploymentRevisionMismatch
        );
        assert_eq!(
            error.into_divergence(),
            RuntimeCertificationDivergenceV2::PersistenceCorrupt
        );
    }

    #[test]
    fn reserved_observation_rejects_a_result_from_another_lookup() {
        let execution = awaiting_execution();
        let lookup =
            RuntimeCertificationReservationScopeLookupV2::from_awaiting_execution(&execution)
                .unwrap();
        let mut other_attempt = execution.clone();
        other_attempt.convergence_attempt = NonZeroU32::new(6).unwrap();
        let other_reservation = RuntimeReservedCertificationIntentV2::new(
            &other_attempt,
            canonical_intent(&other_attempt, "ffeeddccbbaa99887766554433221100"),
        )
        .unwrap();

        let error = RuntimeCertificationReservationScopeObservationV2::reserved(
            lookup,
            execution.snapshot,
            other_reservation,
            at(20),
        )
        .unwrap_err();
        assert_eq!(
            error,
            RuntimeCertificationReservationObservationErrorV2::ReservationMismatch
        );
    }

    #[test]
    fn scope_observation_rejects_a_self_consistent_root_from_another_panel() {
        let (execution, expected) = reservation();
        let lookup =
            RuntimeCertificationReservationScopeLookupV2::from_awaiting_execution(&execution)
                .unwrap();
        let mut mismatched_intent = expected.canonical_intent().intent().clone();
        mismatched_intent.panel.report_digest = PanelReportDigestV1::parse("d".repeat(64)).unwrap();
        let mismatched = RuntimeCanonicalCertificationIntentV2::new(mismatched_intent).unwrap();
        let persisted = RuntimeReservedCertificationIntentV2::from_persisted(
            expected.operation_scope().scope().clone(),
            expected.operation_scope().deployment_revision(),
            expected.operation_scope().convergence_attempt(),
            &mismatched.intent().operation_id,
            mismatched.certification_intent_bytes(),
            mismatched.intent_fingerprint(),
        )
        .unwrap();

        let error = RuntimeCertificationReservationScopeObservationV2::reserved(
            lookup,
            execution.snapshot,
            persisted,
            at(20),
        )
        .unwrap_err();
        assert_eq!(
            error,
            RuntimeCertificationReservationObservationErrorV2::ReservationMismatch
        );
        assert_eq!(
            error.into_divergence(),
            RuntimeCertificationDivergenceV2::PersistenceCorrupt
        );
    }
}
