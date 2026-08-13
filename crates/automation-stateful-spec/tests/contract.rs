use std::collections::BTreeMap;

use automation_spec::{
    ActionButtonRouteV1, ActionButtonV1, ActionTargetV1, ChannelReferenceV1, ConditionExprV1,
    CreatedResourceReferenceV1, InstanceReferenceV1, InstanceResourcesV1, ModalFieldDefinitionV1,
    ModalFieldStyleV1, ModalInputPolicyV1, RoleReferenceV1,
};
use automation_stateful_spec::{
    canonical_stateful_simulation_trace_bytes_v1, canonical_stateful_spec_bytes_v1,
    decode_canonical_stateful_simulation_trace_v1, decode_canonical_stateful_spec_v1,
    evaluate_validated_stateful_workflow_v1, normalize_stateful_event_inputs_v1,
    preview_stateful_spec_v1, simulate_stateful_spec_v1, stateful_spec_deployment_status_v1,
    validate_stateful_spec_v1, ActionNodeV1, ActionV1, IntegerComparisonV1, ModalDefinitionV1,
    StateScopeV1, StateSetNodeV1, StateSimulationCellV1, StateValueTypeV1, StateValueV1,
    StateVariableV1, StatefulBranchSelectionV1, StatefulBranchV1, StatefulConditionExprV1,
    StatefulResponseNodeV1, StatefulSimulationEventV1, StatefulSimulationInputV1,
    StatefulSimulationOutcomeV1, StatefulSpecDeploymentBlockerV1, StatefulSpecV1,
    StatefulValueExprV1, StatefulWorkflowV1, TriggerV1, WorkflowSpecV1, MAX_SAFE_INTEGER_V1,
    MAX_STATEFUL_SIMULATION_CELLS_V1, MAX_STATE_ACTIONS_PER_BRANCH_V1, STATEFUL_SPEC_KIND_V1,
};

fn modal() -> ModalDefinitionV1 {
    ModalDefinitionV1 {
        id: "counter_form".to_string(),
        title: "Counter".to_string(),
        fields: vec![ModalFieldDefinitionV1 {
            id: "note".to_string(),
            label: "Note".to_string(),
            style: ModalFieldStyleV1::Short,
            required: false,
            min_length: None,
            max_length: Some(100),
            input_policy: ModalInputPolicyV1::TrimUnicodeWhitespace,
        }],
    }
}

fn stateful_spec() -> StatefulSpecV1 {
    StatefulSpecV1 {
        schema_version: 1,
        kind: STATEFUL_SPEC_KIND_V1.to_string(),
        key: "counter_program".to_string(),
        display_name: "Counter program".to_string(),
        description: "Increment from a modal.".to_string(),
        panels: vec![],
        modals: vec![modal()],
        stateless_workflows: vec![],
        state_variables: vec![
            StateVariableV1 {
                id: "count".to_string(),
                scope: StateScopeV1::Actor,
                value_type: StateValueTypeV1::Integer { min: 0, max: 100 },
                initial_value: StateValueV1::Integer { value: 0 },
            },
            StateVariableV1 {
                id: "previous".to_string(),
                scope: StateScopeV1::Actor,
                value_type: StateValueTypeV1::Integer { min: 0, max: 100 },
                initial_value: StateValueV1::Integer { value: 0 },
            },
            StateVariableV1 {
                id: "note".to_string(),
                scope: StateScopeV1::Actor,
                value_type: StateValueTypeV1::Text {
                    max_utf8_bytes: 100,
                },
                initial_value: StateValueV1::Text {
                    value: String::new(),
                },
            },
        ],
        stateful_workflows: vec![StatefulWorkflowV1 {
            id: "increment".to_string(),
            trigger: TriggerV1::ModalSubmit {
                modal_id: "counter_form".to_string(),
            },
            condition: StatefulConditionExprV1::IntegerCompare {
                left: StatefulValueExprV1::State {
                    variable_id: "count".to_string(),
                },
                operator: IntegerComparisonV1::LessThan,
                right: StatefulValueExprV1::Literal {
                    value: StateValueV1::Integer { value: 100 },
                },
            },
            on_true: StatefulBranchV1 {
                state_actions: vec![
                    StateSetNodeV1 {
                        id: "increment_count".to_string(),
                        variable_id: "count".to_string(),
                        value: StatefulValueExprV1::CheckedAdd {
                            left: Box::new(StatefulValueExprV1::State {
                                variable_id: "count".to_string(),
                            }),
                            right: Box::new(StatefulValueExprV1::Literal {
                                value: StateValueV1::Integer { value: 1 },
                            }),
                        },
                    },
                    StateSetNodeV1 {
                        id: "remember_previous".to_string(),
                        variable_id: "previous".to_string(),
                        value: StatefulValueExprV1::State {
                            variable_id: "count".to_string(),
                        },
                    },
                    StateSetNodeV1 {
                        id: "save_note".to_string(),
                        variable_id: "note".to_string(),
                        value: StatefulValueExprV1::InputText {
                            input_id: "note".to_string(),
                        },
                    },
                ],
                effects: vec![],
                response: StatefulResponseNodeV1 {
                    id: "increment_response".to_string(),
                    content: "Incremented".to_string(),
                },
            },
            on_false: StatefulBranchV1 {
                state_actions: vec![],
                effects: vec![],
                response: StatefulResponseNodeV1 {
                    id: "limit_response".to_string(),
                    content: "At limit".to_string(),
                },
            },
        }],
    }
}

fn fixture(cells: Vec<StateSimulationCellV1>) -> StatefulSimulationInputV1 {
    StatefulSimulationInputV1 {
        event: StatefulSimulationEventV1 {
            trigger: TriggerV1::ModalSubmit {
                modal_id: "counter_form".to_string(),
            },
            inputs: BTreeMap::from([("note".to_string(), "  hello  ".to_string())]),
        },
        state: cells,
    }
}

fn cross_branch_instance_spec(transitive: bool) -> StatefulSpecV1 {
    let creator = StatefulWorkflowV1 {
        id: "creator".to_string(),
        trigger: TriggerV1::ButtonClick {
            trigger_id: "create".to_string(),
        },
        condition: StatefulConditionExprV1::Always,
        on_true: StatefulBranchV1 {
            state_actions: vec![],
            effects: vec![
                ActionNodeV1 {
                    id: "create_room".to_string(),
                    action: ActionV1::CreateChannel {
                        output: "room".to_string(),
                        name: "room".to_string(),
                    },
                },
                ActionNodeV1 {
                    id: "register_room".to_string(),
                    action: ActionV1::RegisterInstance {
                        output: "instance".to_string(),
                        instance_kind: "room".to_string(),
                        resources: InstanceResourcesV1 {
                            roles: BTreeMap::new(),
                            channels: BTreeMap::from([(
                                "room".to_string(),
                                CreatedResourceReferenceV1 {
                                    output: "room".to_string(),
                                },
                            )]),
                            messages: BTreeMap::new(),
                        },
                    },
                },
                ActionNodeV1 {
                    id: "post_room".to_string(),
                    action: ActionV1::PostPanel {
                        output: "room_panel".to_string(),
                        channel: ChannelReferenceV1::Created {
                            output: "room".to_string(),
                        },
                        content: "room".to_string(),
                        buttons: vec![ActionButtonV1 {
                            label: "Manage".to_string(),
                            route: ActionButtonRouteV1::InstanceAction {
                                instance: InstanceReferenceV1::Created {
                                    output: "instance".to_string(),
                                },
                                action_id: "manage".to_string(),
                            },
                        }],
                    },
                },
            ],
            response: StatefulResponseNodeV1 {
                id: "creator_true_response".to_string(),
                content: "created".to_string(),
            },
        },
        on_false: StatefulBranchV1 {
            state_actions: vec![],
            effects: vec![],
            response: StatefulResponseNodeV1 {
                id: "creator_false_response".to_string(),
                content: "not created".to_string(),
            },
        },
    };
    let manage = StatefulWorkflowV1 {
        id: "manage_handler".to_string(),
        trigger: TriggerV1::InstanceAction {
            action_id: "manage".to_string(),
        },
        condition: StatefulConditionExprV1::Always,
        on_true: StatefulBranchV1 {
            state_actions: vec![],
            effects: if transitive {
                vec![ActionNodeV1 {
                    id: "forward_to_close".to_string(),
                    action: ActionV1::PostPanel {
                        output: "close_panel".to_string(),
                        channel: ChannelReferenceV1::Existing {
                            binding: "lobby".to_string(),
                        },
                        content: "close".to_string(),
                        buttons: vec![ActionButtonV1 {
                            label: "Close".to_string(),
                            route: ActionButtonRouteV1::InstanceAction {
                                instance: InstanceReferenceV1::Event,
                                action_id: "close".to_string(),
                            },
                        }],
                    },
                }]
            } else {
                vec![]
            },
            response: StatefulResponseNodeV1 {
                id: "manage_true_response".to_string(),
                content: "managed".to_string(),
            },
        },
        on_false: StatefulBranchV1 {
            state_actions: vec![],
            effects: if transitive {
                vec![]
            } else {
                vec![event_owner_grant("grant_owner")]
            },
            response: StatefulResponseNodeV1 {
                id: "manage_false_response".to_string(),
                content: "managed false".to_string(),
            },
        },
    };
    let mut stateful_workflows = vec![creator, manage];
    if transitive {
        stateful_workflows.push(StatefulWorkflowV1 {
            id: "close_handler".to_string(),
            trigger: TriggerV1::InstanceAction {
                action_id: "close".to_string(),
            },
            condition: StatefulConditionExprV1::Always,
            on_true: StatefulBranchV1 {
                state_actions: vec![],
                effects: vec![],
                response: StatefulResponseNodeV1 {
                    id: "close_true_response".to_string(),
                    content: "close true".to_string(),
                },
            },
            on_false: StatefulBranchV1 {
                state_actions: vec![],
                effects: vec![event_owner_grant("close_grant_owner")],
                response: StatefulResponseNodeV1 {
                    id: "close_false_response".to_string(),
                    content: "close false".to_string(),
                },
            },
        });
    }
    StatefulSpecV1 {
        schema_version: 1,
        kind: STATEFUL_SPEC_KIND_V1.to_string(),
        key: "instance_program".to_string(),
        display_name: "Instance program".to_string(),
        description: "Cross-branch instance provenance.".to_string(),
        panels: vec![],
        modals: vec![],
        stateless_workflows: vec![],
        state_variables: vec![],
        stateful_workflows,
    }
}

fn event_owner_grant(id: &str) -> ActionNodeV1 {
    ActionNodeV1 {
        id: id.to_string(),
        action: ActionV1::GrantRole {
            role: RoleReferenceV1::Instance {
                instance: InstanceReferenceV1::Event,
                alias: "owner".to_string(),
            },
            target: ActionTargetV1::Actor,
        },
    }
}

#[test]
fn canonical_identity_is_strict_and_stateful_deployment_is_unavailable() {
    let spec = stateful_spec();
    let bytes = canonical_stateful_spec_bytes_v1(&spec).unwrap();
    assert_eq!(decode_canonical_stateful_spec_v1(&bytes).unwrap(), spec);
    let mut whitespace = bytes;
    whitespace.push(b'\n');
    assert!(decode_canonical_stateful_spec_v1(&whitespace).is_err());
    assert!(decode_canonical_stateful_spec_v1(&vec![b' '; 65 * 1024]).is_err());

    let status = stateful_spec_deployment_status_v1(&spec).unwrap();
    assert!(!status.deployable);
    assert!(status.compilation_available);
    assert_eq!(
        status.blockers,
        vec![StatefulSpecDeploymentBlockerV1::StatefulRuntimeUnavailable]
    );
    let preview = preview_stateful_spec_v1(&spec).unwrap();
    assert_eq!(preview.state_variable_count, 3);
    assert_eq!(preview.stateful_workflow_count, 1);
    assert!(!preview.deployment.deployable);
}

#[test]
fn wire_rejects_unknown_and_duplicate_struct_fields() {
    let mut value = serde_json::to_value(stateful_spec()).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("unknown".to_string(), serde_json::Value::Bool(true));
    assert!(serde_json::from_value::<StatefulSpecV1>(value).is_err());

    let duplicate = r#"{
        "schema_version":1,
        "schema_version":1,
        "kind":"starring.stateful-spec.v1",
        "key":"duplicate",
        "display_name":"Duplicate",
        "description":"",
        "stateless_workflows":[],
        "state_variables":[],
        "stateful_workflows":[]
    }"#;
    assert!(serde_json::from_str::<StatefulSpecV1>(duplicate).is_err());
}

#[test]
fn simulation_normalizes_inputs_fills_defaults_and_uses_parallel_pre_state() {
    let result = simulate_stateful_spec_v1(
        &stateful_spec(),
        &fixture(vec![StateSimulationCellV1 {
            variable_id: "count".to_string(),
            value: StateValueV1::Integer { value: 4 },
        }]),
    )
    .unwrap();
    assert_eq!(
        result.trace.outcome,
        StatefulSimulationOutcomeV1::StatefulBranchPlanned
    );
    assert_eq!(result.trace.branch, Some(StatefulBranchSelectionV1::True));
    assert_eq!(result.trace.normalized_inputs["note"], "hello");
    assert_eq!(
        result.trace.state_after["count"],
        StateValueV1::Integer { value: 5 }
    );
    // This proves every RHS observes the same pre-state, not the earlier count assignment.
    assert_eq!(
        result.trace.state_after["previous"],
        StateValueV1::Integer { value: 4 }
    );
    assert_eq!(
        result.trace.state_after["note"],
        StateValueV1::Text {
            value: "hello".to_string()
        }
    );
    assert_eq!(result.trace.external_node_ids, vec!["increment_response"]);
    let trace_bytes = canonical_stateful_simulation_trace_bytes_v1(&result.trace).unwrap();
    assert_eq!(
        decode_canonical_stateful_simulation_trace_v1(&trace_bytes).unwrap(),
        result.trace
    );
}

#[test]
fn empty_fixture_supports_first_event_and_new_additive_variables() {
    let result = simulate_stateful_spec_v1(&stateful_spec(), &fixture(vec![])).unwrap();
    assert_eq!(
        result.trace.state_before["count"],
        StateValueV1::Integer { value: 0 }
    );
    assert_eq!(
        result.trace.state_before["note"],
        StateValueV1::Text {
            value: String::new()
        }
    );
}

#[test]
fn duplicate_fixture_cells_and_nul_text_fail_closed() {
    let duplicate = fixture(vec![
        StateSimulationCellV1 {
            variable_id: "count".to_string(),
            value: StateValueV1::Integer { value: 1 },
        },
        StateSimulationCellV1 {
            variable_id: "count".to_string(),
            value: StateValueV1::Integer { value: 2 },
        },
    ]);
    let error = simulate_stateful_spec_v1(&stateful_spec(), &duplicate).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "simulation_state_duplicate"));

    let mut declaration = stateful_spec();
    declaration.state_variables[2].initial_value = StateValueV1::Text {
        value: "bad\0text".to_string(),
    };
    assert!(validate_stateful_spec_v1(&declaration)
        .unwrap_err()
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "state_text_contains_nul"));

    let mut input = fixture(vec![]);
    input
        .event
        .inputs
        .insert("note".to_string(), "bad\0text".to_string());
    let error = simulate_stateful_spec_v1(&stateful_spec(), &input).unwrap_err();
    assert!(matches!(
        error,
        automation_stateful_spec::StatefulSimulationErrorV1::Evaluation {
            code: "input_text_contains_nul",
            ..
        }
    ));
}

#[test]
fn fixture_count_and_total_size_are_bounded_before_evaluation() {
    let cells = (0..=MAX_STATEFUL_SIMULATION_CELLS_V1)
        .map(|index| StateSimulationCellV1 {
            variable_id: format!("v{index}"),
            value: StateValueV1::Bool { value: false },
        })
        .collect();
    let error = simulate_stateful_spec_v1(&stateful_spec(), &fixture(cells)).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "simulation_state_cell_count_exceeded"));

    let mut oversized = fixture(vec![]);
    oversized
        .event
        .inputs
        .insert("note".to_string(), "x".repeat(70 * 1024));
    let error = simulate_stateful_spec_v1(&stateful_spec(), &oversized).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "simulation_input_prebound_exceeded"));

    let encoded_oversized = fixture(
        (0..MAX_STATEFUL_SIMULATION_CELLS_V1)
            .map(|index| StateSimulationCellV1 {
                variable_id: format!("v{index}"),
                value: StateValueV1::Text {
                    value: "x".repeat(1_000),
                },
            })
            .collect(),
    );
    let error = simulate_stateful_spec_v1(&stateful_spec(), &encoded_oversized).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "simulation_fixture_too_large"));
}

#[test]
fn largest_accepted_fixture_shape_produces_a_bounded_trace_identity() {
    let mut spec = stateful_spec();
    spec.state_variables[2].value_type = StateValueTypeV1::Text {
        max_utf8_bytes: 1_000,
    };
    for index in 0..61 {
        spec.state_variables.push(StateVariableV1 {
            id: format!("v{index}"),
            scope: StateScopeV1::Installation,
            value_type: StateValueTypeV1::Text {
                max_utf8_bytes: 1_000,
            },
            initial_value: StateValueV1::Text {
                value: String::new(),
            },
        });
    }
    let mut cells = vec![
        StateSimulationCellV1 {
            variable_id: "count".to_string(),
            value: StateValueV1::Integer { value: 1 },
        },
        StateSimulationCellV1 {
            variable_id: "previous".to_string(),
            value: StateValueV1::Integer { value: 0 },
        },
        StateSimulationCellV1 {
            variable_id: "note".to_string(),
            value: StateValueV1::Text {
                value: "n".repeat(700),
            },
        },
    ];
    cells.extend((0..61).map(|index| StateSimulationCellV1 {
        variable_id: format!("v{index}"),
        value: StateValueV1::Text {
            value: "x".repeat(700),
        },
    }));
    assert_eq!(cells.len(), MAX_STATEFUL_SIMULATION_CELLS_V1);
    let result = simulate_stateful_spec_v1(&spec, &fixture(cells)).unwrap();
    assert!(canonical_stateful_simulation_trace_bytes_v1(&result.trace).is_ok());
}

#[test]
fn escaped_control_input_can_fan_out_to_every_branch_write_with_a_trace_identity() {
    let mut spec = stateful_spec();
    spec.modals[0].fields[0].max_length = Some(4_000);
    spec.state_variables.clear();
    spec.stateful_workflows[0].condition = StatefulConditionExprV1::Always;
    spec.stateful_workflows[0].on_true.state_actions.clear();
    for index in 0..MAX_STATE_ACTIONS_PER_BRANCH_V1 {
        spec.state_variables.push(StateVariableV1 {
            id: format!("text_{index}"),
            scope: StateScopeV1::Actor,
            value_type: StateValueTypeV1::Text {
                max_utf8_bytes: 4_000,
            },
            initial_value: StateValueV1::Text {
                value: String::new(),
            },
        });
        spec.stateful_workflows[0]
            .on_true
            .state_actions
            .push(StateSetNodeV1 {
                id: format!("set_text_{index}"),
                variable_id: format!("text_{index}"),
                value: StatefulValueExprV1::InputText {
                    input_id: "note".to_string(),
                },
            });
    }
    let escaped_control = "\u{0001}".repeat(4_000);
    let input = StatefulSimulationInputV1 {
        event: StatefulSimulationEventV1 {
            trigger: TriggerV1::ModalSubmit {
                modal_id: "counter_form".to_string(),
            },
            inputs: BTreeMap::from([("note".to_string(), escaped_control)]),
        },
        state: vec![],
    };
    let result = simulate_stateful_spec_v1(&spec, &input).unwrap();
    let bytes = canonical_stateful_simulation_trace_bytes_v1(&result.trace).unwrap();
    assert!(bytes.len() > 1024 * 1024);
    assert!(
        bytes.len() < automation_stateful_spec::MAX_STATEFUL_SIMULATION_TRACE_CANONICAL_BYTES_V1
    );
}

#[test]
fn state_write_set_is_capped_at_runtime_atomic_limit() {
    let mut spec = stateful_spec();
    spec.state_variables.clear();
    spec.stateful_workflows[0].condition = StatefulConditionExprV1::Always;
    spec.stateful_workflows[0].on_true.state_actions.clear();
    for index in 0..=MAX_STATE_ACTIONS_PER_BRANCH_V1 {
        spec.state_variables.push(StateVariableV1 {
            id: format!("flag_{index}"),
            scope: StateScopeV1::Installation,
            value_type: StateValueTypeV1::Bool,
            initial_value: StateValueV1::Bool { value: false },
        });
        spec.stateful_workflows[0]
            .on_true
            .state_actions
            .push(StateSetNodeV1 {
                id: format!("set_flag_{index}"),
                variable_id: format!("flag_{index}"),
                value: StatefulValueExprV1::Literal {
                    value: StateValueV1::Bool { value: true },
                },
            });
    }
    assert!(validate_stateful_spec_v1(&spec)
        .unwrap_err()
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "branch_state_action_count_exceeded"));
}

#[test]
fn event_normalizer_reuses_modal_policy_and_rejects_nonderived_inputs() {
    let spec = stateful_spec();
    let trigger = TriggerV1::ModalSubmit {
        modal_id: "counter_form".to_string(),
    };
    let normalized = normalize_stateful_event_inputs_v1(
        &spec,
        &trigger,
        &BTreeMap::from([("note".to_string(), " hi ".to_string())]),
    )
    .unwrap();
    assert_eq!(normalized["note"], "hi");

    let error = normalize_stateful_event_inputs_v1(
        &spec,
        &TriggerV1::ButtonClick {
            trigger_id: "unknown".to_string(),
        },
        &BTreeMap::from([("note".to_string(), "x".to_string())]),
    )
    .unwrap_err();
    assert!(!error.diagnostics().is_empty());

    let error = normalize_stateful_event_inputs_v1(
        &spec,
        &trigger,
        &BTreeMap::from([("note".to_string(), "bad\0text".to_string())]),
    )
    .unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "event_input_contains_nul"));
}

#[test]
fn stateless_workflows_require_always_and_a_workflow_is_required() {
    let mut spec = stateful_spec();
    spec.stateful_workflows.clear();
    spec.state_variables.clear();
    spec.stateless_workflows = vec![WorkflowSpecV1 {
        id: "static".to_string(),
        trigger: TriggerV1::ButtonClick {
            trigger_id: "static".to_string(),
        },
        condition: ConditionExprV1::InputNonEmpty {
            input_id: "note".to_string(),
        },
        actions: vec![ActionNodeV1 {
            id: "respond".to_string(),
            action: ActionV1::RespondEphemeral {
                content: "ok".to_string(),
            },
        }],
    }];
    let error = validate_stateful_spec_v1(&spec).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "stateless_condition_must_be_always"));
    spec.stateless_workflows.clear();
    assert!(validate_stateful_spec_v1(&spec)
        .unwrap_err()
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "invalid_workflow_count"));
}

#[test]
fn duplicate_branch_write_response_effect_and_js_unsafe_integer_are_rejected() {
    let mut duplicate = stateful_spec();
    duplicate.stateful_workflows[0]
        .on_true
        .state_actions
        .push(StateSetNodeV1 {
            id: "second_count_write".to_string(),
            variable_id: "count".to_string(),
            value: StatefulValueExprV1::Literal {
                value: StateValueV1::Integer { value: 2 },
            },
        });
    assert!(validate_stateful_spec_v1(&duplicate)
        .unwrap_err()
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "duplicate_branch_state_write"));

    let mut response_effect = stateful_spec();
    response_effect.stateful_workflows[0]
        .on_true
        .effects
        .push(ActionNodeV1 {
            id: "illegal_response".to_string(),
            action: ActionV1::EditResponse {
                content: "bad".to_string(),
            },
        });
    assert!(validate_stateful_spec_v1(&response_effect)
        .unwrap_err()
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "response_action_forbidden_in_effects"));

    let mut unsafe_integer = stateful_spec();
    unsafe_integer.state_variables[0].value_type = StateValueTypeV1::Integer {
        min: 0,
        max: 9_007_199_254_740_992,
    };
    assert!(validate_stateful_spec_v1(&unsafe_integer)
        .unwrap_err()
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "invalid_integer_bounds"));
}

#[test]
fn checked_arithmetic_and_assignment_bounds_fail_closed() {
    let mut overflow = stateful_spec();
    overflow.state_variables[0].value_type = StateValueTypeV1::Integer {
        min: 0,
        max: MAX_SAFE_INTEGER_V1,
    };
    overflow.stateful_workflows[0].condition = StatefulConditionExprV1::Always;
    let error = simulate_stateful_spec_v1(
        &overflow,
        &fixture(vec![StateSimulationCellV1 {
            variable_id: "count".to_string(),
            value: StateValueV1::Integer {
                value: MAX_SAFE_INTEGER_V1,
            },
        }]),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        automation_stateful_spec::StatefulSimulationErrorV1::Evaluation {
            code: "integer_overflow",
            ..
        }
    ));

    let mut out_of_bounds = stateful_spec();
    out_of_bounds.stateful_workflows[0].condition = StatefulConditionExprV1::Always;
    let error = simulate_stateful_spec_v1(
        &out_of_bounds,
        &fixture(vec![StateSimulationCellV1 {
            variable_id: "count".to_string(),
            value: StateValueV1::Integer { value: 100 },
        }]),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        automation_stateful_spec::StatefulSimulationErrorV1::Evaluation {
            code: "state_value_out_of_bounds",
            ..
        }
    ));

    let mut unsafe_literal = stateful_spec();
    unsafe_literal.stateful_workflows[0].condition = StatefulConditionExprV1::IntegerCompare {
        left: StatefulValueExprV1::State {
            variable_id: "count".to_string(),
        },
        operator: IntegerComparisonV1::Equal,
        right: StatefulValueExprV1::Literal {
            value: StateValueV1::Integer {
                value: MAX_SAFE_INTEGER_V1 + 1,
            },
        },
    };
    assert!(validate_stateful_spec_v1(&unsafe_literal)
        .unwrap_err()
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "integer_value_not_js_safe"));

    let mut constant_overflow = stateful_spec();
    constant_overflow.stateful_workflows[0]
        .on_true
        .state_actions[0]
        .value = StatefulValueExprV1::CheckedAdd {
        left: Box::new(StatefulValueExprV1::Literal {
            value: StateValueV1::Integer {
                value: MAX_SAFE_INTEGER_V1,
            },
        }),
        right: Box::new(StatefulValueExprV1::Literal {
            value: StateValueV1::Integer { value: 1 },
        }),
    };
    assert!(validate_stateful_spec_v1(&constant_overflow)
        .unwrap_err()
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "constant_checked_arithmetic_overflow"));
}

#[test]
fn mixed_branch_instance_handler_requirements_are_unioned() {
    let spec = cross_branch_instance_spec(false);
    let error = validate_stateful_spec_v1(&spec).unwrap_err();
    assert!(error.diagnostics().iter().any(|diagnostic| {
        diagnostic.code == "created_instance_missing_cross_branch_handler_resource"
    }));
}

#[test]
fn transitive_forwarding_unions_downstream_requirements_across_branches() {
    let spec = cross_branch_instance_spec(true);
    let error = validate_stateful_spec_v1(&spec).unwrap_err();
    assert!(error.diagnostics().iter().any(|diagnostic| {
        diagnostic.code == "created_instance_missing_cross_branch_handler_resource"
            && diagnostic.message.contains("owner")
    }));
}

#[test]
fn generated_branch_views_enforce_route_coverage() {
    let mut spec = cross_branch_instance_spec(false);
    spec.stateful_workflows[1].trigger = TriggerV1::InstanceAction {
        action_id: "different_handler".to_string(),
    };
    let error = validate_stateful_spec_v1(&spec).unwrap_err();
    assert!(error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "runtime_shape_rendered_route_without_handler"));
}

#[test]
fn trace_digest_rejects_condition_branch_mismatch_and_oversized_raw_bytes() {
    let mut result = simulate_stateful_spec_v1(&stateful_spec(), &fixture(vec![])).unwrap();
    result.trace.branch = Some(StatefulBranchSelectionV1::False);
    assert!(canonical_stateful_simulation_trace_bytes_v1(&result.trace).is_err());
    assert!(
        decode_canonical_stateful_simulation_trace_v1(&vec![b' '; 8 * 1024 * 1024 + 1]).is_err()
    );
}

#[test]
fn shared_core_matches_simulation_and_is_not_bound_by_fixture_transport_size() {
    let spec = stateful_spec();
    let input = fixture(vec![]);
    let simulation = simulate_stateful_spec_v1(&spec, &input).unwrap();
    let normalized = simulation.trace.normalized_inputs.clone();
    let core = evaluate_validated_stateful_workflow_v1(
        &spec,
        &input.event.trigger,
        &normalized,
        &simulation.trace.state_before,
    )
    .unwrap();
    assert_eq!(
        core.workflow_id(),
        simulation.trace.workflow_id.as_deref().unwrap()
    );
    assert_eq!(
        core.condition_result(),
        simulation.trace.condition_result.unwrap()
    );
    assert_eq!(core.state_after(), &simulation.trace.state_after);
    assert_eq!(core.external_node_ids(), simulation.trace.external_node_ids);

    let mut large = spec.clone();
    large.state_variables.clear();
    large.stateful_workflows[0].condition = StatefulConditionExprV1::Always;
    large.stateful_workflows[0].on_true.state_actions.clear();
    large.stateful_workflows[0].on_false.state_actions.clear();
    let mut state = BTreeMap::new();
    for index in 0..20 {
        let id = format!("text_{index}");
        large.state_variables.push(StateVariableV1 {
            id: id.clone(),
            scope: StateScopeV1::Actor,
            value_type: StateValueTypeV1::Text {
                max_utf8_bytes: 4_000,
            },
            initial_value: StateValueV1::Text {
                value: String::new(),
            },
        });
        state.insert(
            id,
            StateValueV1::Text {
                value: "x".repeat(4_000),
            },
        );
    }
    let encoded_fixture = serde_json::to_vec(&StatefulSimulationInputV1 {
        event: StatefulSimulationEventV1 {
            trigger: input.event.trigger.clone(),
            inputs: normalized.clone(),
        },
        state: state
            .iter()
            .map(|(variable_id, value)| StateSimulationCellV1 {
                variable_id: variable_id.clone(),
                value: value.clone(),
            })
            .collect(),
    })
    .unwrap();
    assert!(encoded_fixture.len() > 64 * 1_024);
    let core =
        evaluate_validated_stateful_workflow_v1(&large, &input.event.trigger, &normalized, &state)
            .unwrap();
    assert_eq!(
        core.external_node_ids(),
        &["increment_response".to_string()]
    );
}
