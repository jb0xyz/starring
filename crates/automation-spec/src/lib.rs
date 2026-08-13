mod canonical;
mod compile;
mod descriptor;
mod model;
mod preview;
mod simulate;
mod source_map;
mod stateless_fragment;
mod validate;

pub use canonical::{
    canonical_automation_spec_bytes_v1, decode_canonical_automation_spec_v1,
    AutomationSpecDigestErrorV1, AutomationSpecDigestV1,
};
pub use compile::{
    compile_deployable_automation_spec_v1, AutomationSpecCompileErrorV1, CompiledAutomationSpecV1,
};
pub use descriptor::{
    automation_spec_descriptor_v1, AutomationEffectClassV1, AutomationPrimitiveDescriptorV1,
    AutomationPrimitiveRuntimeSupportV1, AutomationSpecDescriptorDigestV1,
    AutomationSpecDescriptorV1, AutomationSpecLimitsV1, AutomationSpecSafetyV1,
    AUTOMATION_SPEC_DESCRIPTOR_KIND_V1, AUTOMATION_SPEC_DESCRIPTOR_REVISION_V1,
    MAX_AUTOMATION_SPEC_PREVIEW_REQUEST_BYTES_V1, MAX_AUTOMATION_SPEC_SIMULATION_REQUEST_BYTES_V1,
};
pub use model::{
    ActionButtonRouteV1, ActionButtonV1, ActionNodeV1, ActionTargetV1, ActionV1, AutomationSpecV1,
    ChannelReferenceV1, ConditionExprV1, CreatedResourceReferenceV1, DeclaredButtonV1,
    DeclaredPanelV1, DiscordPermissionV1, InstanceReferenceV1, InstanceResourcesV1,
    ModalDefinitionV1, ModalFieldDefinitionV1, ModalFieldStyleV1, ModalInputPolicyV1,
    OverwriteTargetV1, RoleReferenceV1, TriggerV1, WorkflowSpecV1, AUTOMATION_SPEC_KIND_V1,
    AUTOMATION_SPEC_SCHEMA_VERSION_V1,
};
pub use preview::{
    preview_automation_spec_v1, AutomationCapabilityV1, AutomationCompilationPreviewV1,
    AutomationContextualReadinessV1, AutomationDeploymentBlockerV1,
    AutomationDeploymentEffectSummaryV1, AutomationEffectSummaryV1, AutomationEventReadinessV1,
    AutomationPerEventEffectSummaryV1, AutomationPreviewErrorV1, AutomationPreviewSummaryV1,
    AutomationPreviewV1, AutomationStaticEligibilityV1,
};
pub use simulate::{
    simulate_automation_spec_v1, AutomationSimulationErrorV1, AutomationSimulationEventV1,
    AutomationSimulationOutcomeV1, AutomationSimulationTraceV1, MAX_SIMULATION_INPUT_BYTES_V1,
    MAX_SIMULATION_PAYLOAD_BYTES_V1,
};
pub use source_map::{
    automation_compilation_binding_digest_v1, automation_source_map_digest_v1,
    canonical_automation_compilation_binding_bytes_v1, canonical_automation_source_map_bytes_v1,
    decode_canonical_automation_compilation_binding_v1, decode_canonical_automation_source_map_v1,
    validate_automation_compilation_v1, ActionSourceMapV1, AutomationCompilationBindingDigestV1,
    AutomationCompilationBindingV1, AutomationCompilationIdentityErrorV1,
    AutomationRuleSetIdentityV1, AutomationSourceMapDigestV1, AutomationSourceMapV1,
    AutomationSpecIdentityV1, WorkflowSourceMapV1,
    AUTOMATION_COMPILATION_BINDING_FORMAT_VERSION_V1, AUTOMATION_COMPILATION_BINDING_KIND_V1,
    AUTOMATION_COMPILER_REVISION_V1, AUTOMATION_SOURCE_MAP_KIND_V1,
    AUTOMATION_SOURCE_MAP_SCHEMA_VERSION_V1,
};
pub use stateless_fragment::{
    compile_structurally_validated_stateless_fragment_v1, CompiledStatelessRuleSetFragmentV1,
    StatelessRuleSetFragmentErrorV1,
};
pub use validate::{
    validate_automation_spec_v1, AutomationSpecDiagnosticV1, AutomationSpecValidationErrorV1,
    MAX_ACTIONS_PER_WORKFLOW_V1, MAX_AUTOMATION_DESCRIPTION_BYTES_V1,
    MAX_AUTOMATION_DISPLAY_NAME_CHARS_V1, MAX_AUTOMATION_PANELS_V1,
    MAX_AUTOMATION_SPEC_CANONICAL_BYTES_V1, MAX_AUTOMATION_WORKFLOWS_V1, MAX_CONDITION_DEPTH_V1,
    MAX_CONDITION_NODES_V1, MAX_DISCORD_CUSTOM_ID_BYTES_V1, MAX_IDENTIFIER_BYTES_V1,
    MAX_INSTANCE_ACTION_ID_BYTES_V1, MAX_MODAL_DEFINITIONS_V1, MAX_MODAL_FIELDS_V1,
    MAX_RESOURCE_ALIAS_BYTES_V1, MAX_SIMULATION_INPUTS_V1, MAX_TOTAL_ACTIONS_V1,
};
