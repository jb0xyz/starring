use std::collections::{BTreeMap, BTreeSet};

use automation_spec::{
    compile_structurally_validated_stateless_fragment_v1, ActionSourceMapV1,
    StatelessRuleSetFragmentErrorV1,
};
use automation_stateful_spec::{
    stateful_spec_digest_v1, validate_stateful_spec_v1, StateScopeV1, StateValueTypeV1,
    StateValueV1, StatefulConditionExprV1, StatefulSpecDigestErrorV1, StatefulSpecV1,
    StatefulSpecValidationErrorV1, StatefulValueExprV1,
};
use sha2::{Digest, Sha256};

use crate::canonical::{
    stateful_artifact_digest_v1, stateful_bundle_parts_digest_v1,
    stateful_compilation_binding_digest_v1, stateful_state_schema_digest_v1,
    stateful_union_source_map_digest_v1, StatefulCompilationIdentityErrorV1,
};
use crate::digest::StateDeclarationDigestV1;
use crate::model::{
    artifact_identity_v1, branch_source_map_v1, compilation_binding_v1, compiled_dependencies_v1,
    compiled_state_schema_v1, compiled_stateful_artifact_v1, compiled_variable_v1,
    compiled_workflow_v1, schema_identity_v1, source_identity_v1, source_map_identity_v1,
    state_variable_source_map_v1, stateful_workflow_source_map_v1, stateless_source_map_v1,
    union_source_map_v1, CompiledStatefulBranchV1, CompiledStatefulBundleV1,
    CompiledWorkflowDependenciesV1, UnionSourceMapPartsV1,
};

const STATE_DECLARATION_DOMAIN_V1: &[u8] = b"starring.stateful_state_declaration.v1\0";

#[derive(Debug, thiserror::Error)]
pub enum StatefulSpecCompileErrorV1 {
    #[error("StatefulSpec source is invalid")]
    InvalidSpec(#[from] StatefulSpecValidationErrorV1),
    #[error("StatefulSpec source identity could not be computed")]
    SourceIdentity(#[from] StatefulSpecDigestErrorV1),
    #[error("the filtered stateless legacy target is invalid")]
    StatelessFragment(#[from] StatelessRuleSetFragmentErrorV1),
    #[error("a validated StatefulSpec contained an unresolved state reference")]
    InvalidValidatedSource,
    #[error("a compiled StatefulSpec identity could not be computed")]
    CompilationIdentity(#[from] StatefulCompilationIdentityErrorV1),
}

/// Compiles an immutable, non-deployable bundle with strictly separated targets.
///
/// Only `stateless_workflows` are passed to the legacy fragment compiler. Stateful workflows are
/// copied into the separate typed artifact and therefore cannot become unconditional legacy
/// rules through this compilation path.
pub fn compile_stateful_spec_bundle_v1(
    spec: &StatefulSpecV1,
) -> Result<CompiledStatefulBundleV1, StatefulSpecCompileErrorV1> {
    validate_stateful_spec_v1(spec)?;
    let source_digest = stateful_spec_digest_v1(spec)?;
    let source = source_identity_v1(spec, source_digest);

    let filtered = compile_structurally_validated_stateless_fragment_v1(
        &spec.key,
        &spec.panels,
        &spec.modals,
        &spec.stateless_workflows,
    )?;
    let filtered_legacy_ruleset = filtered.ruleset().clone();
    let filtered_legacy_target = filtered.target().clone();

    let compiled_variables = spec
        .state_variables
        .iter()
        .map(|variable| {
            compiled_variable_v1(
                variable.id.clone(),
                variable.scope,
                variable.value_type.clone(),
                variable.initial_value.clone(),
                declaration_digest_v1(
                    &spec.key,
                    &variable.id,
                    variable.scope,
                    &variable.value_type,
                    &variable.initial_value,
                ),
            )
        })
        .collect::<Vec<_>>();
    let variable_indices = spec
        .state_variables
        .iter()
        .enumerate()
        .map(|(index, variable)| (variable.id.as_str(), index as u16))
        .collect::<BTreeMap<_, _>>();
    let compiled_workflows = spec
        .stateful_workflows
        .iter()
        .map(|workflow| {
            let dependencies = compile_dependencies_v1(
                &workflow.condition,
                [&workflow.on_true, &workflow.on_false],
                &variable_indices,
            )?;
            Ok(compiled_workflow_v1(
                workflow.id.clone(),
                workflow.trigger.clone(),
                workflow.condition.clone(),
                dependencies,
                CompiledStatefulBranchV1::from(&workflow.on_true),
                CompiledStatefulBranchV1::from(&workflow.on_false),
            ))
        })
        .collect::<Result<Vec<_>, StatefulSpecCompileErrorV1>>()?;

    let state_schema = compiled_state_schema_v1(spec, compiled_variables);
    let state_schema_digest = stateful_state_schema_digest_v1(&state_schema)?;
    let stateful_artifact = compiled_stateful_artifact_v1(
        spec,
        source.clone(),
        state_schema,
        state_schema_digest,
        compiled_workflows,
    );
    let stateful_artifact_digest = stateful_artifact_digest_v1(&stateful_artifact)?;
    let artifact_identity = artifact_identity_v1(stateful_artifact_digest);
    let schema_identity = schema_identity_v1(state_schema_digest);

    let stateless_workflow_maps = spec
        .stateless_workflows
        .iter()
        .enumerate()
        .map(|(workflow_index, workflow)| {
            stateless_source_map_v1(
                workflow.id.clone(),
                workflow_index as u32,
                workflow
                    .actions
                    .iter()
                    .enumerate()
                    .map(|(action_index, action)| ActionSourceMapV1 {
                        action_node_id: action.id.clone(),
                        source_action_index: action_index as u32,
                        target_action_index: action_index as u32,
                    })
                    .collect(),
            )
        })
        .collect();
    let state_variable_maps = stateful_artifact
        .state_schema()
        .variables()
        .iter()
        .enumerate()
        .map(|(index, variable)| state_variable_source_map_v1(variable, index as u32))
        .collect();
    let stateful_workflow_maps = spec
        .stateful_workflows
        .iter()
        .enumerate()
        .map(|(index, workflow)| {
            stateful_workflow_source_map_v1(
                workflow.id.clone(),
                index as u32,
                branch_source_map_v1(&workflow.on_true),
                branch_source_map_v1(&workflow.on_false),
            )
        })
        .collect();
    let union_source_map = union_source_map_v1(UnionSourceMapPartsV1 {
        source: source.clone(),
        filtered_legacy_target: filtered_legacy_target.clone(),
        stateful_artifact: artifact_identity.clone(),
        state_schema: schema_identity.clone(),
        stateless_workflows: stateless_workflow_maps,
        state_variables: state_variable_maps,
        stateful_workflows: stateful_workflow_maps,
    });
    let union_source_map_digest = stateful_union_source_map_digest_v1(&union_source_map)?;
    let binding = compilation_binding_v1(
        source,
        filtered_legacy_target.clone(),
        artifact_identity,
        schema_identity,
        source_map_identity_v1(union_source_map_digest),
    );
    let binding_digest = stateful_compilation_binding_digest_v1(&binding)?;
    let bundle_digest = stateful_bundle_parts_digest_v1(
        spec,
        &filtered_legacy_ruleset,
        &stateful_artifact,
        &union_source_map,
        &binding,
    )?;

    Ok(CompiledStatefulBundleV1 {
        source_spec: spec.clone(),
        filtered_legacy_ruleset,
        filtered_legacy_target,
        stateful_artifact,
        stateful_artifact_digest,
        state_schema_digest,
        union_source_map,
        union_source_map_digest,
        binding,
        binding_digest,
        bundle_digest,
    })
}

fn compile_dependencies_v1<'a>(
    condition: &'a StatefulConditionExprV1,
    branches: impl IntoIterator<Item = &'a automation_stateful_spec::StatefulBranchV1>,
    variable_indices: &BTreeMap<&str, u16>,
) -> Result<CompiledWorkflowDependenciesV1, StatefulSpecCompileErrorV1> {
    let mut reads = BTreeSet::new();
    let mut writes = BTreeSet::new();
    collect_condition_reads(condition, &mut reads);
    for branch in branches {
        for action in &branch.state_actions {
            writes.insert(action.variable_id.as_str());
            // Every write uses the prior row revision/value for compare-and-set evidence, even
            // when its RHS is a literal or input-only expression.
            reads.insert(action.variable_id.as_str());
            collect_value_reads(&action.value, &mut reads);
        }
    }
    let reads = indices_in_declaration_order(&reads, variable_indices)?;
    let writes = indices_in_declaration_order(&writes, variable_indices)?;
    Ok(compiled_dependencies_v1(reads, writes))
}

fn indices_in_declaration_order(
    ids: &BTreeSet<&str>,
    variable_indices: &BTreeMap<&str, u16>,
) -> Result<Vec<u16>, StatefulSpecCompileErrorV1> {
    let mut indices = ids
        .iter()
        .map(|id| {
            variable_indices
                .get(id)
                .copied()
                .ok_or(StatefulSpecCompileErrorV1::InvalidValidatedSource)
        })
        .collect::<Result<Vec<_>, _>>()?;
    indices.sort_unstable();
    Ok(indices)
}

fn collect_condition_reads<'a>(
    condition: &'a StatefulConditionExprV1,
    reads: &mut BTreeSet<&'a str>,
) {
    match condition {
        StatefulConditionExprV1::Always
        | StatefulConditionExprV1::InputNonEmpty { .. }
        | StatefulConditionExprV1::InputEquals { .. } => {}
        StatefulConditionExprV1::StateEquals { variable_id, value } => {
            reads.insert(variable_id);
            collect_value_reads(value, reads);
        }
        StatefulConditionExprV1::IntegerCompare { left, right, .. } => {
            collect_value_reads(left, reads);
            collect_value_reads(right, reads);
        }
        StatefulConditionExprV1::All { conditions }
        | StatefulConditionExprV1::Any { conditions } => {
            for condition in conditions {
                collect_condition_reads(condition, reads);
            }
        }
        StatefulConditionExprV1::Not { condition } => collect_condition_reads(condition, reads),
    }
}

fn collect_value_reads<'a>(expression: &'a StatefulValueExprV1, reads: &mut BTreeSet<&'a str>) {
    match expression {
        StatefulValueExprV1::Literal { .. } | StatefulValueExprV1::InputText { .. } => {}
        StatefulValueExprV1::State { variable_id } => {
            reads.insert(variable_id);
        }
        StatefulValueExprV1::CheckedAdd { left, right }
        | StatefulValueExprV1::CheckedSub { left, right } => {
            collect_value_reads(left, reads);
            collect_value_reads(right, reads);
        }
    }
}

fn declaration_digest_v1(
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
    StateDeclarationDigestV1::from_bytes(hasher.finalize().into())
}

fn append_state_type(hasher: &mut Sha256, value_type: &StateValueTypeV1) {
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

fn append_state_value(hasher: &mut Sha256, value: &StateValueV1) {
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

fn append_string(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}
