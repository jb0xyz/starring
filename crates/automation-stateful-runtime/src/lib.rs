//! Pure, non-integrated Stateful Runtime R0 protocol scaffold.
//!
//! This crate defines authority-bound event/state identities, deterministic plan commitments and
//! an in-memory reference model for atomic state + durable outbox behavior. It does **not** expose
//! production constructors for compiler/evaluator/effect-journal proofs and therefore cannot be
//! used to deploy or execute a stateful program. Those constructors remain crate-private until an
//! integrated artifact validator, live evaluator and durable database adapter exist.

#![allow(dead_code)] // R0 intentionally keeps all proof constructors private until integration.

mod digest;
mod evaluator;
mod event;
mod state;
mod store;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use digest::EvaluationTraceDigestV1;
pub use digest::{
    OutboxPayloadDigestV1, PreparedStatefulCommitV1, StatefulExecutionPlanDigestV1,
    StatefulExecutionPlanErrorV1, StatefulOutboxPayloadErrorV1, StatefulOutboxPayloadV1,
    ACKNOWLEDGEMENT_STRATEGY_V1, FAILURE_TAIL_OBLIGATION_V1, MAX_EXTERNAL_ACTIONS_V1,
    MAX_OUTBOX_PAYLOAD_BYTES_V1, MAX_PLAN_STATE_MATERIAL_BYTES_V1, STATEFUL_COMPILER_REVISION_V1,
    STATEFUL_EVALUATOR_REVISION_V1,
};
pub use evaluator::{
    PreparedStatefulEvaluationV1, StateSnapshotDigestV1, StatefulEvaluationBranchV1,
    StatefulEvaluationErrorV1, StatefulEvaluationExternalNodeKindV1,
    StatefulEvaluationExternalNodeV1, StatefulEvaluationProofDigestV1,
    StatefulEvaluationStateNodeV1, STATEFUL_EVALUATION_PROOF_KIND_V1,
    STATEFUL_EVALUATION_PROOF_SCHEMA_VERSION_V1,
};
pub use event::{
    event_envelope_digest_v1, EventEnvelopeDigestV1, EventEnvelopeErrorV1, EventEnvelopeRouteV1,
    EventEnvelopeScopeV1, EventEnvelopeV1, LegacyRuleSetIdentityV1, OutboxDispatchAuthorityV1,
    StateSchemaDigestV1, StatefulArtifactDigestV1, StatefulBundlePublicationBindingV1,
    StatefulProgramIdentityErrorV1, StatefulProgramIdentityV1, EVENT_ENVELOPE_KIND_V1,
    EVENT_ENVELOPE_SCHEMA_VERSION_V1,
};
pub use state::{
    CompiledStateVariableV1, CompiledWorkflowDependenciesV1, ResolvedStateKeyV1,
    ResolvedStateReadV1, ResolvedStateWriteV1, ScopedStateAddressV1, StateDeclarationDigestV1,
    StateRowRevisionV1, StateSnapshotRequestV1, StateSnapshotV1, StatefulStateContractErrorV1,
    MAX_STATE_READS_V1, MAX_STATE_WRITES_V1,
};
pub use store::{
    AtomicCommitDispositionV1, ClaimedOutboxWorkV1, InMemoryAtomicStateOutboxStoreV1,
    OutboxClaimRequestV1, OutboxClaimRevisionV1, OutboxClaimTokenV1, OutboxClaimantIdV1,
    OutboxEntryMetadataV1, OutboxHeadRevisionV1, OutboxReleaseReasonV1, OutboxStateV1,
    RecoveryRequiredReasonV1, StateTransitionLedgerEntryV1, StatefulAtomicCommitResultV1,
    StatefulCommitReceiptV1, StatefulStoreErrorV1, MAX_OUTBOX_CLAIM_MILLISECONDS_V1,
    MAX_OUTBOX_SCAN_LIMIT_V1,
};

/// Marker returned by higher layers when they attempt to use this R0 protocol scaffold as a live
/// deployment runtime. Live activation is intentionally unavailable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("Stateful Runtime R0 is a non-integrated protocol scaffold and cannot deploy or execute")]
pub struct StatefulRuntimeProtocolUnavailableV1;
