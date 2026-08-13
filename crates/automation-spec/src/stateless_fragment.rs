use automation_core::validate_structural;
use automation_ruleset::{content_hash, RuleSetKey, CURRENT_RULESET_SCHEMA_VERSION};
use automation_state::{InteractionRule, InteractionRuleSet};

use crate::model::{
    lower_action, lower_modal, lower_panel, lower_trigger, DeclaredPanelV1, ModalDefinitionV1,
    WorkflowSpecV1,
};
use crate::source_map::AutomationRuleSetIdentityV1;

/// A structurally validated legacy target containing assets and unconditional stateless rules
/// only. This type deliberately carries no source-spec or deployment authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledStatelessRuleSetFragmentV1 {
    ruleset: InteractionRuleSet,
    target: AutomationRuleSetIdentityV1,
}

impl CompiledStatelessRuleSetFragmentV1 {
    pub fn ruleset(&self) -> &InteractionRuleSet {
        &self.ruleset
    }

    pub fn target(&self) -> &AutomationRuleSetIdentityV1 {
        &self.target
    }

    pub fn into_ruleset(self) -> InteractionRuleSet {
        self.ruleset
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StatelessRuleSetFragmentErrorV1 {
    #[error("the stateless fragment ruleset key is invalid")]
    InvalidRuleSetKey,
    #[error("the stateless fragment contains a conditional workflow")]
    ConditionalWorkflow,
    #[error("the lowered stateless fragment is not a structurally valid legacy ruleset")]
    InvalidLegacyTarget,
    #[error("the stateless fragment legacy identity could not be computed")]
    Identity,
}

/// Lowers one already-authorized stateless fragment into the official legacy RuleSet wire shape.
///
/// The function intentionally accepts no stateful workflow type. It permits zero workflows so a
/// StatefulSpec whose handlers are all stateful can still bind its shared panels/modals to a
/// filtered legacy target with no executable rules. Callers remain responsible for validating
/// their complete source contract before selecting this fragment; this function independently
/// proves that the produced legacy target is structurally valid and canonically identified.
pub fn compile_structurally_validated_stateless_fragment_v1(
    ruleset_key: &str,
    panels: &[DeclaredPanelV1],
    modals: &[ModalDefinitionV1],
    workflows: &[WorkflowSpecV1],
) -> Result<CompiledStatelessRuleSetFragmentV1, StatelessRuleSetFragmentErrorV1> {
    RuleSetKey::parse(ruleset_key)
        .map_err(|_| StatelessRuleSetFragmentErrorV1::InvalidRuleSetKey)?;
    if workflows
        .iter()
        .any(|workflow| !workflow.condition.is_unconditional())
    {
        return Err(StatelessRuleSetFragmentErrorV1::ConditionalWorkflow);
    }

    let ruleset = InteractionRuleSet {
        version: 1,
        panels: panels.iter().map(lower_panel).collect(),
        modals: modals.iter().map(lower_modal).collect(),
        rules: workflows
            .iter()
            .map(|workflow| InteractionRule {
                key: workflow.id.clone(),
                trigger: lower_trigger(&workflow.trigger),
                actions: workflow
                    .actions
                    .iter()
                    .map(|node| lower_action(&node.action))
                    .collect(),
            })
            .collect(),
    };
    validate_structural(&ruleset)
        .map_err(|_| StatelessRuleSetFragmentErrorV1::InvalidLegacyTarget)?;
    let content_hash = content_hash(CURRENT_RULESET_SCHEMA_VERSION, &ruleset)
        .map_err(|_| StatelessRuleSetFragmentErrorV1::Identity)?;
    let target = AutomationRuleSetIdentityV1 {
        ruleset_key: ruleset_key.to_string(),
        schema_version: CURRENT_RULESET_SCHEMA_VERSION.get(),
        content_hash,
    };
    Ok(CompiledStatelessRuleSetFragmentV1 { ruleset, target })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        compile_deployable_automation_spec_v1, ActionNodeV1, ActionV1, AutomationSpecV1,
        ConditionExprV1, TriggerV1, AUTOMATION_SPEC_KIND_V1, AUTOMATION_SPEC_SCHEMA_VERSION_V1,
    };

    fn source() -> AutomationSpecV1 {
        AutomationSpecV1 {
            schema_version: AUTOMATION_SPEC_SCHEMA_VERSION_V1,
            kind: AUTOMATION_SPEC_KIND_V1.to_string(),
            key: "fragment_parity".to_string(),
            display_name: "Fragment parity".to_string(),
            description: String::new(),
            panels: vec![],
            modals: vec![],
            workflows: vec![WorkflowSpecV1 {
                id: "join".to_string(),
                trigger: TriggerV1::InstanceAction {
                    action_id: "join".to_string(),
                },
                condition: ConditionExprV1::Always,
                actions: vec![
                    ActionNodeV1 {
                        id: "defer".to_string(),
                        action: ActionV1::DeferEphemeral,
                    },
                    ActionNodeV1 {
                        id: "finish".to_string(),
                        action: ActionV1::EditResponse {
                            content: "joined".to_string(),
                        },
                    },
                ],
            }],
        }
    }

    #[test]
    fn nonempty_fragment_matches_the_existing_deployable_compiler() {
        let source = source();
        let existing = compile_deployable_automation_spec_v1(&source).unwrap();
        let fragment = compile_structurally_validated_stateless_fragment_v1(
            &source.key,
            &source.panels,
            &source.modals,
            &source.workflows,
        )
        .unwrap();

        assert_eq!(fragment.ruleset(), &existing.ruleset);
        assert_eq!(fragment.target(), &existing.target);
    }

    #[test]
    fn zero_rule_fragment_is_structural_and_officially_identified() {
        let fragment =
            compile_structurally_validated_stateless_fragment_v1("empty", &[], &[], &[]).unwrap();
        assert!(fragment.ruleset().rules.is_empty());
        assert_eq!(fragment.target().ruleset_key, "empty");
    }

    #[test]
    fn conditional_workflow_is_never_lowered() {
        let mut source = source();
        source.workflows[0].condition = ConditionExprV1::InputNonEmpty {
            input_id: "value".to_string(),
        };
        assert_eq!(
            compile_structurally_validated_stateless_fragment_v1(
                &source.key,
                &source.panels,
                &source.modals,
                &source.workflows,
            ),
            Err(StatelessRuleSetFragmentErrorV1::ConditionalWorkflow)
        );
    }
}
