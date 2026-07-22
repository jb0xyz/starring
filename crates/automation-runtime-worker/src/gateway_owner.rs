use std::future::Future;

use automation_runtime_controller::{
    RuntimeAcquireGatewayOwnerLeaseOutcomeV1, RuntimeAcquireGatewayOwnerLeaseV1,
    RuntimeGatewayOwnerLeaseObservationV1, RuntimeGatewayOwnerLeaseReceiptV1,
    RuntimeObserveGatewayOwnerLeaseV1, RuntimeObservedGatewayOwnerLeaseV1,
    RuntimeReleaseGatewayOwnerLeaseOutcomeV1, RuntimeReleaseGatewayOwnerLeaseV1,
    RuntimeRenewGatewayOwnerLeaseOutcomeV1, RuntimeRenewGatewayOwnerLeaseV1,
};

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeGatewayOwnerMutationErrorV1<E> {
    DefinitelyNotApplied { source: E },
    OutcomeUnknown { source: E },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeGatewayOwnerProtocolViolationV1 {
    ShardMismatch,
    StableLeaseMismatch,
    ProcessMismatch,
    BuildMismatch,
    RevisionMismatch,
    NonFreshReceipt,
    InconsistentOutcome,
    RevisionExhausted,
}

pub trait RuntimeGatewayOwnerLeasePortV1 {
    type Error;

    fn observe_gateway_owner(
        &self,
        request: RuntimeObserveGatewayOwnerLeaseV1,
    ) -> impl Future<Output = Result<RuntimeGatewayOwnerLeaseObservationV1, Self::Error>> + Send;

    fn acquire_gateway_owner(
        &self,
        request: RuntimeAcquireGatewayOwnerLeaseV1,
    ) -> impl Future<
        Output = Result<
            RuntimeAcquireGatewayOwnerLeaseOutcomeV1,
            RuntimeGatewayOwnerMutationErrorV1<Self::Error>,
        >,
    > + Send;

    fn renew_gateway_owner(
        &self,
        request: RuntimeRenewGatewayOwnerLeaseV1,
    ) -> impl Future<
        Output = Result<
            RuntimeRenewGatewayOwnerLeaseOutcomeV1,
            RuntimeGatewayOwnerMutationErrorV1<Self::Error>,
        >,
    > + Send;

    fn release_gateway_owner(
        &self,
        request: RuntimeReleaseGatewayOwnerLeaseV1,
    ) -> impl Future<
        Output = Result<
            RuntimeReleaseGatewayOwnerLeaseOutcomeV1,
            RuntimeGatewayOwnerMutationErrorV1<Self::Error>,
        >,
    > + Send;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeGatewayOwnerAcquireRecoveryV1 {
    Adopt(RuntimeGatewayOwnerLeaseReceiptV1),
    ReplaySameRequest,
    Contended(RuntimeGatewayOwnerLeaseReceiptV1),
    ProtocolViolation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeGatewayOwnerRenewRecoveryV1 {
    AdoptSuccessor(RuntimeGatewayOwnerLeaseReceiptV1),
    ReplaySameRequest,
    OwnershipLost(RuntimeGatewayOwnerLeaseObservationV1),
    ProtocolViolation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeGatewayOwnerReleaseRecoveryV1 {
    ReplaySameRequest,
    CompleteWithoutOwnership(RuntimeGatewayOwnerLeaseObservationV1),
    ProtocolViolation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeAcceptedGatewayOwnerAcquireV1 {
    Acquired(RuntimeGatewayOwnerLeaseReceiptV1),
    Contended(RuntimeGatewayOwnerLeaseReceiptV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeAcceptedGatewayOwnerRenewV1 {
    Renewed(RuntimeGatewayOwnerLeaseReceiptV1),
    OwnershipLost(RuntimeGatewayOwnerLeaseObservationV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeAcceptedGatewayOwnerReleaseV1 {
    Released,
    NotHeld(RuntimeGatewayOwnerLeaseObservationV1),
}

pub fn accept_gateway_owner_observation_v1(
    request: &RuntimeObserveGatewayOwnerLeaseV1,
    observation: RuntimeGatewayOwnerLeaseObservationV1,
) -> Result<RuntimeGatewayOwnerLeaseObservationV1, RuntimeGatewayOwnerProtocolViolationV1> {
    match &observation {
        RuntimeGatewayOwnerLeaseObservationV1::Unowned {
            gateway_shard_id, ..
        } => {
            if gateway_shard_id != &request.gateway_shard_id {
                return Err(RuntimeGatewayOwnerProtocolViolationV1::ShardMismatch);
            }
        }
        RuntimeGatewayOwnerLeaseObservationV1::Owned(observed) => {
            current_observed_receipt(observed)?;
            if observed.lease_id.gateway_shard_id != request.gateway_shard_id {
                return Err(RuntimeGatewayOwnerProtocolViolationV1::ShardMismatch);
            }
        }
    }
    Ok(observation)
}

pub fn accept_gateway_owner_acquire_v1(
    request: &RuntimeAcquireGatewayOwnerLeaseV1,
    outcome: RuntimeAcquireGatewayOwnerLeaseOutcomeV1,
) -> Result<RuntimeAcceptedGatewayOwnerAcquireV1, RuntimeGatewayOwnerProtocolViolationV1> {
    match outcome {
        RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Acquired(receipt) => {
            validate_receipt_shard(&receipt, &request.gateway_shard_id)?;
            if receipt.lease_id.process_instance_id != request.process_instance_id {
                return Err(RuntimeGatewayOwnerProtocolViolationV1::ProcessMismatch);
            }
            if receipt.lease_id.expected_build_revision != request.expected_build_revision {
                return Err(RuntimeGatewayOwnerProtocolViolationV1::BuildMismatch);
            }
            Ok(RuntimeAcceptedGatewayOwnerAcquireV1::Acquired(receipt))
        }
        RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Contended(receipt) => {
            validate_receipt_shard(&receipt, &request.gateway_shard_id)?;
            if receipt.lease_id.process_instance_id == request.process_instance_id {
                return Err(RuntimeGatewayOwnerProtocolViolationV1::InconsistentOutcome);
            }
            Ok(RuntimeAcceptedGatewayOwnerAcquireV1::Contended(receipt))
        }
    }
}

pub fn accept_gateway_owner_renew_v1(
    request: &RuntimeRenewGatewayOwnerLeaseV1,
    outcome: RuntimeRenewGatewayOwnerLeaseOutcomeV1,
) -> Result<RuntimeAcceptedGatewayOwnerRenewV1, RuntimeGatewayOwnerProtocolViolationV1> {
    match outcome {
        RuntimeRenewGatewayOwnerLeaseOutcomeV1::Renewed(receipt) => {
            validate_receipt_shard(&receipt, &request.lease_id.gateway_shard_id)?;
            if receipt.lease_id != request.lease_id {
                return Err(RuntimeGatewayOwnerProtocolViolationV1::StableLeaseMismatch);
            }
            let successor = successor_revision(request.expected_owner_revision)?;
            if receipt.owner_revision != successor {
                return Err(RuntimeGatewayOwnerProtocolViolationV1::RevisionMismatch);
            }
            Ok(RuntimeAcceptedGatewayOwnerRenewV1::Renewed(receipt))
        }
        RuntimeRenewGatewayOwnerLeaseOutcomeV1::NotCurrent(observation) => {
            let observation = accept_gateway_owner_observation_v1(
                &RuntimeObserveGatewayOwnerLeaseV1 {
                    gateway_shard_id: request.lease_id.gateway_shard_id.clone(),
                },
                observation,
            )?;
            if let RuntimeGatewayOwnerLeaseObservationV1::Owned(observed) = &observation {
                if observed.lease_id == request.lease_id {
                    return Err(RuntimeGatewayOwnerProtocolViolationV1::InconsistentOutcome);
                }
            }
            Ok(RuntimeAcceptedGatewayOwnerRenewV1::OwnershipLost(
                observation,
            ))
        }
    }
}

pub fn accept_gateway_owner_release_v1(
    request: &RuntimeReleaseGatewayOwnerLeaseV1,
    outcome: RuntimeReleaseGatewayOwnerLeaseOutcomeV1,
) -> Result<RuntimeAcceptedGatewayOwnerReleaseV1, RuntimeGatewayOwnerProtocolViolationV1> {
    match outcome {
        RuntimeReleaseGatewayOwnerLeaseOutcomeV1::Released { lease_id, .. } => {
            if lease_id != request.lease_id {
                return Err(RuntimeGatewayOwnerProtocolViolationV1::StableLeaseMismatch);
            }
            Ok(RuntimeAcceptedGatewayOwnerReleaseV1::Released)
        }
        RuntimeReleaseGatewayOwnerLeaseOutcomeV1::NotHeld(observation) => {
            let observation = accept_gateway_owner_observation_v1(
                &RuntimeObserveGatewayOwnerLeaseV1 {
                    gateway_shard_id: request.lease_id.gateway_shard_id.clone(),
                },
                observation,
            )?;
            if let RuntimeGatewayOwnerLeaseObservationV1::Owned(observed) = &observation {
                if observed.lease_id == request.lease_id {
                    return Err(RuntimeGatewayOwnerProtocolViolationV1::InconsistentOutcome);
                }
            }
            Ok(RuntimeAcceptedGatewayOwnerReleaseV1::NotHeld(observation))
        }
    }
}

pub fn classify_unknown_gateway_owner_acquire_v1(
    request: &RuntimeAcquireGatewayOwnerLeaseV1,
    observation: RuntimeGatewayOwnerLeaseObservationV1,
) -> RuntimeGatewayOwnerAcquireRecoveryV1 {
    match observation {
        RuntimeGatewayOwnerLeaseObservationV1::Unowned {
            gateway_shard_id, ..
        } => {
            if gateway_shard_id == request.gateway_shard_id {
                RuntimeGatewayOwnerAcquireRecoveryV1::ReplaySameRequest
            } else {
                RuntimeGatewayOwnerAcquireRecoveryV1::ProtocolViolation
            }
        }
        RuntimeGatewayOwnerLeaseObservationV1::Owned(observed) => {
            let Ok(receipt) = current_observed_receipt(&observed) else {
                return RuntimeGatewayOwnerAcquireRecoveryV1::ProtocolViolation;
            };
            if receipt.lease_id.gateway_shard_id != request.gateway_shard_id {
                return RuntimeGatewayOwnerAcquireRecoveryV1::ProtocolViolation;
            }
            if receipt.lease_id.process_instance_id == request.process_instance_id {
                if receipt.lease_id.expected_build_revision == request.expected_build_revision {
                    RuntimeGatewayOwnerAcquireRecoveryV1::Adopt(receipt)
                } else {
                    RuntimeGatewayOwnerAcquireRecoveryV1::ProtocolViolation
                }
            } else {
                RuntimeGatewayOwnerAcquireRecoveryV1::Contended(receipt)
            }
        }
    }
}

pub fn classify_unknown_gateway_owner_renew_v1(
    request: &RuntimeRenewGatewayOwnerLeaseV1,
    observation: RuntimeGatewayOwnerLeaseObservationV1,
) -> RuntimeGatewayOwnerRenewRecoveryV1 {
    let Ok(successor_revision) = successor_revision(request.expected_owner_revision) else {
        return RuntimeGatewayOwnerRenewRecoveryV1::ProtocolViolation;
    };
    match observation {
        RuntimeGatewayOwnerLeaseObservationV1::Unowned {
            gateway_shard_id,
            database_now,
        } => {
            if gateway_shard_id != request.lease_id.gateway_shard_id {
                RuntimeGatewayOwnerRenewRecoveryV1::ProtocolViolation
            } else {
                RuntimeGatewayOwnerRenewRecoveryV1::OwnershipLost(
                    RuntimeGatewayOwnerLeaseObservationV1::Unowned {
                        gateway_shard_id,
                        database_now,
                    },
                )
            }
        }
        RuntimeGatewayOwnerLeaseObservationV1::Owned(observed) => {
            let Ok(receipt) = current_observed_receipt(&observed) else {
                return RuntimeGatewayOwnerRenewRecoveryV1::ProtocolViolation;
            };
            if receipt.lease_id.gateway_shard_id != request.lease_id.gateway_shard_id {
                return RuntimeGatewayOwnerRenewRecoveryV1::ProtocolViolation;
            }
            if receipt.lease_id != request.lease_id {
                return RuntimeGatewayOwnerRenewRecoveryV1::OwnershipLost(
                    RuntimeGatewayOwnerLeaseObservationV1::Owned(observed),
                );
            }
            if receipt.owner_revision == successor_revision {
                RuntimeGatewayOwnerRenewRecoveryV1::AdoptSuccessor(receipt)
            } else if receipt.owner_revision == request.expected_owner_revision {
                RuntimeGatewayOwnerRenewRecoveryV1::ReplaySameRequest
            } else {
                RuntimeGatewayOwnerRenewRecoveryV1::ProtocolViolation
            }
        }
    }
}

pub fn classify_unknown_gateway_owner_release_v1(
    request: &RuntimeReleaseGatewayOwnerLeaseV1,
    observation: RuntimeGatewayOwnerLeaseObservationV1,
) -> RuntimeGatewayOwnerReleaseRecoveryV1 {
    match observation {
        RuntimeGatewayOwnerLeaseObservationV1::Unowned {
            gateway_shard_id,
            database_now,
        } => {
            let observation = RuntimeGatewayOwnerLeaseObservationV1::Unowned {
                gateway_shard_id: gateway_shard_id.clone(),
                database_now,
            };
            if gateway_shard_id == request.lease_id.gateway_shard_id {
                RuntimeGatewayOwnerReleaseRecoveryV1::CompleteWithoutOwnership(observation)
            } else {
                RuntimeGatewayOwnerReleaseRecoveryV1::ProtocolViolation
            }
        }
        RuntimeGatewayOwnerLeaseObservationV1::Owned(observed) => {
            let Ok(receipt) = current_observed_receipt(&observed) else {
                return RuntimeGatewayOwnerReleaseRecoveryV1::ProtocolViolation;
            };
            if receipt.lease_id.gateway_shard_id != request.lease_id.gateway_shard_id {
                return RuntimeGatewayOwnerReleaseRecoveryV1::ProtocolViolation;
            }
            if receipt.lease_id == request.lease_id {
                RuntimeGatewayOwnerReleaseRecoveryV1::ReplaySameRequest
            } else {
                RuntimeGatewayOwnerReleaseRecoveryV1::CompleteWithoutOwnership(
                    RuntimeGatewayOwnerLeaseObservationV1::Owned(observed),
                )
            }
        }
    }
}

fn current_observed_receipt(
    observed: &RuntimeObservedGatewayOwnerLeaseV1,
) -> Result<RuntimeGatewayOwnerLeaseReceiptV1, RuntimeGatewayOwnerProtocolViolationV1> {
    observed
        .current_receipt()
        .ok_or(RuntimeGatewayOwnerProtocolViolationV1::NonFreshReceipt)
}

fn validate_receipt_shard(
    receipt: &RuntimeGatewayOwnerLeaseReceiptV1,
    expected_shard: &automation_runtime_controller::GatewayShardIdV1,
) -> Result<(), RuntimeGatewayOwnerProtocolViolationV1> {
    if receipt.database_lease_duration().is_none() {
        return Err(RuntimeGatewayOwnerProtocolViolationV1::NonFreshReceipt);
    }
    if &receipt.lease_id.gateway_shard_id != expected_shard {
        return Err(RuntimeGatewayOwnerProtocolViolationV1::ShardMismatch);
    }
    Ok(())
}

fn successor_revision(
    revision: std::num::NonZeroU64,
) -> Result<std::num::NonZeroU64, RuntimeGatewayOwnerProtocolViolationV1> {
    revision
        .get()
        .checked_add(1)
        .and_then(std::num::NonZeroU64::new)
        .ok_or(RuntimeGatewayOwnerProtocolViolationV1::RevisionExhausted)
}
