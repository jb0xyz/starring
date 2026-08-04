use std::convert::Infallible;
use std::fmt::{Debug, Formatter};

use super::*;
use crate::RuntimeGatewayInvalidationCauseV2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeShutdownCauseV2 {
    SignalTerm,
    SignalInterrupt,
    TransportDisconnected,
    ControlOrphaned,
    OwnershipUncertain,
    CapabilityNotReady,
    FinalizerTerminal,
    SupervisorTerminal,
    ProtocolViolation,
    Explicit,
    GenerationOverflow,
}

enum RuntimeProductionTerminalSourceV2 {
    FixedPoint {
        _state: Box<RuntimeStartupRecoveryFixedPointProcessV2>,
    },
    ProductionHandoff {
        _state: Box<RuntimeProductionHandoffProcessV2>,
    },
    AdmissionAcknowledging {
        _state: Box<RuntimeAdmissionAcknowledgingProcessV2>,
    },
    OpenProduction {
        _state: Box<RuntimeEmptyOpenProcessV2>,
    },
    ServingOpen {
        _state: Box<RuntimeServingOpenProcessV2>,
    },
    Emergency {
        _state: Box<RuntimeProductionEmergencyProcessV2>,
    },
}

impl RuntimeProductionTerminalSourceV2 {
    fn stage(&self) -> RuntimeProductionLifecycleStageV2 {
        match self {
            Self::FixedPoint { .. } => RuntimeProductionLifecycleStageV2::FixedPoint,
            Self::ProductionHandoff { .. } => RuntimeProductionLifecycleStageV2::ProductionHandoff,
            Self::AdmissionAcknowledging { .. } => {
                RuntimeProductionLifecycleStageV2::AdmissionAcknowledging
            }
            Self::OpenProduction { .. } | Self::ServingOpen { .. } => {
                RuntimeProductionLifecycleStageV2::OpenProduction
            }
            Self::Emergency { .. } => RuntimeProductionLifecycleStageV2::Emergency,
        }
    }
}

pub struct RuntimeShuttingDownProcessV2 {
    generation: RuntimeGatewayCoordinatorGenerationV2,
    cause: RuntimeShutdownCauseV2,
    source: RuntimeProductionTerminalSourceV2,
}

impl RuntimeShuttingDownProcessV2 {
    pub fn stage(&self) -> RuntimeProductionLifecycleStageV2 {
        RuntimeProductionLifecycleStageV2::Shutdown
    }

    pub fn coordinator_generation(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.generation
    }

    pub fn cause(&self) -> RuntimeShutdownCauseV2 {
        self.cause
    }

    pub fn source_stage(&self) -> RuntimeProductionLifecycleStageV2 {
        self.source.stage()
    }
}

impl Debug for RuntimeShuttingDownProcessV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeShuttingDownProcessV2(<redacted>)")
    }
}

enum RuntimeProductionEmergencySourceV2 {
    FixedPoint {
        _state: Box<RuntimeStartupRecoveryFixedPointProcessV2>,
    },
    ProductionHandoff {
        _state: Box<RuntimeProductionHandoffProcessV2>,
    },
    AdmissionAcknowledging {
        _state: Box<RuntimeAdmissionAcknowledgingProcessV2>,
    },
    OpenProduction {
        _state: Box<RuntimeEmptyOpenProcessV2>,
    },
    ServingOpen {
        _state: Box<RuntimeServingOpenProcessV2>,
    },
}

impl RuntimeProductionEmergencySourceV2 {
    fn stage(&self) -> RuntimeProductionLifecycleStageV2 {
        match self {
            Self::FixedPoint { .. } => RuntimeProductionLifecycleStageV2::FixedPoint,
            Self::ProductionHandoff { .. } => RuntimeProductionLifecycleStageV2::ProductionHandoff,
            Self::AdmissionAcknowledging { .. } => {
                RuntimeProductionLifecycleStageV2::AdmissionAcknowledging
            }
            Self::OpenProduction { .. } | Self::ServingOpen { .. } => {
                RuntimeProductionLifecycleStageV2::OpenProduction
            }
        }
    }
}

pub struct RuntimeProductionEmergencyProcessV2 {
    generation: RuntimeGatewayCoordinatorGenerationV2,
    cause: RuntimeGatewayInvalidationCauseV2,
    source: RuntimeProductionEmergencySourceV2,
}

impl RuntimeProductionEmergencyProcessV2 {
    pub fn stage(&self) -> RuntimeProductionLifecycleStageV2 {
        RuntimeProductionLifecycleStageV2::Emergency
    }

    pub fn coordinator_generation(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.generation
    }

    pub fn cause(&self) -> RuntimeGatewayInvalidationCauseV2 {
        self.cause
    }

    pub fn source_stage(&self) -> RuntimeProductionLifecycleStageV2 {
        self.source.stage()
    }

    pub fn begin_shutdown(
        self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
        cause: RuntimeShutdownCauseV2,
    ) -> Result<RuntimeShuttingDownProcessV2, RuntimeProductionTransitionFailureV2<Self, Infallible>>
    {
        if expected_generation != self.generation {
            return Err(RuntimeProductionTransitionFailureV2::contract(
                self,
                RuntimeProductionLifecycleErrorV2::StaleGeneration,
            ));
        }
        let generation = successor_generation(self.generation).unwrap_or(self.generation);
        let cause = if generation == self.generation {
            RuntimeShutdownCauseV2::GenerationOverflow
        } else {
            cause
        };
        Ok(RuntimeShuttingDownProcessV2 {
            generation,
            cause,
            source: RuntimeProductionTerminalSourceV2::Emergency {
                _state: Box::new(self),
            },
        })
    }
}

impl Debug for RuntimeProductionEmergencyProcessV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProductionEmergencyProcessV2(<redacted>)")
    }
}

pub enum RuntimeProductionInvalidationOutcomeV2 {
    Emergency(RuntimeProductionEmergencyProcessV2),
    Shutdown(RuntimeShuttingDownProcessV2),
}

impl RuntimeProductionInvalidationOutcomeV2 {
    pub fn stage(&self) -> RuntimeProductionLifecycleStageV2 {
        match self {
            Self::Emergency(state) => state.stage(),
            Self::Shutdown(state) => state.stage(),
        }
    }
}

impl Debug for RuntimeProductionInvalidationOutcomeV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeProductionInvalidationOutcomeV2(<redacted>)")
    }
}

impl RuntimeStartupRecoveryFixedPointProcessV2 {
    pub fn invalidate_production(
        mut self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
        cause: RuntimeGatewayInvalidationCauseV2,
    ) -> Result<
        RuntimeProductionInvalidationOutcomeV2,
        RuntimeProductionTransitionFailureV2<Self, Infallible>,
    > {
        match self
            .authority
            .lifecycle
            .invalidate(expected_generation, cause)
        {
            Ok(snapshot) => Ok(RuntimeProductionInvalidationOutcomeV2::Emergency(
                RuntimeProductionEmergencyProcessV2 {
                    generation: snapshot.generation(),
                    cause,
                    source: RuntimeProductionEmergencySourceV2::FixedPoint {
                        _state: Box::new(self),
                    },
                },
            )),
            Err(RuntimeGatewayClosedTransitionErrorV2::GenerationOverflow) => Ok(
                RuntimeProductionInvalidationOutcomeV2::Shutdown(RuntimeShuttingDownProcessV2 {
                    generation: expected_generation,
                    cause: RuntimeShutdownCauseV2::GenerationOverflow,
                    source: RuntimeProductionTerminalSourceV2::FixedPoint {
                        _state: Box::new(self),
                    },
                }),
            ),
            Err(RuntimeGatewayClosedTransitionErrorV2::StaleGeneration) => {
                Err(RuntimeProductionTransitionFailureV2::contract(
                    self,
                    RuntimeProductionLifecycleErrorV2::StaleGeneration,
                ))
            }
            Err(error) => Err(RuntimeProductionTransitionFailureV2::contract(
                self,
                RuntimeProductionLifecycleErrorV2::FixedPoint(error),
            )),
        }
    }

    pub fn begin_shutdown(
        mut self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
        cause: RuntimeShutdownCauseV2,
    ) -> Result<RuntimeShuttingDownProcessV2, RuntimeProductionTransitionFailureV2<Self, Infallible>>
    {
        match self.authority.lifecycle.shutdown(expected_generation) {
            Ok(snapshot) => Ok(RuntimeShuttingDownProcessV2 {
                generation: snapshot.generation(),
                cause,
                source: RuntimeProductionTerminalSourceV2::FixedPoint {
                    _state: Box::new(self),
                },
            }),
            Err(RuntimeGatewayClosedTransitionErrorV2::GenerationOverflow) => {
                Ok(RuntimeShuttingDownProcessV2 {
                    generation: expected_generation,
                    cause: RuntimeShutdownCauseV2::GenerationOverflow,
                    source: RuntimeProductionTerminalSourceV2::FixedPoint {
                        _state: Box::new(self),
                    },
                })
            }
            Err(RuntimeGatewayClosedTransitionErrorV2::StaleGeneration) => {
                Err(RuntimeProductionTransitionFailureV2::contract(
                    self,
                    RuntimeProductionLifecycleErrorV2::StaleGeneration,
                ))
            }
            Err(error) => Err(RuntimeProductionTransitionFailureV2::contract(
                self,
                RuntimeProductionLifecycleErrorV2::FixedPoint(error),
            )),
        }
    }
}

impl RuntimeProductionHandoffProcessV2 {
    pub fn invalidate_production(
        mut self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
        cause: RuntimeGatewayInvalidationCauseV2,
    ) -> Result<
        RuntimeProductionInvalidationOutcomeV2,
        RuntimeProductionTransitionFailureV2<Self, Infallible>,
    > {
        match self
            .fixed_point
            .authority
            .lifecycle
            .invalidate(expected_generation, cause)
        {
            Ok(snapshot) => Ok(RuntimeProductionInvalidationOutcomeV2::Emergency(
                RuntimeProductionEmergencyProcessV2 {
                    generation: snapshot.generation(),
                    cause,
                    source: RuntimeProductionEmergencySourceV2::ProductionHandoff {
                        _state: Box::new(self),
                    },
                },
            )),
            Err(RuntimeGatewayClosedTransitionErrorV2::GenerationOverflow) => Ok(
                RuntimeProductionInvalidationOutcomeV2::Shutdown(RuntimeShuttingDownProcessV2 {
                    generation: expected_generation,
                    cause: RuntimeShutdownCauseV2::GenerationOverflow,
                    source: RuntimeProductionTerminalSourceV2::ProductionHandoff {
                        _state: Box::new(self),
                    },
                }),
            ),
            Err(RuntimeGatewayClosedTransitionErrorV2::StaleGeneration) => {
                Err(RuntimeProductionTransitionFailureV2::contract(
                    self,
                    RuntimeProductionLifecycleErrorV2::StaleGeneration,
                ))
            }
            Err(error) => Err(RuntimeProductionTransitionFailureV2::contract(
                self,
                RuntimeProductionLifecycleErrorV2::FixedPoint(error),
            )),
        }
    }

    pub fn begin_shutdown(
        mut self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
        cause: RuntimeShutdownCauseV2,
    ) -> Result<RuntimeShuttingDownProcessV2, RuntimeProductionTransitionFailureV2<Self, Infallible>>
    {
        match self
            .fixed_point
            .authority
            .lifecycle
            .shutdown(expected_generation)
        {
            Ok(snapshot) => Ok(RuntimeShuttingDownProcessV2 {
                generation: snapshot.generation(),
                cause,
                source: RuntimeProductionTerminalSourceV2::ProductionHandoff {
                    _state: Box::new(self),
                },
            }),
            Err(RuntimeGatewayClosedTransitionErrorV2::GenerationOverflow) => {
                Ok(RuntimeShuttingDownProcessV2 {
                    generation: expected_generation,
                    cause: RuntimeShutdownCauseV2::GenerationOverflow,
                    source: RuntimeProductionTerminalSourceV2::ProductionHandoff {
                        _state: Box::new(self),
                    },
                })
            }
            Err(RuntimeGatewayClosedTransitionErrorV2::StaleGeneration) => {
                Err(RuntimeProductionTransitionFailureV2::contract(
                    self,
                    RuntimeProductionLifecycleErrorV2::StaleGeneration,
                ))
            }
            Err(error) => Err(RuntimeProductionTransitionFailureV2::contract(
                self,
                RuntimeProductionLifecycleErrorV2::FixedPoint(error),
            )),
        }
    }
}

impl RuntimeAdmissionAcknowledgingProcessV2 {
    pub fn invalidate_production(
        self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
        cause: RuntimeGatewayInvalidationCauseV2,
    ) -> Result<
        RuntimeProductionInvalidationOutcomeV2,
        RuntimeProductionTransitionFailureV2<Self, Infallible>,
    > {
        if expected_generation != self.coordinator_generation() {
            return Err(RuntimeProductionTransitionFailureV2::contract(
                self,
                RuntimeProductionLifecycleErrorV2::StaleGeneration,
            ));
        }
        match successor_generation(expected_generation) {
            Ok(generation) => Ok(RuntimeProductionInvalidationOutcomeV2::Emergency(
                RuntimeProductionEmergencyProcessV2 {
                    generation,
                    cause,
                    source: RuntimeProductionEmergencySourceV2::AdmissionAcknowledging {
                        _state: Box::new(self),
                    },
                },
            )),
            Err(_) => Ok(RuntimeProductionInvalidationOutcomeV2::Shutdown(
                RuntimeShuttingDownProcessV2 {
                    generation: expected_generation,
                    cause: RuntimeShutdownCauseV2::GenerationOverflow,
                    source: RuntimeProductionTerminalSourceV2::AdmissionAcknowledging {
                        _state: Box::new(self),
                    },
                },
            )),
        }
    }

    pub fn begin_shutdown(
        self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
        cause: RuntimeShutdownCauseV2,
    ) -> Result<RuntimeShuttingDownProcessV2, RuntimeProductionTransitionFailureV2<Self, Infallible>>
    {
        shutdown_admission(self, expected_generation, cause)
    }
}

impl RuntimeEmptyOpenProcessV2 {
    pub fn invalidate_production(
        self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
        cause: RuntimeGatewayInvalidationCauseV2,
    ) -> Result<
        RuntimeProductionInvalidationOutcomeV2,
        RuntimeProductionTransitionFailureV2<Self, Infallible>,
    > {
        if expected_generation != self.coordinator_generation() {
            return Err(RuntimeProductionTransitionFailureV2::contract(
                self,
                RuntimeProductionLifecycleErrorV2::StaleGeneration,
            ));
        }
        match successor_generation(expected_generation) {
            Ok(generation) => Ok(RuntimeProductionInvalidationOutcomeV2::Emergency(
                RuntimeProductionEmergencyProcessV2 {
                    generation,
                    cause,
                    source: RuntimeProductionEmergencySourceV2::OpenProduction {
                        _state: Box::new(self),
                    },
                },
            )),
            Err(_) => Ok(RuntimeProductionInvalidationOutcomeV2::Shutdown(
                RuntimeShuttingDownProcessV2 {
                    generation: expected_generation,
                    cause: RuntimeShutdownCauseV2::GenerationOverflow,
                    source: RuntimeProductionTerminalSourceV2::OpenProduction {
                        _state: Box::new(self),
                    },
                },
            )),
        }
    }

    pub fn begin_shutdown(
        self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
        cause: RuntimeShutdownCauseV2,
    ) -> Result<RuntimeShuttingDownProcessV2, RuntimeProductionTransitionFailureV2<Self, Infallible>>
    {
        if expected_generation != self.coordinator_generation() {
            return Err(RuntimeProductionTransitionFailureV2::contract(
                self,
                RuntimeProductionLifecycleErrorV2::StaleGeneration,
            ));
        }
        let generation = successor_generation(expected_generation).unwrap_or(expected_generation);
        let cause = if generation == expected_generation {
            RuntimeShutdownCauseV2::GenerationOverflow
        } else {
            cause
        };
        Ok(RuntimeShuttingDownProcessV2 {
            generation,
            cause,
            source: RuntimeProductionTerminalSourceV2::OpenProduction {
                _state: Box::new(self),
            },
        })
    }
}

impl RuntimeServingOpenProcessV2 {
    pub fn invalidate_production(
        mut self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
        cause: RuntimeGatewayInvalidationCauseV2,
    ) -> Result<
        RuntimeProductionInvalidationOutcomeV2,
        RuntimeProductionTransitionFailureV2<Self, Infallible>,
    > {
        if expected_generation != self.coordinator_generation() {
            return Err(RuntimeProductionTransitionFailureV2::contract(
                self,
                RuntimeProductionLifecycleErrorV2::StaleGeneration,
            ));
        }
        self.seal_slot_work();
        match successor_generation(expected_generation) {
            Ok(generation) => Ok(RuntimeProductionInvalidationOutcomeV2::Emergency(
                RuntimeProductionEmergencyProcessV2 {
                    generation,
                    cause,
                    source: RuntimeProductionEmergencySourceV2::ServingOpen {
                        _state: Box::new(self),
                    },
                },
            )),
            Err(_) => Ok(RuntimeProductionInvalidationOutcomeV2::Shutdown(
                RuntimeShuttingDownProcessV2 {
                    generation: expected_generation,
                    cause: RuntimeShutdownCauseV2::GenerationOverflow,
                    source: RuntimeProductionTerminalSourceV2::ServingOpen {
                        _state: Box::new(self),
                    },
                },
            )),
        }
    }

    pub fn begin_shutdown(
        mut self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
        cause: RuntimeShutdownCauseV2,
    ) -> Result<RuntimeShuttingDownProcessV2, RuntimeProductionTransitionFailureV2<Self, Infallible>>
    {
        if expected_generation != self.coordinator_generation() {
            return Err(RuntimeProductionTransitionFailureV2::contract(
                self,
                RuntimeProductionLifecycleErrorV2::StaleGeneration,
            ));
        }
        self.seal_slot_work();
        let generation = successor_generation(expected_generation).unwrap_or(expected_generation);
        let cause = if generation == expected_generation {
            RuntimeShutdownCauseV2::GenerationOverflow
        } else {
            cause
        };
        Ok(RuntimeShuttingDownProcessV2 {
            generation,
            cause,
            source: RuntimeProductionTerminalSourceV2::ServingOpen {
                _state: Box::new(self),
            },
        })
    }
}

fn shutdown_admission(
    state: RuntimeAdmissionAcknowledgingProcessV2,
    expected_generation: RuntimeGatewayCoordinatorGenerationV2,
    cause: RuntimeShutdownCauseV2,
) -> Result<
    RuntimeShuttingDownProcessV2,
    RuntimeProductionTransitionFailureV2<RuntimeAdmissionAcknowledgingProcessV2, Infallible>,
> {
    if expected_generation != state.coordinator_generation() {
        return Err(RuntimeProductionTransitionFailureV2::contract(
            state,
            RuntimeProductionLifecycleErrorV2::StaleGeneration,
        ));
    }
    let generation = successor_generation(expected_generation).unwrap_or(expected_generation);
    let cause = if generation == expected_generation {
        RuntimeShutdownCauseV2::GenerationOverflow
    } else {
        cause
    };
    Ok(RuntimeShuttingDownProcessV2 {
        generation,
        cause,
        source: RuntimeProductionTerminalSourceV2::AdmissionAcknowledging {
            _state: Box::new(state),
        },
    })
}
