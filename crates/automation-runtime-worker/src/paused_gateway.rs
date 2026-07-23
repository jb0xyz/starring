use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;

use automation_runtime_controller::{RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayReadyKindV2};
use automation_runtime_convergence::ProcessInstanceId;

use crate::RuntimeGatewayCoordinatorGenerationV2;

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimePausedGatewayObservationV2 {
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    process_instance_id: ProcessInstanceId,
    connection_epoch: NonZeroU64,
    kind: RuntimeGatewayReadyKindV2,
    admission_revision: NonZeroU64,
    sequence: RuntimePausedGatewaySequenceV2,
}

impl RuntimePausedGatewayObservationV2 {
    pub fn new(
        coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
        process_instance_id: ProcessInstanceId,
        connection_epoch: NonZeroU64,
        kind: RuntimeGatewayReadyKindV2,
        admission_revision: NonZeroU64,
        sequence: RuntimePausedGatewaySequenceV2,
    ) -> Self {
        Self {
            coordinator_generation,
            process_instance_id,
            connection_epoch,
            kind,
            admission_revision,
            sequence,
        }
    }

    pub fn coordinator_generation(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.coordinator_generation
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub fn connection_epoch(&self) -> NonZeroU64 {
        self.connection_epoch
    }

    pub fn kind(&self) -> RuntimeGatewayReadyKindV2 {
        self.kind
    }

    pub fn admission_revision(&self) -> NonZeroU64 {
        self.admission_revision
    }

    pub fn transition_sequence(&self) -> RuntimeGatewayAdmissionSequenceV2 {
        self.sequence.transition_sequence()
    }

    pub fn connected_event_sequence(&self) -> RuntimeGatewayAdmissionSequenceV2 {
        self.sequence.connected_event_sequence()
    }

    pub fn last_resume_sequence(&self) -> Option<RuntimeGatewayAdmissionSequenceV2> {
        self.sequence.last_resume_sequence()
    }
}

impl Debug for RuntimePausedGatewayObservationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePausedGatewayObservationV2(<redacted>)")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RuntimePausedGatewaySequenceV2 {
    transition_sequence: RuntimeGatewayAdmissionSequenceV2,
    connected_event_sequence: RuntimeGatewayAdmissionSequenceV2,
    last_resume_sequence: Option<RuntimeGatewayAdmissionSequenceV2>,
}

impl RuntimePausedGatewaySequenceV2 {
    pub fn new(
        transition_sequence: RuntimeGatewayAdmissionSequenceV2,
        connected_event_sequence: RuntimeGatewayAdmissionSequenceV2,
        last_resume_sequence: Option<RuntimeGatewayAdmissionSequenceV2>,
    ) -> Result<Self, RuntimePausedGatewayObservationErrorV2> {
        if transition_sequence < connected_event_sequence
            || last_resume_sequence.is_some_and(|resume| {
                resume <= connected_event_sequence || resume > transition_sequence
            })
        {
            return Err(RuntimePausedGatewayObservationErrorV2::SequenceOrder);
        }
        Ok(Self {
            transition_sequence,
            connected_event_sequence,
            last_resume_sequence,
        })
    }

    pub fn transition_sequence(self) -> RuntimeGatewayAdmissionSequenceV2 {
        self.transition_sequence
    }

    pub fn connected_event_sequence(self) -> RuntimeGatewayAdmissionSequenceV2 {
        self.connected_event_sequence
    }

    pub fn last_resume_sequence(self) -> Option<RuntimeGatewayAdmissionSequenceV2> {
        self.last_resume_sequence
    }
}

impl Debug for RuntimePausedGatewaySequenceV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePausedGatewaySequenceV2(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimePausedGatewayObservationErrorV2 {
    #[error("runtime paused gateway observation sequence order is invalid")]
    SequenceOrder,
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use automation_runtime_controller::{
        RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayReadyKindV2,
    };
    use automation_runtime_convergence::ProcessInstanceId;

    use super::{
        RuntimePausedGatewayObservationErrorV2, RuntimePausedGatewayObservationV2,
        RuntimePausedGatewaySequenceV2,
    };
    use crate::RuntimeGatewayCoordinatorGenerationV2;

    fn non_zero(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).unwrap()
    }

    fn sequence(value: u64) -> RuntimeGatewayAdmissionSequenceV2 {
        RuntimeGatewayAdmissionSequenceV2::new(non_zero(value))
    }

    fn observation(
        transition: u64,
        connected: u64,
        resumed: Option<u64>,
    ) -> Result<RuntimePausedGatewayObservationV2, RuntimePausedGatewayObservationErrorV2> {
        let sequence = RuntimePausedGatewaySequenceV2::new(
            sequence(transition),
            sequence(connected),
            resumed.map(sequence),
        )?;
        Ok(RuntimePausedGatewayObservationV2::new(
            RuntimeGatewayCoordinatorGenerationV2::FIRST,
            ProcessInstanceId::parse("process:1").unwrap(),
            non_zero(2),
            RuntimeGatewayReadyKindV2::Ready,
            non_zero(3),
            sequence,
        ))
    }

    #[test]
    fn paused_observation_preserves_exact_atomic_snapshot_evidence() {
        let observed = observation(5, 2, Some(4)).unwrap();

        assert_eq!(
            observed.coordinator_generation(),
            RuntimeGatewayCoordinatorGenerationV2::FIRST
        );
        assert_eq!(observed.process_instance_id().as_str(), "process:1");
        assert_eq!(observed.connection_epoch().get(), 2);
        assert_eq!(observed.kind(), RuntimeGatewayReadyKindV2::Ready);
        assert_eq!(observed.admission_revision().get(), 3);
        assert_eq!(observed.transition_sequence().get(), 5);
        assert_eq!(observed.connected_event_sequence().get(), 2);
        assert_eq!(observed.last_resume_sequence().unwrap().get(), 4);
        assert_eq!(
            format!("{observed:?}"),
            "RuntimePausedGatewayObservationV2(<redacted>)"
        );
    }

    #[test]
    fn paused_observation_rejects_impossible_sequence_orders() {
        for result in [
            observation(1, 2, None),
            observation(5, 2, Some(2)),
            observation(5, 2, Some(6)),
        ] {
            assert_eq!(
                result,
                Err(RuntimePausedGatewayObservationErrorV2::SequenceOrder)
            );
        }
    }
}
