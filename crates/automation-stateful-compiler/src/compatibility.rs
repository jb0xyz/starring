use crate::model::{CompiledStateSchemaV1, CompiledStateVariableV1};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdditiveStateSchemaCompatibilityV1 {
    added_variable_ids: Vec<String>,
}

impl AdditiveStateSchemaCompatibilityV1 {
    pub fn added_variable_ids(&self) -> &[String] {
        &self.added_variable_ids
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StateSchemaCompatibilityErrorV1 {
    #[error("state schema program key changed")]
    ProgramKeyChanged,
    #[error("state schema compiler or format identity changed")]
    FormatChanged,
    #[error("an existing state variable was removed or reordered at index {index}")]
    ExistingVariableRemovedOrReordered { index: usize },
    #[error("the declaration of existing state variable {variable_id} changed")]
    ExistingVariableChanged { variable_id: String },
}

/// Checks conservative append-only schema evolution. Existing variables must remain at the same
/// indices with identical scope, type bounds, default, and declaration digest. New variables may
/// only be appended, which keeps all previously compiled indices stable.
pub fn check_additive_state_schema_compatibility_v1(
    previous: &CompiledStateSchemaV1,
    candidate: &CompiledStateSchemaV1,
) -> Result<AdditiveStateSchemaCompatibilityV1, StateSchemaCompatibilityErrorV1> {
    if previous.program_key() != candidate.program_key() {
        return Err(StateSchemaCompatibilityErrorV1::ProgramKeyChanged);
    }
    if previous.schema_version() != candidate.schema_version()
        || previous.kind() != candidate.kind()
        || previous.compiler_revision() != candidate.compiler_revision()
    {
        return Err(StateSchemaCompatibilityErrorV1::FormatChanged);
    }
    if candidate.variables().len() < previous.variables().len() {
        return Err(
            StateSchemaCompatibilityErrorV1::ExistingVariableRemovedOrReordered {
                index: candidate.variables().len(),
            },
        );
    }
    for (index, (old, new)) in previous
        .variables()
        .iter()
        .zip(candidate.variables())
        .enumerate()
    {
        if old.id() != new.id() {
            return Err(
                StateSchemaCompatibilityErrorV1::ExistingVariableRemovedOrReordered { index },
            );
        }
        if !same_declaration(old, new) {
            return Err(StateSchemaCompatibilityErrorV1::ExistingVariableChanged {
                variable_id: old.id().to_string(),
            });
        }
    }
    Ok(AdditiveStateSchemaCompatibilityV1 {
        added_variable_ids: candidate.variables()[previous.variables().len()..]
            .iter()
            .map(|variable| variable.id().to_string())
            .collect(),
    })
}

fn same_declaration(left: &CompiledStateVariableV1, right: &CompiledStateVariableV1) -> bool {
    left.scope() == right.scope()
        && left.value_type() == right.value_type()
        && left.initial_value() == right.initial_value()
        && left.declaration_digest() == right.declaration_digest()
}
