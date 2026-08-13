//! Compiler-artifact-driven, pure stateful evaluation proof.
//!
//! Construction remains crate-private: this milestone proves deterministic evaluation but does
//! not confer deployment authority, acknowledge an interaction, commit state, or dispatch effects.

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use automation_runtime_interaction::{
    InteractionReceiptClaimRootDigestV1, InteractionReceiptIdentityV1, InteractionRequestDigestV1,
};
use automation_stateful_compiler::{
    CompiledStatefulBranchV1, CompiledStatefulBundleV1, StatefulArtifactDigestV1,
    StatefulBundleDigestV1, StatefulCompilationBindingDigestV1, StatefulStateSchemaDigestV1,
    StatefulUnionSourceMapDigestV1,
};
use automation_stateful_spec::{
    evaluate_validated_stateful_workflow_v1, StatefulCoreBranchSelectionV1,
    StatefulCoreTransitionV1, StatefulSpecDigestV1,
};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::digest::state_material_size;
use crate::state::{
    append_revision, append_state_key, append_state_value, append_string, validate_read_write_sets,
};
use crate::{
    event_envelope_digest_v1, CompiledStateVariableV1, EventEnvelopeDigestV1, EventEnvelopeV1,
    ResolvedStateWriteV1, StateSnapshotRequestV1, StateSnapshotV1, StatefulStateContractErrorV1,
    MAX_EXTERNAL_ACTIONS_V1, MAX_PLAN_STATE_MATERIAL_BYTES_V1, STATEFUL_EVALUATOR_REVISION_V1,
};

pub const STATEFUL_EVALUATION_PROOF_SCHEMA_VERSION_V1: u16 = 1;
pub const STATEFUL_EVALUATION_PROOF_KIND_V1: &str = "starring.prepared-stateful-evaluation.v1";
const STATEFUL_SNAPSHOT_DOMAIN_V1: &[u8] = b"starring.stateful_snapshot.v1\0";
const STATEFUL_EVALUATION_DOMAIN_V1: &[u8] = b"starring.stateful_evaluation_proof.v1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatefulEvaluationBranchV1 {
    True,
    False,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatefulEvaluationExternalNodeKindV1 {
    Effect,
    FinalResponse,
}

/// Compiler/source-map-validated external node projection. There is no free node-ID constructor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatefulEvaluationExternalNodeV1 {
    node_id: String,
    kind: StatefulEvaluationExternalNodeKindV1,
    source_node_index: Option<u32>,
    artifact_node_index: Option<u32>,
    execution_ordinal: u32,
}

impl StatefulEvaluationExternalNodeV1 {
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub fn kind(&self) -> StatefulEvaluationExternalNodeKindV1 {
        self.kind
    }

    pub fn source_node_index(&self) -> Option<u32> {
        self.source_node_index
    }

    pub fn artifact_node_index(&self) -> Option<u32> {
        self.artifact_node_index
    }

    pub fn execution_ordinal(&self) -> u32 {
        self.execution_ordinal
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatefulEvaluationStateNodeV1 {
    node_id: String,
    variable_id: String,
    source_node_index: u32,
    artifact_node_index: u32,
    execution_ordinal: u32,
}

impl StatefulEvaluationStateNodeV1 {
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub fn variable_id(&self) -> &str {
        &self.variable_id
    }

    pub fn source_node_index(&self) -> u32 {
        self.source_node_index
    }

    pub fn artifact_node_index(&self) -> u32 {
        self.artifact_node_index
    }

    pub fn execution_ordinal(&self) -> u32 {
        self.execution_ordinal
    }
}

macro_rules! opaque_digest {
    ($name:ident) => {
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
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

opaque_digest!(StateSnapshotDigestV1);
opaque_digest!(StatefulEvaluationProofDigestV1);

#[cfg(test)]
impl StatefulEvaluationProofDigestV1 {
    pub(crate) fn from_test_trace(trace_digest: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"starring.test_only_legacy_evaluation_trace.v1\0");
        append_string(&mut hasher, trace_digest);
        Self(lower_hex(hasher.finalize().as_slice()))
    }
}

/// Opaque result of evaluating one exact compiled workflow against one coherent snapshot.
///
/// It intentionally has no `Debug` or serialization implementation because the retained event,
/// reads and transitions may contain private modal/state text.
#[derive(Clone, PartialEq, Eq)]
pub struct PreparedStatefulEvaluationV1 {
    envelope: EventEnvelopeV1,
    envelope_digest: EventEnvelopeDigestV1,
    bundle_digest: StatefulBundleDigestV1,
    compilation_binding_digest: StatefulCompilationBindingDigestV1,
    union_source_map_digest: StatefulUnionSourceMapDigestV1,
    source_spec_digest: StatefulSpecDigestV1,
    stateful_artifact_digest: StatefulArtifactDigestV1,
    state_schema_digest: StatefulStateSchemaDigestV1,
    snapshot: StateSnapshotV1,
    snapshot_digest: StateSnapshotDigestV1,
    workflow_id: String,
    source_workflow_index: u32,
    artifact_workflow_index: u32,
    branch: StatefulEvaluationBranchV1,
    condition_result: bool,
    implicit_acknowledgement_ordinal: u32,
    state_nodes: Vec<StatefulEvaluationStateNodeV1>,
    writes: Vec<ResolvedStateWriteV1>,
    external_nodes: Vec<StatefulEvaluationExternalNodeV1>,
    proof_digest: StatefulEvaluationProofDigestV1,
}

impl PreparedStatefulEvaluationV1 {
    pub(crate) fn prepare(
        bundle: &CompiledStatefulBundleV1,
        envelope: &EventEnvelopeV1,
        snapshot: StateSnapshotV1,
    ) -> Result<Self, StatefulEvaluationErrorV1> {
        validate_program(bundle, envelope)?;
        let request = StateSnapshotRequestV1::from_compiled_bundle(bundle, envelope)
            .map_err(map_state_error)?;
        if !request.exact_snapshot(&snapshot) {
            return Err(StatefulEvaluationErrorV1::SnapshotMismatch);
        }
        let artifact_workflow_index = usize::from(request.artifact_workflow_index());
        let workflow = bundle
            .stateful_artifact()
            .workflows()
            .get(artifact_workflow_index)
            .ok_or(StatefulEvaluationErrorV1::WorkflowMismatch)?;
        if workflow.id() != request.workflow_id() || workflow.trigger() != envelope.trigger() {
            return Err(StatefulEvaluationErrorV1::WorkflowMismatch);
        }
        let source_map = bundle
            .union_source_map()
            .stateful_workflows()
            .iter()
            .filter(|map| {
                map.workflow_id() == workflow.id()
                    && usize::try_from(map.artifact_workflow_index()).ok()
                        == Some(artifact_workflow_index)
            })
            .collect::<Vec<_>>();
        let [workflow_map] = source_map.as_slice() else {
            return Err(StatefulEvaluationErrorV1::WorkflowMismatch);
        };
        let source_workflow = bundle
            .source_spec()
            .stateful_workflows
            .get(
                usize::try_from(workflow_map.source_workflow_index())
                    .map_err(|_| StatefulEvaluationErrorV1::WorkflowMismatch)?,
            )
            .ok_or(StatefulEvaluationErrorV1::WorkflowMismatch)?;
        if source_workflow.id != workflow.id() || source_workflow.trigger != *workflow.trigger() {
            return Err(StatefulEvaluationErrorV1::WorkflowMismatch);
        }

        let state_before = resolved_state_for_core(bundle, &request, &snapshot)?;
        let execution = evaluate_validated_stateful_workflow_v1(
            bundle.source_spec(),
            envelope.trigger(),
            envelope.normalized_inputs(),
            &state_before.0,
        )
        .map_err(|_| StatefulEvaluationErrorV1::Evaluation)?;
        if execution.workflow_id() != workflow.id() {
            return Err(StatefulEvaluationErrorV1::Evaluation);
        }
        let (branch, compiled_branch, branch_map) = match execution.branch() {
            StatefulCoreBranchSelectionV1::True => (
                StatefulEvaluationBranchV1::True,
                workflow.on_true(),
                workflow_map.on_true(),
            ),
            StatefulCoreBranchSelectionV1::False => (
                StatefulEvaluationBranchV1::False,
                workflow.on_false(),
                workflow_map.on_false(),
            ),
        };
        let (state_nodes, mut writes) = prepare_writes(
            bundle,
            envelope,
            &snapshot,
            compiled_branch,
            branch_map,
            execution.transitions(),
        )?;
        writes.sort_by(|left, right| left.key().cmp(right.key()));
        if !validate_read_write_sets(snapshot.reads(), &writes) {
            return Err(StatefulEvaluationErrorV1::SnapshotMismatch);
        }
        let external_nodes = prepare_external_nodes(compiled_branch, branch_map)?;
        let trace_external = execution
            .external_node_ids()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let prepared_external = external_nodes
            .iter()
            .map(|node| node.node_id.as_str())
            .collect::<Vec<_>>();
        if trace_external != prepared_external || external_nodes.len() > MAX_EXTERNAL_ACTIONS_V1 {
            return Err(StatefulEvaluationErrorV1::WorkflowMismatch);
        }
        let snapshot_digest = snapshot_digest_v1(&snapshot);
        if state_material_size(snapshot.reads(), &writes) > MAX_PLAN_STATE_MATERIAL_BYTES_V1 {
            return Err(StatefulEvaluationErrorV1::BoundExceeded);
        }
        let envelope_digest = event_envelope_digest_v1(envelope);
        let mut prepared = Self {
            envelope: envelope.clone(),
            envelope_digest,
            bundle_digest: bundle.bundle_digest(),
            compilation_binding_digest: bundle.binding_digest(),
            union_source_map_digest: bundle.union_source_map_digest(),
            source_spec_digest: bundle.stateful_artifact().source().digest(),
            stateful_artifact_digest: bundle.stateful_artifact_digest(),
            state_schema_digest: bundle.state_schema_digest(),
            snapshot,
            snapshot_digest,
            workflow_id: workflow.id().to_string(),
            source_workflow_index: workflow_map.source_workflow_index(),
            artifact_workflow_index: workflow_map.artifact_workflow_index(),
            branch,
            condition_result: execution.condition_result(),
            implicit_acknowledgement_ordinal: branch_map.implicit_acknowledgement_ordinal(),
            state_nodes,
            writes,
            external_nodes,
            proof_digest: StatefulEvaluationProofDigestV1(String::new()),
        };
        prepared.proof_digest = evaluation_proof_digest_v1(&prepared);
        Ok(prepared)
    }

    pub fn receipt_identity(&self) -> InteractionReceiptIdentityV1 {
        self.envelope.receipt_identity()
    }

    pub fn request_digest(&self) -> &InteractionRequestDigestV1 {
        self.envelope.request_digest()
    }

    pub fn claim_root_digest(&self) -> &InteractionReceiptClaimRootDigestV1 {
        self.envelope.claim_root_digest()
    }

    pub fn envelope_digest(&self) -> &EventEnvelopeDigestV1 {
        &self.envelope_digest
    }

    pub fn bundle_digest(&self) -> StatefulBundleDigestV1 {
        self.bundle_digest
    }

    pub fn compilation_binding_digest(&self) -> StatefulCompilationBindingDigestV1 {
        self.compilation_binding_digest
    }

    pub fn union_source_map_digest(&self) -> StatefulUnionSourceMapDigestV1 {
        self.union_source_map_digest
    }

    pub fn source_spec_digest(&self) -> StatefulSpecDigestV1 {
        self.source_spec_digest
    }

    pub fn stateful_artifact_digest(&self) -> StatefulArtifactDigestV1 {
        self.stateful_artifact_digest
    }

    pub fn state_schema_digest(&self) -> StatefulStateSchemaDigestV1 {
        self.state_schema_digest
    }

    pub fn snapshot_digest(&self) -> &StateSnapshotDigestV1 {
        &self.snapshot_digest
    }

    pub fn workflow_id(&self) -> &str {
        &self.workflow_id
    }

    pub fn source_workflow_index(&self) -> u32 {
        self.source_workflow_index
    }

    pub fn artifact_workflow_index(&self) -> u32 {
        self.artifact_workflow_index
    }

    pub fn branch(&self) -> StatefulEvaluationBranchV1 {
        self.branch
    }

    pub fn condition_result(&self) -> bool {
        self.condition_result
    }

    pub fn state_nodes(&self) -> &[StatefulEvaluationStateNodeV1] {
        &self.state_nodes
    }

    pub fn writes(&self) -> &[ResolvedStateWriteV1] {
        &self.writes
    }

    pub fn external_nodes(&self) -> &[StatefulEvaluationExternalNodeV1] {
        &self.external_nodes
    }

    pub fn proof_digest(&self) -> &StatefulEvaluationProofDigestV1 {
        &self.proof_digest
    }

    pub fn verify(&self) -> bool {
        self.envelope_digest == event_envelope_digest_v1(&self.envelope)
            && self.snapshot_digest == snapshot_digest_v1(&self.snapshot)
            && validate_read_write_sets(self.snapshot.reads(), &self.writes)
            && state_material_size(self.snapshot.reads(), &self.writes)
                <= MAX_PLAN_STATE_MATERIAL_BYTES_V1
            && self.proof_digest == evaluation_proof_digest_v1(self)
    }

    pub(crate) fn into_commit_material(
        self,
    ) -> (
        EventEnvelopeV1,
        StateSnapshotV1,
        Vec<ResolvedStateWriteV1>,
        Vec<String>,
        StatefulEvaluationProofDigestV1,
    ) {
        (
            self.envelope,
            self.snapshot,
            self.writes,
            self.external_nodes
                .into_iter()
                .map(|node| node.node_id)
                .collect(),
            self.proof_digest,
        )
    }
}

fn validate_program(
    bundle: &CompiledStatefulBundleV1,
    envelope: &EventEnvelopeV1,
) -> Result<(), StatefulEvaluationErrorV1> {
    let program = envelope.program();
    if program.program_key() != bundle.stateful_artifact().program_key()
        || program.source_spec_digest() != bundle.stateful_artifact().source().digest()
        || program.bundle_digest() != bundle.bundle_digest().to_hex()
        || program.compilation_binding_digest() != bundle.binding_digest().to_hex()
        || program.union_source_map_digest() != bundle.union_source_map_digest().to_hex()
        || program.stateful_artifact_digest().as_str() != bundle.stateful_artifact_digest().to_hex()
        || program.state_schema_digest().as_str() != bundle.state_schema_digest().to_hex()
        || program.evaluator_revision() != STATEFUL_EVALUATOR_REVISION_V1
    {
        return Err(StatefulEvaluationErrorV1::ProgramMismatch);
    }
    Ok(())
}

struct ResolvedCoreStateV1(BTreeMap<String, automation_stateful_spec::StateValueV1>);

impl Drop for ResolvedCoreStateV1 {
    fn drop(&mut self) {
        for value in self.0.values_mut() {
            if let automation_stateful_spec::StateValueV1::Text { value } = value {
                value.zeroize();
            }
        }
    }
}

fn resolved_state_for_core(
    bundle: &CompiledStatefulBundleV1,
    request: &StateSnapshotRequestV1,
    snapshot: &StateSnapshotV1,
) -> Result<ResolvedCoreStateV1, StatefulEvaluationErrorV1> {
    let mut state_before = BTreeMap::new();
    for (index, variable) in bundle
        .stateful_artifact()
        .state_schema()
        .variables()
        .iter()
        .enumerate()
    {
        let index = u16::try_from(index).map_err(|_| StatefulEvaluationErrorV1::BoundExceeded)?;
        let expected = if let Some(read) = snapshot
            .reads()
            .iter()
            .find(|read| request.artifact_index_for_key(read.key()) == Some(index))
        {
            read.value()
        } else {
            variable.initial_value()
        };
        state_before.insert(variable.id().to_string(), expected.clone());
    }
    Ok(ResolvedCoreStateV1(state_before))
}

fn prepare_writes(
    bundle: &CompiledStatefulBundleV1,
    envelope: &EventEnvelopeV1,
    snapshot: &StateSnapshotV1,
    branch: &CompiledStatefulBranchV1,
    branch_map: &automation_stateful_compiler::StatefulBranchSourceMapV1,
    transitions: &[StatefulCoreTransitionV1],
) -> Result<
    (
        Vec<StatefulEvaluationStateNodeV1>,
        Vec<ResolvedStateWriteV1>,
    ),
    StatefulEvaluationErrorV1,
> {
    if branch.state_actions().len() != branch_map.state_actions().len()
        || branch.state_actions().len() != transitions.len()
    {
        return Err(StatefulEvaluationErrorV1::WorkflowMismatch);
    }
    let mut state_nodes = Vec::with_capacity(transitions.len());
    let mut writes = Vec::with_capacity(transitions.len());
    for ((action, map), transition) in branch
        .state_actions()
        .iter()
        .zip(branch_map.state_actions())
        .zip(transitions)
    {
        if action.id != map.node_id()
            || action.id != transition.node_id()
            || action.variable_id != transition.variable_id()
            || usize::try_from(map.artifact_node_index()).ok()
                != branch
                    .state_actions()
                    .iter()
                    .position(|candidate| candidate.id == action.id)
        {
            return Err(StatefulEvaluationErrorV1::WorkflowMismatch);
        }
        let variable_index = bundle
            .stateful_artifact()
            .state_schema()
            .variables()
            .iter()
            .position(|variable| variable.id() == action.variable_id)
            .and_then(|index| u16::try_from(index).ok())
            .ok_or(StatefulEvaluationErrorV1::WorkflowMismatch)?;
        let definition =
            CompiledStateVariableV1::from_compiled_bundle_index(bundle, envelope, variable_index)
                .map_err(map_state_error)?;
        let read = snapshot
            .reads()
            .iter()
            .find(|read| read.key().variable_id() == action.variable_id)
            .ok_or(StatefulEvaluationErrorV1::SnapshotMismatch)?;
        if read.value() != transition.before() {
            return Err(StatefulEvaluationErrorV1::SnapshotMismatch);
        }
        let execution_ordinal = u16::try_from(map.execution_ordinal())
            .map_err(|_| StatefulEvaluationErrorV1::BoundExceeded)?;
        writes.push(
            ResolvedStateWriteV1::set(
                &definition,
                read,
                &action.id,
                execution_ordinal,
                transition.after().clone(),
            )
            .map_err(map_state_error)?,
        );
        state_nodes.push(StatefulEvaluationStateNodeV1 {
            node_id: action.id.clone(),
            variable_id: action.variable_id.clone(),
            source_node_index: map.source_node_index(),
            artifact_node_index: map.artifact_node_index(),
            execution_ordinal: map.execution_ordinal(),
        });
    }
    Ok((state_nodes, writes))
}

fn prepare_external_nodes(
    branch: &CompiledStatefulBranchV1,
    branch_map: &automation_stateful_compiler::StatefulBranchSourceMapV1,
) -> Result<Vec<StatefulEvaluationExternalNodeV1>, StatefulEvaluationErrorV1> {
    if branch.effects().len() != branch_map.effects().len()
        || branch.response().id != branch_map.response().node_id()
    {
        return Err(StatefulEvaluationErrorV1::WorkflowMismatch);
    }
    let mut nodes = branch
        .effects()
        .iter()
        .zip(branch_map.effects())
        .enumerate()
        .map(|(index, (effect, map))| {
            if effect.id != map.node_id()
                || usize::try_from(map.artifact_node_index()).ok() != Some(index)
            {
                return Err(StatefulEvaluationErrorV1::WorkflowMismatch);
            }
            Ok(StatefulEvaluationExternalNodeV1 {
                node_id: effect.id.clone(),
                kind: StatefulEvaluationExternalNodeKindV1::Effect,
                source_node_index: Some(map.source_node_index()),
                artifact_node_index: Some(map.artifact_node_index()),
                execution_ordinal: map.execution_ordinal(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    nodes.push(StatefulEvaluationExternalNodeV1 {
        node_id: branch.response().id.clone(),
        kind: StatefulEvaluationExternalNodeKindV1::FinalResponse,
        source_node_index: None,
        artifact_node_index: None,
        execution_ordinal: branch_map.response().execution_ordinal(),
    });
    let mut ordinals = std::iter::once(branch_map.implicit_acknowledgement_ordinal())
        .chain(
            branch_map
                .state_actions()
                .iter()
                .map(|map| map.execution_ordinal()),
        )
        .chain(nodes.iter().map(|node| node.execution_ordinal));
    let Some(first) = ordinals.next() else {
        return Err(StatefulEvaluationErrorV1::WorkflowMismatch);
    };
    if first != 0
        || !ordinals
            .scan(first, |previous, ordinal| {
                let increasing = ordinal > *previous;
                *previous = ordinal;
                Some(increasing)
            })
            .all(|increasing| increasing)
    {
        return Err(StatefulEvaluationErrorV1::WorkflowMismatch);
    }
    Ok(nodes)
}

fn snapshot_digest_v1(snapshot: &StateSnapshotV1) -> StateSnapshotDigestV1 {
    let mut hasher = Sha256::new();
    hasher.update(STATEFUL_SNAPSHOT_DOMAIN_V1);
    append_string(&mut hasher, snapshot.envelope_digest());
    append_string(&mut hasher, snapshot.bundle_digest());
    append_string(&mut hasher, snapshot.workflow_id());
    hasher.update(snapshot.artifact_workflow_index().to_be_bytes());
    append_string(&mut hasher, snapshot.request_digest());
    hasher.update((snapshot.reads().len() as u64).to_be_bytes());
    for read in snapshot.reads() {
        append_state_key(&mut hasher, read.key());
        append_revision(&mut hasher, read.revision());
        append_string(&mut hasher, read.declaration_digest().as_str());
        append_state_value(&mut hasher, read.value());
    }
    StateSnapshotDigestV1(lower_hex(hasher.finalize().as_slice()))
}

fn evaluation_proof_digest_v1(
    evaluation: &PreparedStatefulEvaluationV1,
) -> StatefulEvaluationProofDigestV1 {
    let mut hasher = Sha256::new();
    hasher.update(STATEFUL_EVALUATION_DOMAIN_V1);
    hasher.update(STATEFUL_EVALUATION_PROOF_SCHEMA_VERSION_V1.to_be_bytes());
    append_string(&mut hasher, STATEFUL_EVALUATION_PROOF_KIND_V1);
    hasher.update(STATEFUL_EVALUATOR_REVISION_V1.to_be_bytes());
    append_string(&mut hasher, evaluation.bundle_digest.to_hex().as_str());
    append_string(
        &mut hasher,
        evaluation.compilation_binding_digest.to_hex().as_str(),
    );
    append_string(
        &mut hasher,
        evaluation.union_source_map_digest.to_hex().as_str(),
    );
    append_string(&mut hasher, evaluation.source_spec_digest.to_hex().as_str());
    append_string(
        &mut hasher,
        evaluation.stateful_artifact_digest.to_hex().as_str(),
    );
    append_string(
        &mut hasher,
        evaluation.state_schema_digest.to_hex().as_str(),
    );
    append_string(&mut hasher, evaluation.envelope_digest.as_str());
    append_string(&mut hasher, evaluation.envelope.request_digest().as_str());
    append_string(
        &mut hasher,
        evaluation.envelope.claim_root_digest().as_str(),
    );
    append_string(&mut hasher, evaluation.snapshot_digest.as_str());
    append_string(&mut hasher, &evaluation.workflow_id);
    hasher.update(evaluation.source_workflow_index.to_be_bytes());
    hasher.update(evaluation.artifact_workflow_index.to_be_bytes());
    hasher.update([match evaluation.branch {
        StatefulEvaluationBranchV1::True => 1,
        StatefulEvaluationBranchV1::False => 0,
    }]);
    hasher.update([u8::from(evaluation.condition_result)]);
    hasher.update(evaluation.implicit_acknowledgement_ordinal.to_be_bytes());
    hasher.update((evaluation.envelope.normalized_inputs().len() as u64).to_be_bytes());
    for (key, value) in evaluation.envelope.normalized_inputs() {
        append_string(&mut hasher, key);
        append_string(&mut hasher, value);
    }
    hasher.update((evaluation.state_nodes.len() as u64).to_be_bytes());
    for node in &evaluation.state_nodes {
        append_string(&mut hasher, &node.node_id);
        append_string(&mut hasher, &node.variable_id);
        hasher.update(node.source_node_index.to_be_bytes());
        hasher.update(node.artifact_node_index.to_be_bytes());
        hasher.update(node.execution_ordinal.to_be_bytes());
    }
    hasher.update((evaluation.writes.len() as u64).to_be_bytes());
    for write in &evaluation.writes {
        append_state_key(&mut hasher, write.key());
        append_revision(&mut hasher, write.expected_revision());
        append_string(&mut hasher, write.declaration_digest().as_str());
        append_state_value(&mut hasher, write.before());
        append_state_value(&mut hasher, write.after());
        append_string(&mut hasher, write.state_action_node_id());
        hasher.update(write.source_ordinal().to_be_bytes());
    }
    hasher.update((evaluation.external_nodes.len() as u64).to_be_bytes());
    for node in &evaluation.external_nodes {
        append_string(&mut hasher, &node.node_id);
        hasher.update([match node.kind {
            StatefulEvaluationExternalNodeKindV1::Effect => 0,
            StatefulEvaluationExternalNodeKindV1::FinalResponse => 1,
        }]);
        append_optional_u32(&mut hasher, node.source_node_index);
        append_optional_u32(&mut hasher, node.artifact_node_index);
        hasher.update(node.execution_ordinal.to_be_bytes());
    }
    StatefulEvaluationProofDigestV1(lower_hex(hasher.finalize().as_slice()))
}

fn append_optional_u32(hasher: &mut Sha256, value: Option<u32>) {
    match value {
        Some(value) => {
            hasher.update([1]);
            hasher.update(value.to_be_bytes());
        }
        None => hasher.update([0]),
    }
}

fn map_state_error(error: StatefulStateContractErrorV1) -> StatefulEvaluationErrorV1 {
    match error {
        StatefulStateContractErrorV1::WorkflowNotFound => {
            StatefulEvaluationErrorV1::WorkflowMismatch
        }
        StatefulStateContractErrorV1::ReadBound => StatefulEvaluationErrorV1::BoundExceeded,
        _ => StatefulEvaluationErrorV1::ProgramMismatch,
    }
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StatefulEvaluationErrorV1 {
    #[error("compiled stateful bundle does not match the verified event authority")]
    ProgramMismatch,
    #[error("compiled stateful workflow or source map is inconsistent")]
    WorkflowMismatch,
    #[error("state snapshot is not the exact coherent dependency snapshot")]
    SnapshotMismatch,
    #[error("stateful evaluation failed")]
    Evaluation,
    #[error("stateful evaluation material exceeds its bound")]
    BoundExceeded,
}
