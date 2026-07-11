use automation_core::{
    handle_event, AdapterErrorKind, AutomationServices, EventKind, MockInteractionResponder,
    MockMutationAdapter, RunningRuleSetIdentity, RuntimeEvent,
};
use automation_instance::{
    AutomationInstance, InstanceId, InstanceRuleSetVersion, InstanceStatus, InstanceStore,
    InstanceStoreError, SequenceInstanceIdGenerator,
};
use automation_state::InteractionRuleSet;
use discord_model::{GuildId, UserId};
use futures::executor::block_on;
use resource_resolution::ResourceBindingMap;

struct PanicInstances;

impl InstanceStore for PanicInstances {
    async fn register(&self, _: AutomationInstance) -> Result<(), InstanceStoreError> {
        panic!("register must not be called on a misrouted InstanceAction")
    }

    async fn get(
        &self,
        _: GuildId,
        _: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
        panic!("get must not be called on a misrouted InstanceAction")
    }

    async fn list_by_guild(
        &self,
        _: GuildId,
    ) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
        panic!("list_by_guild must not be called on a misrouted InstanceAction")
    }

    async fn update_status(
        &self,
        _: GuildId,
        _: &InstanceId,
        _: InstanceStatus,
    ) -> Result<(), InstanceStoreError> {
        panic!("update_status must not be called on a misrouted InstanceAction")
    }
}

#[test]
fn handle_event_rejects_instance_action() {
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let instances = PanicInstances;
    let ids = SequenceInstanceIdGenerator::new("room", 1);
    let services = AutomationServices {
        mutation: &mutation,
        responder: &responder,
        instances: &instances,
        instance_ids: &ids,
    };
    let event = RuntimeEvent {
        guild_id: GuildId(7),
        actor: UserId(42),
        kind: EventKind::InstanceAction {
            instance_id: InstanceId::parse("room_a").unwrap(),
            action: "join".to_string(),
        },
    };
    let identity = RunningRuleSetIdentity {
        key: "studyroom_demo".to_string(),
        version: InstanceRuleSetVersion::new(1).unwrap(),
    };
    let ruleset = InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![],
    };
    let error = block_on(handle_event(
        &event,
        &ruleset,
        &ResourceBindingMap::default(),
        &services,
        "failed",
        &identity,
    ))
    .unwrap_err();
    assert_eq!(error.kind, AdapterErrorKind::InvalidEventRoute);
    assert!(responder.calls().is_empty());
    assert!(mutation.calls().is_empty());
}
