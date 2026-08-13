use std::collections::BTreeSet;

use discord_model::Permissions;
use serde::{Deserialize, Serialize};

use crate::canonical::{
    automation_spec_digest_v1, AutomationSpecDigestErrorV1, AutomationSpecDigestV1,
};
use crate::model::{ActionV1, AutomationSpecV1, ConditionExprV1};
use crate::source_map::{
    build_compiled_target_artifacts_v1, AutomationCompilationBindingDigestV1,
    AutomationCompilationIdentityErrorV1, AutomationRuleSetIdentityV1, AutomationSourceMapDigestV1,
};
use crate::validate::{validate_automation_spec_v1, AutomationSpecValidationErrorV1};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationCapabilityV1 {
    InteractionResponse,
    ManageChannels,
    ManageInstances,
    ManageRoles,
    PostMessages,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationStaticEligibilityV1 {
    CompatibleWithInteractionRuntimeV1,
    RuntimeExtensionRequired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationDeploymentBlockerV1 {
    ConditionalExecutionRuntimeUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationContextualReadinessV1 {
    NotEvaluated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationEventReadinessV1 {
    InputAndSnapshotDependent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum AutomationCompilationPreviewV1 {
    Available {
        target: AutomationRuleSetIdentityV1,
        source_map_digest: AutomationSourceMapDigestV1,
        binding_digest: AutomationCompilationBindingDigestV1,
    },
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationPerEventEffectSummaryV1 {
    pub response_steps: u32,
    pub compensatable_external_writes: u32,
    pub non_compensatable_external_writes: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationDeploymentEffectSummaryV1 {
    pub declared_panel_posts: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationEffectSummaryV1 {
    pub deployment: AutomationDeploymentEffectSummaryV1,
    pub per_event: AutomationPerEventEffectSummaryV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationPreviewSummaryV1 {
    pub panels: u32,
    pub modals: u32,
    pub workflows: u32,
    pub actions: u32,
    pub maximum_actions_per_event: u32,
    pub condition_nodes: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationPreviewV1 {
    pub schema_version: u16,
    pub spec_digest: AutomationSpecDigestV1,
    pub display_name: String,
    pub description: String,
    pub summary: AutomationPreviewSummaryV1,
    pub capabilities: Vec<AutomationCapabilityV1>,
    pub conservative_discord_permission_bits: u64,
    pub effects: AutomationEffectSummaryV1,
    pub static_eligibility: AutomationStaticEligibilityV1,
    pub deployment_blockers: Vec<AutomationDeploymentBlockerV1>,
    pub activation_readiness: AutomationContextualReadinessV1,
    pub panel_installation_readiness: AutomationContextualReadinessV1,
    pub event_execution_readiness: AutomationEventReadinessV1,
    pub compilation: AutomationCompilationPreviewV1,
}

#[derive(Debug, thiserror::Error)]
pub enum AutomationPreviewErrorV1 {
    #[error("automation spec is invalid")]
    Invalid(#[from] AutomationSpecValidationErrorV1),
    #[error("automation spec identity could not be computed")]
    Identity(#[from] AutomationSpecDigestErrorV1),
    #[error("compiled automation identity could not be computed")]
    Compilation(#[from] AutomationCompilationIdentityErrorV1),
}

pub fn preview_automation_spec_v1(
    spec: &AutomationSpecV1,
) -> Result<AutomationPreviewV1, AutomationPreviewErrorV1> {
    validate_automation_spec_v1(spec)?;
    let spec_digest = automation_spec_digest_v1(spec)?;
    let mut capabilities = BTreeSet::new();
    let mut required_permissions = Permissions::empty();
    if !spec.panels.is_empty() {
        capabilities.insert(AutomationCapabilityV1::PostMessages);
        required_permissions |= Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES;
    }
    let mut per_event = AutomationPerEventEffectSummaryV1 {
        response_steps: 0,
        compensatable_external_writes: 0,
        non_compensatable_external_writes: 0,
    };
    for workflow in &spec.workflows {
        for node in &workflow.actions {
            classify_action(
                &node.action,
                &mut capabilities,
                &mut required_permissions,
                &mut per_event,
            );
        }
    }
    let condition_nodes = spec
        .workflows
        .iter()
        .map(|workflow| count_condition_nodes(&workflow.condition))
        .sum::<usize>();
    let conditional = spec
        .workflows
        .iter()
        .any(|workflow| !workflow.condition.is_unconditional());
    let deployment_blockers = if conditional {
        vec![AutomationDeploymentBlockerV1::ConditionalExecutionRuntimeUnavailable]
    } else {
        Vec::new()
    };
    let compilation = if conditional {
        AutomationCompilationPreviewV1::Blocked
    } else {
        let compiled = build_compiled_target_artifacts_v1(spec)?;
        AutomationCompilationPreviewV1::Available {
            target: compiled.target,
            source_map_digest: compiled.source_map_digest,
            binding_digest: compiled.binding_digest,
        }
    };
    Ok(AutomationPreviewV1 {
        schema_version: spec.schema_version,
        spec_digest,
        display_name: spec.display_name.clone(),
        description: spec.description.clone(),
        summary: AutomationPreviewSummaryV1 {
            panels: spec.panels.len() as u32,
            modals: spec.modals.len() as u32,
            workflows: spec.workflows.len() as u32,
            actions: spec
                .workflows
                .iter()
                .map(|workflow| workflow.actions.len() as u32)
                .sum(),
            maximum_actions_per_event: spec
                .workflows
                .iter()
                .map(|workflow| workflow.actions.len() as u32)
                .max()
                .unwrap_or(0),
            condition_nodes: condition_nodes as u32,
        },
        capabilities: capabilities.into_iter().collect(),
        conservative_discord_permission_bits: required_permissions.bits(),
        effects: AutomationEffectSummaryV1 {
            deployment: AutomationDeploymentEffectSummaryV1 {
                declared_panel_posts: spec.panels.len() as u32,
            },
            per_event,
        },
        static_eligibility: if conditional {
            AutomationStaticEligibilityV1::RuntimeExtensionRequired
        } else {
            AutomationStaticEligibilityV1::CompatibleWithInteractionRuntimeV1
        },
        deployment_blockers,
        activation_readiness: AutomationContextualReadinessV1::NotEvaluated,
        panel_installation_readiness: AutomationContextualReadinessV1::NotEvaluated,
        event_execution_readiness: AutomationEventReadinessV1::InputAndSnapshotDependent,
        compilation,
    })
}

fn classify_action(
    action: &ActionV1,
    capabilities: &mut BTreeSet<AutomationCapabilityV1>,
    required_permissions: &mut Permissions,
    effects: &mut AutomationPerEventEffectSummaryV1,
) {
    match action {
        ActionV1::RespondEphemeral { .. }
        | ActionV1::OpenModal { .. }
        | ActionV1::DeferEphemeral => {
            capabilities.insert(AutomationCapabilityV1::InteractionResponse);
            effects.response_steps += 1;
        }
        ActionV1::EditResponse { .. } => {
            capabilities.insert(AutomationCapabilityV1::InteractionResponse);
            effects.non_compensatable_external_writes += 1;
        }
        ActionV1::CreateChannel { .. } => {
            capabilities.insert(AutomationCapabilityV1::ManageChannels);
            *required_permissions |= Permissions::MANAGE_CHANNELS;
            effects.compensatable_external_writes += 1;
        }
        ActionV1::CreateRole { .. } | ActionV1::GrantRole { .. } => {
            capabilities.insert(AutomationCapabilityV1::ManageRoles);
            *required_permissions |= Permissions::MANAGE_ROLES;
            effects.compensatable_external_writes += 1;
        }
        ActionV1::UpsertOverwrite { .. } => {
            capabilities.insert(AutomationCapabilityV1::ManageChannels);
            capabilities.insert(AutomationCapabilityV1::ManageRoles);
            *required_permissions |= Permissions::MANAGE_ROLES;
            effects.compensatable_external_writes += 1;
        }
        ActionV1::PostPanel { .. } => {
            capabilities.insert(AutomationCapabilityV1::PostMessages);
            *required_permissions |= Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES;
            effects.compensatable_external_writes += 1;
        }
        ActionV1::RegisterInstance { .. } => {
            capabilities.insert(AutomationCapabilityV1::ManageInstances);
            effects.compensatable_external_writes += 1;
        }
        ActionV1::TeardownInstance { .. } => {
            capabilities.insert(AutomationCapabilityV1::ManageChannels);
            capabilities.insert(AutomationCapabilityV1::ManageInstances);
            capabilities.insert(AutomationCapabilityV1::ManageRoles);
            *required_permissions |= Permissions::MANAGE_CHANNELS | Permissions::MANAGE_ROLES;
            effects.non_compensatable_external_writes += 1;
        }
    }
}

fn count_condition_nodes(condition: &ConditionExprV1) -> usize {
    match condition {
        ConditionExprV1::Always
        | ConditionExprV1::InputNonEmpty { .. }
        | ConditionExprV1::InputEquals { .. } => 1,
        ConditionExprV1::All { conditions } | ConditionExprV1::Any { conditions } => {
            1 + conditions.iter().map(count_condition_nodes).sum::<usize>()
        }
        ConditionExprV1::Not { condition } => 1 + count_condition_nodes(condition),
    }
}
