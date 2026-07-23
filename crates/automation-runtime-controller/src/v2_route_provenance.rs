use std::num::NonZeroU64;

use automation_runtime_convergence::ProcessInstanceId;
use chrono::{DateTime, Utc};

use crate::{
    RuntimeBarrierIdV1, RuntimeBarrierPauseWitnessV2, RuntimeGatewayAdmissionSequenceV2,
    RuntimeGatewayOwnerLeaseIdV1, RuntimeRecoveryIdV2,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeClosedRecoveryRouteWitnessV2 {
    pub recovery_id: RuntimeRecoveryIdV2,
    pub originating_emergency_generation: NonZeroU64,
    pub recovery_generation: NonZeroU64,
    pub recovery_authority_revision: NonZeroU64,
    pub gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub observed_owner_revision: NonZeroU64,
    pub owner_expires_at: DateTime<Utc>,
    pub process_instance_id: ProcessInstanceId,
    pub connection_epoch: NonZeroU64,
    pub paused_admission_revision: NonZeroU64,
    pub connected_event_sequence: RuntimeGatewayAdmissionSequenceV2,
    pub pause_sequence: RuntimeGatewayAdmissionSequenceV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeShutdownRouteWitnessV2 {
    pub shutdown_generation: NonZeroU64,
    pub gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub observed_owner_revision: NonZeroU64,
    pub owner_expires_at: DateTime<Utc>,
    pub process_instance_id: ProcessInstanceId,
    pub connection_epoch: NonZeroU64,
    pub paused_admission_revision: NonZeroU64,
    pub connected_event_sequence: RuntimeGatewayAdmissionSequenceV2,
    pub pause_sequence: RuntimeGatewayAdmissionSequenceV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeRouteMutationProvenanceV2 {
    Ordinary {
        barrier_id: RuntimeBarrierIdV1,
        pause: RuntimeBarrierPauseWitnessV2,
    },
    ClosedRecovery(RuntimeClosedRecoveryRouteWitnessV2),
    Shutdown(RuntimeShutdownRouteWitnessV2),
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use automation_runtime_convergence::ProcessInstanceId;
    use chrono::{DateTime, Utc};

    use super::{
        RuntimeClosedRecoveryRouteWitnessV2, RuntimeRouteMutationProvenanceV2,
        RuntimeShutdownRouteWitnessV2,
    };
    use crate::{
        GatewayShardIdV1, RuntimeBarrierIdV1, RuntimeBarrierPauseWitnessV2, RuntimeBuildRevisionV1,
        RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1, RuntimeRecoveryIdV2,
    };

    fn non_zero(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).unwrap()
    }

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn process_instance_id() -> ProcessInstanceId {
        ProcessInstanceId::parse("process:1").unwrap()
    }

    fn gateway_owner_lease_id() -> RuntimeGatewayOwnerLeaseIdV1 {
        RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            process_instance_id: process_instance_id(),
            lease_epoch: non_zero(3),
            expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
        }
    }

    fn closed_recovery_witness() -> RuntimeClosedRecoveryRouteWitnessV2 {
        RuntimeClosedRecoveryRouteWitnessV2 {
            recovery_id: RuntimeRecoveryIdV2::parse("00112233445566778899aabbccddeeff").unwrap(),
            originating_emergency_generation: non_zero(4),
            recovery_generation: non_zero(5),
            recovery_authority_revision: non_zero(6),
            gateway_owner_lease_id: gateway_owner_lease_id(),
            observed_owner_revision: non_zero(7),
            owner_expires_at: at(100),
            process_instance_id: process_instance_id(),
            connection_epoch: non_zero(8),
            paused_admission_revision: non_zero(9),
            connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(10)),
            pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(11)),
        }
    }

    fn shutdown_witness() -> RuntimeShutdownRouteWitnessV2 {
        RuntimeShutdownRouteWitnessV2 {
            shutdown_generation: non_zero(12),
            gateway_owner_lease_id: gateway_owner_lease_id(),
            observed_owner_revision: non_zero(13),
            owner_expires_at: at(101),
            process_instance_id: process_instance_id(),
            connection_epoch: non_zero(14),
            paused_admission_revision: non_zero(15),
            connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(16)),
            pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(17)),
        }
    }

    #[test]
    fn provenance_variants_preserve_their_exact_distinct_payloads() {
        let barrier_id = RuntimeBarrierIdV1::parse("ffeeddccbbaa99887766554433221100").unwrap();
        let pause = RuntimeBarrierPauseWitnessV2 {
            coordinator_generation: non_zero(18),
            connection_epoch: non_zero(19),
            paused_admission_revision: non_zero(20),
            pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(21)),
        };
        let ordinary = RuntimeRouteMutationProvenanceV2::Ordinary {
            barrier_id: barrier_id.clone(),
            pause: pause.clone(),
        };
        let closed_witness = closed_recovery_witness();
        let closed = RuntimeRouteMutationProvenanceV2::ClosedRecovery(closed_witness.clone());
        let shutdown_witness = shutdown_witness();
        let shutdown = RuntimeRouteMutationProvenanceV2::Shutdown(shutdown_witness.clone());

        assert!(matches!(
            ordinary,
            RuntimeRouteMutationProvenanceV2::Ordinary {
                barrier_id: actual_barrier,
                pause: actual_pause,
            } if actual_barrier == barrier_id && actual_pause == pause
        ));
        assert_eq!(
            closed,
            RuntimeRouteMutationProvenanceV2::ClosedRecovery(closed_witness)
        );
        assert_eq!(
            shutdown,
            RuntimeRouteMutationProvenanceV2::Shutdown(shutdown_witness)
        );
    }
}
