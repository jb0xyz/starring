use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};
use std::num::NonZeroU64;

use automation_stateful_compiler::CompiledStatefulBundleV1;
#[cfg(test)]
use automation_stateful_spec::{
    stateful_spec_digest_v1, StatefulConditionExprV1, StatefulSpecV1, StatefulValueExprV1,
};
use automation_stateful_spec::{
    StateScopeV1, StateValueTypeV1, StateValueV1, TriggerV1, MAX_STATE_TEXT_BYTES_V1,
    MAX_STATE_TEXT_UTF16_UNITS_V1,
};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

#[cfg(test)]
use crate::StatefulProgramIdentityV1;
use crate::{EventEnvelopeScopeV1, EventEnvelopeV1};

const STATE_DECLARATION_DOMAIN_V1: &[u8] = b"starring.stateful_state_declaration.v1\0";
const SNAPSHOT_REQUEST_DOMAIN_V1: &[u8] = b"starring.stateful_snapshot_request.v1\0";
pub const MAX_STATE_READS_V1: usize = 64;
pub const MAX_STATE_WRITES_V1: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateRowRevisionV1(NonZeroU64);

impl StateRowRevisionV1 {
    pub fn new(value: u64) -> Option<Self> {
        NonZeroU64::new(value).map(Self)
    }

    pub fn get(self) -> u64 {
        self.0.get()
    }

    pub fn next(self) -> Option<Self> {
        self.get().checked_add(1).and_then(Self::new)
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateDeclarationDigestV1(String);

impl StateDeclarationDigestV1 {
    pub fn parse(value: impl Into<String>) -> Result<Self, StatefulStateContractErrorV1> {
        let value = value.into();
        if valid_digest(&value) {
            Ok(Self(value))
        } else {
            Err(StatefulStateContractErrorV1::InvalidDefinition)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for StateDeclarationDigestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// An opaque, spec-derived state declaration. Its default may contain private text, so this type
/// intentionally has no `Debug` implementation.
#[derive(Clone, PartialEq, Eq)]
pub struct CompiledStateVariableV1 {
    program_key: String,
    source_spec_digest: String,
    state_schema_digest: String,
    variable_id: String,
    artifact_variable_index: u16,
    scope: StateScopeV1,
    value_type: StateValueTypeV1,
    default_value: StateValueV1,
    declaration_digest: StateDeclarationDigestV1,
}

impl CompiledStateVariableV1 {
    pub(crate) fn from_compiled_bundle_index(
        bundle: &CompiledStatefulBundleV1,
        envelope: &EventEnvelopeV1,
        artifact_variable_index: u16,
    ) -> Result<Self, StatefulStateContractErrorV1> {
        validate_bundle_program_binding(bundle, envelope)?;
        let variable = bundle
            .stateful_artifact()
            .state_schema()
            .variables()
            .get(usize::from(artifact_variable_index))
            .ok_or(StatefulStateContractErrorV1::InvalidDefinition)?;
        validate_state_value(variable.value_type(), variable.initial_value())?;
        Ok(Self {
            program_key: bundle.stateful_artifact().program_key().to_string(),
            source_spec_digest: bundle.stateful_artifact().source().digest().to_hex(),
            state_schema_digest: bundle.state_schema_digest().to_hex(),
            variable_id: variable.id().to_string(),
            artifact_variable_index,
            scope: variable.scope(),
            value_type: variable.value_type().clone(),
            default_value: variable.initial_value().clone(),
            declaration_digest: StateDeclarationDigestV1(variable.declaration_digest().to_hex()),
        })
    }

    #[cfg(test)]
    pub(crate) fn from_program_spec(
        spec: &StatefulSpecV1,
        program: &StatefulProgramIdentityV1,
        variable_id: &str,
    ) -> Result<Self, StatefulStateContractErrorV1> {
        let source_spec_digest = stateful_spec_digest_v1(spec)
            .map_err(|_| StatefulStateContractErrorV1::InvalidDefinition)?;
        if program.program_key() != spec.key
            || program.source_spec_digest() != source_spec_digest
            || !valid_identifier(variable_id)
        {
            return Err(StatefulStateContractErrorV1::ProgramMismatch);
        }
        let variable = spec
            .state_variables
            .iter()
            .find(|candidate| candidate.id == variable_id)
            .ok_or(StatefulStateContractErrorV1::VariableNotFound)?;
        validate_state_value(&variable.value_type, &variable.initial_value)?;

        let source_spec_digest = source_spec_digest.to_hex();
        let state_schema_digest = program.state_schema_digest().as_str().to_string();
        let declaration_digest = declaration_digest(
            &spec.key,
            &variable.id,
            variable.scope,
            &variable.value_type,
            &variable.initial_value,
        );
        Ok(Self {
            program_key: spec.key.clone(),
            source_spec_digest,
            state_schema_digest,
            variable_id: variable.id.clone(),
            artifact_variable_index: spec
                .state_variables
                .iter()
                .position(|candidate| candidate.id == variable.id)
                .and_then(|index| u16::try_from(index).ok())
                .ok_or(StatefulStateContractErrorV1::InvalidDefinition)?,
            scope: variable.scope,
            value_type: variable.value_type.clone(),
            default_value: variable.initial_value.clone(),
            declaration_digest,
        })
    }

    pub fn variable_id(&self) -> &str {
        &self.variable_id
    }

    pub fn artifact_variable_index(&self) -> u16 {
        self.artifact_variable_index
    }

    pub fn scope(&self) -> StateScopeV1 {
        self.scope
    }

    pub fn declaration_digest(&self) -> &StateDeclarationDigestV1 {
        &self.declaration_digest
    }

    pub fn default_value(&self) -> &StateValueV1 {
        &self.default_value
    }

    pub fn accepts(&self, value: &StateValueV1) -> bool {
        validate_state_value(&self.value_type, value).is_ok()
    }

    fn is_bound_to(&self, envelope: &EventEnvelopeV1) -> bool {
        self.program_key == envelope.program().program_key()
            && self.source_spec_digest == envelope.program().source_spec_digest().to_hex()
            && self.state_schema_digest == envelope.program().state_schema_digest().as_str()
    }
}

impl Drop for CompiledStateVariableV1 {
    fn drop(&mut self) {
        zeroize_state_value(&mut self.default_value);
    }
}

/// Opaque compiler-side proof of the complete syntactic state dependency set for the exact
/// workflow selected by an event. R0 keeps construction crate-private until the stateful compiler
/// artifact validator exists.
#[derive(Clone, PartialEq, Eq)]
pub struct CompiledWorkflowDependenciesV1 {
    envelope_digest: String,
    bundle_digest: String,
    compilation_binding_digest: String,
    union_source_map_digest: String,
    workflow_id: String,
    artifact_workflow_index: u16,
    definitions: Vec<CompiledStateVariableV1>,
}

impl CompiledWorkflowDependenciesV1 {
    pub(crate) fn from_compiled_bundle(
        bundle: &CompiledStatefulBundleV1,
        envelope: &EventEnvelopeV1,
    ) -> Result<Self, StatefulStateContractErrorV1> {
        validate_bundle_program_binding(bundle, envelope)?;
        let mut matches = bundle
            .stateful_artifact()
            .workflows()
            .iter()
            .enumerate()
            .filter(|(_, workflow)| triggers_equal(workflow.trigger(), envelope.trigger()));
        let (workflow_index, workflow) = matches
            .next()
            .ok_or(StatefulStateContractErrorV1::WorkflowNotFound)?;
        if matches.next().is_some() {
            return Err(StatefulStateContractErrorV1::InvalidDefinition);
        }
        let read_indices = workflow.dependencies().read_state_variable_indices();
        let write_indices = workflow.dependencies().write_state_variable_indices();
        // Compiler dependencies are the union of both branches. Each selected branch is capped at
        // MAX_STATE_WRITES_V1, but two disjoint valid branches may contribute up to 64 union writes.
        if read_indices.len() > MAX_STATE_READS_V1
            || write_indices.len() > MAX_STATE_READS_V1
            || !strictly_increasing(read_indices)
            || !strictly_increasing(write_indices)
            || write_indices
                .iter()
                .any(|index| read_indices.binary_search(index).is_err())
        {
            return Err(StatefulStateContractErrorV1::ReadBound);
        }
        let definitions = read_indices
            .iter()
            .map(|index| {
                CompiledStateVariableV1::from_compiled_bundle_index(bundle, envelope, *index)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            envelope_digest: crate::event_envelope_digest_v1(envelope)
                .as_str()
                .to_string(),
            bundle_digest: bundle.bundle_digest().to_hex(),
            compilation_binding_digest: bundle.binding_digest().to_hex(),
            union_source_map_digest: bundle.union_source_map_digest().to_hex(),
            workflow_id: workflow.id().to_string(),
            artifact_workflow_index: u16::try_from(workflow_index)
                .map_err(|_| StatefulStateContractErrorV1::InvalidDefinition)?,
            definitions,
        })
    }

    #[cfg(test)]
    pub(crate) fn from_event_spec(
        spec: &StatefulSpecV1,
        envelope: &EventEnvelopeV1,
    ) -> Result<Self, StatefulStateContractErrorV1> {
        let source_spec_digest = stateful_spec_digest_v1(spec)
            .map_err(|_| StatefulStateContractErrorV1::InvalidDefinition)?;
        if spec.key != envelope.program().program_key()
            || source_spec_digest != envelope.program().source_spec_digest()
        {
            return Err(StatefulStateContractErrorV1::ProgramMismatch);
        }
        let workflow = spec
            .stateful_workflows
            .iter()
            .find(|workflow| triggers_equal(&workflow.trigger, envelope.trigger()))
            .ok_or(StatefulStateContractErrorV1::WorkflowNotFound)?;
        let mut ids = BTreeSet::new();
        collect_condition_dependencies(&workflow.condition, &mut ids);
        for branch in [&workflow.on_true, &workflow.on_false] {
            for action in &branch.state_actions {
                ids.insert(action.variable_id.as_str());
                collect_value_dependencies(&action.value, &mut ids);
            }
        }
        let definitions = ids
            .into_iter()
            .map(|id| CompiledStateVariableV1::from_program_spec(spec, envelope.program(), id))
            .collect::<Result<Vec<_>, _>>()?;
        if definitions.len() > MAX_STATE_READS_V1 {
            return Err(StatefulStateContractErrorV1::ReadBound);
        }
        Ok(Self {
            envelope_digest: crate::event_envelope_digest_v1(envelope)
                .as_str()
                .to_string(),
            bundle_digest: envelope.program().bundle_digest().to_string(),
            compilation_binding_digest: envelope.program().compilation_binding_digest().to_string(),
            union_source_map_digest: envelope.program().union_source_map_digest().to_string(),
            workflow_id: workflow.id.clone(),
            artifact_workflow_index: spec
                .stateful_workflows
                .iter()
                .position(|candidate| candidate.id == workflow.id)
                .and_then(|index| u16::try_from(index).ok())
                .ok_or(StatefulStateContractErrorV1::InvalidDefinition)?,
            definitions,
        })
    }

    pub fn workflow_id(&self) -> &str {
        &self.workflow_id
    }

    pub fn artifact_workflow_index(&self) -> u16 {
        self.artifact_workflow_index
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScopedStateAddressV1 {
    Installation,
    Actor {
        actor_user_id: u64,
    },
    Instance {
        instance_id: String,
    },
    ActorInstance {
        actor_user_id: u64,
        instance_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolvedStateKeyV1 {
    tenant_id: String,
    installation_id: String,
    guild_id: u64,
    program_key: String,
    variable_id: String,
    address: ScopedStateAddressV1,
}

impl ResolvedStateKeyV1 {
    fn from_compiled_definition(
        envelope: &EventEnvelopeV1,
        definition: &CompiledStateVariableV1,
    ) -> Result<Self, StatefulStateContractErrorV1> {
        if !definition.is_bound_to(envelope) {
            return Err(StatefulStateContractErrorV1::ProgramMismatch);
        }
        let scope = envelope.scope();
        let address = match definition.scope {
            StateScopeV1::Installation => ScopedStateAddressV1::Installation,
            StateScopeV1::Actor => ScopedStateAddressV1::Actor {
                actor_user_id: scope.actor_user_id(),
            },
            StateScopeV1::Instance => ScopedStateAddressV1::Instance {
                instance_id: scope
                    .instance_id()
                    .ok_or(StatefulStateContractErrorV1::InstanceUnavailable)?
                    .to_string(),
            },
            StateScopeV1::ActorInstance => ScopedStateAddressV1::ActorInstance {
                actor_user_id: scope.actor_user_id(),
                instance_id: scope
                    .instance_id()
                    .ok_or(StatefulStateContractErrorV1::InstanceUnavailable)?
                    .to_string(),
            },
        };
        Ok(Self {
            tenant_id: scope.tenant_id().to_string(),
            installation_id: scope.installation_id().to_string(),
            guild_id: scope.guild_id(),
            program_key: definition.program_key.clone(),
            variable_id: definition.variable_id.clone(),
            address,
        })
    }

    pub fn tenant_id(&self) -> &str {
        &self.tenant_id
    }

    pub fn installation_id(&self) -> &str {
        &self.installation_id
    }

    pub fn guild_id(&self) -> u64 {
        self.guild_id
    }

    pub fn program_key(&self) -> &str {
        &self.program_key
    }

    pub fn variable_id(&self) -> &str {
        &self.variable_id
    }

    pub fn address(&self) -> &ScopedStateAddressV1 {
        &self.address
    }

    pub(crate) fn authorized_for(&self, envelope: &EventEnvelopeV1) -> bool {
        self.tenant_id == envelope.scope().tenant_id()
            && self.installation_id == envelope.scope().installation_id()
            && self.guild_id == envelope.scope().guild_id()
            && self.program_key == envelope.program().program_key()
            && match &self.address {
                ScopedStateAddressV1::Installation => true,
                ScopedStateAddressV1::Actor { actor_user_id } => {
                    *actor_user_id == envelope.scope().actor_user_id()
                }
                ScopedStateAddressV1::Instance { instance_id } => {
                    Some(instance_id.as_str()) == envelope.scope().instance_id()
                }
                ScopedStateAddressV1::ActorInstance {
                    actor_user_id,
                    instance_id,
                } => {
                    *actor_user_id == envelope.scope().actor_user_id()
                        && Some(instance_id.as_str()) == envelope.scope().instance_id()
                }
            }
    }
}

/// A coherent multi-key read request built only from compiled state declarations and one event
/// authority. It intentionally has no `Debug` implementation because defaults may contain text.
#[derive(Clone, PartialEq, Eq)]
pub struct StateSnapshotRequestV1 {
    envelope_digest: String,
    bundle_digest: String,
    workflow_id: String,
    artifact_workflow_index: u16,
    request_digest: String,
    entries: Vec<StateSnapshotRequestEntryV1>,
}

#[derive(Clone, PartialEq, Eq)]
struct StateSnapshotRequestEntryV1 {
    artifact_variable_index: u16,
    key: ResolvedStateKeyV1,
    declaration_digest: StateDeclarationDigestV1,
    value_type: StateValueTypeV1,
    default_value: StateValueV1,
}

impl Drop for StateSnapshotRequestEntryV1 {
    fn drop(&mut self) {
        zeroize_state_value(&mut self.default_value);
    }
}

impl StateSnapshotRequestV1 {
    pub(crate) fn from_compiled_bundle(
        bundle: &CompiledStatefulBundleV1,
        envelope: &EventEnvelopeV1,
    ) -> Result<Self, StatefulStateContractErrorV1> {
        let dependencies = CompiledWorkflowDependenciesV1::from_compiled_bundle(bundle, envelope)?;
        Self::from_dependencies(envelope, &dependencies)
    }

    #[cfg(test)]
    pub(crate) fn from_event(
        envelope: &EventEnvelopeV1,
        dependencies: &CompiledWorkflowDependenciesV1,
    ) -> Result<Self, StatefulStateContractErrorV1> {
        Self::from_dependencies(envelope, dependencies)
    }

    fn from_dependencies(
        envelope: &EventEnvelopeV1,
        dependencies: &CompiledWorkflowDependenciesV1,
    ) -> Result<Self, StatefulStateContractErrorV1> {
        let envelope_digest = crate::event_envelope_digest_v1(envelope)
            .as_str()
            .to_string();
        if dependencies.envelope_digest != envelope_digest
            || dependencies.bundle_digest != envelope.program().bundle_digest()
            || dependencies.compilation_binding_digest
                != envelope.program().compilation_binding_digest()
            || dependencies.union_source_map_digest != envelope.program().union_source_map_digest()
            || dependencies.definitions.len() > MAX_STATE_READS_V1
        {
            return Err(StatefulStateContractErrorV1::ReadBound);
        }
        let mut entries = dependencies
            .definitions
            .iter()
            .map(|definition| {
                Ok(StateSnapshotRequestEntryV1 {
                    artifact_variable_index: definition.artifact_variable_index,
                    key: ResolvedStateKeyV1::from_compiled_definition(envelope, definition)?,
                    declaration_digest: definition.declaration_digest.clone(),
                    value_type: definition.value_type.clone(),
                    default_value: definition.default_value.clone(),
                })
            })
            .collect::<Result<Vec<_>, StatefulStateContractErrorV1>>()?;
        entries.sort_by(|left, right| left.key.cmp(&right.key));
        if entries.windows(2).any(|pair| pair[0].key == pair[1].key) {
            return Err(StatefulStateContractErrorV1::DuplicateKey);
        }
        let request_digest = snapshot_request_digest(
            &envelope_digest,
            &dependencies.bundle_digest,
            &dependencies.compilation_binding_digest,
            &dependencies.union_source_map_digest,
            &dependencies.workflow_id,
            dependencies.artifact_workflow_index,
            &entries,
        );
        Ok(Self {
            envelope_digest,
            bundle_digest: dependencies.bundle_digest.clone(),
            workflow_id: dependencies.workflow_id.clone(),
            artifact_workflow_index: dependencies.artifact_workflow_index,
            request_digest,
            entries,
        })
    }

    pub fn envelope_digest(&self) -> &str {
        &self.envelope_digest
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn bundle_digest(&self) -> &str {
        &self.bundle_digest
    }

    pub fn workflow_id(&self) -> &str {
        &self.workflow_id
    }

    pub fn artifact_workflow_index(&self) -> u16 {
        self.artifact_workflow_index
    }

    pub fn request_digest(&self) -> &str {
        &self.request_digest
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn entries(
        &self,
    ) -> impl Iterator<
        Item = (
            u16,
            &ResolvedStateKeyV1,
            &StateDeclarationDigestV1,
            &StateValueV1,
        ),
    > {
        self.entries.iter().map(|entry| {
            (
                entry.artifact_variable_index,
                &entry.key,
                &entry.declaration_digest,
                &entry.default_value,
            )
        })
    }

    pub(crate) fn exact_snapshot(&self, snapshot: &StateSnapshotV1) -> bool {
        snapshot.envelope_digest == self.envelope_digest
            && snapshot.bundle_digest == self.bundle_digest
            && snapshot.workflow_id == self.workflow_id
            && snapshot.artifact_workflow_index == self.artifact_workflow_index
            && snapshot.request_digest == self.request_digest
            && snapshot.reads.len() == self.entries.len()
            && self
                .entries
                .iter()
                .zip(&snapshot.reads)
                .all(|(entry, read)| {
                    entry.key == read.key
                        && entry.declaration_digest == read.declaration_digest
                        && validate_state_value(&entry.value_type, &read.value).is_ok()
                        && (read.revision.is_some() || read.value == entry.default_value)
                })
    }

    pub(crate) fn artifact_index_for_key(&self, key: &ResolvedStateKeyV1) -> Option<u16> {
        self.entries
            .binary_search_by(|entry| entry.key.cmp(key))
            .ok()
            .and_then(|index| self.entries.get(index))
            .map(|entry| entry.artifact_variable_index)
    }
}

/// A read result intentionally has no `Debug` implementation because its value may contain text.
#[derive(Clone, PartialEq, Eq)]
pub struct ResolvedStateReadV1 {
    key: ResolvedStateKeyV1,
    revision: Option<StateRowRevisionV1>,
    declaration_digest: StateDeclarationDigestV1,
    value: StateValueV1,
}

impl ResolvedStateReadV1 {
    pub fn key(&self) -> &ResolvedStateKeyV1 {
        &self.key
    }

    pub fn revision(&self) -> Option<StateRowRevisionV1> {
        self.revision
    }

    pub fn declaration_digest(&self) -> &StateDeclarationDigestV1 {
        &self.declaration_digest
    }

    pub fn value(&self) -> &StateValueV1 {
        &self.value
    }

    pub(crate) fn from_snapshot(
        key: ResolvedStateKeyV1,
        revision: Option<StateRowRevisionV1>,
        declaration_digest: StateDeclarationDigestV1,
        value: StateValueV1,
    ) -> Self {
        Self {
            key,
            revision,
            declaration_digest,
            value,
        }
    }
}

impl Drop for ResolvedStateReadV1 {
    fn drop(&mut self) {
        zeroize_state_value(&mut self.value);
    }
}

/// A validated explicit `Set`. Even when `before == after`, committing it advances the row
/// revision. It intentionally has no `Debug` implementation because values may contain text.
#[derive(Clone, PartialEq, Eq)]
pub struct ResolvedStateWriteV1 {
    key: ResolvedStateKeyV1,
    expected_revision: Option<StateRowRevisionV1>,
    declaration_digest: StateDeclarationDigestV1,
    before: StateValueV1,
    after: StateValueV1,
    state_action_node_id: String,
    source_ordinal: u16,
}

impl ResolvedStateWriteV1 {
    pub(crate) fn set(
        definition: &CompiledStateVariableV1,
        read: &ResolvedStateReadV1,
        state_action_node_id: impl Into<String>,
        source_ordinal: u16,
        after: StateValueV1,
    ) -> Result<Self, StatefulStateContractErrorV1> {
        if definition.variable_id != read.key.variable_id
            || definition.program_key != read.key.program_key
            || definition.declaration_digest != read.declaration_digest
            || !definition.accepts(&after)
        {
            return Err(StatefulStateContractErrorV1::DefinitionMismatch);
        }
        let state_action_node_id = state_action_node_id.into();
        if !valid_identifier(&state_action_node_id) {
            return Err(StatefulStateContractErrorV1::InvalidStateAction);
        }
        Ok(Self {
            key: read.key.clone(),
            expected_revision: read.revision,
            declaration_digest: read.declaration_digest.clone(),
            before: read.value.clone(),
            after,
            state_action_node_id,
            source_ordinal,
        })
    }

    pub fn key(&self) -> &ResolvedStateKeyV1 {
        &self.key
    }

    pub fn expected_revision(&self) -> Option<StateRowRevisionV1> {
        self.expected_revision
    }

    pub fn declaration_digest(&self) -> &StateDeclarationDigestV1 {
        &self.declaration_digest
    }

    pub fn before(&self) -> &StateValueV1 {
        &self.before
    }

    pub fn after(&self) -> &StateValueV1 {
        &self.after
    }

    pub fn state_action_node_id(&self) -> &str {
        &self.state_action_node_id
    }

    pub fn source_ordinal(&self) -> u16 {
        self.source_ordinal
    }
}

impl Drop for ResolvedStateWriteV1 {
    fn drop(&mut self) {
        zeroize_state_value(&mut self.before);
        zeroize_state_value(&mut self.after);
    }
}

/// A coherent snapshot intentionally has no `Debug` implementation because values may contain
/// text.
#[derive(Clone, PartialEq, Eq)]
pub struct StateSnapshotV1 {
    envelope_digest: String,
    bundle_digest: String,
    workflow_id: String,
    artifact_workflow_index: u16,
    request_digest: String,
    reads: Vec<ResolvedStateReadV1>,
}

impl StateSnapshotV1 {
    pub fn envelope_digest(&self) -> &str {
        &self.envelope_digest
    }

    pub fn reads(&self) -> &[ResolvedStateReadV1] {
        &self.reads
    }

    pub fn bundle_digest(&self) -> &str {
        &self.bundle_digest
    }

    pub fn workflow_id(&self) -> &str {
        &self.workflow_id
    }

    pub fn artifact_workflow_index(&self) -> u16 {
        self.artifact_workflow_index
    }

    pub fn request_digest(&self) -> &str {
        &self.request_digest
    }

    pub fn into_reads(mut self) -> Vec<ResolvedStateReadV1> {
        std::mem::take(&mut self.reads)
    }

    pub(crate) fn from_request(
        request: &StateSnapshotRequestV1,
        reads: Vec<ResolvedStateReadV1>,
    ) -> Self {
        Self {
            envelope_digest: request.envelope_digest.clone(),
            bundle_digest: request.bundle_digest.clone(),
            workflow_id: request.workflow_id.clone(),
            artifact_workflow_index: request.artifact_workflow_index,
            request_digest: request.request_digest.clone(),
            reads,
        }
    }

    #[cfg(test)]
    pub(crate) fn reads_mut_for_test(&mut self) -> &mut Vec<ResolvedStateReadV1> {
        &mut self.reads
    }
}

#[cfg(test)]
impl ResolvedStateReadV1 {
    pub(crate) fn replace_value_for_test(&mut self, value: StateValueV1) {
        self.value = value;
    }

    pub(crate) fn replace_revision_for_test(&mut self, revision: Option<StateRowRevisionV1>) {
        self.revision = revision;
    }
}

pub(crate) fn zeroize_state_value(value: &mut StateValueV1) {
    if let StateValueV1::Text { value } = value {
        value.zeroize();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StatefulStateContractErrorV1 {
    #[error("state variable definition is invalid")]
    InvalidDefinition,
    #[error("state variable definition does not match the stateful program")]
    ProgramMismatch,
    #[error("state variable was not found in the stateful program")]
    VariableNotFound,
    #[error("stateful workflow was not found for the verified event")]
    WorkflowNotFound,
    #[error("state scope requires an instance route")]
    InstanceUnavailable,
    #[error("state snapshot read count is invalid")]
    ReadBound,
    #[error("state snapshot contains a duplicate resolved key")]
    DuplicateKey,
    #[error("state value does not satisfy its compiled definition")]
    DefinitionMismatch,
    #[error("state action identity is invalid")]
    InvalidStateAction,
}

fn declaration_digest(
    program_key: &str,
    variable_id: &str,
    scope: StateScopeV1,
    value_type: &StateValueTypeV1,
    default_value: &StateValueV1,
) -> StateDeclarationDigestV1 {
    let mut hasher = Sha256::new();
    hasher.update(STATE_DECLARATION_DOMAIN_V1);
    append_string(&mut hasher, program_key);
    append_string(&mut hasher, variable_id);
    hasher.update([match scope {
        StateScopeV1::Installation => 0,
        StateScopeV1::Actor => 1,
        StateScopeV1::Instance => 2,
        StateScopeV1::ActorInstance => 3,
    }]);
    append_state_type(&mut hasher, value_type);
    append_state_value(&mut hasher, default_value);
    StateDeclarationDigestV1(lower_hex(hasher.finalize().as_slice()))
}

fn validate_bundle_program_binding(
    bundle: &CompiledStatefulBundleV1,
    envelope: &EventEnvelopeV1,
) -> Result<(), StatefulStateContractErrorV1> {
    let program = envelope.program();
    if program.program_key() != bundle.stateful_artifact().program_key()
        || program.source_spec_digest() != bundle.stateful_artifact().source().digest()
        || program.bundle_digest() != bundle.bundle_digest().to_hex()
        || program.compilation_binding_digest() != bundle.binding_digest().to_hex()
        || program.union_source_map_digest() != bundle.union_source_map_digest().to_hex()
        || program.stateful_artifact_digest().as_str() != bundle.stateful_artifact_digest().to_hex()
        || program.state_schema_digest().as_str() != bundle.state_schema_digest().to_hex()
        || program.compiler_revision()
            != automation_stateful_compiler::STATEFUL_ARTIFACT_COMPILER_REVISION_V1
    {
        return Err(StatefulStateContractErrorV1::ProgramMismatch);
    }
    Ok(())
}

fn strictly_increasing(indices: &[u16]) -> bool {
    indices.windows(2).all(|pair| pair[0] < pair[1])
}

fn snapshot_request_digest(
    envelope_digest: &str,
    bundle_digest: &str,
    compilation_binding_digest: &str,
    union_source_map_digest: &str,
    workflow_id: &str,
    artifact_workflow_index: u16,
    entries: &[StateSnapshotRequestEntryV1],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(SNAPSHOT_REQUEST_DOMAIN_V1);
    append_string(&mut hasher, envelope_digest);
    append_string(&mut hasher, bundle_digest);
    append_string(&mut hasher, compilation_binding_digest);
    append_string(&mut hasher, union_source_map_digest);
    append_string(&mut hasher, workflow_id);
    hasher.update(artifact_workflow_index.to_be_bytes());
    hasher.update((entries.len() as u64).to_be_bytes());
    for entry in entries {
        hasher.update(entry.artifact_variable_index.to_be_bytes());
        append_state_key(&mut hasher, &entry.key);
        append_string(&mut hasher, entry.declaration_digest.as_str());
        append_state_type(&mut hasher, &entry.value_type);
        append_state_value(&mut hasher, &entry.default_value);
    }
    lower_hex(hasher.finalize().as_slice())
}

pub(crate) fn append_state_value(hasher: &mut Sha256, value: &StateValueV1) {
    match value {
        StateValueV1::Bool { value } => {
            hasher.update([0]);
            hasher.update([u8::from(*value)]);
        }
        StateValueV1::Integer { value } => {
            hasher.update([1]);
            hasher.update(value.to_be_bytes());
        }
        StateValueV1::Text { value } => {
            hasher.update([2]);
            append_string(hasher, value);
        }
    }
}

pub(crate) fn append_state_type(hasher: &mut Sha256, value_type: &StateValueTypeV1) {
    match value_type {
        StateValueTypeV1::Bool => hasher.update([0]),
        StateValueTypeV1::Integer { min, max } => {
            hasher.update([1]);
            hasher.update(min.to_be_bytes());
            hasher.update(max.to_be_bytes());
        }
        StateValueTypeV1::Text { max_utf8_bytes } => {
            hasher.update([2]);
            hasher.update(max_utf8_bytes.to_be_bytes());
        }
    }
}

pub(crate) fn validate_state_value(
    value_type: &StateValueTypeV1,
    value: &StateValueV1,
) -> Result<(), StatefulStateContractErrorV1> {
    if !value_type.accepts(value) {
        return Err(StatefulStateContractErrorV1::DefinitionMismatch);
    }
    if let StateValueV1::Text { value } = value {
        if value.contains('\0')
            || value.len() > MAX_STATE_TEXT_BYTES_V1
            || value.encode_utf16().count() > MAX_STATE_TEXT_UTF16_UNITS_V1
        {
            return Err(StatefulStateContractErrorV1::DefinitionMismatch);
        }
    }
    Ok(())
}

pub(crate) fn append_state_key(hasher: &mut Sha256, key: &ResolvedStateKeyV1) {
    append_string(hasher, key.tenant_id());
    append_string(hasher, key.installation_id());
    hasher.update(key.guild_id().to_be_bytes());
    append_string(hasher, key.program_key());
    append_string(hasher, key.variable_id());
    match key.address() {
        ScopedStateAddressV1::Installation => hasher.update([0]),
        ScopedStateAddressV1::Actor { actor_user_id } => {
            hasher.update([1]);
            hasher.update(actor_user_id.to_be_bytes());
        }
        ScopedStateAddressV1::Instance { instance_id } => {
            hasher.update([2]);
            append_string(hasher, instance_id);
        }
        ScopedStateAddressV1::ActorInstance {
            actor_user_id,
            instance_id,
        } => {
            hasher.update([3]);
            hasher.update(actor_user_id.to_be_bytes());
            append_string(hasher, instance_id);
        }
    }
}

pub(crate) fn append_revision(hasher: &mut Sha256, revision: Option<StateRowRevisionV1>) {
    match revision {
        None => hasher.update([0]),
        Some(revision) => {
            hasher.update([1]);
            hasher.update(revision.get().to_be_bytes());
        }
    }
}

pub(crate) fn append_string(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
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

pub(crate) fn validate_read_write_sets(
    reads: &[ResolvedStateReadV1],
    writes: &[ResolvedStateWriteV1],
) -> bool {
    reads.len() <= MAX_STATE_READS_V1
        && writes.len() <= MAX_STATE_WRITES_V1
        && reads.windows(2).all(|pair| pair[0].key < pair[1].key)
        && writes.windows(2).all(|pair| pair[0].key < pair[1].key)
        && writes.iter().all(|write| {
            reads
                .binary_search_by(|read| read.key.cmp(&write.key))
                .ok()
                .and_then(|index| reads.get(index))
                .is_some_and(|read| {
                    read.revision == write.expected_revision
                        && read.declaration_digest == write.declaration_digest
                        && read.value == write.before
                })
        })
        && writes
            .iter()
            .map(|write| write.state_action_node_id.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            == writes.len()
        && reads
            .iter()
            .map(|read| &read.key)
            .collect::<BTreeSet<_>>()
            .len()
            == reads.len()
}

fn triggers_equal(left: &TriggerV1, right: &TriggerV1) -> bool {
    match (left, right) {
        (
            TriggerV1::ButtonClick { trigger_id: left },
            TriggerV1::ButtonClick { trigger_id: right },
        ) => left == right,
        (TriggerV1::ModalSubmit { modal_id: left }, TriggerV1::ModalSubmit { modal_id: right }) => {
            left == right
        }
        (
            TriggerV1::InstanceAction { action_id: left },
            TriggerV1::InstanceAction { action_id: right },
        ) => left == right,
        _ => false,
    }
}

#[cfg(test)]
fn collect_condition_dependencies<'a>(
    condition: &'a StatefulConditionExprV1,
    ids: &mut BTreeSet<&'a str>,
) {
    match condition {
        StatefulConditionExprV1::Always
        | StatefulConditionExprV1::InputNonEmpty { .. }
        | StatefulConditionExprV1::InputEquals { .. } => {}
        StatefulConditionExprV1::StateEquals { variable_id, value } => {
            ids.insert(variable_id);
            collect_value_dependencies(value, ids);
        }
        StatefulConditionExprV1::IntegerCompare { left, right, .. } => {
            collect_value_dependencies(left, ids);
            collect_value_dependencies(right, ids);
        }
        StatefulConditionExprV1::All { conditions }
        | StatefulConditionExprV1::Any { conditions } => {
            for condition in conditions {
                collect_condition_dependencies(condition, ids);
            }
        }
        StatefulConditionExprV1::Not { condition } => {
            collect_condition_dependencies(condition, ids);
        }
    }
}

#[cfg(test)]
fn collect_value_dependencies<'a>(
    expression: &'a StatefulValueExprV1,
    ids: &mut BTreeSet<&'a str>,
) {
    match expression {
        StatefulValueExprV1::Literal { .. } | StatefulValueExprV1::InputText { .. } => {}
        StatefulValueExprV1::State { variable_id } => {
            ids.insert(variable_id);
        }
        StatefulValueExprV1::CheckedAdd { left, right }
        | StatefulValueExprV1::CheckedSub { left, right } => {
            collect_value_dependencies(left, ids);
            collect_value_dependencies(right, ids);
        }
    }
}

pub(crate) fn scope_matches(
    key: &ResolvedStateKeyV1,
    scope: &EventEnvelopeScopeV1,
    program_key: &str,
) -> bool {
    key.tenant_id == scope.tenant_id()
        && key.installation_id == scope.installation_id()
        && key.guild_id == scope.guild_id()
        && key.program_key == program_key
}
