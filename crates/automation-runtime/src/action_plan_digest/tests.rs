use automation_core::ResolvedInstanceContext;
use automation_instance::{InstanceId, InstanceKind, InstanceMessageRef, InstanceRuleSetVersion};
use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    BindingRevision, DeploymentId, FencingToken, InstallationId, ProcessInstanceId,
    RuntimeDeploymentTargetV1, RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use automation_runtime_interaction::{
    InteractionGatewayOwnerIdentityV1, InteractionGatewayOwnerLeaseEpochV1,
    InteractionGatewayOwnerRevisionV1, InteractionGatewayShardIdentityV1,
    InteractionProductScopeV1, InteractionRouteAttestationDigestV1, InteractionRouteIncarnationV1,
    InteractionRuntimeBuildRevisionV1, InteractionServingLeaseEpochV1,
    InteractionServingLeaseRevisionV1, InteractionServingRouteIdentityV1,
};
use discord_model::{ChannelId, GuildId, MessageId, Permissions, RoleId, UserId};
use resource_resolution::ResourceBindingFingerprint;

use super::*;

fn route() -> InteractionRouteBindingV1 {
    let process = RuntimeProcessIdentityV1 {
        target: RuntimeDeploymentTargetV1 {
            guild_id: GuildId(101),
            ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
            version: RuleSetVersionId::FIRST,
            content_hash: RuleSetContentHash::parse_hex(&"a".repeat(64)).unwrap(),
            binding_revision: BindingRevision::new(2).unwrap(),
            binding_fingerprint: ResourceBindingFingerprint::parse(&"b".repeat(64)).unwrap(),
        },
        runtime_generation: RuntimeGeneration::new(3).unwrap(),
        process_instance_id: ProcessInstanceId::parse("process-4").unwrap(),
    };
    let serving = InteractionServingRouteIdentityV1::new(
        InteractionRouteAttestationDigestV1::parse("c".repeat(64)).unwrap(),
        InteractionServingLeaseEpochV1::new(5).unwrap(),
        InteractionServingLeaseRevisionV1::new(6).unwrap(),
        InteractionGatewayOwnerIdentityV1::new(
            InteractionGatewayShardIdentityV1::parse("gateway-shard-1").unwrap(),
            InteractionGatewayOwnerLeaseEpochV1::new(7).unwrap(),
            InteractionGatewayOwnerRevisionV1::new(8).unwrap(),
            InteractionRuntimeBuildRevisionV1::parse("build-plan-digest-1").unwrap(),
        ),
        FencingToken::new(9).unwrap(),
        InteractionRouteIncarnationV1::new(10).unwrap(),
    );
    InteractionRouteBindingV1::new_static(
        InteractionProductScopeV1::new(
            TenantId::parse("tenant-1").unwrap(),
            InstallationId::parse("installation-1").unwrap(),
            DeploymentId::parse("deployment-1").unwrap(),
        ),
        process,
        serving,
    )
    .unwrap()
}

fn request_digest() -> InteractionRequestDigestV1 {
    InteractionRequestDigestV1::parse("d".repeat(64)).unwrap()
}

fn context(reverse_insertions: bool) -> RuntimeContext {
    let mut inputs = BTreeMap::new();
    let mut roles = BTreeMap::new();
    let mut channels = BTreeMap::new();
    let mut messages = BTreeMap::new();
    if reverse_insertions {
        inputs.insert("topic".to_string(), "rust".to_string());
        inputs.insert("room".to_string(), "night".to_string());
        roles.insert("owner".to_string(), RoleId(203));
        roles.insert("member".to_string(), RoleId(202));
        channels.insert("voice".to_string(), ChannelId(303));
        channels.insert("room".to_string(), ChannelId(302));
        messages.insert(
            "panel".to_string(),
            InstanceMessageRef {
                channel: ChannelId(302),
                id: MessageId(402),
            },
        );
    } else {
        inputs.insert("room".to_string(), "night".to_string());
        inputs.insert("topic".to_string(), "rust".to_string());
        roles.insert("member".to_string(), RoleId(202));
        roles.insert("owner".to_string(), RoleId(203));
        channels.insert("room".to_string(), ChannelId(302));
        channels.insert("voice".to_string(), ChannelId(303));
        messages.insert(
            "panel".to_string(),
            InstanceMessageRef {
                channel: ChannelId(302),
                id: MessageId(402),
            },
        );
    }
    RuntimeContext {
        guild_id: GuildId(101),
        actor: UserId(501),
        ruleset_key: "studyroom".to_string(),
        ruleset_version: InstanceRuleSetVersion::new(1).unwrap(),
        inputs,
        instance: Some(ResolvedInstanceContext {
            instance: AutomationInstance {
                id: InstanceId::parse("instance-1").unwrap(),
                guild_id: GuildId(101),
                ruleset_key: "studyroom".to_string(),
                ruleset_version: InstanceRuleSetVersion::new(1).unwrap(),
                kind: InstanceKind("study_room".to_string()),
                created_by: UserId(501),
                resources: InstanceResources {
                    roles,
                    channels,
                    messages,
                },
                status: InstanceStatus::Active,
            },
            action: "join".to_string(),
        }),
    }
}

fn created(value: &str) -> CreatedRef {
    CreatedRef {
        created: value.to_string(),
    }
}

fn complete_plan() -> ActionPlan {
    ActionPlan {
        steps: vec![
            PlannedAction::GrantRole {
                role: PlannedRole::Resolved(RoleId(601)),
                target: UserId(501),
            },
            PlannedAction::GrantRole {
                role: PlannedRole::Created("member".to_string()),
                target: UserId(501),
            },
            PlannedAction::GrantRole {
                role: PlannedRole::Instance {
                    alias: "member".to_string(),
                },
                target: UserId(502),
            },
            PlannedAction::RespondEphemeral {
                content: "welcome".to_string(),
            },
            PlannedAction::OpenModal(ModalPresentation {
                key: "room_modal".to_string(),
                title: "Create room".to_string(),
                fields: vec![
                    ModalFieldSpec {
                        key: "room".to_string(),
                        label: "Room".to_string(),
                        style: ModalFieldStyle::Short,
                        required: true,
                        min_length: Some(2),
                        max_length: Some(40),
                        input_policy: ModalInputPolicy::TrimUnicodeWhitespace,
                    },
                    ModalFieldSpec {
                        key: "topic".to_string(),
                        label: "Topic".to_string(),
                        style: ModalFieldStyle::Paragraph,
                        required: false,
                        min_length: None,
                        max_length: None,
                        input_policy: ModalInputPolicy::Preserve,
                    },
                ],
            }),
            PlannedAction::CreateChannel {
                key: "room".to_string(),
                name: "night-room".to_string(),
            },
            PlannedAction::CreateRole {
                key: "member".to_string(),
                name: "Night member".to_string(),
            },
            PlannedAction::UpsertOverwrite {
                channel: PlannedChannel::Resolved(ChannelId(701)),
                target: PlannedOverwriteTarget::Everyone,
                allow: Permissions::VIEW_CHANNEL,
                deny: Permissions::SEND_MESSAGES,
            },
            PlannedAction::UpsertOverwrite {
                channel: PlannedChannel::Created("room".to_string()),
                target: PlannedOverwriteTarget::Role(PlannedRole::Created("member".to_string())),
                allow: Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
                deny: Permissions::empty(),
            },
            PlannedAction::PostPanel {
                key: "join_panel".to_string(),
                channel: PlannedChannel::Resolved(ChannelId(701)),
                content: "Join".to_string(),
                buttons: vec![
                    ButtonSpec {
                        label: "Help".to_string(),
                        route: ButtonRoute::Static {
                            key: "help".to_string(),
                        },
                    },
                    ButtonSpec {
                        label: "Join".to_string(),
                        route: ButtonRoute::InstanceAction {
                            instance: InstanceRef::Event,
                            action: "join".to_string(),
                        },
                    },
                    ButtonSpec {
                        label: "Leave".to_string(),
                        route: ButtonRoute::InstanceAction {
                            instance: InstanceRef::Created(created("room_instance")),
                            action: "leave".to_string(),
                        },
                    },
                ],
            },
            PlannedAction::DeferEphemeral,
            PlannedAction::EditResponse {
                content: "ready".to_string(),
            },
            PlannedAction::RegisterInstance {
                key: "room_instance".to_string(),
                kind: InstanceKind("study_room".to_string()),
                resources: InstanceResourceRefs {
                    roles: BTreeMap::from([("member".to_string(), created("member"))]),
                    channels: BTreeMap::from([("room".to_string(), created("room"))]),
                    messages: BTreeMap::from([("panel".to_string(), created("join_panel"))]),
                },
            },
            PlannedAction::TeardownInstance {
                instance: InstanceRef::Event,
            },
            PlannedAction::TeardownInstance {
                instance: InstanceRef::Created(created("room_instance")),
            },
        ],
    }
}

fn digest(
    context: &RuntimeContext,
    plan: &ActionPlan,
    leading: bool,
) -> InteractionActionPlanDigestV1 {
    build_interaction_action_plan_digest_v1(&route(), &request_digest(), context, plan, leading)
        .unwrap()
}

fn assert_action_mutation_changes(
    context: &RuntimeContext,
    plan: &ActionPlan,
    baseline: &InteractionActionPlanDigestV1,
    index: usize,
    mutate: impl FnOnce(&mut PlannedAction),
) {
    let mut changed = plan.clone();
    mutate(&mut changed.steps[index]);
    assert_ne!(baseline, &digest(context, &changed, true));
}

#[test]
fn complete_projection_is_deterministic_and_covers_every_action_shape() {
    let plan = complete_plan();
    let first = digest(&context(false), &plan, true);
    let second = digest(&context(true), &plan, true);

    assert_eq!(first, second);
    assert_eq!(first.as_str().len(), 64);
    assert_eq!(plan.steps.len(), 15);
}

#[test]
fn plan_order_acknowledgement_and_single_action_fields_change_the_digest() {
    let context = context(false);
    let plan = complete_plan();
    let baseline = digest(&context, &plan, true);
    let mut reordered = plan.clone();
    reordered.steps.swap(5, 6);
    assert_ne!(baseline, digest(&context, &reordered, true));
    assert_ne!(baseline, digest(&context, &plan, false));

    let replacements = vec![
        (
            0,
            PlannedAction::GrantRole {
                role: PlannedRole::Resolved(RoleId(999)),
                target: UserId(501),
            },
        ),
        (
            3,
            PlannedAction::RespondEphemeral {
                content: "changed".to_string(),
            },
        ),
        (
            4,
            PlannedAction::OpenModal(ModalPresentation {
                key: "changed".to_string(),
                title: "Create room".to_string(),
                fields: vec![],
            }),
        ),
        (
            5,
            PlannedAction::CreateChannel {
                key: "room".to_string(),
                name: "changed".to_string(),
            },
        ),
        (
            6,
            PlannedAction::CreateRole {
                key: "member".to_string(),
                name: "changed".to_string(),
            },
        ),
        (
            7,
            PlannedAction::UpsertOverwrite {
                channel: PlannedChannel::Resolved(ChannelId(701)),
                target: PlannedOverwriteTarget::Everyone,
                allow: Permissions::MANAGE_CHANNELS,
                deny: Permissions::SEND_MESSAGES,
            },
        ),
        (
            9,
            PlannedAction::PostPanel {
                key: "join_panel".to_string(),
                channel: PlannedChannel::Resolved(ChannelId(701)),
                content: "changed".to_string(),
                buttons: vec![],
            },
        ),
        (
            10,
            PlannedAction::RespondEphemeral {
                content: String::new(),
            },
        ),
        (
            11,
            PlannedAction::EditResponse {
                content: "changed".to_string(),
            },
        ),
        (
            12,
            PlannedAction::RegisterInstance {
                key: "room_instance".to_string(),
                kind: InstanceKind("changed".to_string()),
                resources: InstanceResourceRefs::default(),
            },
        ),
        (
            13,
            PlannedAction::TeardownInstance {
                instance: InstanceRef::Created(created("changed")),
            },
        ),
    ];
    for (index, replacement) in replacements {
        let mut changed = plan.clone();
        changed.steps[index] = replacement;
        assert_ne!(baseline, digest(&context, &changed, true));
    }
}

#[test]
fn every_runtime_context_identity_and_instance_resource_is_bound() {
    let plan = complete_plan();
    let context = context(false);
    let baseline = digest(&context, &plan, false);

    let mut variants = Vec::new();
    let mut changed = context.clone();
    changed.guild_id = GuildId(999);
    variants.push(changed);
    let mut changed = context.clone();
    changed.actor = UserId(999);
    variants.push(changed);
    let mut changed = context.clone();
    changed.ruleset_key = "changed".to_string();
    variants.push(changed);
    let mut changed = context.clone();
    changed.ruleset_version = InstanceRuleSetVersion::new(2).unwrap();
    variants.push(changed);
    let mut changed = context.clone();
    changed
        .inputs
        .insert("room".to_string(), "changed".to_string());
    variants.push(changed);

    let mut change_instance = |change: &dyn Fn(&mut ResolvedInstanceContext)| {
        let mut changed = context.clone();
        change(changed.instance.as_mut().unwrap());
        variants.push(changed);
    };
    change_instance(&|resolved| resolved.action = "changed".to_string());
    change_instance(&|resolved| resolved.instance.id = InstanceId::parse("changed").unwrap());
    change_instance(&|resolved| resolved.instance.guild_id = GuildId(999));
    change_instance(&|resolved| resolved.instance.ruleset_key = "changed".to_string());
    change_instance(&|resolved| {
        resolved.instance.ruleset_version = InstanceRuleSetVersion::new(2).unwrap()
    });
    change_instance(&|resolved| resolved.instance.kind = InstanceKind("changed".to_string()));
    change_instance(&|resolved| resolved.instance.created_by = UserId(999));
    change_instance(&|resolved| {
        resolved
            .instance
            .resources
            .roles
            .insert("member".to_string(), RoleId(999));
    });
    change_instance(&|resolved| {
        resolved
            .instance
            .resources
            .channels
            .insert("room".to_string(), ChannelId(999));
    });
    change_instance(&|resolved| {
        resolved.instance.resources.messages.insert(
            "panel".to_string(),
            InstanceMessageRef {
                channel: ChannelId(999),
                id: MessageId(999),
            },
        );
    });
    change_instance(&|resolved| resolved.instance.status = InstanceStatus::Deleting);

    for changed in variants {
        assert_ne!(baseline, digest(&changed, &plan, false));
    }
    let mut no_instance = context.clone();
    no_instance.instance = None;
    assert_ne!(baseline, digest(&no_instance, &plan, false));
}

#[test]
fn typed_references_modal_buttons_permissions_and_resource_maps_are_sensitive() {
    let context = context(false);
    let plan = complete_plan();
    let baseline = digest(&context, &plan, true);

    assert_action_mutation_changes(&context, &plan, &baseline, 1, |action| {
        let PlannedAction::GrantRole { role, .. } = action else {
            unreachable!()
        };
        *role = PlannedRole::Created("changed".to_string());
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 2, |action| {
        let PlannedAction::GrantRole { role, .. } = action else {
            unreachable!()
        };
        *role = PlannedRole::Instance {
            alias: "changed".to_string(),
        };
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 4, |action| {
        let PlannedAction::OpenModal(modal) = action else {
            unreachable!()
        };
        modal.key = "changed".to_string();
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 4, |action| {
        let PlannedAction::OpenModal(modal) = action else {
            unreachable!()
        };
        modal.title = "changed".to_string();
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 4, |action| {
        let PlannedAction::OpenModal(modal) = action else {
            unreachable!()
        };
        modal.fields.swap(0, 1);
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 4, |action| {
        let PlannedAction::OpenModal(modal) = action else {
            unreachable!()
        };
        modal.fields[0].key = "changed".to_string();
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 4, |action| {
        let PlannedAction::OpenModal(modal) = action else {
            unreachable!()
        };
        modal.fields[0].label = "changed".to_string();
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 4, |action| {
        let PlannedAction::OpenModal(modal) = action else {
            unreachable!()
        };
        modal.fields[0].style = ModalFieldStyle::Paragraph;
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 4, |action| {
        let PlannedAction::OpenModal(modal) = action else {
            unreachable!()
        };
        modal.fields[0].required = false;
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 4, |action| {
        let PlannedAction::OpenModal(modal) = action else {
            unreachable!()
        };
        modal.fields[0].min_length = Some(3);
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 4, |action| {
        let PlannedAction::OpenModal(modal) = action else {
            unreachable!()
        };
        modal.fields[0].max_length = Some(41);
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 4, |action| {
        let PlannedAction::OpenModal(modal) = action else {
            unreachable!()
        };
        modal.fields[0].input_policy = ModalInputPolicy::Preserve;
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 8, |action| {
        let PlannedAction::UpsertOverwrite { channel, .. } = action else {
            unreachable!()
        };
        *channel = PlannedChannel::Created("changed".to_string());
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 8, |action| {
        let PlannedAction::UpsertOverwrite { target, .. } = action else {
            unreachable!()
        };
        *target = PlannedOverwriteTarget::Role(PlannedRole::Created("changed".to_string()));
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 8, |action| {
        let PlannedAction::UpsertOverwrite { allow, .. } = action else {
            unreachable!()
        };
        *allow |= Permissions::MANAGE_CHANNELS;
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 8, |action| {
        let PlannedAction::UpsertOverwrite { deny, .. } = action else {
            unreachable!()
        };
        *deny = Permissions::SEND_MESSAGES;
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 9, |action| {
        let PlannedAction::PostPanel { key, .. } = action else {
            unreachable!()
        };
        *key = "changed".to_string();
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 9, |action| {
        let PlannedAction::PostPanel { channel, .. } = action else {
            unreachable!()
        };
        *channel = PlannedChannel::Created("changed".to_string());
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 9, |action| {
        let PlannedAction::PostPanel { buttons, .. } = action else {
            unreachable!()
        };
        buttons.swap(0, 1);
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 9, |action| {
        let PlannedAction::PostPanel { buttons, .. } = action else {
            unreachable!()
        };
        buttons[0].label = "changed".to_string();
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 9, |action| {
        let PlannedAction::PostPanel { buttons, .. } = action else {
            unreachable!()
        };
        let ButtonRoute::Static { key } = &mut buttons[0].route else {
            unreachable!()
        };
        *key = "changed".to_string();
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 9, |action| {
        let PlannedAction::PostPanel { buttons, .. } = action else {
            unreachable!()
        };
        let ButtonRoute::InstanceAction { instance, .. } = &mut buttons[1].route else {
            unreachable!()
        };
        *instance = InstanceRef::Created(created("changed"));
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 9, |action| {
        let PlannedAction::PostPanel { buttons, .. } = action else {
            unreachable!()
        };
        let ButtonRoute::InstanceAction { action, .. } = &mut buttons[1].route else {
            unreachable!()
        };
        *action = "changed".to_string();
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 12, |action| {
        let PlannedAction::RegisterInstance { key, .. } = action else {
            unreachable!()
        };
        *key = "changed".to_string();
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 12, |action| {
        let PlannedAction::RegisterInstance { resources, .. } = action else {
            unreachable!()
        };
        resources
            .roles
            .insert("member".to_string(), created("changed"));
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 12, |action| {
        let PlannedAction::RegisterInstance { resources, .. } = action else {
            unreachable!()
        };
        resources
            .channels
            .insert("room".to_string(), created("changed"));
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 12, |action| {
        let PlannedAction::RegisterInstance { resources, .. } = action else {
            unreachable!()
        };
        resources
            .messages
            .insert("panel".to_string(), created("changed"));
    });
    assert_action_mutation_changes(&context, &plan, &baseline, 14, |action| {
        let PlannedAction::TeardownInstance { instance } = action else {
            unreachable!()
        };
        *instance = InstanceRef::Created(created("changed"));
    });
}

#[test]
fn builder_limits_and_empty_plan_errors_propagate_without_translation() {
    assert_eq!(
        build_interaction_action_plan_digest_v1(
            &route(),
            &request_digest(),
            &context(false),
            &ActionPlan { steps: vec![] },
            false,
        ),
        Err(InteractionActionPlanDigestBuilderErrorV1::EmptyActionPlan)
    );

    let oversized = ActionPlan {
        steps: vec![PlannedAction::RespondEphemeral {
            content: "x".repeat(70_000),
        }],
    };
    assert_eq!(
        build_interaction_action_plan_digest_v1(
            &route(),
            &request_digest(),
            &context(false),
            &oversized,
            false,
        ),
        Err(InteractionActionPlanDigestBuilderErrorV1::ActionPayloadTooLarge)
    );

    let too_many = ActionPlan {
        steps: (0..257)
            .map(|_| PlannedAction::RespondEphemeral {
                content: "ok".to_string(),
            })
            .collect(),
    };
    assert_eq!(
        build_interaction_action_plan_digest_v1(
            &route(),
            &request_digest(),
            &context(false),
            &too_many,
            false,
        ),
        Err(InteractionActionPlanDigestBuilderErrorV1::TooManyActions)
    );

    let cumulative = ActionPlan {
        steps: (0..18)
            .map(|_| PlannedAction::RespondEphemeral {
                content: "x".repeat(60_000),
            })
            .collect(),
    };
    assert_eq!(
        build_interaction_action_plan_digest_v1(
            &route(),
            &request_digest(),
            &context(false),
            &cumulative,
            false,
        ),
        Err(InteractionActionPlanDigestBuilderErrorV1::ActionPlanTooLarge)
    );
}
