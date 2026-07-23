use std::num::NonZeroU64;

use automation_runtime_convergence::{FencingToken, RuntimeGeneration};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ExactServingRouteError {
    #[error("RuleSet slot does not match the runtime target")]
    RuleSetSlotMismatch,
    #[error("RuleSet version does not match the runtime target")]
    RuleSetVersionMismatch,
    #[error("RuleSet content hash does not match the runtime target")]
    RuleSetContentHashMismatch,
    #[error("RuleSet definition does not match its content hash")]
    RuleSetDefinitionHashMismatch,
    #[error("resource bindings do not match the runtime target fingerprint")]
    BindingFingerprintMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ServingSlotRegistryError {
    #[error("runtime target does not belong to the requested serving slot")]
    TargetSlotMismatch,
    #[error("fencing token {actual} is not newer than {minimum}")]
    StaleFencingToken {
        minimum: FencingToken,
        actual: FencingToken,
    },
    #[error("runtime generation {actual} is older than {minimum}")]
    StaleRuntimeGeneration {
        minimum: RuntimeGeneration,
        actual: RuntimeGeneration,
    },
    #[error("runtime generation cannot identify two different immutable targets")]
    RuntimeGenerationIdentityConflict,
    #[error("slot authority target does not exactly match the installed target")]
    AuthorityTargetMismatch,
    #[error("fencing token {actual} is not the exact successor {expected}")]
    NonSuccessorFencingToken {
        expected: FencingToken,
        actual: FencingToken,
    },
    #[error("serving slot fencing token space is exhausted")]
    FencingTokenExhausted,
    #[error("slot mutation token is stale")]
    StaleMutationToken,
    #[error("activation target does not exactly match the staged target")]
    ActivationTargetMismatch,
    #[error("serving slot is not accepting interactions")]
    NotServing,
    #[error("serving slot has reached its active interaction limit")]
    ActiveInteractionCapacityExceeded,
    #[error("serving slot is not draining")]
    NotDraining,
    #[error("serving slot still has {active} active interactions")]
    ActiveInteractionsRemain { active: u32 },
    #[error("serving slot has reached its retired route limit")]
    RetiredRouteCapacityExceeded,
    #[error("serving slot registry has reached its slot limit")]
    SlotCapacityExceeded,
    #[error("serving slot registry incarnation space is exhausted")]
    IncarnationExhausted,
    #[error("serving slot monotonic state is exhausted")]
    SlotSequenceExhausted,
    #[error("serving slot registry monotonic state is exhausted")]
    RegistrySequenceExhausted,
    #[error("serving slot registry recovery observation is inconsistent")]
    RegistryObservationInvalid,
    #[error("serving slot registry recovery observation exceeds its count domain")]
    RegistryObservationOverflow,
    #[error("serving slot registry is not recovery-empty")]
    RegistryRecoveryNotEmpty,
    #[error("serving slot registry empty recovery cursor is stale")]
    StaleRegistryEmptyRecoveryCursor,
    #[error("serving slot admission generation changed from {expected} to {actual}")]
    AdmissionGenerationMismatch {
        expected: NonZeroU64,
        actual: NonZeroU64,
    },
    #[error("serving slot observation is stale")]
    StaleSlotObservation,
    #[error("serving slot is sealed against admission and ordinary mutation")]
    SlotSealed,
    #[error("serving slot seal capability is stale")]
    StaleSlotSeal,
    #[error("serving slot registry lock is poisoned")]
    RegistryPoisoned,
}
