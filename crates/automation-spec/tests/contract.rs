use std::collections::{BTreeMap, BTreeSet};

use automation_spec::{
    automation_spec_descriptor_v1, canonical_automation_source_map_bytes_v1,
    canonical_automation_spec_bytes_v1, compile_deployable_automation_spec_v1,
    decode_canonical_automation_source_map_v1, decode_canonical_automation_spec_v1,
    preview_automation_spec_v1, simulate_automation_spec_v1, validate_automation_compilation_v1,
    validate_automation_spec_v1, ActionNodeV1, ActionTargetV1, ActionV1, AutomationCapabilityV1,
    AutomationCompilationPreviewV1, AutomationContextualReadinessV1, AutomationSimulationEventV1,
    AutomationSimulationOutcomeV1, AutomationSpecCompileErrorV1, AutomationSpecV1,
    AutomationStaticEligibilityV1, ChannelReferenceV1, ConditionExprV1, DeclaredButtonV1,
    DeclaredPanelV1, DiscordPermissionV1, ModalDefinitionV1, ModalFieldDefinitionV1,
    ModalFieldStyleV1, ModalInputPolicyV1, RoleReferenceV1, TriggerV1, WorkflowSpecV1,
    AUTOMATION_SPEC_KIND_V1,
};
use discord_model::Permissions;

fn unconditional_spec() -> AutomationSpecV1 {
    AutomationSpecV1 {
        schema_version: 1,
        kind: AUTOMATION_SPEC_KIND_V1.to_string(),
        key: "member_onboarding".to_string(),
        display_name: "Member onboarding".to_string(),
        description: "Grant the member role from a button.".to_string(),
        panels: vec![DeclaredPanelV1 {
            id: "onboarding".to_string(),
            channel: "welcome_channel".to_string(),
            content: "Join the community".to_string(),
            buttons: vec![DeclaredButtonV1 {
                label: "Join".to_string(),
                trigger_id: "join".to_string(),
            }],
        }],
        modals: vec![],
        workflows: vec![WorkflowSpecV1 {
            id: "grant_member".to_string(),
            trigger: TriggerV1::ButtonClick {
                trigger_id: "join".to_string(),
            },
            condition: ConditionExprV1::Always,
            actions: vec![
                ActionNodeV1 {
                    id: "grant_member_role".to_string(),
                    action: ActionV1::GrantRole {
                        role: RoleReferenceV1::Existing {
                            binding: "member_role".to_string(),
                        },
                        target: ActionTargetV1::Actor,
                    },
                },
                ActionNodeV1 {
                    id: "confirm_membership".to_string(),
                    action: ActionV1::RespondEphemeral {
                        content: "Welcome".to_string(),
                    },
                },
            ],
        }],
    }
}

fn conditional_spec() -> AutomationSpecV1 {
    let mut spec = unconditional_spec();
    spec.key = "application_form".to_string();
    spec.panels.clear();
    spec.modals = vec![ModalDefinitionV1 {
        id: "application".to_string(),
        title: "Application".to_string(),
        fields: vec![
            ModalFieldDefinitionV1 {
                id: "region".to_string(),
                label: "Region".to_string(),
                style: ModalFieldStyleV1::Short,
                required: true,
                min_length: None,
                max_length: None,
                input_policy: ModalInputPolicyV1::TrimUnicodeWhitespace,
            },
            ModalFieldDefinitionV1 {
                id: "note".to_string(),
                label: "Note".to_string(),
                style: ModalFieldStyleV1::Paragraph,
                required: false,
                min_length: None,
                max_length: Some(40),
                input_policy: ModalInputPolicyV1::TrimUnicodeWhitespace,
            },
        ],
    }];
    spec.workflows[0].trigger = TriggerV1::ModalSubmit {
        modal_id: "application".to_string(),
    };
    spec.workflows[0].condition = ConditionExprV1::All {
        conditions: vec![
            ConditionExprV1::InputEquals {
                input_id: "region".to_string(),
                value: "seoul".to_string(),
            },
            ConditionExprV1::Not {
                condition: Box::new(ConditionExprV1::InputNonEmpty {
                    input_id: "note".to_string(),
                }),
            },
        ],
    };
    spec
}

#[test]
fn unconditional_spec_compiles_to_the_existing_runtime_contract_and_source_map() {
    let spec = unconditional_spec();
    let compiled = compile_deployable_automation_spec_v1(&spec).unwrap();
    assert_eq!(compiled.ruleset.version, 1);
    assert_eq!(compiled.ruleset.rules.len(), 1);
    assert_eq!(compiled.ruleset.rules[0].actions.len(), 2);
    assert_eq!(compiled.target.ruleset_key, "member_onboarding");
    assert_eq!(compiled.source_map.workflows[0].workflow_id, "grant_member");
    assert_eq!(
        compiled.source_map.workflows[0].actions[0].action_node_id,
        "grant_member_role"
    );
    assert_eq!(
        compiled.preview.static_eligibility,
        AutomationStaticEligibilityV1::CompatibleWithInteractionRuntimeV1
    );
    assert!(matches!(
        compiled.preview.compilation,
        AutomationCompilationPreviewV1::Available { .. }
    ));
    assert_eq!(
        compiled
            .preview
            .capabilities
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            AutomationCapabilityV1::InteractionResponse,
            AutomationCapabilityV1::ManageRoles,
            AutomationCapabilityV1::PostMessages,
        ])
    );
    let bits = Permissions::from_bits_retain(compiled.preview.conservative_discord_permission_bits);
    assert!(bits.contains(Permissions::MANAGE_ROLES));
    assert!(bits.contains(Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES));
    assert_eq!(
        compiled.preview.activation_readiness,
        AutomationContextualReadinessV1::NotEvaluated
    );
    validate_automation_compilation_v1(
        &spec,
        &compiled.ruleset,
        &compiled.source_map,
        &compiled.binding,
    )
    .unwrap();
}

#[test]
fn conditions_are_previewed_and_normalized_but_fail_closed_for_deployment() {
    let spec = conditional_spec();
    let preview = preview_automation_spec_v1(&spec).unwrap();
    assert_eq!(
        preview.static_eligibility,
        AutomationStaticEligibilityV1::RuntimeExtensionRequired
    );
    assert_eq!(preview.compilation, AutomationCompilationPreviewV1::Blocked);
    assert!(matches!(
        compile_deployable_automation_spec_v1(&spec),
        Err(AutomationSpecCompileErrorV1::ConditionalRuntimeUnavailable)
    ));
    let event = AutomationSimulationEventV1 {
        trigger: TriggerV1::ModalSubmit {
            modal_id: "application".to_string(),
        },
        inputs: BTreeMap::from([("region".to_string(), " seoul ".to_string())]),
    };
    let trace = simulate_automation_spec_v1(&spec, &event).unwrap();
    assert_eq!(trace.outcome, AutomationSimulationOutcomeV1::ActionsPlanned);
    assert_eq!(trace.normalized_inputs["region"], "seoul");
    assert_eq!(trace.normalized_inputs["note"], "");
    assert_eq!(trace.action_node_ids.len(), 2);
}

#[test]
fn simulation_reuses_runtime_modal_validation() {
    let spec = conditional_spec();
    let missing = AutomationSimulationEventV1 {
        trigger: TriggerV1::ModalSubmit {
            modal_id: "application".to_string(),
        },
        inputs: BTreeMap::new(),
    };
    let error = simulate_automation_spec_v1(&spec, &missing).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "simulation_required_input_missing"));

    let unexpected = AutomationSimulationEventV1 {
        trigger: missing.trigger,
        inputs: BTreeMap::from([
            ("region".to_string(), "seoul".to_string()),
            ("typo".to_string(), "x".to_string()),
        ]),
    };
    let error = simulate_automation_spec_v1(&spec, &unexpected).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "simulation_input_unexpected"));
}

#[test]
fn action_node_identity_is_preserved_without_changing_runtime_hash() {
    let original = compile_deployable_automation_spec_v1(&unconditional_spec()).unwrap();
    let mut renamed_spec = unconditional_spec();
    renamed_spec.workflows[0].actions[0].id = "grant_role_renamed".to_string();
    let renamed = compile_deployable_automation_spec_v1(&renamed_spec).unwrap();
    assert_eq!(original.ruleset_content_hash, renamed.ruleset_content_hash);
    assert_ne!(original.spec_digest, renamed.spec_digest);
    assert_ne!(original.source_map_digest, renamed.source_map_digest);
    assert_ne!(original.binding_digest, renamed.binding_digest);
    assert_eq!(
        renamed.source_map.workflows[0].actions[0].action_node_id,
        "grant_role_renamed"
    );
}

#[test]
fn source_map_bytes_are_canonical_and_tampering_fails_closed() {
    let spec = unconditional_spec();
    let compiled = compile_deployable_automation_spec_v1(&spec).unwrap();
    let bytes = canonical_automation_source_map_bytes_v1(&compiled.source_map).unwrap();
    assert_eq!(
        decode_canonical_automation_source_map_v1(&bytes).unwrap(),
        compiled.source_map
    );
    let mut whitespace = bytes;
    whitespace.push(b'\n');
    assert!(decode_canonical_automation_source_map_v1(&whitespace).is_err());

    let mut tampered = compiled.source_map.clone();
    tampered.workflows[0].actions[0].target_action_index = 1;
    assert!(validate_automation_compilation_v1(
        &spec,
        &compiled.ruleset,
        &tampered,
        &compiled.binding,
    )
    .is_err());

    let mut invalid_spec = spec;
    invalid_spec.display_name.clear();
    assert!(validate_automation_compilation_v1(
        &invalid_spec,
        &compiled.ruleset,
        &compiled.source_map,
        &compiled.binding,
    )
    .is_err());
}

#[test]
fn canonical_spec_bytes_bind_behavior_and_reject_noncanonical_input() {
    let spec = unconditional_spec();
    let canonical = canonical_automation_spec_bytes_v1(&spec).unwrap();
    assert_eq!(
        decode_canonical_automation_spec_v1(&canonical).unwrap(),
        spec
    );
    let mut whitespace = canonical.clone();
    whitespace.push(b'\n');
    assert!(decode_canonical_automation_spec_v1(&whitespace).is_err());

    let original = preview_automation_spec_v1(&spec).unwrap().spec_digest;
    let mut changed = spec;
    changed.workflows[0].actions[1].action = ActionV1::RespondEphemeral {
        content: "Welcome aboard".to_string(),
    };
    let changed = preview_automation_spec_v1(&changed).unwrap().spec_digest;
    assert_ne!(original, changed);
}

#[test]
fn strict_panel_modal_and_custom_id_limits_are_authoring_contracts() {
    let mut panel = unconditional_spec();
    panel.panels[0].buttons[0].label = "x".repeat(81);
    let error = validate_automation_spec_v1(&panel).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "invalid_button_label"));

    let mut modal = conditional_spec();
    modal.modals[0].fields.clear();
    let error = validate_automation_spec_v1(&modal).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "invalid_modal_field_count"));

    let mut oversized_route = unconditional_spec();
    oversized_route.key = "a".repeat(50);
    oversized_route.panels[0].buttons[0].trigger_id = "b".repeat(20);
    oversized_route.workflows[0].trigger = TriggerV1::ButtonClick {
        trigger_id: "b".repeat(20),
    };
    let error = validate_automation_spec_v1(&oversized_route).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "component_custom_id_too_large"));
}

#[test]
fn conditions_must_reference_a_field_on_the_trigger_modal() {
    let mut spec = conditional_spec();
    spec.workflows[0].condition = ConditionExprV1::InputEquals {
        input_id: "typo".to_string(),
        value: "seoul".to_string(),
    };
    let error = validate_automation_spec_v1(&spec).unwrap_err();
    assert!(error.diagnostics().iter().any(|diagnostic| {
        diagnostic.code == "unknown_condition_input"
            && diagnostic.path == "/workflows/0/condition/input_id"
    }));
}

#[test]
fn duplicate_instance_triggers_and_multiple_initial_responses_are_rejected() {
    let mut spec = unconditional_spec();
    spec.panels.clear();
    spec.workflows[0].trigger = TriggerV1::InstanceAction {
        action_id: "close".to_string(),
    };
    spec.workflows[0].actions = vec![
        ActionNodeV1 {
            id: "defer".to_string(),
            action: ActionV1::DeferEphemeral,
        },
        ActionNodeV1 {
            id: "edit".to_string(),
            action: ActionV1::EditResponse {
                content: "done".to_string(),
            },
        },
    ];
    let mut duplicate = spec.workflows[0].clone();
    duplicate.id = "second_close".to_string();
    duplicate.actions[0].id = "second_defer".to_string();
    duplicate.actions[1].id = "second_edit".to_string();
    spec.workflows.push(duplicate);
    let error = validate_automation_spec_v1(&spec).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "duplicate_trigger"));

    let mut responses = unconditional_spec();
    responses.workflows[0].actions.push(ActionNodeV1 {
        id: "second_response".to_string(),
        action: ActionV1::RespondEphemeral {
            content: "again".to_string(),
        },
    });
    let error = validate_automation_spec_v1(&responses).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "multiple_initial_responses"));
}

#[test]
fn every_workflow_acknowledges_the_interaction_exactly_once() {
    let mut mutation_only = unconditional_spec();
    mutation_only.workflows[0].actions.pop();
    let error = validate_automation_spec_v1(&mutation_only).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "invalid_initial_response_count"));

    let mut modal_mutation_only = conditional_spec();
    modal_mutation_only.workflows[0].actions.pop();
    let error = validate_automation_spec_v1(&modal_mutation_only).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "invalid_initial_response_count"));

    let mut deferred = unconditional_spec();
    deferred.panels.clear();
    deferred.workflows[0].trigger = TriggerV1::InstanceAction {
        action_id: "close".to_string(),
    };
    deferred.workflows[0].actions = vec![
        ActionNodeV1 {
            id: "defer".to_string(),
            action: ActionV1::DeferEphemeral,
        },
        ActionNodeV1 {
            id: "edit".to_string(),
            action: ActionV1::EditResponse {
                content: "done".to_string(),
            },
        },
    ];
    validate_automation_spec_v1(&deferred).unwrap();
}

#[test]
fn teardown_is_the_final_mutable_effect_in_the_authored_graph() {
    let mut spec = unconditional_spec();
    spec.panels.clear();
    spec.workflows[0].trigger = TriggerV1::InstanceAction {
        action_id: "close".to_string(),
    };
    spec.workflows[0].actions = vec![
        ActionNodeV1 {
            id: "defer".to_string(),
            action: ActionV1::DeferEphemeral,
        },
        ActionNodeV1 {
            id: "teardown".to_string(),
            action: ActionV1::TeardownInstance {
                instance: automation_spec::InstanceReferenceV1::Event,
            },
        },
        ActionNodeV1 {
            id: "grant_after_teardown".to_string(),
            action: ActionV1::GrantRole {
                role: RoleReferenceV1::Existing {
                    binding: "member_role".to_string(),
                },
                target: ActionTargetV1::Actor,
            },
        },
        ActionNodeV1 {
            id: "edit".to_string(),
            action: ActionV1::EditResponse {
                content: "closed".to_string(),
            },
        },
    ];
    let error = validate_automation_spec_v1(&spec).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "action_after_teardown"));

    spec.workflows[0].actions.remove(2);
    validate_automation_spec_v1(&spec).unwrap();
}

#[test]
fn immediate_responses_are_final_and_literal_templates_render_at_authoring_time() {
    let mut response_first = unconditional_spec();
    response_first.workflows[0].actions.swap(0, 1);
    let error = validate_automation_spec_v1(&response_first).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "initial_response_not_final"));

    let mut empty_channel = unconditional_spec();
    empty_channel.workflows[0].actions[0].action = ActionV1::CreateChannel {
        output: "channel".to_string(),
        name: "!!!!".to_string(),
    };
    let error = validate_automation_spec_v1(&empty_channel).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "literal_template_unrenderable"));

    let mut expanded_message = unconditional_spec();
    expanded_message.workflows[0].actions[1].action = ActionV1::RespondEphemeral {
        content: "@everyone".repeat(220),
    };
    let error = validate_automation_spec_v1(&expanded_message).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "literal_template_unrenderable"));

    let mut fixed_overflow_with_input = conditional_spec();
    fixed_overflow_with_input.workflows[0].actions[0].action = ActionV1::CreateRole {
        output: "role".to_string(),
        name: format!("{}${{input.region}}", "<@".repeat(45)),
    };
    let error = validate_automation_spec_v1(&fixed_overflow_with_input).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "literal_template_unrenderable"));
}

#[test]
fn closed_permission_set_and_runtime_panel_shape_fail_closed() {
    let mut spec = unconditional_spec();
    spec.workflows[0].actions[0].action = ActionV1::UpsertOverwrite {
        channel: ChannelReferenceV1::Existing {
            binding: "welcome_channel".to_string(),
        },
        target: automation_spec::OverwriteTargetV1::Everyone,
        allow: vec![
            DiscordPermissionV1::ViewChannel,
            DiscordPermissionV1::ViewChannel,
        ],
        deny: vec![],
    };
    let error = validate_automation_spec_v1(&spec).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "duplicate_permission"));

    let mut panel = unconditional_spec();
    panel.workflows[0].actions[0].action = ActionV1::PostPanel {
        output: "message".to_string(),
        channel: ChannelReferenceV1::Existing {
            binding: "welcome_channel".to_string(),
        },
        content: "notice".to_string(),
        buttons: vec![],
    };
    let error = validate_automation_spec_v1(&panel).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "invalid_runtime_panel_button_count"));
}

#[test]
fn declared_panel_content_is_safe_for_the_literal_installer_path() {
    let mut mention = unconditional_spec();
    mention.panels[0].content = "@everyone please join".to_string();
    let error = validate_automation_spec_v1(&mention).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "unsafe_declared_panel_content"));

    let mut control_only = unconditional_spec();
    control_only.panels[0].content = "\u{0007}".to_string();
    let error = validate_automation_spec_v1(&control_only).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "unsafe_declared_panel_content"));

    validate_automation_spec_v1(&unconditional_spec()).unwrap();
}

#[test]
fn every_rendered_control_has_exactly_one_workflow_handler() {
    let mut dead_declared_button = unconditional_spec();
    dead_declared_button.panels[0]
        .buttons
        .push(DeclaredButtonV1 {
            label: "Dead".to_string(),
            trigger_id: "dead".to_string(),
        });
    let error = validate_automation_spec_v1(&dead_declared_button).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "rendered_route_without_handler"));

    let mut dead_static_panel = unconditional_spec();
    dead_static_panel.workflows[0].actions[0].action = ActionV1::PostPanel {
        output: "message".to_string(),
        channel: ChannelReferenceV1::Existing {
            binding: "welcome_channel".to_string(),
        },
        content: "Actions".to_string(),
        buttons: vec![automation_spec::ActionButtonV1 {
            label: "Dead".to_string(),
            route: automation_spec::ActionButtonRouteV1::Static {
                trigger_id: "dead".to_string(),
            },
        }],
    };
    let error = validate_automation_spec_v1(&dead_static_panel).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "rendered_route_without_handler"));

    let mut unopened_submission = unconditional_spec();
    unopened_submission.modals.push(ModalDefinitionV1 {
        id: "application".to_string(),
        title: "Application".to_string(),
        fields: vec![ModalFieldDefinitionV1 {
            id: "name".to_string(),
            label: "Name".to_string(),
            style: ModalFieldStyleV1::Short,
            required: true,
            min_length: None,
            max_length: None,
            input_policy: ModalInputPolicyV1::TrimUnicodeWhitespace,
        }],
    });
    unopened_submission.workflows[0].actions[1].action = ActionV1::OpenModal {
        modal_id: "application".to_string(),
    };
    let error = validate_automation_spec_v1(&unopened_submission).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "rendered_route_without_handler"));
}

#[test]
fn runtime_panel_instance_routes_are_unique_and_bound_to_execution_context() {
    let instance_button = automation_spec::ActionButtonV1 {
        label: "Close".to_string(),
        route: automation_spec::ActionButtonRouteV1::InstanceAction {
            instance: automation_spec::InstanceReferenceV1::Event,
            action_id: "close".to_string(),
        },
    };
    let mut invalid = unconditional_spec();
    invalid.workflows[0].actions[0].action = ActionV1::PostPanel {
        output: "panel_message".to_string(),
        channel: ChannelReferenceV1::Existing {
            binding: "welcome_channel".to_string(),
        },
        content: "Manage".to_string(),
        buttons: vec![instance_button.clone(), instance_button],
    };
    let error = validate_automation_spec_v1(&invalid).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "duplicate_runtime_panel_route"));
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| { diagnostic.code == "event_instance_requires_instance_trigger" }));

    let mut valid = unconditional_spec();
    valid.workflows[0].actions = vec![
        ActionNodeV1 {
            id: "post_instance_panel".to_string(),
            action: ActionV1::PostPanel {
                output: "panel_message".to_string(),
                channel: ChannelReferenceV1::Existing {
                    binding: "welcome_channel".to_string(),
                },
                content: "Manage".to_string(),
                buttons: vec![automation_spec::ActionButtonV1 {
                    label: "Close".to_string(),
                    route: automation_spec::ActionButtonRouteV1::InstanceAction {
                        instance: automation_spec::InstanceReferenceV1::Created {
                            output: "room_instance".to_string(),
                        },
                        action_id: "close".to_string(),
                    },
                }],
            },
        },
        ActionNodeV1 {
            id: "register_room".to_string(),
            action: ActionV1::RegisterInstance {
                output: "room_instance".to_string(),
                instance_kind: "managed_room".to_string(),
                resources: automation_spec::InstanceResourcesV1 {
                    roles: BTreeMap::new(),
                    channels: BTreeMap::new(),
                    messages: BTreeMap::from([(
                        "panel".to_string(),
                        automation_spec::CreatedResourceReferenceV1 {
                            output: "panel_message".to_string(),
                        },
                    )]),
                },
            },
        },
        ActionNodeV1 {
            id: "confirm".to_string(),
            action: ActionV1::RespondEphemeral {
                content: "Created".to_string(),
            },
        },
    ];
    let missing_handler = validate_automation_spec_v1(&valid).unwrap_err();
    assert!(missing_handler
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "rendered_route_without_handler"));
    valid.workflows.push(WorkflowSpecV1 {
        id: "close_room".to_string(),
        trigger: TriggerV1::InstanceAction {
            action_id: "close".to_string(),
        },
        condition: ConditionExprV1::Always,
        actions: vec![
            ActionNodeV1 {
                id: "defer_close".to_string(),
                action: ActionV1::DeferEphemeral,
            },
            ActionNodeV1 {
                id: "confirm_close".to_string(),
                action: ActionV1::EditResponse {
                    content: "Closed".to_string(),
                },
            },
        ],
    });
    validate_automation_spec_v1(&valid).unwrap();

    valid.workflows[1].actions.insert(
        1,
        ActionNodeV1 {
            id: "grant_instance_member".to_string(),
            action: ActionV1::GrantRole {
                role: RoleReferenceV1::Instance {
                    instance: automation_spec::InstanceReferenceV1::Event,
                    alias: "member_role".to_string(),
                },
                target: ActionTargetV1::Actor,
            },
        },
    );
    let error = validate_automation_spec_v1(&valid).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| { diagnostic.code == "created_instance_missing_handler_resource" }));
    valid.workflows[1].actions.remove(1);

    valid.workflows[1].actions.insert(
        1,
        ActionNodeV1 {
            id: "forward_instance_action".to_string(),
            action: ActionV1::PostPanel {
                output: "forward_panel".to_string(),
                channel: ChannelReferenceV1::Existing {
                    binding: "welcome_channel".to_string(),
                },
                content: "Continue".to_string(),
                buttons: vec![automation_spec::ActionButtonV1 {
                    label: "Join".to_string(),
                    route: automation_spec::ActionButtonRouteV1::InstanceAction {
                        instance: automation_spec::InstanceReferenceV1::Event,
                        action_id: "join".to_string(),
                    },
                }],
            },
        },
    );
    valid.workflows.push(WorkflowSpecV1 {
        id: "join_room".to_string(),
        trigger: TriggerV1::InstanceAction {
            action_id: "join".to_string(),
        },
        condition: ConditionExprV1::Always,
        actions: vec![
            ActionNodeV1 {
                id: "grant_forwarded_member".to_string(),
                action: ActionV1::GrantRole {
                    role: RoleReferenceV1::Instance {
                        instance: automation_spec::InstanceReferenceV1::Event,
                        alias: "member_role".to_string(),
                    },
                    target: ActionTargetV1::Actor,
                },
            },
            ActionNodeV1 {
                id: "confirm_forwarded_member".to_string(),
                action: ActionV1::RespondEphemeral {
                    content: "Joined".to_string(),
                },
            },
        ],
    });
    let error = validate_automation_spec_v1(&valid).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| { diagnostic.code == "created_instance_missing_handler_resource" }));
    valid.workflows.pop();
    valid.workflows[1].actions.remove(1);

    if let ActionV1::PostPanel { buttons, .. } = &mut valid.workflows[0].actions[0].action {
        buttons[0].route = automation_spec::ActionButtonRouteV1::InstanceAction {
            instance: automation_spec::InstanceReferenceV1::Created {
                output: "missing_instance".to_string(),
            },
            action_id: "close".to_string(),
        };
    }
    let error = validate_automation_spec_v1(&valid).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "unknown_panel_instance_output"));
}

#[test]
fn descriptor_pins_the_closed_primitive_surface_and_safety_flags() {
    let descriptor = automation_spec_descriptor_v1();
    assert_eq!(descriptor.triggers.len(), 3);
    assert_eq!(descriptor.conditions.len(), 6);
    assert_eq!(descriptor.actions.len(), 11);
    assert_eq!(descriptor.capabilities.len(), 5);
    assert_eq!(descriptor.limits.maximum_preview_request_bytes, 48 * 1_024);
    assert_eq!(
        descriptor.limits.maximum_simulation_request_bytes,
        256 * 1_024
    );
    assert_eq!(descriptor.limits.maximum_identifier_bytes, 64);
    assert_eq!(descriptor.limits.maximum_instance_action_id_bytes, 56);
    assert_eq!(descriptor.limits.maximum_resource_alias_bytes, 32);
    assert_eq!(descriptor.limits.maximum_discord_custom_id_bytes, 100);
    assert_eq!(descriptor.limits.maximum_simulation_input_bytes, 4_000);
    assert_eq!(descriptor.limits.maximum_simulation_payload_bytes, 20_000);
    assert!(!descriptor.safety.arbitrary_code);
    assert!(!descriptor.safety.arbitrary_http);
    assert!(!descriptor.safety.event_time_llm);
    assert!(!descriptor.safety.secret_reference_fields);
    assert_eq!(descriptor.installation_readiness, "not_evaluated");
    assert_eq!(descriptor.simulation_input_stage, "post_gateway_admission");
    assert_eq!(descriptor.descriptor_digest.to_hex().len(), 64);
}

#[test]
fn wire_contract_owns_its_shapes_and_rejects_unknown_fields() {
    let mut value = serde_json::to_value(unconditional_spec()).unwrap();
    value["arbitrary_code"] = serde_json::json!("fetch('https://example.test')");
    assert!(serde_json::from_value::<AutomationSpecV1>(value).is_err());

    let action = serde_json::to_value(ActionV1::GrantRole {
        role: RoleReferenceV1::Existing {
            binding: "member_role".to_string(),
        },
        target: ActionTargetV1::Actor,
    })
    .unwrap();
    assert_eq!(
        action,
        serde_json::json!({
            "type": "grant_role",
            "role": {"type": "existing", "binding": "member_role"},
            "target": "actor"
        })
    );
}
