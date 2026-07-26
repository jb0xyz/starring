use std::num::{NonZeroU32, NonZeroU64};

use automation_runtime_convergence::{
    DeploymentRevision, RuntimeDeploymentSnapshotV1, RuntimeProcessIdentityV1, TransitionOutcomeV1,
};
use chrono::{DateTime, Utc};

use crate::{
    RuntimeCertificationIntentFingerprintV2, RuntimeCertificationOperationIdV2,
    RuntimeCertificationRequestDigestV2, RuntimeDeploymentScopeV1, RuntimeLiveAttestationDigestV2,
    RuntimeRouteAdmissionAttestationV2, RuntimeSessionActionIdV1,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeServingIdentityV2 {
    pub scope: RuntimeDeploymentScopeV1,
    pub operation_id: RuntimeCertificationOperationIdV2,
    pub attestation_digest: RuntimeLiveAttestationDigestV2,
    pub process_identity: RuntimeProcessIdentityV1,
    pub lease_epoch: NonZeroU64,
    pub revision: NonZeroU64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeServingReceiptV2 {
    pub identity: RuntimeServingIdentityV2,
    pub acquired_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub connected: bool,
    pub serving: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCertificationReceiptV2 {
    pub action_id: RuntimeSessionActionIdV1,
    pub outcome: TransitionOutcomeV1,
    pub snapshot: RuntimeDeploymentSnapshotV1,
    pub convergence_attempt: NonZeroU32,
    pub operation_id: RuntimeCertificationOperationIdV2,
    pub intent_fingerprint: RuntimeCertificationIntentFingerprintV2,
    pub request_digest: RuntimeCertificationRequestDigestV2,
    pub attestation_digest: RuntimeLiveAttestationDigestV2,
    pub route_admission: RuntimeRouteAdmissionAttestationV2,
    pub serving: RuntimeServingReceiptV2,
    pub certified_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeCertificationDivergenceV2 {
    OwnershipLost,
    DeploymentAdvanced {
        snapshot: RuntimeDeploymentSnapshotV1,
    },
    AuthorityChanged {
        snapshot: RuntimeDeploymentSnapshotV1,
    },
    Superseded {
        snapshot: RuntimeDeploymentSnapshotV1,
    },
    Terminal {
        snapshot: RuntimeDeploymentSnapshotV1,
    },
    ReservationMismatch,
    CommittedRequestMismatch,
    PersistenceCorrupt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeCertificationRecoveryDispositionV2 {
    StopOwnership,
    DrainAndReplan,
    DrainAndStop,
    EmergencyHalt,
}

impl RuntimeCertificationDivergenceV2 {
    pub const fn recovery_disposition(&self) -> RuntimeCertificationRecoveryDispositionV2 {
        match self {
            Self::OwnershipLost => RuntimeCertificationRecoveryDispositionV2::StopOwnership,
            Self::DeploymentAdvanced { .. } | Self::AuthorityChanged { .. } => {
                RuntimeCertificationRecoveryDispositionV2::DrainAndReplan
            }
            Self::Superseded { .. } | Self::Terminal { .. } => {
                RuntimeCertificationRecoveryDispositionV2::DrainAndStop
            }
            Self::ReservationMismatch
            | Self::CommittedRequestMismatch
            | Self::PersistenceCorrupt => RuntimeCertificationRecoveryDispositionV2::EmergencyHalt,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCertificationLookupV2 {
    pub scope: RuntimeDeploymentScopeV1,
    pub deployment_revision: DeploymentRevision,
    pub convergence_attempt: NonZeroU32,
    pub operation_id: RuntimeCertificationOperationIdV2,
    pub request_digest: RuntimeCertificationRequestDigestV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "the accepted V2 observation contract keeps all payloads inline"
)]
pub enum RuntimeCertificationObservationV2 {
    NotCommitted {
        snapshot: RuntimeDeploymentSnapshotV1,
        convergence_attempt: NonZeroU32,
        operation_id: RuntimeCertificationOperationIdV2,
        request_digest: RuntimeCertificationRequestDigestV2,
        observed_deployment_revision: DeploymentRevision,
        observed_at: DateTime<Utc>,
    },
    Committed(RuntimeCertificationReceiptV2),
    Diverged(RuntimeCertificationDivergenceV2),
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "the accepted V2 observation contract keeps all payloads inline"
)]
pub enum AwaitingCertificationScopeObservationV2 {
    Committed(RuntimeCertificationReceiptV2),
    NoOperationReserved {
        snapshot: RuntimeDeploymentSnapshotV1,
        observed_at: DateTime<Utc>,
    },
    NoAttestationForReservedOperation {
        snapshot: RuntimeDeploymentSnapshotV1,
        reserved_operation_id: RuntimeCertificationOperationIdV2,
        observed_at: DateTime<Utc>,
    },
    Diverged(RuntimeCertificationDivergenceV2),
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU32, NonZeroU64};

    use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
    use automation_runtime_convergence::{
        ActivationRequestId, BindingRevision, DeploymentId, DeploymentRevision, FencingToken,
        InstallationId, ProcessInstanceId, PromotionId, RuntimeDeployment,
        RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1, RuntimeGeneration,
        RuntimeProcessIdentityV1, TenantId, TransitionOutcomeV1,
    };
    use chrono::{DateTime, Utc};
    use discord_model::GuildId;
    use resource_resolution::ResourceBindingFingerprint;

    use super::{
        AwaitingCertificationScopeObservationV2, RuntimeCertificationDivergenceV2,
        RuntimeCertificationLookupV2, RuntimeCertificationObservationV2,
        RuntimeCertificationReceiptV2, RuntimeCertificationRecoveryDispositionV2,
        RuntimeServingIdentityV2, RuntimeServingReceiptV2,
    };
    use crate::{
        GatewayShardIdV1, RuntimeBarrierIdV1, RuntimeBarrierPauseWitnessV2, RuntimeBuildRevisionV1,
        RuntimeCertificationIntentFingerprintV2, RuntimeCertificationOperationIdV2,
        RuntimeCertificationRequestDigestV2, RuntimeDeploymentScopeV1,
        RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1,
        RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyKindV2,
        RuntimeLiveAttestationDigestV2, RuntimeRouteAdmissionAttestationV2,
        RuntimeServingRouteAttestationV2, RuntimeSessionActionIdV1,
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
            promotion_id: PromotionId::parse("c".repeat(64)).unwrap(),
            activation_request_id: ActivationRequestId::parse("activation:1").unwrap(),
        }
    }

    fn snapshot() -> automation_runtime_convergence::RuntimeDeploymentSnapshotV1 {
        RuntimeDeployment::request(
            deployment_identity(),
            target(),
            RuntimeGeneration::new(4).unwrap(),
            None,
            at(100),
        )
        .unwrap()
        .snapshot()
    }

    fn process_identity() -> RuntimeProcessIdentityV1 {
        RuntimeProcessIdentityV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::new(4).unwrap(),
            process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
        }
    }

    fn operation_id() -> RuntimeCertificationOperationIdV2 {
        RuntimeCertificationOperationIdV2::parse("00112233445566778899aabbccddeeff").unwrap()
    }

    fn route_admission() -> RuntimeRouteAdmissionAttestationV2 {
        let process = process_identity();
        RuntimeRouteAdmissionAttestationV2 {
            barrier_id: RuntimeBarrierIdV1::parse("ffeeddccbbaa99887766554433221100").unwrap(),
            pause: RuntimeBarrierPauseWitnessV2 {
                coordinator_generation: non_zero(8),
                connection_epoch: non_zero(9),
                paused_admission_revision: non_zero(10),
                pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(12)),
            },
            gateway: RuntimeGatewayReadyAttestationV2 {
                process_instance_id: process.process_instance_id.clone(),
                connection_epoch: non_zero(9),
                kind: RuntimeGatewayReadyKindV2::Resumed,
                admission_revision: non_zero(10),
                connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(11)),
                resume_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(13)),
            },
            gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1 {
                gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
                process_instance_id: process.process_instance_id.clone(),
                lease_epoch: non_zero(5),
                expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
            },
            attested_owner_revision: non_zero(7),
            route: RuntimeServingRouteAttestationV2 {
                identity: process,
                controller_fencing_token: FencingToken::new(3).unwrap(),
                route_incarnation: non_zero(14),
                activation_sequence: non_zero(15),
            },
        }
    }

    fn receipt() -> RuntimeCertificationReceiptV2 {
        let snapshot = snapshot();
        let scope = RuntimeDeploymentScopeV1::from_identity(&snapshot.identity);
        let process = process_identity();
        let operation_id = operation_id();
        let attestation_digest = RuntimeLiveAttestationDigestV2::parse("e".repeat(64)).unwrap();
        RuntimeCertificationReceiptV2 {
            action_id: RuntimeSessionActionIdV1::new(non_zero(1)),
            outcome: TransitionOutcomeV1::Applied {
                revision: DeploymentRevision::new(2).unwrap(),
            },
            snapshot,
            convergence_attempt: NonZeroU32::new(6).unwrap(),
            operation_id: operation_id.clone(),
            intent_fingerprint: RuntimeCertificationIntentFingerprintV2::parse("d".repeat(64))
                .unwrap(),
            request_digest: RuntimeCertificationRequestDigestV2::parse("f".repeat(64)).unwrap(),
            attestation_digest: attestation_digest.clone(),
            route_admission: route_admission(),
            serving: RuntimeServingReceiptV2 {
                identity: RuntimeServingIdentityV2 {
                    scope,
                    operation_id,
                    attestation_digest,
                    process_identity: process,
                    lease_epoch: non_zero(16),
                    revision: non_zero(17),
                },
                acquired_at: at(101),
                last_heartbeat_at: at(102),
                expires_at: at(130),
                connected: true,
                serving: true,
            },
            certified_at: at(101),
        }
    }

    #[test]
    fn divergence_maps_to_one_closed_recovery_disposition() {
        let snapshot = snapshot();
        let cases = [
            (
                RuntimeCertificationDivergenceV2::OwnershipLost,
                RuntimeCertificationRecoveryDispositionV2::StopOwnership,
            ),
            (
                RuntimeCertificationDivergenceV2::DeploymentAdvanced {
                    snapshot: snapshot.clone(),
                },
                RuntimeCertificationRecoveryDispositionV2::DrainAndReplan,
            ),
            (
                RuntimeCertificationDivergenceV2::AuthorityChanged {
                    snapshot: snapshot.clone(),
                },
                RuntimeCertificationRecoveryDispositionV2::DrainAndReplan,
            ),
            (
                RuntimeCertificationDivergenceV2::Superseded {
                    snapshot: snapshot.clone(),
                },
                RuntimeCertificationRecoveryDispositionV2::DrainAndStop,
            ),
            (
                RuntimeCertificationDivergenceV2::Terminal { snapshot },
                RuntimeCertificationRecoveryDispositionV2::DrainAndStop,
            ),
            (
                RuntimeCertificationDivergenceV2::ReservationMismatch,
                RuntimeCertificationRecoveryDispositionV2::EmergencyHalt,
            ),
            (
                RuntimeCertificationDivergenceV2::CommittedRequestMismatch,
                RuntimeCertificationRecoveryDispositionV2::EmergencyHalt,
            ),
            (
                RuntimeCertificationDivergenceV2::PersistenceCorrupt,
                RuntimeCertificationRecoveryDispositionV2::EmergencyHalt,
            ),
        ];

        for (divergence, expected) in cases {
            assert_eq!(divergence.recovery_disposition(), expected);
        }
    }

    #[test]
    fn exact_and_scope_only_observations_preserve_the_reserved_operation() {
        let receipt = receipt();
        let lookup = RuntimeCertificationLookupV2 {
            scope: receipt.serving.identity.scope.clone(),
            deployment_revision: DeploymentRevision::new(1).unwrap(),
            convergence_attempt: receipt.convergence_attempt,
            operation_id: receipt.operation_id.clone(),
            request_digest: receipt.request_digest.clone(),
        };
        let not_committed = RuntimeCertificationObservationV2::NotCommitted {
            snapshot: snapshot(),
            convergence_attempt: lookup.convergence_attempt,
            operation_id: lookup.operation_id.clone(),
            request_digest: lookup.request_digest.clone(),
            observed_deployment_revision: lookup.deployment_revision,
            observed_at: at(105),
        };
        let awaiting = AwaitingCertificationScopeObservationV2::NoAttestationForReservedOperation {
            snapshot: snapshot(),
            reserved_operation_id: lookup.operation_id.clone(),
            observed_at: at(106),
        };

        assert!(matches!(
            not_committed,
            RuntimeCertificationObservationV2::NotCommitted {
                convergence_attempt,
                operation_id,
                request_digest,
                observed_deployment_revision,
                observed_at,
                ..
            } if convergence_attempt == lookup.convergence_attempt
                && operation_id == lookup.operation_id
                && request_digest == lookup.request_digest
                && observed_deployment_revision == lookup.deployment_revision
                && observed_at == at(105)
        ));
        assert!(matches!(
            awaiting,
            AwaitingCertificationScopeObservationV2::NoAttestationForReservedOperation {
                reserved_operation_id,
                observed_at,
                ..
            } if reserved_operation_id == lookup.operation_id && observed_at == at(106)
        ));
        assert!(matches!(
            RuntimeCertificationObservationV2::Committed(receipt.clone()),
            RuntimeCertificationObservationV2::Committed(committed)
                if committed.operation_id == lookup.operation_id
                    && committed.serving.identity.operation_id == lookup.operation_id
                    && committed.route_admission.validate().is_ok()
        ));
        assert!(matches!(
            AwaitingCertificationScopeObservationV2::Committed(receipt),
            AwaitingCertificationScopeObservationV2::Committed(committed)
                if committed.certified_at == at(101)
                    && committed.serving.identity.revision == non_zero(17)
        ));
    }
}
