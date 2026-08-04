use std::collections::BTreeMap;

use automation_core::adapter::{AdapterError, AdapterErrorKind};
use automation_core::mock::{MockInteractionResponder, MockMutationAdapter, MutationCall};
use automation_core::plan::{ActionPlan, PlannedAction, PlannedChannel, PlannedRole};
use automation_core::preflight::{
    execute_preflighted_action_plan_v1, preflight_action_plan_v1, prepare_action_plan_v1,
    ActionInputDependencyV1, ActionPlanPreflightErrorV1, ActionPlanSnapshotIdentityV1,
    ActionPlanSnapshotV1, FreshObservationV1, PreflightChannelRefV1, PreflightRoleRefV1,
    PreparedPlanActionV1,
};
use automation_core::{AutomationServices, RuntimeContext};
use automation_instance::{
    InMemoryInstanceStore, InstanceKind, InstanceRuleSetVersion, SequenceInstanceIdGenerator,
};
use automation_state::{CreatedRef, InstanceResourceRefs};
use discord_model::{
    Channel, ChannelId, ChannelType, GuildId, Member, Permissions, Role, RoleId, UserId,
};
use futures::executor::block_on;

const GUILD: GuildId = GuildId(7);
const ACTOR: UserId = UserId(42);
const BOT: UserId = UserId(900);

fn context() -> RuntimeContext {
    RuntimeContext {
        guild_id: GUILD,
        actor: ACTOR,
        ruleset_key: "studyroom".to_string(),
        ruleset_version: InstanceRuleSetVersion::new(1).unwrap(),
        inputs: BTreeMap::from([("room_name".to_string(), "cozy".to_string())]),
        instance: None,
    }
}

fn created(key: &str) -> CreatedRef {
    CreatedRef {
        created: key.to_string(),
    }
}

fn complete_plan() -> ActionPlan {
    ActionPlan {
        steps: vec![
            PlannedAction::CreateRole {
                key: "member".to_string(),
                name: "${input.room_name} member".to_string(),
            },
            PlannedAction::CreateChannel {
                key: "room".to_string(),
                name: "study-${input.room_name}".to_string(),
            },
            PlannedAction::GrantRole {
                role: PlannedRole::Created("member".to_string()),
                target: ACTOR,
            },
            PlannedAction::PostPanel {
                key: "panel".to_string(),
                channel: PlannedChannel::Created("room".to_string()),
                content: "Welcome".to_string(),
                buttons: vec![],
            },
            PlannedAction::RegisterInstance {
                key: "instance".to_string(),
                kind: InstanceKind("study_room".to_string()),
                resources: InstanceResourceRefs {
                    roles: BTreeMap::from([("member".to_string(), created("member"))]),
                    channels: BTreeMap::from([("room".to_string(), created("room"))]),
                    messages: BTreeMap::from([("panel".to_string(), created("panel"))]),
                },
            },
        ],
    }
}

fn snapshot(identity: &str, permissions: Permissions) -> ActionPlanSnapshotV1 {
    ActionPlanSnapshotV1 {
        guild_id: GUILD,
        identity: ActionPlanSnapshotIdentityV1::new(identity.to_string()).unwrap(),
        roles: Some(vec![
            Role {
                id: RoleId(GUILD.0),
                name: "everyone".to_string(),
                permissions: Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
                position: 0,
                managed: false,
            },
            Role {
                id: RoleId(70),
                name: "bot".to_string(),
                permissions,
                position: 10,
                managed: false,
            },
        ]),
        channels: Some(vec![Channel {
            id: ChannelId(99),
            name: "hub".to_string(),
            channel_type: ChannelType::Text,
            parent_id: None,
            position: 0,
            overwrites: vec![],
        }]),
        bot_member: Some(Member {
            user_id: BOT,
            roles: vec![RoleId(70)],
        }),
        actor_member: Some(Member {
            user_id: ACTOR,
            roles: vec![],
        }),
    }
}

#[test]
fn whole_plan_preparation_is_deterministic_and_types_cross_action_edges() {
    let first = prepare_action_plan_v1(
        &context(),
        &complete_plan(),
        &SequenceInstanceIdGenerator::new("room", 1),
    )
    .unwrap();
    let second = prepare_action_plan_v1(
        &context(),
        &complete_plan(),
        &SequenceInstanceIdGenerator::new("room", 1),
    )
    .unwrap();

    assert_eq!(first, second);
    assert_eq!(first.digest_material_v1(), second.digest_material_v1());
    assert!(first
        .snapshot_request()
        .observations()
        .contains(&FreshObservationV1::ActorMember));
    match &first.actions()[2] {
        PreparedPlanActionV1::GrantRole {
            role: PreflightRoleRefV1::Produced(reference),
            ..
        } => assert_eq!(reference.producer().ordinal(), 0),
        other => panic!("unexpected action: {other:?}"),
    }
    match &first.actions()[3] {
        PreparedPlanActionV1::PostPanel {
            channel: PreflightChannelRefV1::Produced(reference),
            ..
        } => assert_eq!(reference.producer().ordinal(), 1),
        other => panic!("unexpected action: {other:?}"),
    }
    assert!(first.dependencies()[&first.actions()[2].entry()].contains(
        &ActionInputDependencyV1::PriorEffect(first.actions()[0].entry())
    ));
}

#[test]
fn whole_plan_malformed_late_template_has_zero_effects() {
    let plan = ActionPlan {
        steps: vec![
            PlannedAction::CreateRole {
                key: "member".to_string(),
                name: "member".to_string(),
            },
            PlannedAction::RespondEphemeral {
                content: "${input.missing}".to_string(),
            },
        ],
    };
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let result = prepare_action_plan_v1(
        &context(),
        &plan,
        &SequenceInstanceIdGenerator::new("room", 1),
    );

    assert!(matches!(
        result,
        Err(ActionPlanPreflightErrorV1::Template { .. })
    ));
    assert!(mutation.calls().is_empty());
    assert!(responder.calls().is_empty());
}

#[test]
fn forward_created_reference_is_typed_preflight_failure() {
    let plan = ActionPlan {
        steps: vec![
            PlannedAction::GrantRole {
                role: PlannedRole::Created("later".to_string()),
                target: ACTOR,
            },
            PlannedAction::CreateRole {
                key: "later".to_string(),
                name: "later".to_string(),
            },
        ],
    };
    let result = prepare_action_plan_v1(
        &context(),
        &plan,
        &SequenceInstanceIdGenerator::new("room", 1),
    );

    assert!(matches!(
        result,
        Err(ActionPlanPreflightErrorV1::ProducerNotPrior { .. })
    ));
}

#[test]
fn preflighted_executor_consumes_exact_successful_outputs() {
    let prepared = prepare_action_plan_v1(
        &context(),
        &complete_plan(),
        &SequenceInstanceIdGenerator::new("room", 1),
    )
    .unwrap();
    let identity = ActionPlanSnapshotIdentityV1::new("snap:1".to_string()).unwrap();
    let preflighted =
        preflight_action_plan_v1(prepared, snapshot("snap:1", Permissions::ADMINISTRATOR)).unwrap();
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let instances = InMemoryInstanceStore::new();
    let instance_ids = SequenceInstanceIdGenerator::new("unused", 1);
    let teardown = automation_core::MockInstanceTeardownService::new();
    let result = block_on(execute_preflighted_action_plan_v1(
        preflighted,
        &identity,
        &AutomationServices {
            mutation: &mutation,
            responder: &responder,
            instances: &instances,
            instance_ids: &instance_ids,
            teardown: &teardown,
        },
    ))
    .unwrap();

    assert_eq!(result.created.len(), 4);
    assert_eq!(
        mutation.calls(),
        vec![
            MutationCall::CreateRole {
                guild: GUILD,
                name: "cozy member".to_string(),
            },
            MutationCall::CreateChannel {
                guild: GUILD,
                name: "study-cozy".to_string(),
            },
            MutationCall::GrantRole {
                guild: GUILD,
                member: ACTOR,
                role: RoleId(800_000),
            },
            MutationCall::PostPanel {
                guild: GUILD,
                channel: ChannelId(800_001),
                content: "Welcome".to_string(),
                buttons: vec![],
            },
        ]
    );
}

#[test]
fn failed_producer_never_exposes_generated_id_to_consumer() {
    let prepared = prepare_action_plan_v1(
        &context(),
        &ActionPlan {
            steps: vec![
                PlannedAction::CreateRole {
                    key: "member".to_string(),
                    name: "member".to_string(),
                },
                PlannedAction::GrantRole {
                    role: PlannedRole::Created("member".to_string()),
                    target: ACTOR,
                },
            ],
        },
        &SequenceInstanceIdGenerator::new("room", 1),
    )
    .unwrap();
    let identity = ActionPlanSnapshotIdentityV1::new("snap:1".to_string()).unwrap();
    let preflighted =
        preflight_action_plan_v1(prepared, snapshot("snap:1", Permissions::ADMINISTRATOR)).unwrap();
    let mutation =
        MockMutationAdapter::failing(AdapterError::new(AdapterErrorKind::Forbidden, "denied"));
    let result = block_on(execute_preflighted_action_plan_v1(
        preflighted,
        &identity,
        &AutomationServices {
            mutation: &mutation,
            responder: &MockInteractionResponder::new(),
            instances: &InMemoryInstanceStore::new(),
            instance_ids: &SequenceInstanceIdGenerator::new("unused", 1),
            teardown: &automation_core::MockInstanceTeardownService::new(),
        },
    ));

    assert_eq!(result.unwrap_err().kind, AdapterErrorKind::Forbidden);
    assert_eq!(mutation.calls().len(), 1);
    assert!(matches!(
        mutation.calls()[0],
        MutationCall::CreateRole { .. }
    ));
}

#[test]
fn snapshot_identity_drift_rejects_before_any_effect() {
    let prepared = prepare_action_plan_v1(
        &context(),
        &complete_plan(),
        &SequenceInstanceIdGenerator::new("room", 1),
    )
    .unwrap();
    let preflighted =
        preflight_action_plan_v1(prepared, snapshot("snap:1", Permissions::ADMINISTRATOR)).unwrap();
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let result = block_on(execute_preflighted_action_plan_v1(
        preflighted,
        &ActionPlanSnapshotIdentityV1::new("snap:2".to_string()).unwrap(),
        &AutomationServices {
            mutation: &mutation,
            responder: &responder,
            instances: &InMemoryInstanceStore::new(),
            instance_ids: &SequenceInstanceIdGenerator::new("unused", 1),
            teardown: &automation_core::MockInstanceTeardownService::new(),
        },
    ));

    assert_eq!(result.unwrap_err().kind, AdapterErrorKind::BadRequest);
    assert!(mutation.calls().is_empty());
    assert!(responder.calls().is_empty());
}

#[test]
fn insufficient_fresh_authority_rejects_before_execution() {
    let prepared = prepare_action_plan_v1(
        &context(),
        &complete_plan(),
        &SequenceInstanceIdGenerator::new("room", 1),
    )
    .unwrap();
    let result = preflight_action_plan_v1(prepared, snapshot("snap:1", Permissions::VIEW_CHANNEL));

    assert!(matches!(
        result,
        Err(ActionPlanPreflightErrorV1::BotPermissionMissing { .. })
    ));
}
