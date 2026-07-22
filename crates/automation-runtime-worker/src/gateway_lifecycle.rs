use std::num::NonZeroU64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeGatewayCoordinatorGenerationV2(NonZeroU64);

impl RuntimeGatewayCoordinatorGenerationV2 {
    pub const FIRST: Self = Self(NonZeroU64::MIN);

    pub fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0.get()
    }

    fn successor(self) -> Option<Self> {
        self.get()
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .map(Self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeGatewayEmergencyCauseV2 {
    Starting,
    TransportDisconnected,
    ControlOrphaned,
    OwnershipUncertain,
    CapabilityNotReady,
    ProtocolViolation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeGatewayInvalidationCauseV2 {
    TransportDisconnected,
    ControlOrphaned,
    OwnershipUncertain,
    CapabilityNotReady,
    ProtocolViolation,
}

impl From<RuntimeGatewayInvalidationCauseV2> for RuntimeGatewayEmergencyCauseV2 {
    fn from(value: RuntimeGatewayInvalidationCauseV2) -> Self {
        match value {
            RuntimeGatewayInvalidationCauseV2::TransportDisconnected => Self::TransportDisconnected,
            RuntimeGatewayInvalidationCauseV2::ControlOrphaned => Self::ControlOrphaned,
            RuntimeGatewayInvalidationCauseV2::OwnershipUncertain => Self::OwnershipUncertain,
            RuntimeGatewayInvalidationCauseV2::CapabilityNotReady => Self::CapabilityNotReady,
            RuntimeGatewayInvalidationCauseV2::ProtocolViolation => Self::ProtocolViolation,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeGatewayClosedSnapshotV2 {
    Emergency {
        generation: RuntimeGatewayCoordinatorGenerationV2,
        cause: RuntimeGatewayEmergencyCauseV2,
    },
    Shutdown {
        generation: RuntimeGatewayCoordinatorGenerationV2,
    },
}

impl RuntimeGatewayClosedSnapshotV2 {
    pub fn generation(self) -> RuntimeGatewayCoordinatorGenerationV2 {
        match self {
            Self::Emergency { generation, .. } | Self::Shutdown { generation } => generation,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeGatewayClosedTransitionErrorV2 {
    #[error("runtime gateway coordinator generation is stale")]
    StaleGeneration,
    #[error("runtime gateway coordinator generation overflowed")]
    GenerationOverflow,
    #[error("runtime gateway coordinator is shut down")]
    Shutdown,
}

#[derive(Debug, PartialEq, Eq)]
pub struct RuntimeGatewayClosedLifecycleV2 {
    snapshot: RuntimeGatewayClosedSnapshotV2,
}

impl RuntimeGatewayClosedLifecycleV2 {
    pub fn starting() -> Self {
        Self {
            snapshot: RuntimeGatewayClosedSnapshotV2::Emergency {
                generation: RuntimeGatewayCoordinatorGenerationV2::FIRST,
                cause: RuntimeGatewayEmergencyCauseV2::Starting,
            },
        }
    }

    pub fn snapshot(&self) -> RuntimeGatewayClosedSnapshotV2 {
        self.snapshot
    }

    pub fn invalidate(
        &mut self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
        cause: RuntimeGatewayInvalidationCauseV2,
    ) -> Result<RuntimeGatewayClosedSnapshotV2, RuntimeGatewayClosedTransitionErrorV2> {
        self.require_generation(expected_generation)?;
        if matches!(
            self.snapshot,
            RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
        ) {
            return Err(RuntimeGatewayClosedTransitionErrorV2::Shutdown);
        }
        let generation = self.advance_generation()?;
        self.snapshot = RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: cause.into(),
        };
        Ok(self.snapshot)
    }

    pub fn shutdown(
        &mut self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
    ) -> Result<RuntimeGatewayClosedSnapshotV2, RuntimeGatewayClosedTransitionErrorV2> {
        self.require_generation(expected_generation)?;
        if matches!(
            self.snapshot,
            RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
        ) {
            return Ok(self.snapshot);
        }
        let generation = self.advance_generation()?;
        self.snapshot = RuntimeGatewayClosedSnapshotV2::Shutdown { generation };
        Ok(self.snapshot)
    }

    fn require_generation(
        &self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
    ) -> Result<(), RuntimeGatewayClosedTransitionErrorV2> {
        if self.snapshot.generation() != expected_generation {
            return Err(RuntimeGatewayClosedTransitionErrorV2::StaleGeneration);
        }
        Ok(())
    }

    fn advance_generation(
        &mut self,
    ) -> Result<RuntimeGatewayCoordinatorGenerationV2, RuntimeGatewayClosedTransitionErrorV2> {
        let current = self.snapshot.generation();
        let Some(successor) = current.successor() else {
            self.snapshot = RuntimeGatewayClosedSnapshotV2::Shutdown {
                generation: current,
            };
            return Err(RuntimeGatewayClosedTransitionErrorV2::GenerationOverflow);
        };
        Ok(successor)
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::{
        RuntimeGatewayClosedLifecycleV2, RuntimeGatewayClosedSnapshotV2,
        RuntimeGatewayClosedTransitionErrorV2, RuntimeGatewayCoordinatorGenerationV2,
        RuntimeGatewayEmergencyCauseV2, RuntimeGatewayInvalidationCauseV2,
    };

    #[test]
    fn overflow_becomes_terminally_closed() {
        let maximum = RuntimeGatewayCoordinatorGenerationV2::new(NonZeroU64::MAX);
        let mut lifecycle = RuntimeGatewayClosedLifecycleV2 {
            snapshot: RuntimeGatewayClosedSnapshotV2::Emergency {
                generation: maximum,
                cause: RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
            },
        };

        assert_eq!(
            lifecycle.invalidate(
                maximum,
                RuntimeGatewayInvalidationCauseV2::CapabilityNotReady,
            ),
            Err(RuntimeGatewayClosedTransitionErrorV2::GenerationOverflow)
        );
        assert_eq!(
            lifecycle.snapshot(),
            RuntimeGatewayClosedSnapshotV2::Shutdown {
                generation: maximum,
            }
        );
        assert_eq!(
            lifecycle.invalidate(maximum, RuntimeGatewayInvalidationCauseV2::ControlOrphaned),
            Err(RuntimeGatewayClosedTransitionErrorV2::Shutdown)
        );

        let mut shutdown = RuntimeGatewayClosedLifecycleV2 {
            snapshot: RuntimeGatewayClosedSnapshotV2::Emergency {
                generation: maximum,
                cause: RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
            },
        };
        assert_eq!(
            shutdown.shutdown(maximum),
            Err(RuntimeGatewayClosedTransitionErrorV2::GenerationOverflow)
        );
        assert_eq!(
            shutdown.snapshot(),
            RuntimeGatewayClosedSnapshotV2::Shutdown {
                generation: maximum,
            }
        );
    }
}
