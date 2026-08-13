use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};

use automation_runtime_interaction::InteractionActionPlanDigestV1;
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::state::{
    append_revision, append_state_key, append_state_value, append_string, validate_read_write_sets,
};
use crate::{
    OutboxDispatchAuthorityV1, PreparedStatefulEvaluationV1, ResolvedStateReadV1,
    ResolvedStateWriteV1, StatefulEvaluationProofDigestV1,
};

const EXECUTION_PLAN_DOMAIN_V1: &[u8] = b"starring.stateful_execution_plan.v1\0";
const OUTBOX_PAYLOAD_DOMAIN_V1: &[u8] = b"starring.stateful_outbox_payload.v1\0";
pub const STATEFUL_COMPILER_REVISION_V1: u32 = 1;
pub const STATEFUL_EVALUATOR_REVISION_V1: u32 = 1;
pub const MAX_OUTBOX_PAYLOAD_BYTES_V1: usize = 2 * 1_024 * 1_024;
pub const MAX_EXTERNAL_ACTIONS_V1: usize = 64;
pub const MAX_PLAN_STATE_MATERIAL_BYTES_V1: usize = 1_024 * 1_024;
pub const ACKNOWLEDGEMENT_STRATEGY_V1: &str = "durable_defer_then_atomic_commit_then_edit_v1";
pub const FAILURE_TAIL_OBLIGATION_V1: &str = "receipt_fenced_durable_failure_edit_v1";

macro_rules! define_digest {
    ($name:ident, $error:ident) => {
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub(crate) fn parse(value: impl Into<String>) -> Result<Self, $error> {
                let value = value.into();
                if valid_digest(&value) {
                    Ok(Self(value))
                } else {
                    Err($error::InvalidDigest)
                }
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

#[cfg(test)]
define_digest!(EvaluationTraceDigestV1, StatefulExecutionPlanErrorV1);
define_digest!(OutboxPayloadDigestV1, StatefulOutboxPayloadErrorV1);
define_digest!(StatefulExecutionPlanDigestV1, StatefulExecutionPlanErrorV1);

/// Durable bytes of the fully materialized ordered effect-journal plan. This structure has no
/// `Debug` implementation: payload bytes may include private response or modal-derived text.
#[derive(Clone, PartialEq, Eq)]
pub struct StatefulOutboxPayloadV1 {
    digest: OutboxPayloadDigestV1,
    external_node_ids: Vec<String>,
    canonical_effect_plan_bytes: Vec<u8>,
}

impl StatefulOutboxPayloadV1 {
    pub(crate) fn from_canonical_effect_plan(
        external_node_ids: Vec<String>,
        canonical_effect_plan_bytes: Vec<u8>,
    ) -> Result<Self, StatefulOutboxPayloadErrorV1> {
        if external_node_ids.is_empty()
            || external_node_ids.len() > MAX_EXTERNAL_ACTIONS_V1
            || external_node_ids.iter().any(|id| !valid_identifier(id))
            || external_node_ids.iter().collect::<BTreeSet<_>>().len() != external_node_ids.len()
            || canonical_effect_plan_bytes.is_empty()
            || canonical_effect_plan_bytes.len() > MAX_OUTBOX_PAYLOAD_BYTES_V1
        {
            return Err(StatefulOutboxPayloadErrorV1::InvalidPayload);
        }
        let digest = outbox_payload_digest(&external_node_ids, &canonical_effect_plan_bytes);
        Ok(Self {
            digest,
            external_node_ids,
            canonical_effect_plan_bytes,
        })
    }

    pub fn digest(&self) -> &OutboxPayloadDigestV1 {
        &self.digest
    }

    pub fn external_node_ids(&self) -> &[String] {
        &self.external_node_ids
    }

    pub fn canonical_effect_plan_bytes(&self) -> &[u8] {
        &self.canonical_effect_plan_bytes
    }

    pub fn verify(&self) -> bool {
        outbox_payload_digest(&self.external_node_ids, &self.canonical_effect_plan_bytes)
            == self.digest
    }
}

impl Drop for StatefulOutboxPayloadV1 {
    fn drop(&mut self) {
        self.canonical_effect_plan_bytes.zeroize();
    }
}

fn outbox_payload_digest(
    external_node_ids: &[String],
    canonical_effect_plan_bytes: &[u8],
) -> OutboxPayloadDigestV1 {
    let mut hasher = Sha256::new();
    hasher.update(OUTBOX_PAYLOAD_DOMAIN_V1);
    hasher.update((external_node_ids.len() as u64).to_be_bytes());
    for node_id in external_node_ids {
        append_string(&mut hasher, node_id);
    }
    hasher.update((canonical_effect_plan_bytes.len() as u64).to_be_bytes());
    hasher.update(canonical_effect_plan_bytes);
    OutboxPayloadDigestV1(lower_hex(hasher.finalize().as_slice()))
}

/// Opaque planner-validated commit material. Public construction recomputes every digest and
/// checks all authority, state-set and payload invariants. It intentionally has no `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub struct PreparedStatefulCommitV1 {
    receipt_identity: automation_runtime_interaction::InteractionReceiptIdentityV1,
    request_digest: automation_runtime_interaction::InteractionRequestDigestV1,
    envelope_digest: String,
    source_spec_digest: String,
    stateful_artifact_digest: String,
    state_schema_digest: String,
    evaluation_proof_digest: StatefulEvaluationProofDigestV1,
    external_action_plan_digest: InteractionActionPlanDigestV1,
    dispatch_authority: OutboxDispatchAuthorityV1,
    acknowledgement_strategy: String,
    failure_tail_obligation: String,
    reads: Vec<ResolvedStateReadV1>,
    writes: Vec<ResolvedStateWriteV1>,
    outbox_payload: StatefulOutboxPayloadV1,
    plan_digest: StatefulExecutionPlanDigestV1,
}

impl PreparedStatefulCommitV1 {
    pub(crate) fn prepare(
        evaluation: PreparedStatefulEvaluationV1,
        external_action_plan_digest: InteractionActionPlanDigestV1,
        outbox_payload: StatefulOutboxPayloadV1,
    ) -> Result<Self, StatefulExecutionPlanErrorV1> {
        if !evaluation.verify() {
            return Err(StatefulExecutionPlanErrorV1::InvalidInput);
        }
        let (envelope, snapshot, writes, expected_external_node_ids, evaluation_proof_digest) =
            evaluation.into_commit_material();
        let envelope_digest = crate::event_envelope_digest_v1(&envelope)
            .as_str()
            .to_string();
        if snapshot.envelope_digest() != envelope_digest
            || !outbox_payload.verify()
            || outbox_payload.external_node_ids() != expected_external_node_ids
            || !validate_read_write_sets(snapshot.reads(), &writes)
            || snapshot
                .reads()
                .iter()
                .any(|read| !read.key().authorized_for(&envelope))
            || writes
                .iter()
                .any(|write| !write.key().authorized_for(&envelope))
            || outbox_payload.external_node_ids().len() > MAX_EXTERNAL_ACTIONS_V1
            || state_material_size(snapshot.reads(), &writes) > MAX_PLAN_STATE_MATERIAL_BYTES_V1
        {
            return Err(StatefulExecutionPlanErrorV1::InvalidInput);
        }
        let reads = snapshot.into_reads();
        let source_spec_digest = envelope.program().source_spec_digest().to_hex();
        let stateful_artifact_digest = envelope
            .program()
            .stateful_artifact_digest()
            .as_str()
            .to_string();
        let state_schema_digest = envelope
            .program()
            .state_schema_digest()
            .as_str()
            .to_string();
        let acknowledgement_strategy = ACKNOWLEDGEMENT_STRATEGY_V1.to_string();
        let failure_tail_obligation = FAILURE_TAIL_OBLIGATION_V1.to_string();
        let dispatch_authority = OutboxDispatchAuthorityV1::from_envelope(&envelope);
        let plan_digest = execution_plan_digest(ExecutionPlanMaterialV1 {
            envelope_digest: &envelope_digest,
            source_spec_digest: &source_spec_digest,
            stateful_artifact_digest: &stateful_artifact_digest,
            state_schema_digest: &state_schema_digest,
            evaluation_proof_digest: evaluation_proof_digest.as_str(),
            reads: &reads,
            writes: &writes,
            external_action_plan_digest: external_action_plan_digest.as_str(),
            outbox_payload: &outbox_payload,
            acknowledgement_strategy: &acknowledgement_strategy,
            failure_tail_obligation: &failure_tail_obligation,
        });
        Ok(Self {
            receipt_identity: envelope.receipt_identity(),
            request_digest: envelope.request_digest().clone(),
            envelope_digest,
            source_spec_digest,
            stateful_artifact_digest,
            state_schema_digest,
            evaluation_proof_digest,
            external_action_plan_digest,
            dispatch_authority,
            acknowledgement_strategy,
            failure_tail_obligation,
            reads,
            writes,
            outbox_payload,
            plan_digest,
        })
    }

    /// Legacy reference-store helper retained only for pre-evaluator unit tests. Production code
    /// has no path that accepts a caller-supplied trace digest, snapshot, or writes.
    #[cfg(test)]
    pub(crate) fn prepare_test_scaffold(
        envelope: &crate::EventEnvelopeV1,
        evaluation_trace_digest: EvaluationTraceDigestV1,
        snapshot: crate::StateSnapshotV1,
        writes: Vec<ResolvedStateWriteV1>,
        external_action_plan_digest: InteractionActionPlanDigestV1,
        outbox_payload: StatefulOutboxPayloadV1,
    ) -> Result<Self, StatefulExecutionPlanErrorV1> {
        let envelope_digest = crate::event_envelope_digest_v1(envelope)
            .as_str()
            .to_string();
        if snapshot.envelope_digest() != envelope_digest
            || !outbox_payload.verify()
            || !validate_read_write_sets(snapshot.reads(), &writes)
            || snapshot
                .reads()
                .iter()
                .any(|read| !read.key().authorized_for(envelope))
            || writes
                .iter()
                .any(|write| !write.key().authorized_for(envelope))
            || outbox_payload.external_node_ids().len() > MAX_EXTERNAL_ACTIONS_V1
            || state_material_size(snapshot.reads(), &writes) > MAX_PLAN_STATE_MATERIAL_BYTES_V1
        {
            return Err(StatefulExecutionPlanErrorV1::InvalidInput);
        }
        let evaluation_proof_digest =
            StatefulEvaluationProofDigestV1::from_test_trace(evaluation_trace_digest.as_str());
        let reads = snapshot.into_reads();
        let source_spec_digest = envelope.program().source_spec_digest().to_hex();
        let stateful_artifact_digest = envelope
            .program()
            .stateful_artifact_digest()
            .as_str()
            .to_string();
        let state_schema_digest = envelope
            .program()
            .state_schema_digest()
            .as_str()
            .to_string();
        let acknowledgement_strategy = ACKNOWLEDGEMENT_STRATEGY_V1.to_string();
        let failure_tail_obligation = FAILURE_TAIL_OBLIGATION_V1.to_string();
        let dispatch_authority = OutboxDispatchAuthorityV1::from_envelope(envelope);
        let plan_digest = execution_plan_digest(ExecutionPlanMaterialV1 {
            envelope_digest: &envelope_digest,
            source_spec_digest: &source_spec_digest,
            stateful_artifact_digest: &stateful_artifact_digest,
            state_schema_digest: &state_schema_digest,
            evaluation_proof_digest: evaluation_proof_digest.as_str(),
            reads: &reads,
            writes: &writes,
            external_action_plan_digest: external_action_plan_digest.as_str(),
            outbox_payload: &outbox_payload,
            acknowledgement_strategy: &acknowledgement_strategy,
            failure_tail_obligation: &failure_tail_obligation,
        });
        Ok(Self {
            receipt_identity: envelope.receipt_identity(),
            request_digest: envelope.request_digest().clone(),
            envelope_digest,
            source_spec_digest,
            stateful_artifact_digest,
            state_schema_digest,
            evaluation_proof_digest,
            external_action_plan_digest,
            dispatch_authority,
            acknowledgement_strategy,
            failure_tail_obligation,
            reads,
            writes,
            outbox_payload,
            plan_digest,
        })
    }

    pub fn receipt_identity(&self) -> automation_runtime_interaction::InteractionReceiptIdentityV1 {
        self.receipt_identity
    }

    pub fn request_digest(&self) -> &automation_runtime_interaction::InteractionRequestDigestV1 {
        &self.request_digest
    }

    pub fn envelope_digest(&self) -> &str {
        &self.envelope_digest
    }

    pub fn source_spec_digest(&self) -> &str {
        &self.source_spec_digest
    }

    pub fn stateful_artifact_digest(&self) -> &str {
        &self.stateful_artifact_digest
    }

    pub fn state_schema_digest(&self) -> &str {
        &self.state_schema_digest
    }

    pub fn evaluation_proof_digest(&self) -> &StatefulEvaluationProofDigestV1 {
        &self.evaluation_proof_digest
    }

    pub fn external_action_plan_digest(&self) -> &InteractionActionPlanDigestV1 {
        &self.external_action_plan_digest
    }

    pub fn dispatch_authority(&self) -> &OutboxDispatchAuthorityV1 {
        &self.dispatch_authority
    }

    pub fn acknowledgement_strategy(&self) -> &str {
        &self.acknowledgement_strategy
    }

    pub fn failure_tail_obligation(&self) -> &str {
        &self.failure_tail_obligation
    }

    pub fn reads(&self) -> &[ResolvedStateReadV1] {
        &self.reads
    }

    pub fn writes(&self) -> &[ResolvedStateWriteV1] {
        &self.writes
    }

    pub fn outbox_payload(&self) -> &StatefulOutboxPayloadV1 {
        &self.outbox_payload
    }

    pub fn plan_digest(&self) -> &StatefulExecutionPlanDigestV1 {
        &self.plan_digest
    }

    pub fn verify(&self) -> bool {
        self.acknowledgement_strategy == ACKNOWLEDGEMENT_STRATEGY_V1
            && self.failure_tail_obligation == FAILURE_TAIL_OBLIGATION_V1
            && self.outbox_payload.verify()
            && validate_read_write_sets(&self.reads, &self.writes)
            && state_material_size(&self.reads, &self.writes) <= MAX_PLAN_STATE_MATERIAL_BYTES_V1
            && execution_plan_digest(ExecutionPlanMaterialV1 {
                envelope_digest: &self.envelope_digest,
                source_spec_digest: &self.source_spec_digest,
                stateful_artifact_digest: &self.stateful_artifact_digest,
                state_schema_digest: &self.state_schema_digest,
                evaluation_proof_digest: self.evaluation_proof_digest.as_str(),
                reads: &self.reads,
                writes: &self.writes,
                external_action_plan_digest: self.external_action_plan_digest.as_str(),
                outbox_payload: &self.outbox_payload,
                acknowledgement_strategy: &self.acknowledgement_strategy,
                failure_tail_obligation: &self.failure_tail_obligation,
            }) == self.plan_digest
    }
}

struct ExecutionPlanMaterialV1<'a> {
    envelope_digest: &'a str,
    source_spec_digest: &'a str,
    stateful_artifact_digest: &'a str,
    state_schema_digest: &'a str,
    evaluation_proof_digest: &'a str,
    reads: &'a [ResolvedStateReadV1],
    writes: &'a [ResolvedStateWriteV1],
    external_action_plan_digest: &'a str,
    outbox_payload: &'a StatefulOutboxPayloadV1,
    acknowledgement_strategy: &'a str,
    failure_tail_obligation: &'a str,
}

fn execution_plan_digest(material: ExecutionPlanMaterialV1<'_>) -> StatefulExecutionPlanDigestV1 {
    let mut hasher = Sha256::new();
    hasher.update(EXECUTION_PLAN_DOMAIN_V1);
    append_string(&mut hasher, material.envelope_digest);
    append_string(&mut hasher, material.source_spec_digest);
    append_string(&mut hasher, material.stateful_artifact_digest);
    append_string(&mut hasher, material.state_schema_digest);
    append_string(&mut hasher, material.evaluation_proof_digest);
    hasher.update(STATEFUL_COMPILER_REVISION_V1.to_be_bytes());
    hasher.update(STATEFUL_EVALUATOR_REVISION_V1.to_be_bytes());
    hasher.update((material.reads.len() as u64).to_be_bytes());
    for read in material.reads {
        append_state_key(&mut hasher, read.key());
        append_revision(&mut hasher, read.revision());
        append_string(&mut hasher, read.declaration_digest().as_str());
        append_state_value(&mut hasher, read.value());
    }
    hasher.update((material.writes.len() as u64).to_be_bytes());
    for write in material.writes {
        append_state_key(&mut hasher, write.key());
        append_revision(&mut hasher, write.expected_revision());
        append_string(&mut hasher, write.declaration_digest().as_str());
        append_state_value(&mut hasher, write.before());
        append_state_value(&mut hasher, write.after());
        append_string(&mut hasher, write.state_action_node_id());
        hasher.update(write.source_ordinal().to_be_bytes());
    }
    append_string(&mut hasher, material.external_action_plan_digest);
    hasher.update((material.outbox_payload.external_node_ids().len() as u64).to_be_bytes());
    for node_id in material.outbox_payload.external_node_ids() {
        append_string(&mut hasher, node_id);
    }
    append_string(&mut hasher, material.outbox_payload.digest().as_str());
    append_string(&mut hasher, material.acknowledgement_strategy);
    append_string(&mut hasher, material.failure_tail_obligation);
    StatefulExecutionPlanDigestV1(lower_hex(hasher.finalize().as_slice()))
}

pub(crate) fn state_material_size(
    reads: &[ResolvedStateReadV1],
    writes: &[ResolvedStateWriteV1],
) -> usize {
    reads
        .iter()
        .map(|read| state_key_size(read.key()) + 8 + 64 + state_value_size(read.value()))
        .chain(writes.iter().map(|write| {
            state_key_size(write.key())
                + 8
                + 64
                + state_value_size(write.before())
                + state_value_size(write.after())
                + write.state_action_node_id().len()
                + 2
        }))
        .fold(0usize, usize::saturating_add)
}

fn state_key_size(key: &crate::ResolvedStateKeyV1) -> usize {
    let address = match key.address() {
        crate::ScopedStateAddressV1::Installation => 1,
        crate::ScopedStateAddressV1::Actor { .. } => 9,
        crate::ScopedStateAddressV1::Instance { instance_id } => 9 + instance_id.len(),
        crate::ScopedStateAddressV1::ActorInstance { instance_id, .. } => 17 + instance_id.len(),
    };
    key.tenant_id().len()
        + key.installation_id().len()
        + key.program_key().len()
        + key.variable_id().len()
        + 8
        + address
}

fn state_value_size(value: &automation_stateful_spec::StateValueV1) -> usize {
    match value {
        automation_stateful_spec::StateValueV1::Bool { .. } => 2,
        automation_stateful_spec::StateValueV1::Integer { .. } => 9,
        automation_stateful_spec::StateValueV1::Text { value } => 9 + value.len(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StatefulOutboxPayloadErrorV1 {
    #[error("stateful outbox payload is invalid")]
    InvalidPayload,
    #[error("stateful outbox payload digest is invalid")]
    InvalidDigest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StatefulExecutionPlanErrorV1 {
    #[error("stateful execution plan input is invalid")]
    InvalidInput,
    #[error("stateful execution plan digest is invalid")]
    InvalidDigest,
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}
