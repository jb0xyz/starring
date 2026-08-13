use automation_ruleset::RuleSetContentHash;
use automation_state::InteractionRuleSet;

use crate::canonical::{AutomationSpecDigestErrorV1, AutomationSpecDigestV1};
use crate::model::AutomationSpecV1;
use crate::preview::{preview_automation_spec_v1, AutomationPreviewErrorV1, AutomationPreviewV1};
use crate::source_map::{
    build_compiled_target_artifacts_v1, AutomationCompilationBindingDigestV1,
    AutomationCompilationBindingV1, AutomationCompilationIdentityErrorV1,
    AutomationRuleSetIdentityV1, AutomationSourceMapDigestV1, AutomationSourceMapV1,
};
use crate::validate::{validate_automation_spec_v1, AutomationSpecValidationErrorV1};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledAutomationSpecV1 {
    pub spec_digest: AutomationSpecDigestV1,
    pub ruleset_content_hash: RuleSetContentHash,
    pub target: AutomationRuleSetIdentityV1,
    pub ruleset: InteractionRuleSet,
    pub source_map: AutomationSourceMapV1,
    pub source_map_digest: AutomationSourceMapDigestV1,
    pub binding: AutomationCompilationBindingV1,
    pub binding_digest: AutomationCompilationBindingDigestV1,
    pub preview: AutomationPreviewV1,
}

#[derive(Debug, thiserror::Error)]
pub enum AutomationSpecCompileErrorV1 {
    #[error("automation spec is invalid")]
    Invalid(#[from] AutomationSpecValidationErrorV1),
    #[error("automation spec uses conditions not supported by interaction runtime V1")]
    ConditionalRuntimeUnavailable,
    #[error("automation spec identity could not be computed")]
    Identity(#[from] AutomationSpecDigestErrorV1),
    #[error("compiled automation identity could not be computed")]
    CompilationIdentity(#[from] AutomationCompilationIdentityErrorV1),
    #[error("automation preview could not be computed")]
    Preview(#[from] AutomationPreviewErrorV1),
}

pub fn compile_deployable_automation_spec_v1(
    spec: &AutomationSpecV1,
) -> Result<CompiledAutomationSpecV1, AutomationSpecCompileErrorV1> {
    validate_automation_spec_v1(spec)?;
    if spec
        .workflows
        .iter()
        .any(|workflow| !workflow.condition.is_unconditional())
    {
        return Err(AutomationSpecCompileErrorV1::ConditionalRuntimeUnavailable);
    }
    let compiled = build_compiled_target_artifacts_v1(spec)?;
    let preview = preview_automation_spec_v1(spec)?;
    Ok(CompiledAutomationSpecV1 {
        spec_digest: compiled.binding.source.digest,
        ruleset_content_hash: compiled.target.content_hash,
        target: compiled.target,
        ruleset: compiled.ruleset,
        source_map: compiled.source_map,
        source_map_digest: compiled.source_map_digest,
        binding: compiled.binding,
        binding_digest: compiled.binding_digest,
        preview,
    })
}
