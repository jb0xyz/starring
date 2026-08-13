use automation_spec::{
    ActionNodeV1, ActionSourceMapV1, AutomationRuleSetIdentityV1, TriggerV1, WorkflowSourceMapV1,
};
use automation_state::InteractionRuleSet;
use automation_stateful_spec::{
    StateScopeV1, StateSetNodeV1, StateValueTypeV1, StateValueV1, StatefulBranchV1,
    StatefulConditionExprV1, StatefulResponseNodeV1, StatefulSpecDigestV1, StatefulSpecV1,
};
use serde::Serialize;

use crate::digest::{
    StateDeclarationDigestV1, StatefulArtifactDigestV1, StatefulBundleDigestV1,
    StatefulCompilationBindingDigestV1, StatefulStateSchemaDigestV1,
    StatefulUnionSourceMapDigestV1,
};

pub const STATEFUL_ARTIFACT_COMPILER_REVISION_V1: u32 = 1;
pub const STATEFUL_STATE_SCHEMA_VERSION_V1: u16 = 1;
pub const STATEFUL_STATE_SCHEMA_KIND_V1: &str = "starring.compiled-state-schema.v1";
pub const STATEFUL_ARTIFACT_SCHEMA_VERSION_V1: u16 = 1;
pub const STATEFUL_ARTIFACT_KIND_V1: &str = "starring.stateful-artifact.v1";
pub const STATEFUL_UNION_SOURCE_MAP_SCHEMA_VERSION_V1: u16 = 1;
pub const STATEFUL_UNION_SOURCE_MAP_KIND_V1: &str = "starring.stateful-union-source-map.v1";
pub const STATEFUL_COMPILATION_BINDING_FORMAT_VERSION_V1: u16 = 1;
pub const STATEFUL_COMPILATION_BINDING_KIND_V1: &str = "starring.stateful-compilation-binding.v1";
pub const STATEFUL_BUNDLE_FORMAT_VERSION_V1: u16 = 1;
pub const STATEFUL_BUNDLE_KIND_V1: &str = "starring.stateful-compiled-bundle.v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulSourceSpecIdentityV1 {
    schema_version: u16,
    digest: StatefulSpecDigestV1,
}

impl StatefulSourceSpecIdentityV1 {
    pub fn schema_version(&self) -> u16 {
        self.schema_version
    }

    pub fn digest(&self) -> StatefulSpecDigestV1 {
        self.digest
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledStateVariableV1 {
    id: String,
    scope: StateScopeV1,
    value_type: StateValueTypeV1,
    initial_value: StateValueV1,
    declaration_digest: StateDeclarationDigestV1,
}

impl CompiledStateVariableV1 {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn scope(&self) -> StateScopeV1 {
        self.scope
    }

    pub fn value_type(&self) -> &StateValueTypeV1 {
        &self.value_type
    }

    pub fn initial_value(&self) -> &StateValueV1 {
        &self.initial_value
    }

    pub fn declaration_digest(&self) -> StateDeclarationDigestV1 {
        self.declaration_digest
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledStateSchemaV1 {
    schema_version: u16,
    kind: String,
    compiler_revision: u32,
    program_key: String,
    variables: Vec<CompiledStateVariableV1>,
}

impl CompiledStateSchemaV1 {
    pub fn schema_version(&self) -> u16 {
        self.schema_version
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn compiler_revision(&self) -> u32 {
        self.compiler_revision
    }

    pub fn program_key(&self) -> &str {
        &self.program_key
    }

    pub fn variables(&self) -> &[CompiledStateVariableV1] {
        &self.variables
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledWorkflowDependenciesV1 {
    read_state_variable_indices: Vec<u16>,
    write_state_variable_indices: Vec<u16>,
}

impl CompiledWorkflowDependenciesV1 {
    pub fn read_state_variable_indices(&self) -> &[u16] {
        &self.read_state_variable_indices
    }

    pub fn write_state_variable_indices(&self) -> &[u16] {
        &self.write_state_variable_indices
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompiledAcknowledgementStrategyV1 {
    DeferEphemeralBeforeCommit,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledStatefulBranchV1 {
    state_actions: Vec<StateSetNodeV1>,
    effects: Vec<ActionNodeV1>,
    response: StatefulResponseNodeV1,
}

impl CompiledStatefulBranchV1 {
    pub fn state_actions(&self) -> &[StateSetNodeV1] {
        &self.state_actions
    }

    pub fn effects(&self) -> &[ActionNodeV1] {
        &self.effects
    }

    pub fn response(&self) -> &StatefulResponseNodeV1 {
        &self.response
    }
}

impl From<&StatefulBranchV1> for CompiledStatefulBranchV1 {
    fn from(branch: &StatefulBranchV1) -> Self {
        Self {
            state_actions: branch.state_actions.clone(),
            effects: branch.effects.clone(),
            response: branch.response.clone(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledStatefulWorkflowV1 {
    id: String,
    trigger: TriggerV1,
    condition: StatefulConditionExprV1,
    dependencies: CompiledWorkflowDependenciesV1,
    acknowledgement: CompiledAcknowledgementStrategyV1,
    on_true: CompiledStatefulBranchV1,
    on_false: CompiledStatefulBranchV1,
}

impl CompiledStatefulWorkflowV1 {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn trigger(&self) -> &TriggerV1 {
        &self.trigger
    }

    pub fn condition(&self) -> &StatefulConditionExprV1 {
        &self.condition
    }

    pub fn dependencies(&self) -> &CompiledWorkflowDependenciesV1 {
        &self.dependencies
    }

    pub fn acknowledgement(&self) -> CompiledAcknowledgementStrategyV1 {
        self.acknowledgement
    }

    pub fn on_true(&self) -> &CompiledStatefulBranchV1 {
        &self.on_true
    }

    pub fn on_false(&self) -> &CompiledStatefulBranchV1 {
        &self.on_false
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledStatefulArtifactV1 {
    schema_version: u16,
    kind: String,
    compiler_revision: u32,
    source: StatefulSourceSpecIdentityV1,
    program_key: String,
    state_schema: CompiledStateSchemaV1,
    state_schema_digest: StatefulStateSchemaDigestV1,
    workflows: Vec<CompiledStatefulWorkflowV1>,
}

impl CompiledStatefulArtifactV1 {
    pub fn source(&self) -> &StatefulSourceSpecIdentityV1 {
        &self.source
    }

    pub fn program_key(&self) -> &str {
        &self.program_key
    }

    pub fn state_schema(&self) -> &CompiledStateSchemaV1 {
        &self.state_schema
    }

    pub fn state_schema_digest(&self) -> StatefulStateSchemaDigestV1 {
        self.state_schema_digest
    }

    pub fn workflows(&self) -> &[CompiledStatefulWorkflowV1] {
        &self.workflows
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulArtifactIdentityV1 {
    schema_version: u16,
    kind: String,
    compiler_revision: u32,
    digest: StatefulArtifactDigestV1,
}

impl StatefulArtifactIdentityV1 {
    pub fn digest(&self) -> StatefulArtifactDigestV1 {
        self.digest
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulStateSchemaIdentityV1 {
    schema_version: u16,
    kind: String,
    digest: StatefulStateSchemaDigestV1,
}

impl StatefulStateSchemaIdentityV1 {
    pub fn digest(&self) -> StatefulStateSchemaDigestV1 {
        self.digest
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulStateVariableSourceMapV1 {
    variable_id: String,
    source_variable_index: u32,
    artifact_variable_index: u32,
    declaration_digest: StateDeclarationDigestV1,
}

impl StatefulStateVariableSourceMapV1 {
    pub fn variable_id(&self) -> &str {
        &self.variable_id
    }

    pub fn source_variable_index(&self) -> u32 {
        self.source_variable_index
    }

    pub fn artifact_variable_index(&self) -> u32 {
        self.artifact_variable_index
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulNodeSourceMapV1 {
    node_id: String,
    source_node_index: u32,
    artifact_node_index: u32,
    execution_ordinal: u32,
}

impl StatefulNodeSourceMapV1 {
    pub fn node_id(&self) -> &str {
        &self.node_id
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulResponseSourceMapV1 {
    node_id: String,
    execution_ordinal: u32,
}

impl StatefulResponseSourceMapV1 {
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub fn execution_ordinal(&self) -> u32 {
        self.execution_ordinal
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulBranchSourceMapV1 {
    implicit_acknowledgement_ordinal: u32,
    state_actions: Vec<StatefulNodeSourceMapV1>,
    effects: Vec<StatefulNodeSourceMapV1>,
    response: StatefulResponseSourceMapV1,
}

impl StatefulBranchSourceMapV1 {
    pub fn implicit_acknowledgement_ordinal(&self) -> u32 {
        self.implicit_acknowledgement_ordinal
    }

    pub fn state_actions(&self) -> &[StatefulNodeSourceMapV1] {
        &self.state_actions
    }

    pub fn effects(&self) -> &[StatefulNodeSourceMapV1] {
        &self.effects
    }

    pub fn response(&self) -> &StatefulResponseSourceMapV1 {
        &self.response
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulStatelessWorkflowSourceMapV1 {
    workflow: WorkflowSourceMapV1,
}

impl StatefulStatelessWorkflowSourceMapV1 {
    pub fn workflow(&self) -> &WorkflowSourceMapV1 {
        &self.workflow
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulWorkflowSourceMapV1 {
    workflow_id: String,
    source_workflow_index: u32,
    artifact_workflow_index: u32,
    on_true: StatefulBranchSourceMapV1,
    on_false: StatefulBranchSourceMapV1,
}

impl StatefulWorkflowSourceMapV1 {
    pub fn workflow_id(&self) -> &str {
        &self.workflow_id
    }

    pub fn source_workflow_index(&self) -> u32 {
        self.source_workflow_index
    }

    pub fn artifact_workflow_index(&self) -> u32 {
        self.artifact_workflow_index
    }

    pub fn on_true(&self) -> &StatefulBranchSourceMapV1 {
        &self.on_true
    }

    pub fn on_false(&self) -> &StatefulBranchSourceMapV1 {
        &self.on_false
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulUnionSourceMapV1 {
    schema_version: u16,
    kind: String,
    compiler_revision: u32,
    source: StatefulSourceSpecIdentityV1,
    filtered_legacy_target: AutomationRuleSetIdentityV1,
    stateful_artifact: StatefulArtifactIdentityV1,
    state_schema: StatefulStateSchemaIdentityV1,
    stateless_workflows: Vec<StatefulStatelessWorkflowSourceMapV1>,
    state_variables: Vec<StatefulStateVariableSourceMapV1>,
    stateful_workflows: Vec<StatefulWorkflowSourceMapV1>,
}

impl StatefulUnionSourceMapV1 {
    pub fn stateless_workflows(&self) -> &[StatefulStatelessWorkflowSourceMapV1] {
        &self.stateless_workflows
    }

    pub fn state_variables(&self) -> &[StatefulStateVariableSourceMapV1] {
        &self.state_variables
    }

    pub fn stateful_workflows(&self) -> &[StatefulWorkflowSourceMapV1] {
        &self.stateful_workflows
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulUnionSourceMapIdentityV1 {
    schema_version: u16,
    kind: String,
    digest: StatefulUnionSourceMapDigestV1,
}

impl StatefulUnionSourceMapIdentityV1 {
    pub fn digest(&self) -> StatefulUnionSourceMapDigestV1 {
        self.digest
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulCompilationBindingV1 {
    format_version: u16,
    kind: String,
    compiler_revision: u32,
    source: StatefulSourceSpecIdentityV1,
    filtered_legacy_target: AutomationRuleSetIdentityV1,
    stateful_artifact: StatefulArtifactIdentityV1,
    state_schema: StatefulStateSchemaIdentityV1,
    source_map: StatefulUnionSourceMapIdentityV1,
}

impl StatefulCompilationBindingV1 {
    pub fn source(&self) -> &StatefulSourceSpecIdentityV1 {
        &self.source
    }

    pub fn filtered_legacy_target(&self) -> &AutomationRuleSetIdentityV1 {
        &self.filtered_legacy_target
    }

    pub fn stateful_artifact(&self) -> &StatefulArtifactIdentityV1 {
        &self.stateful_artifact
    }

    pub fn state_schema(&self) -> &StatefulStateSchemaIdentityV1 {
        &self.state_schema
    }

    pub fn source_map(&self) -> &StatefulUnionSourceMapIdentityV1 {
        &self.source_map
    }
}

/// Immutable compiler output. Generated fields have no public constructors and are not
/// independently deserializable. The only decoder recompiles the embedded validated source and
/// compares every generated component before returning this type.
#[derive(Clone, PartialEq, Eq)]
pub struct CompiledStatefulBundleV1 {
    pub(crate) source_spec: StatefulSpecV1,
    pub(crate) filtered_legacy_ruleset: InteractionRuleSet,
    pub(crate) filtered_legacy_target: AutomationRuleSetIdentityV1,
    pub(crate) stateful_artifact: CompiledStatefulArtifactV1,
    pub(crate) stateful_artifact_digest: StatefulArtifactDigestV1,
    pub(crate) state_schema_digest: StatefulStateSchemaDigestV1,
    pub(crate) union_source_map: StatefulUnionSourceMapV1,
    pub(crate) union_source_map_digest: StatefulUnionSourceMapDigestV1,
    pub(crate) binding: StatefulCompilationBindingV1,
    pub(crate) binding_digest: StatefulCompilationBindingDigestV1,
    pub(crate) bundle_digest: StatefulBundleDigestV1,
}

impl CompiledStatefulBundleV1 {
    pub fn source_spec(&self) -> &StatefulSpecV1 {
        &self.source_spec
    }

    pub fn filtered_legacy_ruleset(&self) -> &InteractionRuleSet {
        &self.filtered_legacy_ruleset
    }

    pub fn filtered_legacy_target(&self) -> &AutomationRuleSetIdentityV1 {
        &self.filtered_legacy_target
    }

    pub fn stateful_artifact(&self) -> &CompiledStatefulArtifactV1 {
        &self.stateful_artifact
    }

    pub fn stateful_artifact_digest(&self) -> StatefulArtifactDigestV1 {
        self.stateful_artifact_digest
    }

    pub fn state_schema_digest(&self) -> StatefulStateSchemaDigestV1 {
        self.state_schema_digest
    }

    pub fn union_source_map(&self) -> &StatefulUnionSourceMapV1 {
        &self.union_source_map
    }

    pub fn union_source_map_digest(&self) -> StatefulUnionSourceMapDigestV1 {
        self.union_source_map_digest
    }

    pub fn binding(&self) -> &StatefulCompilationBindingV1 {
        &self.binding
    }

    pub fn binding_digest(&self) -> StatefulCompilationBindingDigestV1 {
        self.binding_digest
    }

    pub fn bundle_digest(&self) -> StatefulBundleDigestV1 {
        self.bundle_digest
    }
}

pub(crate) fn source_identity_v1(
    spec: &StatefulSpecV1,
    source_digest: StatefulSpecDigestV1,
) -> StatefulSourceSpecIdentityV1 {
    StatefulSourceSpecIdentityV1 {
        schema_version: spec.schema_version,
        digest: source_digest,
    }
}

pub(crate) fn compiled_state_schema_v1(
    spec: &StatefulSpecV1,
    variables: Vec<CompiledStateVariableV1>,
) -> CompiledStateSchemaV1 {
    CompiledStateSchemaV1 {
        schema_version: STATEFUL_STATE_SCHEMA_VERSION_V1,
        kind: STATEFUL_STATE_SCHEMA_KIND_V1.to_string(),
        compiler_revision: STATEFUL_ARTIFACT_COMPILER_REVISION_V1,
        program_key: spec.key.clone(),
        variables,
    }
}

pub(crate) fn compiled_stateful_artifact_v1(
    spec: &StatefulSpecV1,
    source: StatefulSourceSpecIdentityV1,
    state_schema: CompiledStateSchemaV1,
    state_schema_digest: StatefulStateSchemaDigestV1,
    workflows: Vec<CompiledStatefulWorkflowV1>,
) -> CompiledStatefulArtifactV1 {
    CompiledStatefulArtifactV1 {
        schema_version: STATEFUL_ARTIFACT_SCHEMA_VERSION_V1,
        kind: STATEFUL_ARTIFACT_KIND_V1.to_string(),
        compiler_revision: STATEFUL_ARTIFACT_COMPILER_REVISION_V1,
        source: source.clone(),
        program_key: spec.key.clone(),
        state_schema: state_schema.clone(),
        state_schema_digest,
        workflows,
    }
}

pub(crate) fn compiled_variable_v1(
    id: String,
    scope: StateScopeV1,
    value_type: StateValueTypeV1,
    initial_value: StateValueV1,
    declaration_digest: StateDeclarationDigestV1,
) -> CompiledStateVariableV1 {
    CompiledStateVariableV1 {
        id,
        scope,
        value_type,
        initial_value,
        declaration_digest,
    }
}

pub(crate) fn compiled_workflow_v1(
    id: String,
    trigger: TriggerV1,
    condition: StatefulConditionExprV1,
    dependencies: CompiledWorkflowDependenciesV1,
    on_true: CompiledStatefulBranchV1,
    on_false: CompiledStatefulBranchV1,
) -> CompiledStatefulWorkflowV1 {
    CompiledStatefulWorkflowV1 {
        id,
        trigger,
        condition,
        dependencies,
        acknowledgement: CompiledAcknowledgementStrategyV1::DeferEphemeralBeforeCommit,
        on_true,
        on_false,
    }
}

pub(crate) fn compiled_dependencies_v1(
    reads: Vec<u16>,
    writes: Vec<u16>,
) -> CompiledWorkflowDependenciesV1 {
    CompiledWorkflowDependenciesV1 {
        read_state_variable_indices: reads,
        write_state_variable_indices: writes,
    }
}

pub(crate) fn artifact_identity_v1(digest: StatefulArtifactDigestV1) -> StatefulArtifactIdentityV1 {
    StatefulArtifactIdentityV1 {
        schema_version: STATEFUL_ARTIFACT_SCHEMA_VERSION_V1,
        kind: STATEFUL_ARTIFACT_KIND_V1.to_string(),
        compiler_revision: STATEFUL_ARTIFACT_COMPILER_REVISION_V1,
        digest,
    }
}

pub(crate) fn schema_identity_v1(
    digest: StatefulStateSchemaDigestV1,
) -> StatefulStateSchemaIdentityV1 {
    StatefulStateSchemaIdentityV1 {
        schema_version: STATEFUL_STATE_SCHEMA_VERSION_V1,
        kind: STATEFUL_STATE_SCHEMA_KIND_V1.to_string(),
        digest,
    }
}

pub(crate) fn stateless_source_map_v1(
    workflow_id: String,
    workflow_index: u32,
    actions: Vec<ActionSourceMapV1>,
) -> StatefulStatelessWorkflowSourceMapV1 {
    StatefulStatelessWorkflowSourceMapV1 {
        workflow: WorkflowSourceMapV1 {
            workflow_id: workflow_id.clone(),
            source_workflow_index: workflow_index,
            target_rule_index: workflow_index,
            target_rule_key: workflow_id,
            actions,
        },
    }
}

pub(crate) fn state_variable_source_map_v1(
    variable: &CompiledStateVariableV1,
    index: u32,
) -> StatefulStateVariableSourceMapV1 {
    StatefulStateVariableSourceMapV1 {
        variable_id: variable.id.clone(),
        source_variable_index: index,
        artifact_variable_index: index,
        declaration_digest: variable.declaration_digest,
    }
}

pub(crate) fn branch_source_map_v1(branch: &StatefulBranchV1) -> StatefulBranchSourceMapV1 {
    let state_actions = branch
        .state_actions
        .iter()
        .enumerate()
        .map(|(index, node)| StatefulNodeSourceMapV1 {
            node_id: node.id.clone(),
            source_node_index: index as u32,
            artifact_node_index: index as u32,
            execution_ordinal: 1 + index as u32,
        })
        .collect::<Vec<_>>();
    let effects_start = 1 + state_actions.len() as u32;
    let effects = branch
        .effects
        .iter()
        .enumerate()
        .map(|(index, node)| StatefulNodeSourceMapV1 {
            node_id: node.id.clone(),
            source_node_index: index as u32,
            artifact_node_index: index as u32,
            execution_ordinal: effects_start + index as u32,
        })
        .collect::<Vec<_>>();
    StatefulBranchSourceMapV1 {
        implicit_acknowledgement_ordinal: 0,
        response: StatefulResponseSourceMapV1 {
            node_id: branch.response.id.clone(),
            execution_ordinal: effects_start + effects.len() as u32,
        },
        state_actions,
        effects,
    }
}

pub(crate) fn stateful_workflow_source_map_v1(
    workflow_id: String,
    index: u32,
    on_true: StatefulBranchSourceMapV1,
    on_false: StatefulBranchSourceMapV1,
) -> StatefulWorkflowSourceMapV1 {
    StatefulWorkflowSourceMapV1 {
        workflow_id,
        source_workflow_index: index,
        artifact_workflow_index: index,
        on_true,
        on_false,
    }
}

pub(crate) struct UnionSourceMapPartsV1 {
    pub source: StatefulSourceSpecIdentityV1,
    pub filtered_legacy_target: AutomationRuleSetIdentityV1,
    pub stateful_artifact: StatefulArtifactIdentityV1,
    pub state_schema: StatefulStateSchemaIdentityV1,
    pub stateless_workflows: Vec<StatefulStatelessWorkflowSourceMapV1>,
    pub state_variables: Vec<StatefulStateVariableSourceMapV1>,
    pub stateful_workflows: Vec<StatefulWorkflowSourceMapV1>,
}

pub(crate) fn union_source_map_v1(parts: UnionSourceMapPartsV1) -> StatefulUnionSourceMapV1 {
    StatefulUnionSourceMapV1 {
        schema_version: STATEFUL_UNION_SOURCE_MAP_SCHEMA_VERSION_V1,
        kind: STATEFUL_UNION_SOURCE_MAP_KIND_V1.to_string(),
        compiler_revision: STATEFUL_ARTIFACT_COMPILER_REVISION_V1,
        source: parts.source,
        filtered_legacy_target: parts.filtered_legacy_target,
        stateful_artifact: parts.stateful_artifact,
        state_schema: parts.state_schema,
        stateless_workflows: parts.stateless_workflows,
        state_variables: parts.state_variables,
        stateful_workflows: parts.stateful_workflows,
    }
}

pub(crate) fn source_map_identity_v1(
    digest: StatefulUnionSourceMapDigestV1,
) -> StatefulUnionSourceMapIdentityV1 {
    StatefulUnionSourceMapIdentityV1 {
        schema_version: STATEFUL_UNION_SOURCE_MAP_SCHEMA_VERSION_V1,
        kind: STATEFUL_UNION_SOURCE_MAP_KIND_V1.to_string(),
        digest,
    }
}

pub(crate) fn compilation_binding_v1(
    source: StatefulSourceSpecIdentityV1,
    filtered_legacy_target: AutomationRuleSetIdentityV1,
    stateful_artifact: StatefulArtifactIdentityV1,
    state_schema: StatefulStateSchemaIdentityV1,
    source_map: StatefulUnionSourceMapIdentityV1,
) -> StatefulCompilationBindingV1 {
    StatefulCompilationBindingV1 {
        format_version: STATEFUL_COMPILATION_BINDING_FORMAT_VERSION_V1,
        kind: STATEFUL_COMPILATION_BINDING_KIND_V1.to_string(),
        compiler_revision: STATEFUL_ARTIFACT_COMPILER_REVISION_V1,
        source,
        filtered_legacy_target,
        stateful_artifact,
        state_schema,
        source_map,
    }
}
