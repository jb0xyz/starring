use automation_state::TriggerSpec;
use automation_stateful_compiler::{
    canonical_stateful_bundle_bytes_v1, check_additive_state_schema_compatibility_v1,
    compile_stateful_spec_bundle_v1, decode_canonical_stateful_bundle_v1,
    stateful_artifact_digest_v1, stateful_bundle_digest_v1, stateful_compilation_binding_digest_v1,
    stateful_state_schema_digest_v1, stateful_union_source_map_digest_v1,
    CompiledAcknowledgementStrategyV1, StateSchemaCompatibilityErrorV1,
    StatefulCompilationIdentityErrorV1,
};
use automation_stateful_spec::{
    ActionNodeV1, ActionV1, IntegerComparisonV1, ModalDefinitionV1, ModalFieldDefinitionV1,
    ModalFieldStyleV1, ModalInputPolicyV1, StateScopeV1, StateSetNodeV1, StateValueTypeV1,
    StateValueV1, StateVariableV1, StatefulBranchV1, StatefulConditionExprV1,
    StatefulResponseNodeV1, StatefulSpecV1, StatefulValueExprV1, StatefulWorkflowV1, TriggerV1,
    WorkflowSpecV1, STATEFUL_SPEC_KIND_V1,
};

fn fixture() -> StatefulSpecV1 {
    StatefulSpecV1 {
        schema_version: 1,
        kind: STATEFUL_SPEC_KIND_V1.to_string(),
        key: "compiled_counter".to_string(),
        display_name: "Compiled counter".to_string(),
        description: "Compiler contract fixture".to_string(),
        panels: vec![],
        modals: vec![ModalDefinitionV1 {
            id: "counter_form".to_string(),
            title: "Counter".to_string(),
            fields: vec![ModalFieldDefinitionV1 {
                id: "note_input".to_string(),
                label: "Note".to_string(),
                style: ModalFieldStyleV1::Short,
                required: true,
                min_length: Some(1),
                max_length: Some(10),
                input_policy: ModalInputPolicyV1::Preserve,
            }],
        }],
        stateless_workflows: vec![WorkflowSpecV1 {
            id: "ping".to_string(),
            trigger: TriggerV1::InstanceAction {
                action_id: "ping".to_string(),
            },
            condition: Default::default(),
            actions: vec![
                ActionNodeV1 {
                    id: "ping_defer".to_string(),
                    action: ActionV1::DeferEphemeral,
                },
                ActionNodeV1 {
                    id: "ping_response".to_string(),
                    action: ActionV1::EditResponse {
                        content: "pong".to_string(),
                    },
                },
            ],
        }],
        state_variables: vec![
            StateVariableV1 {
                id: "count".to_string(),
                scope: StateScopeV1::Actor,
                value_type: StateValueTypeV1::Integer { min: 0, max: 10 },
                initial_value: StateValueV1::Integer { value: 0 },
            },
            StateVariableV1 {
                id: "note".to_string(),
                scope: StateScopeV1::Actor,
                value_type: StateValueTypeV1::Text { max_utf8_bytes: 10 },
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
                    value: StateValueV1::Integer { value: 10 },
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
                        id: "save_note".to_string(),
                        variable_id: "note".to_string(),
                        value: StatefulValueExprV1::InputText {
                            input_id: "note_input".to_string(),
                        },
                    },
                ],
                effects: vec![ActionNodeV1 {
                    id: "create_audit".to_string(),
                    action: ActionV1::CreateChannel {
                        output: "audit".to_string(),
                        name: "audit".to_string(),
                    },
                }],
                response: StatefulResponseNodeV1 {
                    id: "increment_response".to_string(),
                    content: "incremented".to_string(),
                },
            },
            on_false: StatefulBranchV1 {
                state_actions: vec![],
                effects: vec![],
                response: StatefulResponseNodeV1 {
                    id: "limit_response".to_string(),
                    content: "at limit".to_string(),
                },
            },
        }],
    }
}

#[test]
fn stateful_workflow_never_appears_in_the_legacy_runtime_target() {
    let bundle = compile_stateful_spec_bundle_v1(&fixture()).unwrap();
    assert_eq!(bundle.filtered_legacy_ruleset().rules.len(), 1);
    assert_eq!(bundle.filtered_legacy_ruleset().rules[0].key, "ping");
    assert!(bundle
        .filtered_legacy_ruleset()
        .rules
        .iter()
        .all(|rule| rule.key != "increment"));
    assert!(bundle
        .filtered_legacy_ruleset()
        .rules
        .iter()
        .all(|rule| !matches!(
            &rule.trigger,
            TriggerSpec::ModalSubmit { modal } if modal == "counter_form"
        )));

    let stateful = &bundle.stateful_artifact().workflows()[0];
    assert_eq!(stateful.id(), "increment");
    assert_eq!(
        stateful.acknowledgement(),
        CompiledAcknowledgementStrategyV1::DeferEphemeralBeforeCommit
    );
    assert_eq!(
        stateful.dependencies().read_state_variable_indices(),
        &[0, 1]
    );
    assert_eq!(
        stateful.dependencies().write_state_variable_indices(),
        &[0, 1]
    );
}

#[test]
fn source_map_preserves_every_source_order_and_branch_ordinal() {
    let bundle = compile_stateful_spec_bundle_v1(&fixture()).unwrap();
    let map = bundle.union_source_map();
    let stateless = map.stateless_workflows()[0].workflow();
    assert_eq!(stateless.workflow_id, "ping");
    assert_eq!(stateless.source_workflow_index, 0);
    assert_eq!(stateless.target_rule_index, 0);
    assert_eq!(stateless.actions[0].action_node_id, "ping_defer");
    assert_eq!(stateless.actions[0].source_action_index, 0);
    assert_eq!(stateless.actions[1].target_action_index, 1);

    assert_eq!(map.state_variables()[0].variable_id(), "count");
    assert_eq!(map.state_variables()[0].source_variable_index(), 0);
    assert_eq!(map.state_variables()[1].artifact_variable_index(), 1);

    let workflow = &map.stateful_workflows()[0];
    assert_eq!(workflow.workflow_id(), "increment");
    assert_eq!(workflow.source_workflow_index(), 0);
    assert_eq!(workflow.artifact_workflow_index(), 0);
    let branch = workflow.on_true();
    assert_eq!(branch.implicit_acknowledgement_ordinal(), 0);
    assert_eq!(branch.state_actions()[0].node_id(), "increment_count");
    assert_eq!(branch.state_actions()[0].source_node_index(), 0);
    assert_eq!(branch.state_actions()[0].execution_ordinal(), 1);
    assert_eq!(branch.state_actions()[1].execution_ordinal(), 2);
    assert_eq!(branch.effects()[0].node_id(), "create_audit");
    assert_eq!(branch.effects()[0].execution_ordinal(), 3);
    assert_eq!(branch.response().node_id(), "increment_response");
    assert_eq!(branch.response().execution_ordinal(), 4);
    assert_eq!(workflow.on_false().response().execution_ordinal(), 1);
}

#[test]
fn zero_stateless_workflows_yields_a_structural_zero_rule_target() {
    let mut spec = fixture();
    spec.stateless_workflows.clear();
    let bundle = compile_stateful_spec_bundle_v1(&spec).unwrap();
    assert!(bundle.filtered_legacy_ruleset().rules.is_empty());
    assert!(automation_core::validate_structural(bundle.filtered_legacy_ruleset()).is_ok());
    assert!(bundle.union_source_map().stateless_workflows().is_empty());
    assert_eq!(bundle.stateful_artifact().workflows().len(), 1);
}

#[test]
fn every_identity_is_reproducible_and_bundle_decode_recompiles_source() {
    let bundle = compile_stateful_spec_bundle_v1(&fixture()).unwrap();
    assert_eq!(
        stateful_state_schema_digest_v1(bundle.stateful_artifact().state_schema()).unwrap(),
        bundle.state_schema_digest()
    );
    assert_eq!(
        stateful_artifact_digest_v1(bundle.stateful_artifact()).unwrap(),
        bundle.stateful_artifact_digest()
    );
    assert_eq!(
        stateful_union_source_map_digest_v1(bundle.union_source_map()).unwrap(),
        bundle.union_source_map_digest()
    );
    assert_eq!(
        stateful_compilation_binding_digest_v1(bundle.binding()).unwrap(),
        bundle.binding_digest()
    );
    assert_eq!(
        bundle.binding().source().digest(),
        bundle.stateful_artifact().source().digest()
    );
    assert_eq!(
        bundle.binding().filtered_legacy_target(),
        bundle.filtered_legacy_target()
    );
    assert_eq!(
        bundle.binding().stateful_artifact().digest(),
        bundle.stateful_artifact_digest()
    );
    assert_eq!(
        bundle.binding().state_schema().digest(),
        bundle.state_schema_digest()
    );
    assert_eq!(
        bundle.binding().source_map().digest(),
        bundle.union_source_map_digest()
    );
    assert_eq!(
        stateful_bundle_digest_v1(&bundle).unwrap(),
        bundle.bundle_digest()
    );

    let bytes = canonical_stateful_bundle_bytes_v1(&bundle).unwrap();
    let decoded = decode_canonical_stateful_bundle_v1(&bytes).unwrap();
    assert!(decoded == bundle);
}

#[test]
fn generated_component_tampering_and_noncanonical_bytes_fail_closed() {
    let bundle = compile_stateful_spec_bundle_v1(&fixture()).unwrap();
    let bytes = canonical_stateful_bundle_bytes_v1(&bundle).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value["stateful_artifact"]["workflows"][0]["id"] =
        serde_json::Value::String("forged".to_string());
    let tampered = serde_json::to_vec(&value).unwrap();
    assert_eq!(
        decode_canonical_stateful_bundle_v1(&tampered)
            .err()
            .unwrap(),
        StatefulCompilationIdentityErrorV1::BundleMismatch
    );

    let mut extra = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap();
    extra
        .as_object_mut()
        .unwrap()
        .insert("unknown".to_string(), serde_json::Value::Bool(true));
    assert_eq!(
        decode_canonical_stateful_bundle_v1(&serde_json::to_vec(&extra).unwrap())
            .err()
            .unwrap(),
        StatefulCompilationIdentityErrorV1::NonCanonicalBundle
    );

    let mut whitespace = bytes;
    whitespace.push(b'\n');
    assert_eq!(
        decode_canonical_stateful_bundle_v1(&whitespace)
            .err()
            .unwrap(),
        StatefulCompilationIdentityErrorV1::BundleMismatch
    );

    for (component, field) in [
        ("filtered_legacy_ruleset", "version"),
        ("union_source_map", "compiler_revision"),
        ("binding", "compiler_revision"),
    ] {
        let canonical = canonical_stateful_bundle_bytes_v1(&bundle).unwrap();
        let mut tampered: serde_json::Value = serde_json::from_slice(&canonical).unwrap();
        tampered[component][field] = serde_json::Value::from(99);
        assert_eq!(
            decode_canonical_stateful_bundle_v1(&serde_json::to_vec(&tampered).unwrap())
                .err()
                .unwrap(),
            StatefulCompilationIdentityErrorV1::BundleMismatch
        );
    }
}

#[test]
fn stateful_source_changes_are_bound_without_changing_filtered_legacy_identity() {
    let baseline = compile_stateful_spec_bundle_v1(&fixture()).unwrap();
    let mut changed = fixture();
    changed.stateful_workflows[0].on_true.response.content = "changed".to_string();
    let changed = compile_stateful_spec_bundle_v1(&changed).unwrap();

    assert_eq!(
        baseline.filtered_legacy_target(),
        changed.filtered_legacy_target()
    );
    assert_ne!(
        baseline.stateful_artifact_digest(),
        changed.stateful_artifact_digest()
    );
    assert_ne!(
        baseline.union_source_map_digest(),
        changed.union_source_map_digest()
    );
    assert_ne!(baseline.binding_digest(), changed.binding_digest());
    assert_ne!(baseline.bundle_digest(), changed.bundle_digest());
}

#[test]
fn schema_additions_are_append_only_and_every_existing_change_is_forbidden() {
    let baseline = compile_stateful_spec_bundle_v1(&fixture()).unwrap();
    let mut additive = fixture();
    additive.state_variables.push(StateVariableV1 {
        id: "enabled".to_string(),
        scope: StateScopeV1::Installation,
        value_type: StateValueTypeV1::Bool,
        initial_value: StateValueV1::Bool { value: true },
    });
    let additive = compile_stateful_spec_bundle_v1(&additive).unwrap();
    let compatibility = check_additive_state_schema_compatibility_v1(
        baseline.stateful_artifact().state_schema(),
        additive.stateful_artifact().state_schema(),
    )
    .unwrap();
    assert_eq!(compatibility.added_variable_ids(), &["enabled"]);
    assert_ne!(
        baseline.state_schema_digest(),
        additive.state_schema_digest()
    );

    let removal = check_additive_state_schema_compatibility_v1(
        additive.stateful_artifact().state_schema(),
        baseline.stateful_artifact().state_schema(),
    );
    assert!(matches!(
        removal,
        Err(StateSchemaCompatibilityErrorV1::ExistingVariableRemovedOrReordered { .. })
    ));

    let mut reordered = fixture();
    reordered.state_variables.swap(0, 1);
    let reordered = compile_stateful_spec_bundle_v1(&reordered).unwrap();
    assert!(matches!(
        check_additive_state_schema_compatibility_v1(
            baseline.stateful_artifact().state_schema(),
            reordered.stateful_artifact().state_schema()
        ),
        Err(StateSchemaCompatibilityErrorV1::ExistingVariableRemovedOrReordered { index: 0 })
    ));

    for mutation in [
        |spec: &mut StatefulSpecV1| spec.state_variables[0].scope = StateScopeV1::Installation,
        |spec: &mut StatefulSpecV1| {
            spec.state_variables[0].initial_value = StateValueV1::Integer { value: 1 }
        },
        |spec: &mut StatefulSpecV1| {
            spec.state_variables[0].value_type = StateValueTypeV1::Integer { min: 0, max: 11 }
        },
    ] {
        let mut candidate = fixture();
        mutation(&mut candidate);
        let candidate = compile_stateful_spec_bundle_v1(&candidate).unwrap();
        assert!(matches!(
            check_additive_state_schema_compatibility_v1(
                baseline.stateful_artifact().state_schema(),
                candidate.stateful_artifact().state_schema()
            ),
            Err(StateSchemaCompatibilityErrorV1::ExistingVariableChanged { .. })
        ));
    }

    let mut renamed_program = fixture();
    renamed_program.key = "other_program".to_string();
    let renamed_program = compile_stateful_spec_bundle_v1(&renamed_program).unwrap();
    assert_eq!(
        check_additive_state_schema_compatibility_v1(
            baseline.stateful_artifact().state_schema(),
            renamed_program.stateful_artifact().state_schema()
        ),
        Err(StateSchemaCompatibilityErrorV1::ProgramKeyChanged)
    );
}

#[test]
fn official_filtered_target_identity_changes_only_with_legacy_material() {
    let baseline = compile_stateful_spec_bundle_v1(&fixture()).unwrap();
    let mut changed = fixture();
    if let ActionV1::EditResponse { content } =
        &mut changed.stateless_workflows[0].actions[1].action
    {
        *content = "different pong".to_string();
    }
    let changed = compile_stateful_spec_bundle_v1(&changed).unwrap();
    assert_ne!(
        baseline.filtered_legacy_target().content_hash,
        changed.filtered_legacy_target().content_hash
    );
    assert_ne!(baseline.binding_digest(), changed.binding_digest());
}

#[test]
fn dependency_indices_follow_state_declaration_order_not_expression_order() {
    let mut spec = fixture();
    spec.stateful_workflows[0].condition = StatefulConditionExprV1::StateEquals {
        variable_id: "note".to_string(),
        value: StatefulValueExprV1::State {
            variable_id: "note".to_string(),
        },
    };
    let bundle = compile_stateful_spec_bundle_v1(&spec).unwrap();
    assert_eq!(
        bundle.stateful_artifact().workflows()[0]
            .dependencies()
            .read_state_variable_indices(),
        &[0, 1]
    );
}
