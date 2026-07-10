use automation_core::event::{EventKind, RuntimeEvent};
use automation_core::mock::{
    MockInteractionResponder, MockMutationAdapter, MutationCall, ResponderCall,
};
use automation_core::run::{handle_event, HandleOutcome};
use automation_state::{
    ActionSpec, ActionTarget, ButtonSpec, InteractionRule, InteractionRuleSet, PanelSpec,
    TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::{GuildId, RoleId, UserId};
use futures::executor::block_on;
use resource_resolution::ResourceBindingMap;

fn fixture() -> (InteractionRuleSet, ResourceBindingMap) {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![PanelSpec {
            key: "verify_panel".to_string(),
            channel: ResourceKey("verify_channel".to_string()),
            content: "click".to_string(),
            buttons: vec![ButtonSpec {
                key: "verify_button".to_string(),
                label: "Verify".to_string(),
            }],
        }],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "verify_rule".to_string(),
            trigger: TriggerSpec::ButtonClick {
                component: "verify_button".to_string(),
            },
            actions: vec![
                ActionSpec::GrantRole {
                    role: ResourceKey("verified_member".to_string()),
                    target: ActionTarget::Actor,
                },
                ActionSpec::RespondEphemeral {
                    content: "welcome".to_string(),
                },
            ],
        }],
    };
    let mut bindings = ResourceBindingMap::default();
    bindings
        .role_bindings
        .insert(ResourceKey("verified_member".to_string()), RoleId(555));
    (set, bindings)
}

fn click(component: &str) -> RuntimeEvent {
    RuntimeEvent {
        guild_id: GuildId(9),
        actor: UserId(42),
        kind: EventKind::ButtonClick {
            component: component.to_string(),
        },
    }
}

#[test]
fn matching_event_grants_role_and_responds() {
    let (set, bindings) = fixture();
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();

    let outcome = block_on(handle_event(
        &click("verify_button"),
        &set,
        &bindings,
        &mutation,
        &responder,
    ))
    .unwrap();

    assert_eq!(outcome, HandleOutcome::Executed);
    assert_eq!(
        mutation.calls(),
        vec![MutationCall::GrantRole {
            guild: GuildId(9),
            member: UserId(42),
            role: RoleId(555),
        }]
    );
    assert_eq!(
        responder.calls(),
        vec![ResponderCall::RespondEphemeral {
            content: "welcome".to_string(),
        }]
    );
}

#[test]
fn unmatched_event_is_noop_with_no_calls() {
    let (set, bindings) = fixture();
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();

    let outcome = block_on(handle_event(
        &click("other_button"),
        &set,
        &bindings,
        &mutation,
        &responder,
    ))
    .unwrap();

    assert_eq!(outcome, HandleOutcome::NoOp);
    assert!(mutation.calls().is_empty());
    assert!(responder.calls().is_empty());
}

#[test]
fn mutation_failure_propagates() {
    use automation_core::adapter::{AdapterError, AdapterErrorKind};

    let (set, bindings) = fixture();
    let mutation = MockMutationAdapter::failing(AdapterError::new(
        AdapterErrorKind::Forbidden,
        "missing perms",
    ));
    let responder = MockInteractionResponder::new();

    let result = block_on(handle_event(
        &click("verify_button"),
        &set,
        &bindings,
        &mutation,
        &responder,
    ));

    assert_eq!(result.unwrap_err().kind, AdapterErrorKind::Forbidden);
    assert!(responder.calls().is_empty());
}
